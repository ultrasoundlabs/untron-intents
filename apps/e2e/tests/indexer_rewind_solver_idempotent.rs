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
    services::{spawn_indexer, spawn_solver_mock},
    util::{find_free_port, require_bins},
};
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_indexer_rewind_does_not_cause_solver_to_double_send() -> Result<()> {
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

    let anvil_port = find_free_port()?;
    let rpc_url = format!("http://127.0.0.1:{anvil_port}");
    let _anvil = KillOnDrop::new(spawn_anvil(anvil_port)?);
    tokio::time::sleep(Duration::from_millis(500)).await;

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
        "0x0000000000000000000000000000000000000002",
    )?;
    let intents_addr =
        run_forge_create_untron_intents_with_args(&rpc_url, pk0, owner0, &v3, &usdt)?;

    // fund claim deposit
    run_cast_mint_mock_erc20(&rpc_url, pk0, &usdt, owner0, "5000000")?;

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
    wait_for_http_ok(
        &format!("{}/health", pgrst.base_url),
        Duration::from_secs(30),
    )
    .await?;

    // Start indexer + solver (EOA + mock tron).
    let mut indexer = spawn_indexer(&db_url, &rpc_url, &intents_addr, "pool", None)?;
    let _solver = KillOnDrop::new(spawn_solver_mock(
        &db_url,
        &pgrst.base_url,
        &rpc_url,
        &intents_addr,
        pk0,
        &mock_reader,
        "solver-rewind",
    )?);

    // Create 1 intent and let solver settle it.
    let to = "0x00000000000000000000000000000000000000aa";
    let _ = run_cast_create_trx_transfer_intent(&rpc_url, pk0, &intents_addr, to, "1234", 1)?;
    wait_for_pool_current_intents_count(&db_url, 1, Duration::from_secs(60)).await?;
    let rows = wait_for_intents_solved_and_settled(&db_url, 1, Duration::from_secs(180)).await?;
    let intent_id = rows[0].id.clone();

    let pool = sqlx::PgPool::connect(&db_url).await?;
    let (job_id, claim_tx_hash, prove_tx_hash): (i64, Vec<u8>, Vec<u8>) = sqlx::query_as(
        "select job_id, claim_tx_hash, prove_tx_hash from solver.jobs where encode(intent_id,'hex') = $1",
    )
    .bind(intent_id.trim_start_matches("0x"))
    .fetch_one(&pool)
    .await
    .context("select solver.jobs done row")?;

    // Stop indexer and rewind pool projection to just before the claim event.
    let claim_seq: i64 =
        sqlx::query_scalar("select min(event_seq) from pool.intent_claimed_ledger where id = $1")
            .bind(&intent_id)
            .fetch_one(&pool)
            .await
            .context("select claim_seq")?;

    // Stop indexer (best-effort). Don't hang the test if the OS is slow to reap the process.
    let _ = indexer.kill();
    for _ in 0..50 {
        if indexer.try_wait().ok().flatten().is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    sqlx::query("select pool.rollback_from(31337, $1::evm_address, $2)")
        .bind(&intents_addr)
        .bind(claim_seq)
        .execute(&pool)
        .await
        .context("pool.rollback_from")?;

    // The open intent should re-appear in projections, but the solver must not double-send since:
    // - `solver.jobs` is unique on intent_id, and the job is already `done`
    // - job leasing does not include `done`.
    tokio::time::sleep(Duration::from_secs(3)).await;

    let (claim2, prove2): (Vec<u8>, Vec<u8>) =
        sqlx::query_as("select claim_tx_hash, prove_tx_hash from solver.jobs where job_id = $1")
            .bind(job_id)
            .fetch_one(&pool)
            .await
            .context("reselect solver.jobs hashes")?;
    assert_eq!(claim_tx_hash, claim2);
    assert_eq!(prove_tx_hash, prove2);

    Ok(())
}
