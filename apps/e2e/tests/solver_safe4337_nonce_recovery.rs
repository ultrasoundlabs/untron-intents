use anyhow::{Context, Result};
use e2e::{
    alto::{AltoOptions, start_alto},
    anvil::spawn_anvil_with_block_time,
    binaries::{cargo_build_indexer_bins, cargo_build_solver_bin, run_migrations},
    cast::{
        cast_abi_encode, run_cast_create_trx_transfer_intent, run_cast_entrypoint_deposit_to,
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
    pool_db::{CurrentIntentRowWithId, fetch_current_intents, wait_for_pool_current_intents_count},
    postgres::{configure_postgrest_roles, wait_for_postgres},
    process::KillOnDrop,
    services::{spawn_indexer, spawn_solver_safe4337_mock_custom},
    solver_db::fetch_job_by_intent_id,
    util::{find_free_port, require_bins},
};
use sqlx::Row;
use std::time::{Duration, Instant};

async fn wait_for_tx_success(rpc_url: &str, tx_hash: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let start = Instant::now();
    loop {
        if start.elapsed() > Duration::from_secs(120) {
            anyhow::bail!("timed out waiting for tx receipt: {tx_hash}");
        }
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_getTransactionReceipt",
            "params": [tx_hash]
        });
        let resp = client.post(rpc_url).json(&payload).send().await;
        if let Ok(resp) = resp
            && let Ok(val) = resp.json::<serde_json::Value>().await
            && let Some(r) = val.get("result")
        {
            if r.is_null() {
                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            }
            let status = r.get("status").and_then(|v| v.as_str()).unwrap_or("");
            if status.eq_ignore_ascii_case("0x1") {
                return Ok(());
            }
            anyhow::bail!("tx failed: {tx_hash} receipt={r}");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn wait_for_userop_tx_hash(alto_url: &str, userop_hash: &str) -> Result<String> {
    let client = reqwest::Client::new();
    let start = Instant::now();
    loop {
        if start.elapsed() > Duration::from_secs(120) {
            anyhow::bail!("timed out waiting for userop receipt: {userop_hash}");
        }

        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_getUserOperationReceipt",
            "params": [userop_hash]
        });

        let resp = client.post(alto_url).json(&payload).send().await;
        if let Ok(resp) = resp
            && let Ok(val) = resp.json::<serde_json::Value>().await
        {
            if val.get("error").is_some() {
                tokio::time::sleep(Duration::from_millis(250)).await;
                continue;
            }
            let txh = val
                .pointer("/result/transactionHash")
                .and_then(|v| v.as_str())
                .or_else(|| {
                    val.pointer("/result/receipt/transactionHash")
                        .and_then(|v| v.as_str())
                });
            if let Some(txh) = txh
                && !txh.is_empty()
                && txh != "0x"
            {
                return Ok(txh.to_string());
            }
        }

        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

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

async fn insert_ready_job_for_intent(db_url: &str, intent: &CurrentIntentRowWithId) -> Result<i64> {
    let pool = sqlx::PgPool::connect(db_url).await?;
    let intent_id_hex = intent.id.trim_start_matches("0x");
    let specs_hex = intent.row.intent_specs.trim_start_matches("0x");
    let job_id: i64 = sqlx::query_scalar(
        "insert into solver.jobs(intent_id, intent_type, intent_specs, deadline, state) \
         values (decode($1,'hex'), $2, decode($3,'hex'), $4, 'ready') \
         on conflict (intent_id) do update set intent_type = excluded.intent_type \
         returning job_id",
    )
    .bind(intent_id_hex)
    .bind(intent.row.intent_type)
    .bind(specs_hex)
    .bind(intent.row.deadline)
    .fetch_one(&pool)
    .await
    .context("insert solver.jobs")?;
    Ok(job_id)
}

async fn insert_claimed_job_with_invalid_specs(db_url: &str) -> Result<i64> {
    let pool = sqlx::PgPool::connect(db_url).await?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut intent_id = [0u8; 32];
    intent_id[16..].copy_from_slice(&now.to_be_bytes());
    let deadline = i64::try_from((now / 1_000_000_000) as i128)
        .unwrap_or(i64::MAX)
        .saturating_add(3600);

    let job_id: i64 = sqlx::query_scalar(
        "insert into solver.jobs(intent_id, intent_type, intent_specs, deadline, state, claim_window_expires_at) \
         values ($1, $2, $3, $4, 'claimed', now() + interval '120 seconds') \
         returning job_id",
    )
    .bind(intent_id.to_vec())
    .bind(2i16) // trx_transfer
    .bind(vec![0xdeu8, 0xadu8]) // invalid TRXTransferIntent ABI payload
    .bind(deadline)
    .fetch_one(&pool)
    .await
    .context("insert claimed job with invalid specs")?;
    Ok(job_id)
}

async fn fetch_job_attempts_by_id(db_url: &str, job_id: i64) -> Result<i32> {
    let pool = sqlx::PgPool::connect(db_url).await?;
    let attempts: i32 = sqlx::query_scalar("select attempts from solver.jobs where job_id = $1")
        .bind(job_id)
        .fetch_one(&pool)
        .await
        .context("select attempts by job_id")?;
    Ok(attempts)
}

async fn wait_for_job_attempts_by_id(
    db_url: &str,
    job_id: i64,
    min_attempts: i32,
    timeout: Duration,
) -> Result<()> {
    let start = Instant::now();
    loop {
        let attempts = fetch_job_attempts_by_id(db_url, job_id).await?;
        if attempts >= min_attempts {
            return Ok(());
        }
        if start.elapsed() > timeout {
            anyhow::bail!(
                "timed out waiting for attempts >= {min_attempts} for job_id={job_id}; last={attempts}"
            );
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn insert_stale_prepared_claim_userop(
    db_url: &str,
    job_id: i64,
    userop_json: &str,
) -> Result<()> {
    let pool = sqlx::PgPool::connect(db_url).await?;
    sqlx::query(
        "insert into solver.hub_userops(job_id, kind, state, userop, userop_hash) \
         values ($1, 'claim', 'prepared', $2::jsonb, null) \
         on conflict (job_id, kind) do update set \
            state = excluded.state, \
            userop = excluded.userop, \
            userop_hash = null, \
            updated_at = now()",
    )
    .bind(job_id)
    .bind(userop_json)
    .execute(&pool)
    .await
    .context("insert solver.hub_userops stale prepared")?;
    Ok(())
}

async fn wait_for_claim_userop_nonce_ge(
    db_url: &str,
    job_id: i64,
    min_nonce_hex: &str,
    timeout: Duration,
) -> Result<()> {
    let pool = sqlx::PgPool::connect(db_url).await?;
    let start = Instant::now();
    loop {
        let row = sqlx::query(
            "select userop::text as userop_json, userop_hash, state::text as state \
             from solver.hub_userops \
             where job_id=$1 and kind='claim'",
        )
        .bind(job_id)
        .fetch_optional(&pool)
        .await?;

        if let Some(row) = row {
            let userop_json: String = row.get("userop_json");
            let userop_hash: Option<String> = row.get("userop_hash");
            let state: String = row.get("state");

            let u: alloy::rpc::types::eth::erc4337::PackedUserOperation =
                serde_json::from_str(&userop_json).context("deserialize hub userop json")?;

            let min_nonce =
                alloy::primitives::U256::from_str_radix(min_nonce_hex.trim_start_matches("0x"), 16)
                    .context("parse min_nonce")?;

            if u.nonce >= min_nonce && userop_hash.is_some() && state != "failed_fatal" {
                return Ok(());
            }
        }

        if start.elapsed() > timeout {
            anyhow::bail!("timed out waiting for claim userop nonce >= {min_nonce_hex}");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_solver_safe4337_deletes_stale_prepared_userop_and_recovers() -> Result<()> {
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
    let safe_proxy_factory = run_forge_create_safe_proxy_factory(&rpc_url, pk0)?;
    let safe_module_setup = run_forge_create_safe_module_setup(&rpc_url, pk0)?;
    let safe_4337_module = run_forge_create_safe_4337_module(&rpc_url, pk0, &entrypoint)?;

    // Start Alto bundler.
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

    // Deploy protocol contracts.
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

    // Pre-deploy a Safe for the solver.
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

    // Ensure the solver schema is migrated, but don't let it claim anything yet.
    let mut migrator = KillOnDrop::new(spawn_solver_safe4337_mock_custom(
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
        "solver-aa-migrate",
        &[
            ("INDEXER_MAX_HEAD_LAG_BLOCKS", "1000000"),
            ("SOLVER_ENABLED_INTENT_TYPES", ""),
        ],
    )?);
    wait_for_solver_table(&db_url, "jobs", Duration::from_secs(30)).await?;
    wait_for_solver_table(&db_url, "hub_userops", Duration::from_secs(30)).await?;
    migrator.kill_now();

    // Advance Safe4337 chain nonce by sending a benign userop (USDT approve).
    // This makes our seeded prepared op stale (nonce 0 < chain nonce).
    let approve_data = cast_abi_encode(
        "approve(address,uint256)",
        &[
            &intents_addr,
            "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        ],
    )?;
    let approve_bytes =
        hex::decode(approve_data.trim_start_matches("0x")).context("decode approve calldata")?;

    let mut sender = aa::Safe4337UserOpSender::new(aa::Safe4337UserOpSenderConfig {
        rpc_url: rpc_url.clone(),
        chain_id: Some(31337),
        entrypoint: entrypoint.parse().context("parse entrypoint")?,
        safe: Some(safe_addr),
        safe_4337_module: safe_4337_module.parse().context("parse safe_4337_module")?,
        safe_deployment: None,
        bundler_urls: vec![alto_url.clone()],
        owner_private_key: owner_key,
        paymasters: vec![],
        options: aa::Safe4337UserOpSenderOptions::default(),
    })
    .await
    .context("init aa sender")?;

    let chain_nonce_before = sender.chain_nonce().await.context("read chain nonce")?;
    let submission = sender
        .send_call(usdt.parse().context("parse usdt")?, approve_bytes)
        .await
        .context("send approve userop")?;
    let tx_hash = wait_for_userop_tx_hash(&alto_url, &submission.userop_hash)
        .await
        .context("wait approve userop receipt")?;
    wait_for_tx_success(&rpc_url, &tx_hash).await?;
    let chain_nonce_after = sender
        .chain_nonce()
        .await
        .context("read chain nonce (after)")?;
    if chain_nonce_after <= chain_nonce_before {
        anyhow::bail!(
            "expected chain nonce to advance: before={chain_nonce_before} after={chain_nonce_after}"
        );
    }

    // Create a TRX transfer intent.
    let to = "0x00000000000000000000000000000000000000aa";
    let _ = run_cast_create_trx_transfer_intent(&rpc_url, pk0, &intents_addr, to, "1234", 1)?;
    wait_for_pool_current_intents_count(&db_url, 1, Duration::from_secs(60)).await?;

    let intent = fetch_current_intents(&db_url)
        .await?
        .into_iter()
        .next()
        .context("missing intent row")?;

    // Seed a job + a stale prepared claim userop (nonce 0).
    let job_id = insert_ready_job_for_intent(&db_url, &intent).await?;

    // Reuse a valid userop shape but force nonce=0 to make it stale.
    let mut stale_userop = sender
        .build_call_userop_unestimated(usdt.parse().context("parse usdt")?, Vec::new())
        .await
        .context("build dummy userop")?;
    stale_userop.nonce = alloy::primitives::U256::ZERO;
    let mut v = serde_json::to_value(&stale_userop).context("serialize stale userop")?;
    match v.get_mut("nonce") {
        Some(serde_json::Value::String(_)) => {
            v["nonce"] = serde_json::Value::String("0x0".to_string());
        }
        Some(serde_json::Value::Number(_)) => {
            v["nonce"] = serde_json::Value::Number(0.into());
        }
        _ => {
            v["nonce"] = serde_json::Value::String("0x0".to_string());
        }
    }
    let stale_json = serde_json::to_string(&v).context("stale userop json")?;

    // Sanity check: it's really stale vs current chain nonce.
    {
        let u: alloy::rpc::types::eth::erc4337::PackedUserOperation =
            serde_json::from_str(&stale_json).context("deserialize stale json")?;
        if u.nonce >= chain_nonce_after {
            anyhow::bail!(
                "expected stale seeded nonce < chain nonce: seeded={} chain={}",
                u.nonce,
                chain_nonce_after
            );
        }
    }

    insert_stale_prepared_claim_userop(&db_url, job_id, &stale_json).await?;

    // Start solver: it should delete the stale prepared row (nonce < chain nonce) and rebuild/send.
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
        "solver-aa-nonce-recovery",
        &[("INDEXER_MAX_HEAD_LAG_BLOCKS", "1000000")],
    )?);

    // Assert the persisted claim op is no longer stale and has been submitted.
    wait_for_claim_userop_nonce_ge(
        &db_url,
        job_id,
        &format!("{chain_nonce_after:#x}"),
        Duration::from_secs(120),
    )
    .await?;

    // End-to-end completion.
    let start = Instant::now();
    loop {
        if start.elapsed() > Duration::from_secs(240) {
            anyhow::bail!("timed out waiting for intent to be solved+settled");
        }
        let rows = fetch_current_intents(&db_url).await?;
        if rows.len() == 1
            && rows[0].row.solver.is_some()
            && rows[0].row.solved
            && rows[0].row.funded
            && rows[0].row.settled
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_solver_safe4337_claim_window_prioritizes_and_backpressures() -> Result<()> {
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
    let alto_rpc_url = format!("http://host.docker.internal:{anvil_port}");
    let _anvil = KillOnDrop::new(spawn_anvil_with_block_time(anvil_port, 2)?);
    tokio::time::sleep(Duration::from_millis(500)).await;

    run_forge_build()?;
    let pk0 = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    let pk1 = "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d";
    let pk2 = "0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a";
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
            salt_nonce: alloy::primitives::U256::from(124u64),
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

    let _indexer = KillOnDrop::new(spawn_indexer(
        &db_url,
        &rpc_url,
        &intents_addr,
        "pool",
        None,
    )?);

    let mut migrator = KillOnDrop::new(spawn_solver_safe4337_mock_custom(
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
        "solver-aa-claim-window-migrate",
        &[
            ("INDEXER_MAX_HEAD_LAG_BLOCKS", "1000000"),
            ("SOLVER_ENABLED_INTENT_TYPES", ""),
        ],
    )?);
    wait_for_solver_table(&db_url, "jobs", Duration::from_secs(30)).await?;
    migrator.kill_now();

    let to = "0x00000000000000000000000000000000000000aa";
    let _ = run_cast_create_trx_transfer_intent(&rpc_url, pk0, &intents_addr, to, "4321", 1)?;
    wait_for_pool_current_intents_count(&db_url, 1, Duration::from_secs(60)).await?;
    let intent = fetch_current_intents(&db_url)
        .await?
        .into_iter()
        .next()
        .context("missing intent row")?;
    let _ready_job_id = insert_ready_job_for_intent(&db_url, &intent).await?;
    let claimed_job_id = insert_claimed_job_with_invalid_specs(&db_url).await?;

    // Phase 1: with max_in_flight=1 and a high cap, claimed/unproved jobs should be leased first.
    let mut solver_priority = KillOnDrop::new(spawn_solver_safe4337_mock_custom(
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
        "solver-aa-claim-priority",
        &[
            ("INDEXER_MAX_HEAD_LAG_BLOCKS", "1000000"),
            ("SOLVER_MAX_IN_FLIGHT_JOBS", "1"),
            ("SOLVER_TICK_INTERVAL_SECS", "5"),
            ("SOLVER_SAFE4337_MAX_CLAIMED_UNPROVED_JOBS", "10"),
        ],
    )?);
    wait_for_job_attempts_by_id(&db_url, claimed_job_id, 1, Duration::from_secs(45)).await?;
    let ready_job = fetch_job_by_intent_id(&db_url, &intent.id).await?;
    if ready_job.state != "ready" || ready_job.attempts != 0 {
        anyhow::bail!(
            "expected ready job to remain untouched while claimed job is prioritized: state={} attempts={}",
            ready_job.state,
            ready_job.attempts
        );
    }
    solver_priority.kill_now();

    // Phase 2: with cap=1 and max_in_flight=2, ready jobs should be backpressured.
    let _solver_backpressure = KillOnDrop::new(spawn_solver_safe4337_mock_custom(
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
        "solver-aa-claim-backpressure",
        &[
            ("INDEXER_MAX_HEAD_LAG_BLOCKS", "1000000"),
            ("SOLVER_MAX_IN_FLIGHT_JOBS", "2"),
            ("SOLVER_TICK_INTERVAL_SECS", "1"),
            ("SOLVER_SAFE4337_MAX_CLAIMED_UNPROVED_JOBS", "1"),
        ],
    )?);

    let start = Instant::now();
    loop {
        let ready_job = fetch_job_by_intent_id(&db_url, &intent.id).await?;
        let has_backpressure = ready_job
            .last_error
            .as_deref()
            .unwrap_or("")
            .contains("claim_window_backpressure");
        if ready_job.attempts >= 1 && has_backpressure {
            if ready_job.state != "ready" || ready_job.claim_tx_hash.is_some() {
                anyhow::bail!(
                    "expected backpressured job to remain ready/unclaimed: state={} claim_tx_hash={:?}",
                    ready_job.state,
                    ready_job.claim_tx_hash
                );
            }
            break;
        }
        if start.elapsed() > Duration::from_secs(60) {
            anyhow::bail!("timed out waiting for claim_window_backpressure on ready job");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    Ok(())
}
