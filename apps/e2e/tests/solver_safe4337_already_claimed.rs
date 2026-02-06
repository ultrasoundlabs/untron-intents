use anyhow::{Context, Result};
use e2e::{
    alto::{AltoOptions, start_alto},
    anvil::spawn_anvil_with_block_time,
    binaries::{cargo_build_indexer_bins, cargo_build_solver_bin, run_migrations},
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
    pool_db::{fetch_current_intents, wait_for_pool_current_intents_count},
    postgres::{configure_postgrest_roles, wait_for_postgres},
    process::KillOnDrop,
    services::{spawn_indexer, spawn_solver_safe4337_mock_custom},
    solver_db::fetch_job_by_intent_id,
    util::{find_free_port, require_bins},
};
use std::time::{Duration, Instant};

async fn wait_for_job_state(
    db_url: &str,
    intent_id: &str,
    expected: &str,
    timeout: Duration,
) -> Result<()> {
    let start = Instant::now();
    loop {
        if let Ok(job) = fetch_job_by_intent_id(db_url, intent_id).await
            && job.state == expected
        {
            return Ok(());
        }
        if start.elapsed() > timeout {
            anyhow::bail!("timed out waiting for job state={expected} for intent_id={intent_id}");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_solver_safe4337_marks_already_claimed_as_fatal() -> Result<()> {
    if !require_bins(&["docker", "forge", "cast"]) {
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

    // Hub chain (host-run Anvil; Alto reaches it via host.docker.internal).
    let anvil_port = find_free_port()?;
    let rpc_url = format!("http://127.0.0.1:{anvil_port}");
    let alto_rpc_url = format!("http://host.docker.internal:{anvil_port}");
    let _anvil = KillOnDrop::new(spawn_anvil_with_block_time(anvil_port, 2)?);
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Deploy AA stack + pool contracts.
    run_forge_build()?;
    let pk0 = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    let owner0 = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";

    let entrypoint = run_forge_create_entrypoint_v07(&rpc_url, pk0)?;
    let safe_singleton = run_forge_create_safe_singleton(&rpc_url, pk0)?;
    let safe_proxy_factory = run_forge_create_safe_proxy_factory(&rpc_url, pk0)?;
    let safe_module_setup = run_forge_create_safe_module_setup(&rpc_url, pk0)?;
    let safe_4337_module = run_forge_create_safe_4337_module(&rpc_url, pk0, &entrypoint)?;

    let alto = start_alto(AltoOptions {
        network: None,
        container_name: Some(format!("untron-e2e-alto-{}", find_free_port()?)),
        rpc_url: alto_rpc_url,
        entrypoints_csv: entrypoint.clone(),
        executor_private_keys_csv: pk0.to_string(),
        utility_private_key_hex: pk0.to_string(),
        safe_mode: false,
        log_level: "info".to_string(),
        deploy_simulations_contract: true,
        ..Default::default()
    })
    .await?;
    let alto_url = alto.base_url.clone();
    wait_for_http_ok(&format!("{alto_url}/health"), Duration::from_secs(30)).await?;

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
            proxy_factory: safe_proxy_factory.parse().context("parse proxy_factory")?,
            singleton: safe_singleton.parse().context("parse singleton")?,
            module_setup: safe_module_setup.parse().context("parse module_setup")?,
            salt_nonce: alloy::primitives::U256::from(77u64),
        },
    )
    .await
    .context("ensure safe deployed")?;
    let safe_address = format!("{safe_addr:#x}");

    // Fund the Safe for self-paid userops + claim deposit.
    run_cast_transfer_eth(&rpc_url, pk0, &safe_address, "1000000000000000000")?;
    run_cast_entrypoint_deposit_to(
        &rpc_url,
        pk0,
        &entrypoint,
        &safe_address,
        "1000000000000000000",
    )?;
    run_cast_mint_mock_erc20(&rpc_url, pk0, &usdt, &safe_address, "5000000")?;

    // Also fund the EOA so it can claim the intent first.
    run_cast_mint_mock_erc20(&rpc_url, pk0, &usdt, owner0, "5000000")?;

    // Start indexer (pool-only).
    let mut indexer = KillOnDrop::new(spawn_indexer(
        &db_url,
        &rpc_url,
        &intents_addr,
        "pool",
        None,
    )?);

    // Create a TRX transfer intent.
    let to = "0x00000000000000000000000000000000000000aa";
    let _ = run_cast_create_trx_transfer_intent(&rpc_url, pk0, &intents_addr, to, "1234", 1)?;
    wait_for_pool_current_intents_count(&db_url, 1, Duration::from_secs(60)).await?;

    // Freeze indexer DB state: we want the solver to see a still-open intent even after the hub
    // chain claim succeeds (so the solver hits an onchain AlreadyClaimed revert).
    indexer.kill_now();

    // Claim it immediately from the EOA (pk0) so the Safe4337 solver hits an onchain
    // AlreadyClaimed revert.
    // Ensure EOA allowance for the pool claim deposit.
    let status = std::process::Command::new("cast")
        .args([
            "send",
            "--rpc-url",
            &rpc_url,
            "--private-key",
            pk0,
            &usdt,
            "approve(address,uint256)",
            &intents_addr,
            "1000000",
        ])
        .current_dir(e2e::util::repo_root())
        .stdin(std::process::Stdio::null())
        .status()
        .context("cast send approve (EOA)")?;
    if !status.success() {
        anyhow::bail!("EOA approve failed (required for claimIntent)");
    }
    let intent_id = fetch_current_intents(&db_url).await?[0].id.clone();
    let status = std::process::Command::new("cast")
        .args([
            "send",
            "--rpc-url",
            &rpc_url,
            "--private-key",
            pk0,
            &intents_addr,
            "claimIntent(bytes32)",
            &intent_id,
        ])
        .current_dir(e2e::util::repo_root())
        .stdin(std::process::Stdio::null())
        .status()
        .context("cast send claimIntent (EOA)")?;
    if !status.success() {
        anyhow::bail!("EOA claimIntent failed");
    }

    // PostgREST.
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

    // Start solver (safe4337 + mock tron) after the intent is already claimed.
    let _solver = KillOnDrop::new(spawn_solver_safe4337_mock_custom(
        &db_url,
        &postgrest_url,
        &rpc_url,
        &intents_addr,
        pk0,
        &safe_address,
        &entrypoint,
        &safe_4337_module,
        &alto_url,
        &mock_reader,
        "solver-aa-already-claimed",
        &[("INDEXER_MAX_HEAD_LAG_BLOCKS", "1000000")],
    )?);

    // Solver should mark the job as fatal (success=false receipt on claim userop).
    wait_for_job_state(
        &db_url,
        &intent_id,
        "failed_fatal",
        Duration::from_secs(120),
    )
    .await?;

    // Ensure it isn't retry-looping: attempts should not grow once in failed_fatal.
    let job1 = fetch_job_by_intent_id(&db_url, &intent_id).await?;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let job2 = fetch_job_by_intent_id(&db_url, &intent_id).await?;
    if job2.attempts != job1.attempts {
        anyhow::bail!(
            "expected attempts to stop increasing after failed_fatal: before={} after={}",
            job1.attempts,
            job2.attempts
        );
    }
    if job2.last_error.as_deref().unwrap_or("").is_empty() {
        anyhow::bail!("expected last_error to be populated for failed_fatal job");
    }

    Ok(())
}
