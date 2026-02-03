use anyhow::Result;
use e2e::{
    anvil::spawn_anvil,
    binaries::{cargo_build_indexer_bins, cargo_build_solver_bin, run_migrations},
    cast::{run_cast_create_trx_transfer_intent, run_cast_mint_mock_erc20, run_cast_rpc},
    docker::{PostgresOptions, PostgrestOptions, start_postgres, start_postgrest},
    docker_cleanup::cleanup_untron_e2e_containers,
    forge::{
        run_forge_build, run_forge_create_mock_erc20, run_forge_create_mock_tron_tx_reader,
        run_forge_create_mock_untron_v3, run_forge_create_untron_intents_with_args,
    },
    http::wait_for_http_ok,
    pool_db::{fetch_current_intents, wait_for_intents_solved_and_settled, wait_for_pool_current_intents_count},
    postgres::{configure_postgrest_roles, wait_for_postgres},
    process::KillOnDrop,
    services::{spawn_indexer, spawn_solver_mock_custom},
    util::{find_free_port, require_bins},
};
use std::time::{Duration, Instant};

async fn wait_for_solver_table(db_url: &str, table: &str, timeout: Duration) -> Result<()> {
    let pool = sqlx::PgPool::connect(db_url).await?;
    let start = Instant::now();
    loop {
        let exists: bool = sqlx::query_scalar(
            "select exists( \
                select 1 \
                from information_schema.tables \
                where table_schema = 'solver' and table_name = $1 \
            )",
        )
        .bind(table)
        .fetch_one(&pool)
        .await?;
        if exists {
            return Ok(());
        }
        if start.elapsed() > timeout {
            anyhow::bail!("timed out waiting for solver.{table} to exist");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn wait_for_no_job_for_intent(db_url: &str, intent_id_hex: &str, timeout: Duration) -> Result<()> {
    let pool = sqlx::PgPool::connect(db_url).await?;
    let start = Instant::now();
    loop {
        let n: i64 = sqlx::query_scalar(
            "select count(*)::bigint \
             from solver.jobs \
             where intent_id = decode($1, 'hex')",
        )
        .bind(intent_id_hex.trim_start_matches("0x"))
        .fetch_one(&pool)
        .await?;
        if n == 0 {
            return Ok(());
        }
        if start.elapsed() > timeout {
            anyhow::bail!("expected no solver.jobs row for intent_id={intent_id_hex}, got {n}");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn wait_for_job_exists(db_url: &str, intent_id_hex: &str, timeout: Duration) -> Result<()> {
    let pool = sqlx::PgPool::connect(db_url).await?;
    let start = Instant::now();
    loop {
        let n: i64 = sqlx::query_scalar(
            "select count(*)::bigint \
             from solver.jobs \
             where intent_id = decode($1, 'hex')",
        )
        .bind(intent_id_hex.trim_start_matches("0x"))
        .fetch_one(&pool)
        .await?;
        if n > 0 {
            return Ok(());
        }
        if start.elapsed() > timeout {
            anyhow::bail!("timed out waiting for solver.jobs row for intent_id={intent_id_hex}");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_solver_indexer_lag_guard_blocks_then_recovers_mock_tron() -> Result<()> {
    if !require_bins(&["docker", "anvil", "forge", "cast"]) {
        return Ok(());
    }

    cleanup_untron_e2e_containers().ok();

    let network = format!("e2e-net-{}", find_free_port()?);
    let pg_name = format!("untron-e2e-pg-{}", find_free_port()?);
    let pg = start_postgres(PostgresOptions {
        network: Some(network.clone()),
        container_name: Some(pg_name.clone()),
        ..Default::default()
    })
    .await?;
    let db_url = pg.db_url.clone();
    wait_for_postgres(&db_url, Duration::from_secs(30)).await?;

    cargo_build_indexer_bins()?;
    cargo_build_solver_bin()?;
    run_migrations(&db_url, true)?;

    // Hub chain.
    let anvil_port = find_free_port()?;
    let rpc_url = format!("http://127.0.0.1:{anvil_port}");
    let _anvil = KillOnDrop::new(spawn_anvil(anvil_port)?);
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Deploy contracts.
    run_forge_build()?;
    let pk0 = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    let owner0 = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
    let usdt = run_forge_create_mock_erc20(&rpc_url, pk0, "USDT", "USDT", 6)?;
    let mock_reader = run_forge_create_mock_tron_tx_reader(&rpc_url, pk0)?;
    let v3 = run_forge_create_mock_untron_v3(
        &rpc_url,
        pk0,
        &mock_reader,
        "0x0000000000000000000000000000000000000001",
        &usdt,
    )?;
    let intents_addr =
        run_forge_create_untron_intents_with_args(&rpc_url, pk0, owner0, &v3, &usdt)?;

    // Fund solver deposit USDT.
    run_cast_mint_mock_erc20(&rpc_url, pk0, &usdt, owner0, "5000000")?;

    // Start indexer (pool-only) and index the intent.
    let mut indexer = KillOnDrop::new(spawn_indexer(
        &db_url,
        &rpc_url,
        &intents_addr,
        "pool",
        None,
    )?);

    let to = "0x00000000000000000000000000000000000000aa";
    let _ = run_cast_create_trx_transfer_intent(&rpc_url, pk0, &intents_addr, to, "1234", 1)?;
    wait_for_pool_current_intents_count(&db_url, 1, Duration::from_secs(45)).await?;
    let intent_id = fetch_current_intents(&db_url).await?[0].id.clone();

    // Freeze indexing and advance chain head to create lag.
    indexer.kill_now();
    // Mine 20 blocks without indexing them.
    let _ = run_cast_rpc(&rpc_url, "anvil_mine", &["0x14"])?;
    // Emit a new pool event at the advanced head, but keep it unindexed for now.
    let _ = run_cast_create_trx_transfer_intent(&rpc_url, pk0, &intents_addr, to, "4321", 2)?;

    // PostgREST.
    let pgrst_pw = "pgrst_pw";
    configure_postgrest_roles(&db_url, pgrst_pw).await?;
    let pgrst = start_postgrest(PostgrestOptions {
        network: network.clone(),
        container_name: Some(format!("untron-e2e-pgrst-{}", find_free_port()?)),
        db_uri: format!("postgres://pgrst_authenticator:{pgrst_pw}@{pg_name}:5432/untron"),
        ..Default::default()
    })
    .await?;
    let postgrest_url = pgrst.base_url.clone();
    wait_for_http_ok(&format!("{postgrest_url}/health"), Duration::from_secs(30)).await?;

    // Start solver with a very low lag threshold: it should skip ticks and not create a job.
    let _solver = KillOnDrop::new(spawn_solver_mock_custom(
        &db_url,
        &postgrest_url,
        &rpc_url,
        &intents_addr,
        pk0,
        &mock_reader,
        "solver-lag-guard",
        "trx_transfer",
        &[("INDEXER_MAX_HEAD_LAG_BLOCKS", "1")],
    )?);

    wait_for_solver_table(&db_url, "jobs", Duration::from_secs(30)).await?;
    wait_for_no_job_for_intent(&db_url, &intent_id, Duration::from_secs(3)).await?;

    // Resume indexing: the lag guard should clear and the solver should proceed.
    let _indexer2 = KillOnDrop::new(spawn_indexer(
        &db_url,
        &rpc_url,
        &intents_addr,
        "pool",
        None,
    )?);

    wait_for_pool_current_intents_count(&db_url, 2, Duration::from_secs(45)).await?;
    wait_for_job_exists(&db_url, &intent_id, Duration::from_secs(60)).await?;
    let _ = wait_for_intents_solved_and_settled(&db_url, 2, Duration::from_secs(240)).await?;

    Ok(())
}
