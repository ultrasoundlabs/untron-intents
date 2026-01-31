use anyhow::{Context, Result};
use e2e::{
    anvil::spawn_anvil,
    binaries::{cargo_build_indexer_bins, cargo_build_solver_bin, run_migrations},
    cast::{
        run_cast_create_delegate_resource_intent, run_cast_create_trx_transfer_intent,
        run_cast_mint_mock_erc20,
    },
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
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_solver_mvp_fills_trx_transfer_and_delegate_resource_mock_tron() -> Result<()> {
    if !require_bins(&["docker", "anvil", "forge", "cast"]) {
        return Ok(());
    }

    let network = format!("e2e-net-{}", find_free_port()?);
    let pg_name = format!("pg-{}", find_free_port()?);

    let pg = GenericImage::new("postgres", "18.1")
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stdout(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_DB", "untron")
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_network(network.clone())
        .with_container_name(pg_name.clone())
        .start()
        .await
        .context("start postgres container")?;

    let pg_port = pg.get_host_port_ipv4(5432).await?;
    let db_url = format!("postgres://postgres:postgres@127.0.0.1:{pg_port}/untron");
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

    // Create two intents.
    let to = "0x00000000000000000000000000000000000000aa";
    let _ = run_cast_create_trx_transfer_intent(&rpc_url, pk0, &intents_addr, to, "1234", 1)?;
    let _ = run_cast_create_delegate_resource_intent(
        &rpc_url,
        pk0,
        &intents_addr,
        to,
        1,
        "1000000",
        "10",
        1,
    )?;

    wait_for_pool_current_intents_count(&db_url, 2, Duration::from_secs(45)).await?;

    // PostgREST.
    let pgrst_pw = "pgrst_pw";
    configure_postgrest_roles(&db_url, pgrst_pw).await?;

    let pgrst = GenericImage::new("postgrest/postgrest", "v14.2")
        .with_exposed_port(3000.tcp())
        .with_wait_for(WaitFor::Nothing)
        .with_env_var(
            "PGRST_DB_URI",
            format!("postgres://pgrst_authenticator:{pgrst_pw}@{pg_name}:5432/untron"),
        )
        .with_env_var("PGRST_DB_SCHEMA", "api")
        .with_env_var("PGRST_DB_ANON_ROLE", "pgrst_anon")
        .with_network(network)
        .start()
        .await
        .context("start postgrest container")?;

    let pgrst_port = pgrst.get_host_port_ipv4(3000).await?;
    let postgrest_url = format!("http://127.0.0.1:{pgrst_port}");
    wait_for_http_ok(&format!("{postgrest_url}/health"), Duration::from_secs(30)).await?;

    // Start solver (mock tron).
    let _solver = KillOnDrop::new(spawn_solver_mock(
        &db_url,
        &postgrest_url,
        &rpc_url,
        &intents_addr,
        pk0,
        &mock_reader,
    )?);

    let _rows = wait_for_intents_solved_and_settled(&db_url, 2, Duration::from_secs(180)).await?;
    Ok(())
}
