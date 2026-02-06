use anyhow::Result;
use e2e::{
    anvil::spawn_anvil,
    binaries::{cargo_build_indexer_bins, cargo_build_solver_bin, run_migrations},
    cast::{run_cast_create_trx_transfer_intent, run_cast_mint_mock_erc20},
    docker::{start_postgres, start_postgrest, PostgresOptions, PostgrestOptions},
    docker_cleanup::cleanup_untron_e2e_containers,
    forge::{
        run_forge_build, run_forge_create_mock_erc20, run_forge_create_mock_tron_tx_reader,
        run_forge_create_mock_untron_v3, run_forge_create_untron_intents_with_args,
    },
    http::wait_for_http_ok,
    pool_db::{
        fetch_current_intents, wait_for_intents_solved_and_settled,
        wait_for_pool_current_intents_count,
    },
    postgres::{configure_postgrest_roles, wait_for_postgres},
    process::KillOnDrop,
    services::{spawn_indexer, spawn_solver_mock},
    solver_db::fetch_job_by_intent_id,
    util::{find_free_port, require_bins},
};
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_solver_done_state_is_idempotent_across_restart() -> Result<()> {
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
        &usdt,
    )?;
    let intents_addr =
        run_forge_create_untron_intents_with_args(&rpc_url, pk0, owner0, &v3, &usdt)?;

    run_cast_mint_mock_erc20(&rpc_url, pk0, &usdt, owner0, "5000000")?;

    let _indexer = KillOnDrop::new(spawn_indexer(
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

    let pgrst_pw = "pgrst_pw";
    configure_postgrest_roles(&db_url, pgrst_pw).await?;
    let pgrst = start_postgrest(PostgrestOptions {
        network,
        container_name: Some(format!("untron-e2e-pgrst-{}", find_free_port()?)),
        db_uri: format!("postgres://pgrst_authenticator:{pgrst_pw}@{pg_name}:5432/untron"),
        ..Default::default()
    })
    .await?;
    let postgrest_url = pgrst.base_url.clone();
    wait_for_http_ok(&format!("{postgrest_url}/health"), Duration::from_secs(30)).await?;

    let solver1 = KillOnDrop::new(spawn_solver_mock(
        &db_url,
        &postgrest_url,
        &rpc_url,
        &intents_addr,
        pk0,
        &mock_reader,
        "solver1",
    )?);

    let _rows = wait_for_intents_solved_and_settled(&db_url, 1, Duration::from_secs(180)).await?;

    let before = fetch_job_by_intent_id(&db_url, &intent_id).await?;
    assert_eq!(before.state, "done");

    drop(solver1);

    let _solver2 = KillOnDrop::new(spawn_solver_mock(
        &db_url,
        &postgrest_url,
        &rpc_url,
        &intents_addr,
        pk0,
        &mock_reader,
        "solver2",
    )?);

    tokio::time::sleep(Duration::from_secs(3)).await;

    let after = fetch_job_by_intent_id(&db_url, &intent_id).await?;
    assert_eq!(after.state, "done");
    assert_eq!(after.attempts, before.attempts);
    assert_eq!(after.claim_tx_hash, before.claim_tx_hash);
    assert_eq!(after.prove_tx_hash, before.prove_tx_hash);
    assert_eq!(after.tron_txid, before.tron_txid);

    Ok(())
}
