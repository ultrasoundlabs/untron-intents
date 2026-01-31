use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::Value;
use sqlx::{Connection, PgConnection, Row};
use std::{
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};

#[derive(Debug, Clone)]
struct CurrentIntentRow {
    creator: String,
    intent_type: i16,
    escrow_token: String,
    escrow_amount: String,
    refund_beneficiary: String,
    deadline: i64,
    intent_specs: String,
    solver: Option<String>,
    solver_claimed_at: Option<i64>,
    tron_tx_id: Option<String>,
    tron_block_number: Option<i64>,
    solved: bool,
    funded: bool,
    settled: bool,
    closed: bool,
}

struct KillOnDrop(Option<Child>);

impl KillOnDrop {
    fn new(child: Child) -> Self {
        Self(Some(child))
    }

    fn kill_now(&mut self) {
        if let Some(mut c) = self.0.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        self.kill_now();
    }
}

fn find_free_port() -> Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).context("bind ephemeral port")?;
    let port = listener.local_addr().context("local_addr")?.port();
    Ok(port)
}

fn repo_root() -> PathBuf {
    // apps/e2e -> apps -> repo root
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(Path::to_path_buf)
        .expect("CARGO_MANIFEST_DIR has apps/e2e shape")
}

fn command_exists(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

async fn wait_for_postgres(db_url: &str, timeout: Duration) -> Result<()> {
    let start = Instant::now();
    loop {
        match PgConnection::connect(db_url).await {
            Ok(mut c) => {
                // Simple liveness query.
                sqlx::query("select 1").execute(&mut c).await?;
                return Ok(());
            }
            Err(e) => {
                if start.elapsed() > timeout {
                    return Err(e).context("postgres not ready before timeout");
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }
}

async fn configure_postgrest_roles(db_url: &str, pgrst_auth_password: &str) -> Result<()> {
    let mut conn = PgConnection::connect(db_url).await?;
    sqlx::query(
        "do $$ \
         begin \
           if not exists (select 1 from pg_roles where rolname = 'pgrst_authenticator') then \
             create role pgrst_authenticator login inherit; \
           end if; \
           if not exists (select 1 from pg_roles where rolname = 'pgrst_anon') then \
             create role pgrst_anon nologin; \
           end if; \
           grant pgrst_anon to pgrst_authenticator; \
           grant connect on database untron to pgrst_authenticator; \
           grant usage on schema api to pgrst_authenticator; \
           grant usage on schema api to pgrst_anon; \
           grant select on all tables in schema api to pgrst_anon; \
           alter default privileges in schema api grant select on tables to pgrst_anon; \
         end $$;",
    )
    .execute(&mut conn)
    .await
    .context("configure postgrest roles (create/grants)")?;

    // PostgreSQL does not accept bind params for ALTER ROLE PASSWORD, so we interpolate a safely-escaped literal.
    let pw = pgrst_auth_password.replace('\'', "''");
    sqlx::query(&format!("alter role pgrst_authenticator password '{pw}'"))
        .execute(&mut conn)
        .await
        .context("configure postgrest roles (password)")?;
    Ok(())
}

fn spawn_anvil(port: u16) -> Result<Child> {
    // Default anvil mnemonic account #0 private key:
    // 0xac0974...ff80 (well-known dev key). We also force a mnemonic to be explicit/stable.
    let mut cmd = Command::new("anvil");
    cmd.arg("--port")
        .arg(port.to_string())
        .arg("--chain-id")
        .arg("31337")
        .arg("--mnemonic")
        .arg("test test test test test test test test test test test junk")
        .arg("--silent")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());

    cmd.spawn().context("spawn anvil")
}

fn run_forge_build() -> Result<()> {
    let root = repo_root();
    let status = Command::new("forge")
        .args(["build", "--root", "packages/contracts"])
        .current_dir(&root)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("forge build")?;
    if !status.success() {
        anyhow::bail!("forge build failed");
    }
    Ok(())
}

fn run_forge_create_untron_intents(
    rpc_url: &str,
    private_key: &str,
    owner: &str,
) -> Result<String> {
    let root = repo_root();
    // We intentionally deploy with v3=address(0) and usdt=address(0) for the indexer e2e test:
    // we only need events emitted by createIntent, which doesn't call V3.
    let out = Command::new("forge")
        .args([
            "create",
            "--root",
            "packages/contracts",
            "--rpc-url",
            rpc_url,
            "--private-key",
            private_key,
            "--broadcast",
            "src/UntronIntents.sol:UntronIntents",
            "--constructor-args",
            owner,
            "0x0000000000000000000000000000000000000000",
            "0x0000000000000000000000000000000000000000",
        ])
        .current_dir(&root)
        .stdin(Stdio::null())
        .output()
        .context("forge create UntronIntents")?;

    if !out.status.success() {
        return Err(anyhow::anyhow!(
            "forge create failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }

    // Parse "Deployed to: 0x..." from forge output (foundry sometimes writes logs to stderr).
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if let Some(pos) = combined.find("Deployed to:") {
        let tail = &combined[pos + "Deployed to:".len()..];
        if let Some(addr_pos) = tail.find("0x") {
            let addr_tail = &tail[addr_pos..];
            let addr = addr_tail
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            if addr.len() == 42 {
                return Ok(addr);
            }
        }
    }

    Err(anyhow::anyhow!(
        "failed to parse deployed address from forge output: {combined}"
    ))
}

fn run_forge_create_intents_forwarder(
    rpc_url: &str,
    private_key: &str,
    usdt: &str,
    usdc: &str,
    owner: &str,
) -> Result<String> {
    let root = repo_root();
    let out = Command::new("forge")
        .args([
            "create",
            "--root",
            "packages/contracts",
            "--rpc-url",
            rpc_url,
            "--private-key",
            private_key,
            "--broadcast",
            "src/IntentsForwarder.sol:IntentsForwarder",
            "--constructor-args",
            usdt,
            usdc,
            owner,
        ])
        .current_dir(&root)
        .stdin(Stdio::null())
        .output()
        .context("forge create IntentsForwarder")?;

    if !out.status.success() {
        return Err(anyhow::anyhow!(
            "forge create failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }

    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if let Some(pos) = combined.find("Deployed to:") {
        let tail = &combined[pos + "Deployed to:".len()..];
        if let Some(addr_pos) = tail.find("0x") {
            let addr_tail = &tail[addr_pos..];
            let addr = addr_tail
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            if addr.len() == 42 {
                return Ok(addr);
            }
        }
    }

    Err(anyhow::anyhow!(
        "failed to parse deployed address from forge output: {combined}"
    ))
}

fn run_cast_create_intent(
    rpc_url: &str,
    private_key: &str,
    intents: &str,
    amount_wei: u64,
) -> Result<u64> {
    // createIntent((intentType, intentSpecs, refundBeneficiary, token, amount), deadline)
    //
    // Use token=ETH (0x0) and amount=1 wei to avoid ERC-20 setup.
    // Must send msg.value == amount.
    let deadline_u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600;
    let deadline = deadline_u64.to_string();
    let amount_str = amount_wei.to_string();

    let status = Command::new("cast")
        .args([
            "send",
            "--rpc-url",
            rpc_url,
            "--private-key",
            private_key,
            "--value",
            &amount_str,
            intents,
            "createIntent((uint8,bytes,address,address,uint256),uint256)",
            &format!(
                "(0,0x,0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266,0x0000000000000000000000000000000000000000,{amount_str})"
            ),
            &deadline,
        ])
        .current_dir(repo_root())
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("cast send createIntent")?;

    if !status.success() {
        anyhow::bail!("cast send failed");
    }
    Ok(deadline_u64)
}

fn spawn_indexer(
    db_url: &str,
    rpc_url: &str,
    pool_contract: &str,
    stream: &str,
    forwarders_chains_json: Option<&str>,
) -> Result<Child> {
    let root = repo_root();
    let mut cmd = Command::new(root.join("target/debug/indexer"));
    cmd.current_dir(&root)
        .env("DATABASE_URL", db_url)
        .env("DB_MAX_CONNECTIONS", "20")
        .env("POOL_RPC_URLS", rpc_url)
        .env("POOL_CHAIN_ID", "31337")
        .env("POOL_CONTRACT_ADDRESS", pool_contract)
        .env("POOL_DEPLOYMENT_BLOCK", "0")
        .env("INDEXER_STREAM", stream)
        .env("RUST_LOG", "info")
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    if let Some(json) = forwarders_chains_json {
        cmd.env("FORWARDERS_CHAINS", json);
    }

    cmd.spawn().context("spawn indexer (cargo run)")
}

async fn fetch_current_intent(db_url: &str) -> Result<Option<CurrentIntentRow>> {
    let mut conn = PgConnection::connect(db_url).await?;
    let row = sqlx::query(
        "select \
           lower(creator) as creator, \
           intent_type, \
           lower(escrow_token) as escrow_token, \
           escrow_amount::text as escrow_amount, \
           lower(refund_beneficiary) as refund_beneficiary, \
           deadline, \
           intent_specs, \
           lower(solver) as solver, \
           solver_claimed_at, \
           tron_tx_id, \
           tron_block_number, \
           solved, funded, settled, closed \
         from pool.intent_versions \
         where valid_to_seq is null \
         order by valid_from_seq desc \
         limit 1",
    )
    .fetch_optional(&mut conn)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    Ok(Some(CurrentIntentRow {
        creator: row.get::<String, _>("creator"),
        intent_type: row.get::<i16, _>("intent_type"),
        escrow_token: row.get::<String, _>("escrow_token"),
        escrow_amount: row.get::<String, _>("escrow_amount"),
        refund_beneficiary: row.get::<String, _>("refund_beneficiary"),
        deadline: row.get::<i64, _>("deadline"),
        intent_specs: row.get::<String, _>("intent_specs"),
        solver: row.get::<Option<String>, _>("solver"),
        solver_claimed_at: row.get::<Option<i64>, _>("solver_claimed_at"),
        tron_tx_id: row.get::<Option<String>, _>("tron_tx_id"),
        tron_block_number: row.get::<Option<i64>, _>("tron_block_number"),
        solved: row.get::<bool, _>("solved"),
        funded: row.get::<bool, _>("funded"),
        settled: row.get::<bool, _>("settled"),
        closed: row.get::<bool, _>("closed"),
    }))
}

async fn fetch_pool_current_intents_count(db_url: &str) -> Result<i64> {
    let mut conn = PgConnection::connect(db_url).await?;
    Ok(
        sqlx::query("select count(*) from pool.intent_versions where valid_to_seq is null")
            .fetch_one(&mut conn)
            .await?
            .get::<i64, _>(0),
    )
}

async fn wait_for_pool_current_intents_count(
    db_url: &str,
    expected: i64,
    timeout: Duration,
) -> Result<()> {
    let start = Instant::now();
    loop {
        let cur = fetch_pool_current_intents_count(db_url).await?;
        if cur == expected {
            return Ok(());
        }
        if start.elapsed() > timeout {
            anyhow::bail!("timed out waiting for current intent count={expected}, got={cur}");
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn wait_for_current_intent_match(
    db_url: &str,
    expected: &CurrentIntentRow,
    timeout: Duration,
) -> Result<()> {
    let start = Instant::now();
    loop {
        if let Some(cur) = fetch_current_intent(db_url).await? {
            if cur.creator == expected.creator
                && cur.intent_type == expected.intent_type
                && cur.escrow_token == expected.escrow_token
                && cur.escrow_amount == expected.escrow_amount
                && cur.refund_beneficiary == expected.refund_beneficiary
                && cur.deadline == expected.deadline
                && cur.intent_specs == expected.intent_specs
                && cur.solver.is_none()
                && cur.solver_claimed_at.is_none()
                && cur.tron_tx_id.is_none()
                && cur.tron_block_number.is_none()
                && cur.solved == expected.solved
                && cur.funded == expected.funded
                && cur.settled == expected.settled
                && cur.closed == expected.closed
            {
                return Ok(());
            }
        }

        if start.elapsed() > timeout {
            anyhow::bail!(
                "timed out waiting for expected current intent row; expected={expected:?}, got={:?}",
                fetch_current_intent(db_url).await?
            );
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

fn run_cast_rpc(rpc_url: &str, method: &str, params: &[&str]) -> Result<String> {
    let mut args = vec!["rpc", "--rpc-url", rpc_url, method];
    args.extend_from_slice(params);

    let out = Command::new("cast")
        .args(args)
        .current_dir(repo_root())
        .stdin(Stdio::null())
        .output()
        .context("cast rpc")?;

    if !out.status.success() {
        return Err(anyhow::anyhow!(
            "cast rpc failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }

    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

async fn http_get_json(url: &str) -> Result<Value> {
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .context("build reqwest client")?;
    let resp = client.get(url).send().await.context("http get")?;
    let status = resp.status();
    let body = resp.text().await.context("read body")?;
    if !status.is_success() {
        anyhow::bail!("http {status} for {url}: {body}");
    }
    serde_json::from_str(&body).context("parse json")
}

async fn wait_for_http_ok(url: &str, timeout: Duration) -> Result<()> {
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .context("build reqwest client")?;
    let start = Instant::now();
    loop {
        match client.get(url).send().await {
            Ok(resp) if resp.status().is_success() => return Ok(()),
            _ => {
                if start.elapsed() > timeout {
                    anyhow::bail!("timed out waiting for http ok: {url}");
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_indexer_ingests_pool_intent_created() -> Result<()> {
    // Tooling prerequisites.
    for bin in ["docker", "anvil", "forge", "cast"] {
        if !command_exists(bin) {
            eprintln!("skipping e2e test: missing `{bin}` in PATH");
            return Ok(());
        }
    }

    let pg = GenericImage::new("postgres", "18.1")
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stdout(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_DB", "untron")
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .start()
        .await
        .context("start postgres container")?;

    let pg_port = pg.get_host_port_ipv4(5432).await?;
    let db_url = format!("postgres://postgres:postgres@127.0.0.1:{pg_port}/untron");
    wait_for_postgres(&db_url, Duration::from_secs(30)).await?;

    // Build the indexer binaries we will execute.
    {
        let root = repo_root();
        let status = Command::new("cargo")
            .args([
                "build", "-p", "indexer", "--bin", "indexer", "--bin", "migrate", "--quiet",
            ])
            .current_dir(&root)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .context("cargo build indexer binaries")?;
        if !status.success() {
            anyhow::bail!("failed to build indexer binaries");
        }
    }

    // Apply migrations via indexer's migrate binary to keep behavior identical to prod.
    {
        let root = repo_root();
        let status = Command::new(root.join("target/debug/migrate"))
            .arg("--no-notify-pgrst")
            .current_dir(&root)
            .env("DATABASE_URL", &db_url)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .context("run migrations")?;
        if !status.success() {
            anyhow::bail!("migrations failed");
        }
    }

    // Start anvil.
    let anvil_port = find_free_port()?;
    let rpc_url = format!("http://127.0.0.1:{anvil_port}");
    let _anvil = KillOnDrop::new(spawn_anvil(anvil_port)?);
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Deploy contracts.
    run_forge_build()?;
    let pk0 = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    let owner0 = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
    let intents_addr = run_forge_create_untron_intents(&rpc_url, pk0, owner0)?;

    // Start indexer (pool-only).
    let _indexer = KillOnDrop::new(spawn_indexer(
        &db_url,
        &rpc_url,
        &intents_addr,
        "pool",
        None,
    )?);

    // Generate one pool event.
    let deadline = run_cast_create_intent(&rpc_url, pk0, &intents_addr, 1)?;

    // Wait for DB row and validate decoded values.
    let expected = CurrentIntentRow {
        creator: owner0.to_ascii_lowercase(),
        intent_type: 0,
        escrow_token: "0x0000000000000000000000000000000000000000".to_string(),
        escrow_amount: "1".to_string(),
        refund_beneficiary: owner0.to_ascii_lowercase(),
        deadline: i64::try_from(deadline).context("deadline out of range")?,
        intent_specs: "0x".to_string(),
        solver: None,
        solver_claimed_at: None,
        tron_tx_id: None,
        tron_block_number: None,
        solved: false,
        funded: true,
        settled: false,
        closed: false,
    };
    wait_for_current_intent_match(&db_url, &expected, Duration::from_secs(30)).await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_indexer_rolls_back_pool_on_anvil_revert() -> Result<()> {
    // Tooling prerequisites.
    for bin in ["docker", "anvil", "forge", "cast"] {
        if !command_exists(bin) {
            eprintln!("skipping e2e test: missing `{bin}` in PATH");
            return Ok(());
        }
    }

    let pg = GenericImage::new("postgres", "18.1")
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stdout(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_DB", "untron")
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .start()
        .await
        .context("start postgres container")?;

    let pg_port = pg.get_host_port_ipv4(5432).await?;
    let db_url = format!("postgres://postgres:postgres@127.0.0.1:{pg_port}/untron");
    wait_for_postgres(&db_url, Duration::from_secs(30)).await?;

    // Build indexer binaries.
    {
        let root = repo_root();
        let status = Command::new("cargo")
            .args([
                "build", "-p", "indexer", "--bin", "indexer", "--bin", "migrate", "--quiet",
            ])
            .current_dir(&root)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .context("cargo build indexer binaries")?;
        if !status.success() {
            anyhow::bail!("failed to build indexer binaries");
        }
    }

    // Migrations.
    {
        let root = repo_root();
        let status = Command::new(root.join("target/debug/migrate"))
            .arg("--no-notify-pgrst")
            .current_dir(&root)
            .env("DATABASE_URL", &db_url)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .context("run migrations")?;
        if !status.success() {
            anyhow::bail!("migrations failed");
        }
    }

    // Start anvil.
    let anvil_port = find_free_port()?;
    let rpc_url = format!("http://127.0.0.1:{anvil_port}");
    let _anvil = KillOnDrop::new(spawn_anvil(anvil_port)?);
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Deploy contracts.
    run_forge_build()?;
    let pk0 = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    let owner0 = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
    let intents_addr = run_forge_create_untron_intents(&rpc_url, pk0, owner0)?;

    // Snapshot the chain *after* deployment (so revert preserves the contract).
    let snapshot_id = run_cast_rpc(&rpc_url, "evm_snapshot", &[])?;

    // Start indexer.
    let _indexer = KillOnDrop::new(spawn_indexer(
        &db_url,
        &rpc_url,
        &intents_addr,
        "pool",
        None,
    )?);

    // Intent #1 (amount=1) and wait for it to be indexed.
    let deadline1 = run_cast_create_intent(&rpc_url, pk0, &intents_addr, 1)?;
    let expected1 = CurrentIntentRow {
        creator: owner0.to_ascii_lowercase(),
        intent_type: 0,
        escrow_token: "0x0000000000000000000000000000000000000000".to_string(),
        escrow_amount: "1".to_string(),
        refund_beneficiary: owner0.to_ascii_lowercase(),
        deadline: i64::try_from(deadline1).context("deadline out of range")?,
        intent_specs: "0x".to_string(),
        solver: None,
        solver_claimed_at: None,
        tron_tx_id: None,
        tron_block_number: None,
        solved: false,
        funded: true,
        settled: false,
        closed: false,
    };
    wait_for_current_intent_match(&db_url, &expected1, Duration::from_secs(30)).await?;

    // Revert the chain back to the snapshot, effectively removing intent #1 from canonical history.
    // Then emit a different intent at the same height so the indexer should detect a reorg and rollback.
    let _ = run_cast_rpc(&rpc_url, "evm_revert", &[&snapshot_id])?;

    let deadline2 = run_cast_create_intent(&rpc_url, pk0, &intents_addr, 2)?;
    let expected2 = CurrentIntentRow {
        creator: owner0.to_ascii_lowercase(),
        intent_type: 0,
        escrow_token: "0x0000000000000000000000000000000000000000".to_string(),
        escrow_amount: "2".to_string(),
        refund_beneficiary: owner0.to_ascii_lowercase(),
        deadline: i64::try_from(deadline2).context("deadline out of range")?,
        intent_specs: "0x".to_string(),
        solver: None,
        solver_claimed_at: None,
        tron_tx_id: None,
        tron_block_number: None,
        solved: false,
        funded: true,
        settled: false,
        closed: false,
    };
    wait_for_current_intent_match(&db_url, &expected2, Duration::from_secs(45)).await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_postgrest_pool_intents_smoke() -> Result<()> {
    // Tooling prerequisites.
    for bin in ["docker", "anvil", "forge", "cast"] {
        if !command_exists(bin) {
            eprintln!("skipping e2e test: missing `{bin}` in PATH");
            return Ok(());
        }
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

    // Build indexer binaries.
    {
        let root = repo_root();
        let status = Command::new("cargo")
            .args([
                "build", "-p", "indexer", "--bin", "indexer", "--bin", "migrate", "--quiet",
            ])
            .current_dir(&root)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .context("cargo build indexer binaries")?;
        if !status.success() {
            anyhow::bail!("failed to build indexer binaries");
        }
    }

    // Migrations.
    {
        let root = repo_root();
        let status = Command::new(root.join("target/debug/migrate"))
            .arg("--no-notify-pgrst")
            .current_dir(&root)
            .env("DATABASE_URL", &db_url)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .context("run migrations")?;
        if !status.success() {
            anyhow::bail!("migrations failed");
        }
    }

    // Configure PostgREST roles so the container can connect as pgrst_authenticator and expose api schema.
    let pgrst_pw = "pgrst_auth_pw";
    configure_postgrest_roles(&db_url, pgrst_pw).await?;

    // Start anvil + deploy pool + run indexer + emit one intent.
    let anvil_port = find_free_port()?;
    let rpc_url = format!("http://127.0.0.1:{anvil_port}");
    let _anvil = KillOnDrop::new(spawn_anvil(anvil_port)?);
    tokio::time::sleep(Duration::from_millis(500)).await;

    run_forge_build()?;
    let pk0 = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    let owner0 = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
    let intents_addr = run_forge_create_untron_intents(&rpc_url, pk0, owner0)?;

    let mut indexer = KillOnDrop::new(spawn_indexer(
        &db_url,
        &rpc_url,
        &intents_addr,
        "pool",
        None,
    )?);
    let _deadline = run_cast_create_intent(&rpc_url, pk0, &intents_addr, 1)?;
    wait_for_pool_current_intents_count(&db_url, 1, Duration::from_secs(30)).await?;

    // Indexer no longer needed for the PostgREST smoke check; stop it to reduce DB load during container startup.
    indexer.kill_now();

    // Start PostgREST container pointed at the host-mapped postgres port.
    let pgrst = GenericImage::new("postgrest/postgrest", "v14.2")
        .with_exposed_port(3000.tcp())
        // Don't rely on logs for readiness; we'll poll the HTTP endpoint.
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
    let health_url = format!("http://127.0.0.1:{pgrst_port}/health");
    wait_for_http_ok(&health_url, Duration::from_secs(30)).await?;

    let url =
        format!("http://127.0.0.1:{pgrst_port}/pool_intents?order=valid_from_seq.desc&limit=1");
    let json = http_get_json(&url).await?;
    let rows = json
        .as_array()
        .with_context(|| format!("expected array response, got: {json}"))?;
    if rows.is_empty() {
        anyhow::bail!("expected at least 1 row from PostgREST, got empty array");
    }
    let row = &rows[0];
    let escrow_amount_ok = match row.get("escrow_amount") {
        Some(Value::Number(n)) => n.as_u64() == Some(1),
        Some(Value::String(s)) => s == "1",
        other => {
            anyhow::bail!("unexpected escrow_amount in PostgREST response: {other:?} (row={row})");
        }
    };
    if !escrow_amount_ok {
        anyhow::bail!("unexpected escrow_amount in PostgREST response: row={row}");
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_pool_multi_intent_ordering() -> Result<()> {
    // Tooling prerequisites.
    for bin in ["docker", "anvil", "forge", "cast"] {
        if !command_exists(bin) {
            eprintln!("skipping e2e test: missing `{bin}` in PATH");
            return Ok(());
        }
    }

    let pg = GenericImage::new("postgres", "18.1")
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stdout(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_DB", "untron")
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .start()
        .await
        .context("start postgres container")?;

    let pg_port = pg.get_host_port_ipv4(5432).await?;
    let db_url = format!("postgres://postgres:postgres@127.0.0.1:{pg_port}/untron");
    wait_for_postgres(&db_url, Duration::from_secs(30)).await?;

    // Build indexer binaries.
    {
        let root = repo_root();
        let status = Command::new("cargo")
            .args([
                "build", "-p", "indexer", "--bin", "indexer", "--bin", "migrate", "--quiet",
            ])
            .current_dir(&root)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .context("cargo build indexer binaries")?;
        if !status.success() {
            anyhow::bail!("failed to build indexer binaries");
        }
    }

    // Migrations.
    {
        let root = repo_root();
        let status = Command::new(root.join("target/debug/migrate"))
            .arg("--no-notify-pgrst")
            .current_dir(&root)
            .env("DATABASE_URL", &db_url)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .context("run migrations")?;
        if !status.success() {
            anyhow::bail!("migrations failed");
        }
    }

    // Start anvil.
    let anvil_port = find_free_port()?;
    let rpc_url = format!("http://127.0.0.1:{anvil_port}");
    let _anvil = KillOnDrop::new(spawn_anvil(anvil_port)?);
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Deploy contracts.
    run_forge_build()?;
    let pk0 = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    let owner0 = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
    let intents_addr = run_forge_create_untron_intents(&rpc_url, pk0, owner0)?;

    // Start indexer (pool-only).
    let _indexer = KillOnDrop::new(spawn_indexer(
        &db_url,
        &rpc_url,
        &intents_addr,
        "pool",
        None,
    )?);

    // Emit three distinct intents.
    let _ = run_cast_create_intent(&rpc_url, pk0, &intents_addr, 1)?;
    let _ = run_cast_create_intent(&rpc_url, pk0, &intents_addr, 2)?;
    let _ = run_cast_create_intent(&rpc_url, pk0, &intents_addr, 3)?;

    wait_for_pool_current_intents_count(&db_url, 3, Duration::from_secs(30)).await?;

    // Validate ordering + uniqueness of current rows.
    let mut conn = PgConnection::connect(&db_url).await?;
    let rows = sqlx::query(
        "select id, valid_from_seq, escrow_amount::text as escrow_amount \
         from pool.intent_versions \
         where valid_to_seq is null \
         order by valid_from_seq asc",
    )
    .fetch_all(&mut conn)
    .await?;

    if rows.len() != 3 {
        anyhow::bail!("expected 3 current intents, got {}", rows.len());
    }

    let mut ids = std::collections::HashSet::new();
    let mut prev_seq: Option<i64> = None;
    let mut amounts = Vec::new();
    for r in rows {
        let id: String = r.get("id");
        let seq: i64 = r.get("valid_from_seq");
        let amt: String = r.get("escrow_amount");
        if !ids.insert(id) {
            anyhow::bail!("expected unique intent ids, got duplicate");
        }
        if let Some(p) = prev_seq {
            if seq <= p {
                anyhow::bail!("expected increasing valid_from_seq, got {p} then {seq}");
            }
        }
        prev_seq = Some(seq);
        amounts.push(amt);
    }
    amounts.sort();
    if amounts != ["1", "2", "3"] {
        anyhow::bail!("unexpected escrow_amount set for multi-intent test: {amounts:?}");
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_forwarder_stream_ingests_bridgers_set() -> Result<()> {
    // Tooling prerequisites.
    for bin in ["docker", "anvil", "forge", "cast"] {
        if !command_exists(bin) {
            eprintln!("skipping e2e test: missing `{bin}` in PATH");
            return Ok(());
        }
    }

    let pg = GenericImage::new("postgres", "18.1")
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stdout(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_DB", "untron")
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .start()
        .await
        .context("start postgres container")?;

    let pg_port = pg.get_host_port_ipv4(5432).await?;
    let db_url = format!("postgres://postgres:postgres@127.0.0.1:{pg_port}/untron");
    wait_for_postgres(&db_url, Duration::from_secs(30)).await?;

    // Build indexer binaries.
    {
        let root = repo_root();
        let status = Command::new("cargo")
            .args([
                "build", "-p", "indexer", "--bin", "indexer", "--bin", "migrate", "--quiet",
            ])
            .current_dir(&root)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .context("cargo build indexer binaries")?;
        if !status.success() {
            anyhow::bail!("failed to build indexer binaries");
        }
    }

    // Migrations.
    {
        let root = repo_root();
        let status = Command::new(root.join("target/debug/migrate"))
            .arg("--no-notify-pgrst")
            .current_dir(&root)
            .env("DATABASE_URL", &db_url)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .context("run migrations")?;
        if !status.success() {
            anyhow::bail!("migrations failed");
        }
    }

    // Start anvil.
    let anvil_port = find_free_port()?;
    let rpc_url = format!("http://127.0.0.1:{anvil_port}");
    let _anvil = KillOnDrop::new(spawn_anvil(anvil_port)?);
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Deploy contracts (pool contract is required for env validation, even when INDEXER_STREAM=forwarder).
    run_forge_build()?;
    let pk0 = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    let owner0 = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
    let intents_addr = run_forge_create_untron_intents(&rpc_url, pk0, owner0)?;
    let forwarder_addr = run_forge_create_intents_forwarder(
        &rpc_url,
        pk0,
        "0x0000000000000000000000000000000000000000",
        "0x0000000000000000000000000000000000000000",
        owner0,
    )?;

    let forwarders_chains = format!(
        "[{{\"chainId\":31337,\"rpcs\":[\"{rpc_url}\"],\"forwarderDeploymentBlock\":0,\"forwarderContractAddress\":\"{forwarder_addr}\"}}]"
    );

    // Start indexer (forwarder stream only).
    let _indexer = KillOnDrop::new(spawn_indexer(
        &db_url,
        &rpc_url,
        &intents_addr,
        "forwarder",
        Some(&forwarders_chains),
    )?);

    // Emit BridgersSet.
    let bridger_a = "0x1111111111111111111111111111111111111111";
    let bridger_b = "0x2222222222222222222222222222222222222222";
    let status = Command::new("cast")
        .args([
            "send",
            "--rpc-url",
            &rpc_url,
            "--private-key",
            pk0,
            &forwarder_addr,
            "setBridgers(address,address)",
            bridger_a,
            bridger_b,
        ])
        .current_dir(repo_root())
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("cast send setBridgers")?;
    if !status.success() {
        anyhow::bail!("cast send setBridgers failed");
    }

    // Wait for projection row.
    let start = Instant::now();
    loop {
        let mut conn = PgConnection::connect(&db_url).await?;
        let row = sqlx::query(
            "select lower(usdt_bridger) as usdt_bridger, lower(usdc_bridger) as usdc_bridger \
             from forwarder.bridgers_versions \
             where valid_to_seq is null and chain_id = 31337 and lower(contract_address) = lower($1) \
             order by valid_from_seq desc \
             limit 1",
        )
        .bind(&forwarder_addr)
        .fetch_optional(&mut conn)
        .await?;

        if let Some(r) = row {
            let usdt: String = r.get("usdt_bridger");
            let usdc: String = r.get("usdc_bridger");
            if usdt == bridger_a.to_ascii_lowercase() && usdc == bridger_b.to_ascii_lowercase() {
                break;
            }
        }

        if start.elapsed() > Duration::from_secs(45) {
            anyhow::bail!(
                "timed out waiting for forwarder.bridgers_versions to reflect BridgersSet"
            );
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    Ok(())
}
