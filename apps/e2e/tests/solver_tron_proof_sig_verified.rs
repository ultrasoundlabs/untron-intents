use anyhow::{Context, Result};
use e2e::{
    anvil::spawn_anvil,
    binaries::{cargo_build_indexer_bins, cargo_build_solver_bin, run_migrations},
    cast::{run_cast_create_trx_transfer_intent, run_cast_mint_mock_erc20},
    docker::{PostgresOptions, PostgrestOptions, start_postgres, start_postgrest},
    docker_cleanup::cleanup_untron_e2e_containers,
    forge::{
        run_forge_build, run_forge_create_mock_erc20, run_forge_create_mock_untron_v3,
        run_forge_create_test_tron_tx_reader_sig,
        run_forge_create_test_tron_tx_reader_sig_allowlist,
        run_forge_create_untron_intents_with_args,
    },
    http::wait_for_http_ok,
    pool_db::{wait_for_intents_solved_and_settled, wait_for_pool_current_intents_count},
    postgres::{configure_postgrest_roles, wait_for_postgres},
    process::KillOnDrop,
    services::{spawn_indexer, spawn_solver_tron_grpc_custom},
    solver_db::fetch_job_by_intent_id,
    tronbox::{decode_hex32, wait_for_tronbox_accounts, wait_for_tronbox_admin},
    util::{find_free_port, require_bins},
};
use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
use sha2::Digest;
use std::collections::BTreeSet;
use std::time::{Duration, Instant};
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};
use tokio::time::sleep;

fn recover_signer_from_packed_block(block: &[u8]) -> Result<[u8; 20]> {
    if block.len() != 174 {
        anyhow::bail!(
            "unexpected packed header length: expected 174, got {}",
            block.len()
        );
    }
    let digest = sha2::Sha256::digest(&block[2..107]);
    let sig = &block[109..174];
    let sig = Signature::from_slice(&sig[0..64]).context("parse signature (r||s)")?;
    let mut v = block[173];
    if v == 27 || v == 28 {
        v -= 27;
    }
    if v > 1 {
        anyhow::bail!("invalid recovery id v={v}");
    }
    let recid = RecoveryId::from_byte(v).context("parse recovery id")?;
    let vk = VerifyingKey::recover_from_prehash(digest.as_ref(), &sig, recid)
        .context("recover verifying key")?;
    let uncompressed = vk.to_encoded_point(false);
    let bytes = uncompressed.as_bytes();
    if bytes.len() != 65 || bytes[0] != 0x04 {
        anyhow::bail!("unexpected recovered pubkey format/len={}", bytes.len());
    }
    let hash = alloy::primitives::keccak256(&bytes[1..]);
    let mut out = [0u8; 20];
    out.copy_from_slice(&hash.as_slice()[12..]);
    Ok(out)
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
        sleep(Duration::from_millis(200)).await;
    }
}

