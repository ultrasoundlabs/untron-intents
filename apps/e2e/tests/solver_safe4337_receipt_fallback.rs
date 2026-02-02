use anyhow::{Context, Result};
use e2e::{
    alto::{AltoOptions, start_alto},
    anvil::spawn_anvil_with_block_time,
    binaries::{cargo_build_indexer_bins, cargo_build_solver_bin, run_migrations},
    bundler_proxy::BundlerProxy,
    cast::{
        run_cast_create_trx_transfer_intent, run_cast_entrypoint_deposit_to,
        run_cast_mint_mock_erc20, run_cast_transfer_eth,
    },
    docker::{PostgresOptions, PostgrestOptions, start_postgres, start_postgrest},
    docker_cleanup::cleanup_untron_e2e_containers,
    forge::{
        run_forge_build, run_forge_create_entrypoint_v07, run_forge_create_mock_erc20,
        run_forge_create_mock_tron_tx_reader, run_forge_create_mock_untron_v3,
        run_forge_create_safe_4337_module, run_forge_create_safe_module_setup,
        run_forge_create_safe_proxy_factory, run_forge_create_safe_singleton,
        run_forge_create_untron_intents_with_args,
    },
    http::wait_for_http_ok,
    pool_db::{wait_for_intents_solved_and_settled, wait_for_pool_current_intents_count},
    postgres::{configure_postgrest_roles, wait_for_postgres},
    process::KillOnDrop,
    services::{spawn_indexer, spawn_solver_safe4337_mock},
    util::{find_free_port, require_bins},
};
use sqlx::Row;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_safe4337_works_when_bundler_receipts_are_missing() -> Result<()> {
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
    let _anvil = KillOnDrop::new(spawn_anvil_with_block_time(anvil_port, 2)?);
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Deploy AA stack + pool contracts.
    run_forge_build()?;
    let pk0 = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    let owner0 = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";

    let safe_singleton = run_forge_create_safe_singleton(&rpc_url, pk0)?;
    let safe_proxy_factory = run_forge_create_safe_proxy_factory(&rpc_url, pk0)?;
    let safe_module_setup = run_forge_create_safe_module_setup(&rpc_url, pk0)?;
    let entrypoint = run_forge_create_entrypoint_v07(&rpc_url, pk0)?;
    let safe_4337_module = run_forge_create_safe_4337_module(&rpc_url, pk0, &entrypoint)?;

    let alto = start_alto(AltoOptions {
        container_name: Some(format!("untron-e2e-alto-{}", find_free_port()?)),
        rpc_url: format!("http://host.docker.internal:{anvil_port}"),
        entrypoints_csv: entrypoint.clone(),
        executor_private_keys_csv: pk0.to_string(),
        utility_private_key_hex: pk0.to_string(),
        safe_mode: false,
        log_level: "info".to_string(),
        deploy_simulations_contract: true,
        ..Default::default()
    })
    .await?;
    wait_for_http_ok(
        &format!("{}/health", alto.base_url),
        Duration::from_secs(30),
    )
    .await?;

    // Proxy the bundler and intentionally drop receipt responses.
    let proxy = BundlerProxy::start(alto.base_url.clone(), true)
        .await
        .context("start bundler proxy")?;

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

    // For self-paid userops, fund + deposit for the Safe that the solver will deploy.
    let owner_key = hex::decode(pk0.trim_start_matches("0x")).context("decode pk0")?;
    let owner_key: [u8; 32] = owner_key
        .try_into()
        .map_err(|_| anyhow::anyhow!("pk0 must be 32 bytes"))?;
    let safe_addr = aa::ensure_safe_deployed(
        &rpc_url,
        31337,
        owner_key,
        &aa::Safe4337Config {
            entrypoint: entrypoint.parse().context("parse entrypoint")?,
            safe_4337_module: safe_4337_module.parse().context("parse safe_4337_module")?,
        },
        &aa::SafeDeterministicDeploymentConfig {
            proxy_factory: safe_proxy_factory.parse().context("parse proxy_factory")?,
            singleton: safe_singleton.parse().context("parse singleton")?,
            module_setup: safe_module_setup.parse().context("parse module_setup")?,
            salt_nonce: alloy::primitives::U256::from(123u64),
        },
    )
    .await
    .context("ensure safe deployed")?;
    let safe_address = format!("{safe_addr:#x}");

    run_cast_transfer_eth(&rpc_url, pk0, &safe_address, "1000000000000000000")?;
    run_cast_entrypoint_deposit_to(
        &rpc_url,
        pk0,
        &entrypoint,
        &safe_address,
        "1000000000000000000",
    )?;
    run_cast_mint_mock_erc20(&rpc_url, pk0, &usdt, &safe_address, "5000000")?;

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

    // Start indexer (pool-only) + solver (safe4337 + mock tron).
    let _indexer = KillOnDrop::new(spawn_indexer(
        &db_url,
        &rpc_url,
        &intents_addr,
        "pool",
        None,
    )?);

    // Create 1 intent (TRX transfer) to keep the scenario focused on hub receipt fallback.
    let to = "0x00000000000000000000000000000000000000aa";
    let _ = run_cast_create_trx_transfer_intent(&rpc_url, pk0, &intents_addr, to, "1234", 1)?;
    wait_for_pool_current_intents_count(&db_url, 1, Duration::from_secs(60)).await?;

    let _solver = KillOnDrop::new(spawn_solver_safe4337_mock(
        &db_url,
        &pgrst.base_url,
        &rpc_url,
        &intents_addr,
        pk0,
        &safe_address,
        &entrypoint,
        &safe_4337_module,
        &proxy.base_url,
        &mock_reader,
        "solver-aa-receipt-fallback",
    )?);

    let _rows = wait_for_intents_solved_and_settled(&db_url, 1, Duration::from_secs(180)).await?;

    // Assert we exercised the EntryPoint log fallback (not bundler receipts).
    let pool = sqlx::PgPool::connect(&db_url).await?;
    let rows = sqlx::query(
        "select receipt \
         from solver.hub_userops \
         order by userop_id asc",
    )
    .fetch_all(&pool)
    .await?;
    assert!(!rows.is_empty(), "expected solver.hub_userops rows");
    for r in rows {
        let receipt: serde_json::Value = r.try_get("receipt").unwrap_or(serde_json::Value::Null);
        let src = receipt
            .get("source")
            .or_else(|| receipt.get("costSource"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert_eq!(src, "entrypoint_log");
    }

    Ok(())
}
