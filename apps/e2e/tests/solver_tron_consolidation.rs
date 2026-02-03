use anyhow::{Context, Result};
use e2e::{
    anvil::spawn_anvil,
    binaries::{cargo_build_indexer_bins, cargo_build_solver_bin, run_migrations},
    cast::{run_cast_create_trx_transfer_intent, run_cast_mint_mock_erc20},
    docker::{PostgresOptions, PostgrestOptions, start_postgres, start_postgrest},
    docker_cleanup::cleanup_untron_e2e_containers,
    forge::{
        run_forge_build, run_forge_create_mock_erc20, run_forge_create_mock_untron_v3,
        run_forge_create_test_tron_tx_reader_no_sig, run_forge_create_untron_intents_with_args,
    },
    http::wait_for_http_ok,
    pool_db::{wait_for_intents_solved_and_settled, wait_for_pool_current_intents_count},
    postgres::{configure_postgrest_roles, wait_for_postgres},
    process::KillOnDrop,
    services::{spawn_indexer, spawn_solver_tron_grpc_custom},
    tronbox::{decode_hex32, wait_for_tronbox_accounts, wait_for_tronbox_admin},
    util::{find_free_port, require_bins},
};
use sqlx::Row;
use std::time::{Duration, Instant};
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};

fn is_missing_relation(err: &sqlx::Error) -> bool {
    let s = err.to_string();
    s.contains("does not exist") && (s.contains("solver.") || s.contains("schema \"solver\""))
}

async fn fetch_tron_balance_sun(grpc: &mut tron::TronGrpc, addr: tron::TronAddress) -> Result<i64> {
    let account = grpc
        .get_account(addr.prefixed_bytes().to_vec())
        .await
        .context("GetAccount")?;
    Ok(account.balance)
}

