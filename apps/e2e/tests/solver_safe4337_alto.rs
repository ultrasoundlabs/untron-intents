use anyhow::{Context, Result};
use e2e::{
    alto::{AltoOptions, start_alto},
    anvil::spawn_anvil_with_block_time,
    binaries::{cargo_build_indexer_bins, cargo_build_solver_bin, run_migrations},
    cast::{
        run_cast_call, run_cast_create_delegate_resource_intent,
        run_cast_create_trx_transfer_intent, run_cast_create_usdt_transfer_intent,
        run_cast_entrypoint_deposit_to, run_cast_mint_mock_erc20, run_cast_transfer_eth,
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
    services::spawn_indexer,
    util::{find_free_port, require_bins},
};
use sqlx::Row;
use std::io::{BufRead, BufReader};
use std::process::Command;
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct LogCapture {
    name: &'static str,
    buf: Arc<Mutex<Vec<String>>>,
}

impl LogCapture {
    fn new(name: &'static str, child: &mut Child) -> Self {
        let buf: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        if let Some(stdout) = stdout {
            let buf = Arc::clone(&buf);
            std::thread::spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines().flatten() {
                    let mut b = buf.lock().unwrap();
                    b.push(line);
                    if b.len() > 5000 {
                        let drain_to = b.len().saturating_sub(2500);
                        b.drain(..drain_to);
                    }
                }
            });
        }
        if let Some(stderr) = stderr {
            let buf = Arc::clone(&buf);
            std::thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines().flatten() {
                    let mut b = buf.lock().unwrap();
                    b.push(line);
                    if b.len() > 5000 {
                        let drain_to = b.len().saturating_sub(2500);
                        b.drain(..drain_to);
                    }
                }
            });
        }

        Self { name, buf }
    }

    fn dump_last(&self, n: usize) {
        let b = self.buf.lock().unwrap();
        let start = b.len().saturating_sub(n);
        eprintln!("===== {} logs (last {} lines) =====", self.name, n);
        for line in &b[start..] {
            eprintln!("{line}");
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_solver_safe4337_mock_capture(
    db_url: &str,
    postgrest_url: &str,
    rpc_url: &str,
    pool_contract: &str,
    owner_private_key_hex: &str,
    safe_address: &str,
    entrypoint_address: &str,
    safe_4337_module_address: &str,
    bundler_url: &str,
    mock_reader: &str,
    instance_id: &'static str,
) -> Result<(Child, LogCapture)> {
    let root = e2e::util::repo_root();
    let mut cmd = Command::new(root.join("target/debug/solver"));
    cmd.current_dir(&root)
        .env("SOLVER_DB_URL", db_url)
        .env("INDEXER_API_BASE_URL", postgrest_url)
        .env("HUB_RPC_URL", rpc_url)
        .env("HUB_POOL_ADDRESS", pool_contract)
        .env("HUB_TX_MODE", "safe4337")
        .env("HUB_SIGNER_PRIVATE_KEY_HEX", owner_private_key_hex)
        .env("HUB_SAFE_ADDRESS", safe_address)
        .env("HUB_ENTRYPOINT_ADDRESS", entrypoint_address)
        .env("HUB_SAFE_4337_MODULE_ADDRESS", safe_4337_module_address)
        .env("HUB_BUNDLER_URLS", bundler_url)
        .env("TRON_MODE", "mock")
        .env("TRON_MOCK_READER_ADDRESS", mock_reader)
        .env("SOLVER_INSTANCE_ID", instance_id)
        .env(
            "SOLVER_ENABLED_INTENT_TYPES",
            "trx_transfer,delegate_resource,usdt_transfer",
        )
        .env("SOLVER_TICK_INTERVAL_SECS", "1")
        .env("RUST_LOG", "info")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().context("spawn solver (safe4337 + mock tron)")?;
    let logs = LogCapture::new(instance_id, &mut child);
    Ok((child, logs))
}

async fn wait_for_alto_supported_entrypoints(alto_url: &str, expected: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let start = Instant::now();
    loop {
        if start.elapsed() > Duration::from_secs(120) {
            anyhow::bail!("timed out waiting for alto to be ready");
        }

        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_supportedEntryPoints",
            "params": []
        });

        let resp = client.post(alto_url).json(&payload).send().await;
        if let Ok(resp) = resp {
            if let Ok(val) = resp.json::<serde_json::Value>().await {
                if let Some(arr) = val.get("result").and_then(|v| v.as_array()) {
                    let expected_lc = expected.to_ascii_lowercase();
                    if arr
                        .iter()
                        .filter_map(|v| v.as_str())
                        .any(|s| s.to_ascii_lowercase() == expected_lc)
                    {
                        return Ok(());
                    }
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(250)).await;
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
        if let Ok(resp) = resp {
            if let Ok(val) = resp.json::<serde_json::Value>().await {
                if val.get("error").is_some() {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    continue;
                }
                // Bundlers differ: some return `transactionHash` at the top level, others nest it
                // under `receipt.transactionHash`.
                let txh = val
                    .pointer("/result/transactionHash")
                    .and_then(|v| v.as_str())
                    .or_else(|| {
                        val.pointer("/result/receipt/transactionHash")
                            .and_then(|v| v.as_str())
                    });
                if let Some(txh) = txh {
                    if !txh.is_empty() && txh != "0x" {
                        return Ok(txh.to_string());
                    }
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

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
        if let Ok(resp) = resp {
            if let Ok(val) = resp.json::<serde_json::Value>().await {
                if let Some(r) = val.get("result") {
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
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn assert_safe_fallback_is_configured(rpc_url: &str, safe: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_call",
        "params": [
            { "to": safe, "data": "0xdeadbeef" },
            "latest"
        ]
    });
    let resp = client
        .post(rpc_url)
        .json(&payload)
        .send()
        .await
        .context("eth_call (deadbeef)")?;
    let val = resp
        .json::<serde_json::Value>()
        .await
        .context("decode eth_call")?;

    // If no handler is configured, Safe's fallback returns empty data successfully.
    if val.get("result").and_then(|v| v.as_str()) == Some("0x") {
        anyhow::bail!(
            "Safe fallback returned empty data; fallback handler likely not set: safe={safe}"
        );
    }
    // If a handler is configured, the call should revert (the exact revert data is handler-specific).
    if val.get("error").is_none() {
        anyhow::bail!("expected Safe fallback to revert; got response: {val}");
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_solver_safe4337_uses_alto_and_recovers_after_crash() -> Result<()> {
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

    // Start Alto bundler.
    let alto = start_alto(AltoOptions {
        network: None,
        container_name: Some(format!("untron-e2e-alto-{}", find_free_port()?)),
        rpc_url: alto_rpc_url,
        entrypoints_csv: entrypoint.clone(),
        executor_private_keys_csv: pk1.to_string(),
        utility_private_key_hex: pk2.to_string(),
        safe_mode: false,
        log_level: "debug".to_string(),
        deploy_simulations_contract: true,
        ..Default::default()
    })
    .await?;
    let alto_url = alto.base_url.clone();
    wait_for_alto_supported_entrypoints(&alto_url, &entrypoint).await?;

    // Deploy our protocol contracts.
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
            proxy_factory: proxy_factory.parse().context("parse proxy_factory")?,
            singleton: safe_singleton.parse().context("parse safe_singleton")?,
            module_setup: module_setup.parse().context("parse module_setup")?,
            // Use a non-zero salt to avoid accidental address collisions with other test deployments.
            salt_nonce: alloy::primitives::U256::from(1u64),
        },
    )
    .await
    .context("ensure safe deployed (e2e)")?;
    let safe_address = format!("{safe_addr:#x}");

    // Sanity check the Safe was initialized and points at the expected singleton code.
    let version = run_cast_call(&rpc_url, &safe_address, "VERSION()(string)", &[])
        .context("cast call Safe.VERSION")?;
    if !version.contains("1.") {
        anyhow::bail!("unexpected Safe.VERSION() value: {version}");
    }
    let enabled = run_cast_call(
        &rpc_url,
        &safe_address,
        "isModuleEnabled(address)(bool)",
        &[&safe_4337_module],
    )
    .context("cast call Safe.isModuleEnabled")?;
    if enabled.trim() != "true" {
        anyhow::bail!(
            "Safe4337 module is not enabled: safe={safe_address} module={safe_4337_module} enabled={enabled}"
        );
    }
    assert_safe_fallback_is_configured(&rpc_url, &safe_address).await?;

    // Fund solver deposit USDT + give the Safe an EntryPoint deposit for self-paid userops.
    run_cast_mint_mock_erc20(&rpc_url, pk0, &usdt, &safe_address, "5000000")?;
    run_cast_transfer_eth(&rpc_url, pk0, &safe_address, "1000000000000000000")?;
    run_cast_entrypoint_deposit_to(
        &rpc_url,
        pk0,
        &entrypoint,
        &safe_address,
        "1000000000000000000",
    )?;

    // Pre-approve the pool to pull the solver's USDT claim deposit. Without this, the solver will
    // first submit an ERC20 approve userop and block on its inclusion, which makes it hard to
    // deterministically crash after the *claim* userop is submitted.
    {
        let approve_data = e2e::cast::cast_abi_encode(
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
        .context("init aa sender (pre-approve)")?;

        let submission = match sender
            .send_call(usdt.parse().context("parse usdt")?, approve_bytes)
            .await
        {
            Ok(s) => s,
            Err(err) => {
                // Dump Alto logs to make bundler failures actionable.
                let out = Command::new("docker")
                    .args(["logs", alto.container.id()])
                    .output();
                if let Ok(out) = out {
                    eprintln!("alto logs:\n{}", String::from_utf8_lossy(&out.stdout));
                    eprintln!(
                        "alto logs (stderr):\n{}",
                        String::from_utf8_lossy(&out.stderr)
                    );
                }
                return Err(err).context("send approve userop");
            }
        };
        let tx_hash = match wait_for_userop_tx_hash(&alto_url, &submission.userop_hash).await {
            Ok(v) => v,
            Err(err) => {
                let out = Command::new("docker")
                    .args(["logs", alto.container.id()])
                    .output();
                if let Ok(out) = out {
                    eprintln!("alto logs:\n{}", String::from_utf8_lossy(&out.stdout));
                    eprintln!(
                        "alto logs (stderr):\n{}",
                        String::from_utf8_lossy(&out.stderr)
                    );
                }
                return Err(err).context("wait approve userop receipt");
            }
        };
        wait_for_tx_success(&rpc_url, &tx_hash).await?;
    }

    // Start indexer (pool-only).
    let _indexer = KillOnDrop::new(spawn_indexer(
        &db_url,
        &rpc_url,
        &intents_addr,
        "pool",
        None,
    )?);

    // Create intents.
    let to = "0x00000000000000000000000000000000000000aa";
    let _ = run_cast_create_trx_transfer_intent(&rpc_url, pk0, &intents_addr, to, "1234", 1)?;
    let _ = run_cast_create_usdt_transfer_intent(&rpc_url, pk0, &intents_addr, to, "555", 1)?;
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

    wait_for_pool_current_intents_count(&db_url, 3, Duration::from_secs(60)).await?;

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

    // Start solver (safe4337 + mock tron).
    let (mut solver1_child, solver1_logs) = spawn_solver_safe4337_mock_capture(
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
        "solver-aa-1",
    )?;
    tokio::time::sleep(Duration::from_secs(1)).await;
    if let Some(status) = solver1_child.try_wait().context("check solver1 status")? {
        solver1_logs.dump_last(200);
        anyhow::bail!("solver-aa-1 exited early: {status}");
    }
    let mut solver1 = KillOnDrop::new(solver1_child);

    // Wait until claim userOp has been submitted, then crash and ensure we recover.
    let start = Instant::now();
    let pool = sqlx::PgPool::connect(&db_url)
        .await
        .context("connect db (wait userop)")?;
    loop {
        if start.elapsed() > Duration::from_secs(90) {
            // Debug dump to make failures actionable when running in CI.
            if let Ok(rows) = sqlx::query(
                "select job_id, state, coalesce(leased_by,'') as leased_by, \
                        coalesce(lease_until::text,'') as lease_until, \
                        attempts, coalesce(last_error, '') as last_error \
                 from solver.jobs \
                 order by job_id asc",
            )
            .fetch_all(&pool)
            .await
            {
                eprintln!("solver.jobs:");
                for r in rows {
                    let job_id: i64 = r.try_get("job_id").unwrap_or_default();
                    let state: String = r.try_get("state").unwrap_or_default();
                    let leased_by: String = r.try_get("leased_by").unwrap_or_default();
                    let lease_until: String = r.try_get("lease_until").unwrap_or_default();
                    let attempts: i32 = r.try_get("attempts").unwrap_or_default();
                    let last_error: String = r.try_get("last_error").unwrap_or_default();
                    eprintln!(
                        "  job_id={job_id} state={state} leased_by={leased_by} lease_until={lease_until} attempts={attempts} last_error={last_error}"
                    );
                }
            }
            if let Ok(rows) = sqlx::query(
                "select kind::text as kind, state::text as state, coalesce(userop_hash, '') as userop_hash, attempts, coalesce(last_error, '') as last_error \
                 from solver.hub_userops \
                 order by userop_id asc",
            )
            .fetch_all(&pool)
            .await
            {
                eprintln!("solver.hub_userops:");
                for r in rows {
                    let kind: String = r.try_get("kind").unwrap_or_default();
                    let state: String = r.try_get("state").unwrap_or_default();
                    let userop_hash: String = r.try_get("userop_hash").unwrap_or_default();
                    let attempts: i32 = r.try_get("attempts").unwrap_or_default();
                    let last_error: String = r.try_get("last_error").unwrap_or_default();
                    eprintln!(
                        "  kind={kind} state={state} attempts={attempts} userop_hash={userop_hash} last_error={last_error}"
                    );
                }
            }

            solver1_logs.dump_last(250);
            anyhow::bail!("timed out waiting for claim userop submission");
        }

        let userop_hash_res: Result<Option<String>> = sqlx::query_scalar(
            "select userop_hash \
             from solver.hub_userops \
             where kind = 'claim' and state = 'submitted' and userop_hash is not null \
             order by userop_id asc \
             limit 1",
        )
        .fetch_optional(&pool)
        .await
        .map_err(|e| e.into());

        let userop_hash = match userop_hash_res {
            Ok(v) => v,
            Err(err) => {
                let msg = err.to_string();
                if msg.contains("solver.hub_userops") && msg.contains("does not exist") {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    continue;
                }
                return Err(err).context("query solver.hub_userops claim submitted");
            }
        };
        if userop_hash.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // Crash solver1 mid-flight (after submission, before inclusion/prove).
    solver1.kill_now();
    tokio::time::sleep(Duration::from_secs(1)).await;

    let (mut solver2_child, solver2_logs) = spawn_solver_safe4337_mock_capture(
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
        "solver-aa-2",
    )?;
    tokio::time::sleep(Duration::from_secs(2)).await;
    if let Some(status) = solver2_child.try_wait().context("check solver2 status")? {
        solver2_logs.dump_last(200);
        anyhow::bail!("solver-aa-2 exited early: {status}");
    }
    let _solver2 = KillOnDrop::new(solver2_child);

    let _rows = match wait_for_intents_solved_and_settled(&db_url, 3, Duration::from_secs(240))
        .await
    {
        Ok(rows) => rows,
        Err(err) => {
            let pool = sqlx::PgPool::connect(&db_url).await?;

            if let Ok(rows) = sqlx::query(
                "select job_id, state, coalesce(leased_by,'') as leased_by, \
                            coalesce(lease_until::text,'') as lease_until, \
                            attempts, coalesce(last_error, '') as last_error, \
                            coalesce(tron_txid::text,'') as tron_txid \
                     from solver.jobs \
                     order by job_id asc",
            )
            .fetch_all(&pool)
            .await
            {
                eprintln!("solver.jobs:");
                for r in rows {
                    let job_id: i64 = r.try_get("job_id").unwrap_or_default();
                    let state: String = r.try_get("state").unwrap_or_default();
                    let leased_by: String = r.try_get("leased_by").unwrap_or_default();
                    let lease_until: String = r.try_get("lease_until").unwrap_or_default();
                    let attempts: i32 = r.try_get("attempts").unwrap_or_default();
                    let last_error: String = r.try_get("last_error").unwrap_or_default();
                    let tron_txid: String = r.try_get("tron_txid").unwrap_or_default();
                    eprintln!(
                        "  job_id={job_id} state={state} leased_by={leased_by} lease_until={lease_until} attempts={attempts} tron_txid={tron_txid} last_error={last_error}"
                    );
                }
            }

            if let Ok(rows) = sqlx::query(
                "select job_id, kind::text as kind, state::text as state, \
                            coalesce(userop_hash, '') as userop_hash, \
                            attempts, coalesce(last_error, '') as last_error \
                     from solver.hub_userops \
                     order by userop_id asc",
            )
            .fetch_all(&pool)
            .await
            {
                eprintln!("solver.hub_userops:");
                for r in rows {
                    let job_id: i64 = r.try_get("job_id").unwrap_or_default();
                    let kind: String = r.try_get("kind").unwrap_or_default();
                    let state: String = r.try_get("state").unwrap_or_default();
                    let userop_hash: String = r.try_get("userop_hash").unwrap_or_default();
                    let attempts: i32 = r.try_get("attempts").unwrap_or_default();
                    let last_error: String = r.try_get("last_error").unwrap_or_default();
                    eprintln!(
                        "  job_id={job_id} kind={kind} state={state} attempts={attempts} userop_hash={userop_hash} last_error={last_error}"
                    );
                }
            }

            solver1_logs.dump_last(250);
            solver2_logs.dump_last(250);
            return Err(err);
        }
    };

    // Assert AA persistence surface: all userops reached "included" and we stored the raw receipt.
    // (This is what the solver needs to be restart-safe and debuggable in production.)
    let rows = sqlx::query(
        "select kind::text as kind, state::text as state, tx_hash, success, receipt \
         from solver.hub_userops \
         order by userop_id asc",
    )
    .fetch_all(&pool)
    .await
    .context("select solver.hub_userops final")?;

    if rows.len() < 6 {
        anyhow::bail!(
            "expected at least 6 hub_userops (3 jobs x claim+prove), got {}",
            rows.len()
        );
    }

    for r in rows {
        let kind: String = r.try_get("kind").unwrap_or_default();
        let state: String = r.try_get("state").unwrap_or_default();
        let tx_hash: Option<Vec<u8>> = r.try_get("tx_hash").ok();
        let success: Option<bool> = r.try_get("success").ok();
        let receipt: Option<serde_json::Value> = r.try_get("receipt").ok();

        if state != "included" {
            anyhow::bail!("expected hub_userops state=included, got kind={kind} state={state}");
        }
        if tx_hash.as_ref().map(|v| v.len()).unwrap_or(0) != 32 {
            anyhow::bail!("expected hub_userops tx_hash(32) for kind={kind}, got {tx_hash:?}");
        }
        if success != Some(true) {
            anyhow::bail!("expected hub_userops success=true for kind={kind}, got {success:?}");
        }
        if receipt.is_none() {
            anyhow::bail!("expected hub_userops receipt json for kind={kind}, got None");
        }
    }
    Ok(())
}
