use anyhow::{Context, Result};
use e2e::{
    alto::{AltoOptions, start_alto},
    anvil::spawn_anvil_with_block_time,
    binaries::{cargo_build_indexer_bins, cargo_build_solver_bin, run_migrations},
    cast::{
        cast_abi_encode, run_cast_call, run_cast_create_delegate_resource_intent,
        run_cast_create_trx_transfer_intent, run_cast_entrypoint_deposit_to,
        run_cast_mint_mock_erc20, run_cast_transfer_eth,
    },
    docker::{PostgresOptions, PostgrestOptions, start_postgres, start_postgrest},
    docker_cleanup::cleanup_untron_e2e_containers,
    forge::{
        run_forge_build, run_forge_create_entrypoint_v07, run_forge_create_mock_erc20,
        run_forge_create_mock_untron_v3, run_forge_create_safe_4337_module,
        run_forge_create_safe_module_setup, run_forge_create_safe_proxy_factory,
        run_forge_create_safe_singleton, run_forge_create_test_tron_tx_reader_no_sig,
        run_forge_create_untron_intents_with_args,
    },
    http::wait_for_http_ok,
    pool_db::{wait_for_intents_solved_and_settled, wait_for_pool_current_intents_count},
    postgres::{configure_postgrest_roles, wait_for_postgres},
    process::KillOnDrop,
    services::{spawn_indexer, spawn_solver_safe4337_tron_grpc},
    tronbox::{decode_hex32, wait_for_tronbox_accounts, wait_for_tronbox_admin},
    util::{find_free_port, require_bins},
};
use std::time::Duration;
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_solver_safe4337_tron_grpc_fills_trx_transfer_and_delegate_resource() -> Result<()> {
    if !require_bins(&["docker", "anvil", "forge", "cast"]) {
        return Ok(());
    }

    cleanup_untron_e2e_containers().ok();

    // Start a private Tron network (tronbox/tre).
    let tron_tag = std::env::var("TRON_TRE_TAG").unwrap_or_else(|_| "1.0.4".to_string());
    let tron = GenericImage::new("tronbox/tre".to_string(), tron_tag)
        .with_exposed_port(9090.tcp())
        .with_exposed_port(50051.tcp())
        .with_exposed_port(50052.tcp())
        .with_wait_for(WaitFor::Nothing)
        .with_container_name(format!("untron-e2e-tron-{}", find_free_port()?))
        .start()
        .await
        .context("start tronbox/tre container")?;

    let tron_http_port = tron.get_host_port_ipv4(9090).await?;
    let tron_grpc_port = tron.get_host_port_ipv4(50051).await?;
    let tron_http_base = format!("http://127.0.0.1:{tron_http_port}");
    let tron_grpc_url = format!("http://127.0.0.1:{tron_grpc_port}");

    wait_for_tronbox_admin(&tron_http_base, Duration::from_secs(240)).await?;
    let keys = wait_for_tronbox_accounts(&tron_http_base, Duration::from_secs(240)).await?;
    let tron_pk0 = keys[0].clone();
    let tron_pk1 = keys[1].clone();

    let tron_pk0_bytes = decode_hex32(&tron_pk0)?;
    let tron_pk1_bytes = decode_hex32(&tron_pk1)?;
    let tron_wallet0 = tron::TronWallet::new(tron_pk0_bytes).context("tron wallet0")?;
    let tron_wallet1 = tron::TronWallet::new(tron_pk1_bytes).context("tron wallet1")?;

    let to_evm = format!("{:#x}", tron_wallet1.address().evm());
    let receiver_evm = to_evm.clone();
    let tron_controller_address = tron_wallet0.address().to_base58check();

    // Pre-stake some TRX for ENERGY so DelegateResource is permitted on fresh private chains.
    {
        let mut grpc = tron::TronGrpc::connect(&tron_grpc_url, None)
            .await
            .context("connect tron grpc (freeze)")?;
        let _freeze_txid = tron_wallet0
            .broadcast_freeze_balance_v2(&mut grpc, 2_000_000, tron::protocol::ResourceCode::Energy)
            .await
            .context("freeze_balance_v2")?;
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    // Postgres (+ docker network for PostgREST).
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

    // Hub chain (host-run Anvil; Alto reaches it via host.docker.internal).
    let anvil_port = find_free_port()?;
    let rpc_url = format!("http://127.0.0.1:{anvil_port}");
    let alto_rpc_url = format!("http://host.docker.internal:{anvil_port}");
    let _anvil = KillOnDrop::new(spawn_anvil_with_block_time(anvil_port, 2)?);
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Deploy contracts.
    run_forge_build()?;
    let pk0 = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    let pk1 = "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d";
    let pk2 = "0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a";
    let owner0 = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";

    // ERC-4337 stack (EntryPoint v0.7 + Safe4337).
    let entrypoint = run_forge_create_entrypoint_v07(&rpc_url, pk0)?;
    let safe_singleton = run_forge_create_safe_singleton(&rpc_url, pk0)?;
    let proxy_factory = run_forge_create_safe_proxy_factory(&rpc_url, pk0)?;
    let module_setup = run_forge_create_safe_module_setup(&rpc_url, pk0)?;
    let safe_4337_module = run_forge_create_safe_4337_module(&rpc_url, pk0, &entrypoint)?;

    // Start Alto bundler (docker).
    let alto = start_alto(AltoOptions {
        network: None,
        container_name: Some(format!("untron-e2e-alto-{}", find_free_port()?)),
        rpc_url: alto_rpc_url,
        entrypoints_csv: entrypoint.clone(),
        executor_private_keys_csv: pk1.to_string(),
        utility_private_key_hex: pk2.to_string(),
        safe_mode: false,
        log_level: "info".to_string(),
        deploy_simulations_contract: true,
        ..Default::default()
    })
    .await?;
    let alto_url = alto.base_url.clone();
    wait_for_http_ok(&format!("{alto_url}/health"), Duration::from_secs(30)).await?;

    // Protocol contracts.
    let usdt = run_forge_create_mock_erc20(&rpc_url, pk0, "USDT", "USDT", 6)?;
    let test_reader = run_forge_create_test_tron_tx_reader_no_sig(&rpc_url, pk0)?;
    let v3 = run_forge_create_mock_untron_v3(
        &rpc_url,
        pk0,
        &test_reader,
        "0x0000000000000000000000000000000000000001",
        "0x0000000000000000000000000000000000000002",
    )?;
    let intents_addr =
        run_forge_create_untron_intents_with_args(&rpc_url, pk0, owner0, &v3, &usdt)?;

    // Pre-deploy a Safe for the solver (so we can fund it before the process starts).
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
            proxy_factory: proxy_factory.parse().context("parse proxy_factory")?,
            singleton: safe_singleton.parse().context("parse safe_singleton")?,
            module_setup: module_setup.parse().context("parse module_setup")?,
            salt_nonce: alloy::primitives::U256::from(1u64),
        },
    )
    .await
    .context("ensure safe deployed")?;
    let safe_address = format!("{safe_addr:#x}");

    // Fund solver (Safe) with USDT claim deposit + ETH and EntryPoint deposit for self-paid userops.
    run_cast_mint_mock_erc20(&rpc_url, pk0, &usdt, &safe_address, "5000000")?;
    run_cast_transfer_eth(&rpc_url, pk0, &safe_address, "1000000000000000000")?;
    run_cast_entrypoint_deposit_to(
        &rpc_url,
        pk0,
        &entrypoint,
        &safe_address,
        "1000000000000000000",
    )?;

    // Sanity: Safe looks initialized.
    let version = run_cast_call(&rpc_url, &safe_address, "VERSION()(string)", &[])
        .context("cast call Safe.VERSION")?;
    if !version.contains("1.") {
        anyhow::bail!("unexpected Safe.VERSION() value: {version}");
    }

    // Pre-approve the pool to pull the solver's USDT claim deposit. This avoids needing an extra
    // approve userop on startup which can be noisy/flaky in bundler e2e.
    {
        let approve_data = cast_abi_encode(
            "approve(address,uint256)",
            &[
                &intents_addr,
                "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            ],
        )?;
        let approve_bytes = hex::decode(approve_data.trim_start_matches("0x"))
            .context("decode approve calldata")?;

        let mut sender = aa::Safe4337UserOpSender::new(aa::Safe4337UserOpSenderConfig {
            rpc_url: rpc_url.clone(),
            chain_id: Some(31337),
            entrypoint: entrypoint.parse().context("parse entrypoint (approve)")?,
            safe: Some(safe_addr),
            safe_4337_module: safe_4337_module
                .parse()
                .context("parse safe_4337_module (approve)")?,
            safe_deployment: None,
            bundler_urls: vec![alto_url.clone()],
            owner_private_key: owner_key,
            paymasters: vec![],
            options: aa::Safe4337UserOpSenderOptions::default(),
        })
        .await
        .context("init aa sender (pre-approve)")?;

        let submission = sender
            .send_call(usdt.parse().context("parse usdt")?, approve_bytes)
            .await
            .context("send approve userop")?;

        // Wait for the underlying tx to be mined.
        let start = std::time::Instant::now();
        let tx_hash = loop {
            if start.elapsed() > Duration::from_secs(120) {
                anyhow::bail!("timeout waiting for approve userop receipt");
            }
            let payload = serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "eth_getUserOperationReceipt",
                "params": [submission.userop_hash],
            });
            let resp = reqwest::Client::new()
                .post(&alto_url)
                .json(&payload)
                .send()
                .await
                .context("post bundler jsonrpc (approve receipt)")?;
            let val: serde_json::Value =
                resp.json().await.context("decode approve receipt json")?;
            if let Some(txh) = val
                .get("result")
                .and_then(|r| r.get("transactionHash"))
                .and_then(|v| v.as_str())
                .or_else(|| {
                    val.get("result")
                        .and_then(|r| r.get("receipt"))
                        .and_then(|r| r.get("transactionHash"))
                        .and_then(|v| v.as_str())
                })
            {
                break txh.to_string();
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        };
        let _ = tx_hash;
    }

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

    // Start indexer (pool-only).
    let _indexer = KillOnDrop::new(spawn_indexer(
        &db_url,
        &rpc_url,
        &intents_addr,
        "pool",
        None,
    )?);

    // Create two intents that require real Tron txs.
    let _ = run_cast_create_trx_transfer_intent(&rpc_url, pk0, &intents_addr, &to_evm, "1234", 1)?;
    let _ = run_cast_create_delegate_resource_intent(
        &rpc_url,
        pk0,
        &intents_addr,
        &receiver_evm,
        1,
        "1000000",
        "10",
        1,
    )?;
    wait_for_pool_current_intents_count(&db_url, 2, Duration::from_secs(60)).await?;

    // Start solver (Safe4337 + Alto + Tron gRPC).
    let _solver = KillOnDrop::new(spawn_solver_safe4337_tron_grpc(
        &db_url,
        &postgrest_url,
        &rpc_url,
        &intents_addr,
        pk0,
        &safe_address,
        &entrypoint,
        &safe_4337_module,
        &alto_url,
        &tron_grpc_url,
        &tron_pk0,
        &tron_controller_address,
        "solver-aa-tron-grpc",
    )?);

    let _rows = wait_for_intents_solved_and_settled(&db_url, 2, Duration::from_secs(300)).await?;
    Ok(())
}
