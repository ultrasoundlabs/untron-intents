use anyhow::{Context, Result};
use e2e::{
    anvil::spawn_anvil,
    binaries::{cargo_build_indexer_bins, cargo_build_solver_bin, run_migrations},
    cast::{run_cast_create_trx_transfer_intent, run_cast_mint_mock_erc20},
    docker::{PostgresOptions, PostgrestOptions, start_postgres, start_postgrest},
    docker_cleanup::cleanup_untron_e2e_containers,
    forge::{
        run_forge_build, run_forge_create_mock_erc20, run_forge_create_mock_tron_tx_reader,
        run_forge_create_mock_untron_v3, run_forge_create_untron_intents_with_args,
    },
    http::wait_for_http_ok,
    pool_db::{wait_for_intents_solved_and_settled, wait_for_pool_current_intents_count},
    postgres::{configure_postgrest_roles, wait_for_postgres},
    process::KillOnDrop,
    services::{spawn_indexer, spawn_solver_mock_custom},
    util::{find_free_port, require_bins},
};
use std::process::{Command, Stdio};
use std::time::Duration;

fn docker_pause(name: &str) -> Result<()> {
    let status = Command::new("docker")
        .args(["pause", name])
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("docker pause")?;
    if !status.success() {
        anyhow::bail!("docker pause failed for {name}");
    }
    Ok(())
}

fn docker_unpause(name: &str) -> Result<()> {
    let status = Command::new("docker")
        .args(["unpause", name])
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("docker unpause")?;
    if !status.success() {
        anyhow::bail!("docker unpause failed for {name}");
    }
    Ok(())
}

async fn wait_for_job_count_for_intent(
    db_url: &str,
    intent_id_hex: &str,
    expected: i64,
    timeout: Duration,
) -> Result<()> {
    let pool = sqlx::PgPool::connect(db_url).await?;
    let start = std::time::Instant::now();
    loop {
        let n: i64 = sqlx::query_scalar(
            "select count(*)::bigint from solver.jobs where intent_id = decode($1,'hex')",
        )
        .bind(intent_id_hex.trim_start_matches("0x"))
        .fetch_one(&pool)
        .await?;
        if n == expected {
            return Ok(());
        }
        if start.elapsed() > timeout {
            anyhow::bail!(
                "timed out waiting for solver.jobs count={expected} for intent_id={intent_id_hex}, got {n}"
            );
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_solver_recovers_from_postgrest_outage() -> Result<()> {
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

    // Start indexer (pool-only).
    let _indexer = KillOnDrop::new(spawn_indexer(
        &db_url,
        &rpc_url,
        &intents_addr,
        "pool",
        None,
    )?);

    // Create one intent.
    let to = "0x00000000000000000000000000000000000000aa";
    let _ = run_cast_create_trx_transfer_intent(&rpc_url, pk0, &intents_addr, to, "1234", 1)?;
    wait_for_pool_current_intents_count(&db_url, 1, Duration::from_secs(45)).await?;
    let intent1_id = e2e::pool_db::fetch_current_intents(&db_url)
        .await?
        .first()
        .context("missing intent row")?
        .id
        .clone();

    // PostgREST.
    let pgrst_pw = "pgrst_pw";
    configure_postgrest_roles(&db_url, pgrst_pw).await?;
    let pgrst_name = format!("untron-e2e-pgrst-{}", find_free_port()?);
    let pgrst = start_postgrest(PostgrestOptions {
        network: network.clone(),
        container_name: Some(pgrst_name.clone()),
        db_uri: format!("postgres://pgrst_authenticator:{pgrst_pw}@{pg_name}:5432/untron"),
        ..Default::default()
    })
    .await?;
    let postgrest_url = pgrst.base_url.clone();
    wait_for_http_ok(&format!("{postgrest_url}/health"), Duration::from_secs(30)).await?;

    // Start solver (mock tron).
    let _solver = KillOnDrop::new(spawn_solver_mock_custom(
        &db_url,
        &postgrest_url,
        &rpc_url,
        &intents_addr,
        pk0,
        &mock_reader,
        "solver-postgrest-outage",
        "trx_transfer",
        &[],
    )?);

    // Let the solver complete the first intent while PostgREST is healthy.
    let _ = wait_for_intents_solved_and_settled(&db_url, 1, Duration::from_secs(120)).await?;

    // Pause PostgREST, create a new intent while it's down, then resume it.
    // Using pause/unpause avoids changing port mappings (which would break the running solver's
    // INDEXER_API_BASE_URL).
    docker_pause(&pgrst_name)?;

    // Create a second intent while PostgREST is down. The solver cannot discover it until PostgREST
    // comes back up, so we can assert "recovery and continues" deterministically.
    let _ = run_cast_create_trx_transfer_intent(&rpc_url, pk0, &intents_addr, to, "4321", 1)?;
    wait_for_pool_current_intents_count(&db_url, 2, Duration::from_secs(45)).await?;
    let intent2_id = e2e::pool_db::fetch_current_intents(&db_url)
        .await?
        .get(1)
        .context("missing second intent row")?
        .id
        .clone();

    // While PostgREST is paused, the solver should not have a job row for the new intent.
    // This ensures the outage actually affected progress.
    tokio::time::sleep(Duration::from_secs(3)).await;
    wait_for_job_count_for_intent(&db_url, &intent2_id, 0, Duration::from_secs(5)).await?;

    docker_unpause(&pgrst_name)?;
    wait_for_http_ok(&format!("{postgrest_url}/health"), Duration::from_secs(60)).await?;

    // Solver should eventually recover and complete.
    let _rows = wait_for_intents_solved_and_settled(&db_url, 2, Duration::from_secs(240)).await?;

    // Harden: no duplicate jobs per intent (unique constraint), and both intents were processed.
    wait_for_job_count_for_intent(&db_url, &intent1_id, 1, Duration::from_secs(5)).await?;
    wait_for_job_count_for_intent(&db_url, &intent2_id, 1, Duration::from_secs(5)).await?;
    Ok(())
}