async fn drain_to_target_balance_sun(
    grpc: &mut tron::TronGrpc,
    from: &tron::TronWallet,
    sink: tron::TronAddress,
    target_sun: i64,
) -> Result<()> {
    let start = Instant::now();
    loop {
        let cur = fetch_tron_balance_sun(grpc, from.address()).await?;
        if cur <= target_sun {
            return Ok(());
        }
        let delta = cur.saturating_sub(target_sun);
        if delta > 0 {
            let _ = from
                .broadcast_transfer_contract(grpc, sink, delta)
                .await
                .context("broadcast transfer (drain)")?;
        }
        if start.elapsed() > Duration::from_secs(30) {
            anyhow::bail!(
                "timed out draining tron balance: addr={} target_sun={target_sun} cur={cur}",
                from.address().to_base58check()
            );
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn wait_for_job_id(db_url: &str, intent_id_hex: &str) -> Result<Option<i64>> {
    let pool = sqlx::PgPool::connect(db_url).await?;
    let row = sqlx::query("select job_id from solver.jobs where intent_id = decode($1,'hex')")
        .bind(intent_id_hex.trim_start_matches("0x"))
        .fetch_optional(&pool)
        .await;
    let row = match row {
        Ok(r) => r,
        Err(e) if is_missing_relation(&e) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    Ok(row.map(|r| r.get::<i64, _>("job_id")))
}

async fn fetch_tron_plan(db_url: &str, job_id: i64) -> Result<Vec<(String, [u8; 32])>> {
    let pool = sqlx::PgPool::connect(db_url).await?;
    let rows = sqlx::query(
        "select step, txid \
         from solver.tron_signed_txs \
         where job_id = $1 \
         order by (step = 'final')::int, step asc",
    )
    .bind(job_id)
    .fetch_all(&pool)
    .await;
    let rows = match rows {
        Ok(r) => r,
        Err(e) if is_missing_relation(&e) => return Ok(vec![]),
        Err(e) => return Err(e.into()),
    };

    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let step: String = r.get("step");
        let txid: Vec<u8> = r.get("txid");
        let mut t = [0u8; 32];
        t.copy_from_slice(&txid);
        out.push((step, t));
    }
    Ok(out)
}

async fn wait_for_tron_plan_persisted(
    db_url: &str,
    intent_id_hex: &str,
    timeout: Duration,
) -> Result<(i64, Vec<(String, [u8; 32])>)> {
    let start = Instant::now();
    loop {
        if let Some(job_id) = wait_for_job_id(db_url, intent_id_hex).await? {
            let plan = fetch_tron_plan(db_url, job_id).await?;
            let has_pre = plan.iter().any(|(s, _)| s.starts_with("pre:"));
            let has_final = plan.iter().any(|(s, _)| s == "final");
            if plan.len() >= 2 && has_pre && has_final {
                return Ok((job_id, plan));
            }
        }

        if start.elapsed() > timeout {
            anyhow::bail!("timed out waiting for tron plan intent_id={intent_id_hex}");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn wait_for_tron_tx_known(
    grpc: &mut tron::TronGrpc,
    txid: [u8; 32],
    timeout: Duration,
) -> Result<()> {
    let start = Instant::now();
    loop {
        let info = grpc.get_transaction_info_by_id(txid).await;
        if let Ok(info) = info {
            let id_matches = info.id.len() == 32 && info.id.as_slice() == txid;
            let confirmed = info.block_number > 0;
            if id_matches || confirmed {
                return Ok(());
            }
        }
        if start.elapsed() > timeout {
            anyhow::bail!(
                "timed out waiting for tron tx to be known: 0x{}",
                hex::encode(txid)
            );
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn assert_tron_tx_included(grpc: &mut tron::TronGrpc, txid: [u8; 32]) -> Result<()> {
    let info = grpc
        .get_transaction_info_by_id(txid)
        .await
        .context("get_transaction_info_by_id")?;
    if info.block_number <= 0 {
        anyhow::bail!(
            "expected tron tx to be included (block_number>0): txid=0x{} info={info:?}",
            hex::encode(txid)
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_solver_tron_consolidation_is_restart_safe() -> Result<()> {
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
    if keys.len() < 6 {
        anyhow::bail!("expected at least 6 tronbox accounts, got {}", keys.len());
    }

    let tron_pk0 = keys[0].clone();
    let tron_pk1 = keys[1].clone();
    let tron_pk2 = keys[2].clone();
    let tron_pk3 = keys[3].clone();
    let tron_pk_sink = keys[5].clone();

    let w0 = tron::TronWallet::new(decode_hex32(&tron_pk0)?).context("tron wallet0")?;
    let w1 = tron::TronWallet::new(decode_hex32(&tron_pk1)?).context("tron wallet1")?;
    let w2 = tron::TronWallet::new(decode_hex32(&tron_pk2)?).context("tron wallet2")?;
    let w3 = tron::TronWallet::new(decode_hex32(&tron_pk3)?).context("tron wallet3")?;
    let sink = tron::TronWallet::new(decode_hex32(&tron_pk_sink)?).context("tron wallet sink")?;
    let sink_addr = sink.address();

    // Force consolidation by draining balances so no single key can cover (amount + reserve).
    // NOTE: Tron backend reserves 2_000_000 SUN, so for amount=1_000_000 SUN we require 3_000_000.
    let mut grpc = tron::TronGrpc::connect(&tron_grpc_url, None)
        .await
        .context("connect tron grpc (drain)")?;
    drain_to_target_balance_sun(&mut grpc, &w0, sink_addr, 900_000).await?;
    drain_to_target_balance_sun(&mut grpc, &w1, sink_addr, 1_400_000).await?;
    drain_to_target_balance_sun(&mut grpc, &w2, sink_addr, 1_600_000).await?;
    drain_to_target_balance_sun(&mut grpc, &w3, sink_addr, 1_900_000).await?;

    let b0 = fetch_tron_balance_sun(&mut grpc, w0.address()).await?;
    let b1 = fetch_tron_balance_sun(&mut grpc, w1.address()).await?;
    let b2 = fetch_tron_balance_sun(&mut grpc, w2.address()).await?;
    let b3 = fetch_tron_balance_sun(&mut grpc, w3.address()).await?;
    if b0 >= 3_000_000 || b1 >= 3_000_000 || b2 >= 3_000_000 || b3 >= 3_000_000 {
        anyhow::bail!(
            "expected each key to be below 3_000_000 SUN to force consolidation; balances=[{b0},{b1},{b2},{b3}]"
        );
    }
    if b0 + b1 + b2 + b3 < 3_000_000 {
        anyhow::bail!(
            "expected combined balances to cover 3_000_000 SUN; balances=[{b0},{b1},{b2},{b3}]"
        );
    }

    let to_evm = format!("{:#x}", sink_addr.evm());
    let tron_controller_address = w0.address().to_base58check();

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

    // Create one TRX transfer intent: amount=1_000_000 SUN, which requires consolidation because
    // of the 2_000_000 SUN reserve.
    let _ =
        run_cast_create_trx_transfer_intent(&rpc_url, pk0, &intents_addr, &to_evm, "1000000", 1)?;
    wait_for_pool_current_intents_count(&db_url, 1, Duration::from_secs(60)).await?;

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

    // Resolve intent_id.
    let intent_id = e2e::pool_db::fetch_current_intents(&db_url)
        .await?
        .first()
        .context("missing intent row")?
        .id
        .clone();

    // Start solver with multiple Tron keys + consolidation enabled.
    let tron_keys_csv = format!("{tron_pk0},{tron_pk1},{tron_pk2},{tron_pk3}");
    let mut solver = KillOnDrop::new(spawn_solver_tron_grpc_custom(
        &db_url,
        &postgrest_url,
        &rpc_url,
        &intents_addr,
        pk0,
        &tron_grpc_url,
        &tron_pk0,
        &tron_keys_csv,
        &tron_controller_address,
        "solver-tron-consolidation-1",
        "trx_transfer",
        &[
            ("SOLVER_CONSOLIDATION_ENABLED", "true"),
            ("SOLVER_CONSOLIDATION_MAX_PRE_TXS", "2"),
            ("SOLVER_CONSOLIDATION_MAX_TOTAL_TRX_PULL_SUN", "0"),
            ("SOLVER_CONSOLIDATION_MAX_PER_TX_TRX_PULL_SUN", "0"),
        ],
    )?);

    // Wait until the solver persisted a consolidation plan (pre:* + final).
    let (job_id, plan) =
        wait_for_tron_plan_persisted(&db_url, &intent_id, Duration::from_secs(120)).await?;

    // Kill the solver after the first pre-tx becomes known onchain, before it can finish the plan.
    // On restart, it should not get stuck or double-send in a way that prevents completion.
    let pre0 = plan
        .iter()
        .find(|(s, _)| s.starts_with("pre:"))
        .map(|(_, t)| *t)
        .context("missing pre tx in plan")?;
    wait_for_tron_tx_known(&mut grpc, pre0, Duration::from_secs(120)).await?;
    solver.kill_now();

    // Restart solver and ensure the job completes.
    let _solver2 = KillOnDrop::new(spawn_solver_tron_grpc_custom(
        &db_url,
        &postgrest_url,
        &rpc_url,
        &intents_addr,
        pk0,
        &tron_grpc_url,
        &tron_pk0,
        &tron_keys_csv,
        &tron_controller_address,
        "solver-tron-consolidation-2",
        "trx_transfer",
        &[
            ("SOLVER_CONSOLIDATION_ENABLED", "true"),
            ("SOLVER_CONSOLIDATION_MAX_PRE_TXS", "2"),
            ("SOLVER_CONSOLIDATION_MAX_TOTAL_TRX_PULL_SUN", "0"),
            ("SOLVER_CONSOLIDATION_MAX_PER_TX_TRX_PULL_SUN", "0"),
        ],
    )?);

    let _rows = wait_for_intents_solved_and_settled(&db_url, 1, Duration::from_secs(420)).await?;

    // Assert all planned txids (pre + final) are included onchain.
    for (_, txid) in fetch_tron_plan(&db_url, job_id).await? {
        assert_tron_tx_included(&mut grpc, txid).await?;
    }

    Ok(())
}