async fn wait_for_job_state(
    db_url: &str,
    intent_id: &str,
    state: &str,
    timeout: Duration,
) -> Result<()> {
    let pool = sqlx::PgPool::connect(db_url).await?;
    let intent_hex = intent_id.trim_start_matches("0x");
    let start = Instant::now();
    loop {
        let got: Option<String> = sqlx::query_scalar(
            "select state::text from solver.jobs where intent_id = decode($1,'hex')",
        )
        .bind(intent_hex)
        .fetch_optional(&pool)
        .await
        .unwrap_or(None);

        if got.as_deref() == Some(state) {
            return Ok(());
        }
        if start.elapsed() > timeout {
            let job = fetch_job_by_intent_id(db_url, intent_id).await.ok();
            anyhow::bail!("timed out waiting for job.state={state}; job={job:?}");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn wait_for_prove_failure(
    db_url: &str,
    intent_id: &str,
    min_attempts: i32,
    timeout: Duration,
) -> Result<()> {
    let start = Instant::now();
    loop {
        let job = fetch_job_by_intent_id(db_url, intent_id).await?;
        if job.state == "proof_built"
            && job.attempts >= min_attempts
            && job.prove_tx_hash.is_none()
            && let Some(err) = job.last_error.as_ref()
            && !err.is_empty()
        {
            return Ok(());
        }
        if start.elapsed() > timeout {
            anyhow::bail!("timed out waiting for prove failure; job={job:?}");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_solver_tron_grpc_proof_requires_sig_verified_reader_and_rejects_mutated_tx_bytes()
-> Result<()> {
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
    if keys.is_empty() {
        anyhow::bail!("expected at least 1 tronbox account, got {}", keys.len());
    }
    let tron_pk0 = keys[0].clone();
    let tron_wallet0 = tron::TronWallet::new(decode_hex32(&tron_pk0)?).context("tron wallet0")?;
    let tron_controller_address = tron_wallet0.address().to_base58check();

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
    // Start with a signature-verified reader configured with a wrong delegatee so proving fails and
    // the job remains in `proof_built` long enough for us to mutate proof bytes deterministically.
    let bad_delegatee = "0x1111111111111111111111111111111111111111";
    let reader_bad_sig = run_forge_create_test_tron_tx_reader_sig(
        &rpc_url,
        pk0,
        "0x0000000000000000000000000000000000000000",
        bad_delegatee,
    )?;
    let v3 = run_forge_create_mock_untron_v3(
        &rpc_url,
        pk0,
        &reader_bad_sig,
        "0x0000000000000000000000000000000000000001",
        "0x0000000000000000000000000000000000000002",
    )?;
    let intents_addr =
        run_forge_create_untron_intents_with_args(&rpc_url, pk0, owner0, &v3, &usdt)?;

    // Fund hub solver with claim deposit USDT.
    run_cast_mint_mock_erc20(&rpc_url, pk0, &usdt, owner0, "5000000")?;

    // Start indexer (pool-only).
    let _indexer = KillOnDrop::new(spawn_indexer(
        &db_url,
        &rpc_url,
        &intents_addr,
        "pool",
        None,
    )?);

    // Create one TRX transfer intent.
    let to = "0x00000000000000000000000000000000000000aa";
    let _ = run_cast_create_trx_transfer_intent(&rpc_url, pk0, &intents_addr, to, "1234", 1)?;
    wait_for_pool_current_intents_count(&db_url, 1, Duration::from_secs(60)).await?;
    let intent_id = e2e::pool_db::fetch_current_intents(&db_url)
        .await?
        .first()
        .context("missing intent row")?
        .id
        .clone();

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

    // Start solver (Tron gRPC) and stop it once the proof is built (before prove submission).
    let mut solver = KillOnDrop::new(spawn_solver_tron_grpc_custom(
        &db_url,
        &postgrest_url,
        &rpc_url,
        &intents_addr,
        pk0,
        &tron_grpc_url,
        &tron_pk0,
        &tron_pk0,
        &tron_controller_address,
        "solver-tron-proof-sig-verified",
        "trx_transfer",
        &[],
    )?);

    wait_for_solver_table(&db_url, "jobs", Duration::from_secs(30)).await?;
    wait_for_job_state(&db_url, &intent_id, "proof_built", Duration::from_secs(300)).await?;
    // Confirm prove is actually failing (so state sticks).
    wait_for_prove_failure(&db_url, &intent_id, 1, Duration::from_secs(120)).await?;
    solver.kill_now();

    // Build and install the signature-verified reader using the actual packed headers in the proof.
    let job = fetch_job_by_intent_id(&db_url, &intent_id).await?;
    let txid_hex = job
        .tron_txid
        .clone()
        .context("missing tron_txid at proof_built")?
        .trim_start_matches("0x")
        .to_string();
    let pool = sqlx::PgPool::connect(&db_url).await?;

    let blocks: Vec<Vec<u8>> =
        sqlx::query_scalar("select blocks from solver.tron_proofs where txid = decode($1,'hex')")
            .bind(&txid_hex)
            .fetch_one(&pool)
            .await
            .context("load solver.tron_proofs.blocks")?;
    if blocks.len() != 20 {
        anyhow::bail!(
            "unexpected blocks length: expected 20, got {}",
            blocks.len()
        );
    }

    // Configure a signature-verified reader against an allowlist derived from the proof's packed
    // headers, and assert no signer recovers to 0x0 (which would indicate invalid signatures).
    let mut allowed: BTreeSet<[u8; 20]> = BTreeSet::new();
    for b in &blocks {
        let signer = recover_signer_from_packed_block(b)?;
        if signer == [0u8; 20] {
            anyhow::bail!("invalid packed header signature: recovered signer is 0x0");
        }
        allowed.insert(signer);
    }
    let allowed_hex: Vec<String> = allowed
        .into_iter()
        .map(|a| format!("0x{}", hex::encode(a)))
        .collect();
    let strict_reader =
        run_forge_create_test_tron_tx_reader_sig_allowlist(&rpc_url, pk0, &allowed_hex)?;

    // Point v3 at the strict reader (before proving).
    let status = std::process::Command::new("cast")
        .args([
            "send",
            "--rpc-url",
            &rpc_url,
            "--private-key",
            pk0,
            &v3,
            "setTronReader(address)",
            &strict_reader,
        ])
        .current_dir(e2e::util::repo_root())
        .status()
        .context("cast send setTronReader")?;
    if !status.success() {
        anyhow::bail!("cast send setTronReader failed");
    }

    // Tamper the persisted proof's encoded_tx so prove must fail.
    let mut encoded_tx: Vec<u8> = sqlx::query_scalar(
        "select encoded_tx from solver.tron_proofs where txid = decode($1,'hex')",
    )
    .bind(&txid_hex)
    .fetch_one(&pool)
    .await
    .context("load solver.tron_proofs.encoded_tx")?;
    let original = encoded_tx.clone();
    if let Some(b) = encoded_tx.last_mut() {
        *b ^= 0x01;
    } else {
        anyhow::bail!("unexpected empty encoded_tx");
    }

    sqlx::query("update solver.tron_proofs set encoded_tx = $1 where txid = decode($2,'hex')")
        .bind(&encoded_tx)
        .bind(&txid_hex)
        .execute(&pool)
        .await
        .context("tamper solver.tron_proofs.encoded_tx")?;

    // Restart solver and assert prove fails (attempts++ with last_error populated), but job remains retryable.
    let attempts_before = job.attempts;
    let mut solver = KillOnDrop::new(spawn_solver_tron_grpc_custom(
        &db_url,
        &postgrest_url,
        &rpc_url,
        &intents_addr,
        pk0,
        &tron_grpc_url,
        &tron_pk0,
        &tron_pk0,
        &tron_controller_address,
        "solver-tron-proof-sig-verified-2",
        "trx_transfer",
        &[],
    )?);

    wait_for_prove_failure(
        &db_url,
        &intent_id,
        attempts_before + 1,
        Duration::from_secs(120),
    )
    .await?;
    solver.kill_now();

    // Restore encoded_tx and verify the job can complete.
    sqlx::query("update solver.tron_proofs set encoded_tx = $1 where txid = decode($2,'hex')")
        .bind(&original)
        .bind(&txid_hex)
        .execute(&pool)
        .await
        .context("restore solver.tron_proofs.encoded_tx")?;

    let _solver = KillOnDrop::new(spawn_solver_tron_grpc_custom(
        &db_url,
        &postgrest_url,
        &rpc_url,
        &intents_addr,
        pk0,
        &tron_grpc_url,
        &tron_pk0,
        &tron_pk0,
        &tron_controller_address,
        "solver-tron-proof-sig-verified-3",
        "trx_transfer",
        &[],
    )?);

    let _rows =
        match wait_for_intents_solved_and_settled(&db_url, 1, Duration::from_secs(300)).await {
            Ok(rows) => rows,
            Err(e) => {
                let job = fetch_job_by_intent_id(&db_url, &intent_id).await.ok();
                eprintln!("solver.jobs diagnostic: intent_id={intent_id} job={job:?}");
                return Err(e);
            }
        };
    Ok(())
}
