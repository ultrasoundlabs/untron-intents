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
    solver_db::fetch_job_by_intent_id,
    tronbox::{decode_hex32, wait_for_tronbox_accounts, wait_for_tronbox_admin},
    util::{find_free_port, require_bins},
};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};
use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

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

async fn wait_until_blocked(mut rx: watch::Receiver<bool>) {
    loop {
        if *rx.borrow() {
            return;
        }
        if rx.changed().await.is_err() {
            return;
        }
    }
}

async fn spawn_grpc_tcp_proxy(upstream: SocketAddr) -> Result<(String, watch::Sender<bool>)> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .context("bind tcp proxy")?;
    let addr = listener.local_addr().context("tcp proxy local_addr")?;
    let (tx, rx) = watch::channel(false);

    tokio::spawn(async move {
        loop {
            let (mut inbound, _) = match listener.accept().await {
                Ok(v) => v,
                Err(_) => return,
            };
            let rx = rx.clone();
            tokio::spawn(async move {
                // If blocked, immediately drop.
                if *rx.borrow() {
                    return;
                }
                let mut outbound = match TcpStream::connect(upstream).await {
                    Ok(s) => s,
                    Err(_) => return,
                };

                let copy = copy_bidirectional(&mut inbound, &mut outbound);
                let blocked = wait_until_blocked(rx.clone());
                tokio::select! {
                    _ = copy => {}
                    _ = blocked => {}
                }
            });
        }
    });

    Ok((format!("http://127.0.0.1:{}", addr.port()), tx))
}

async fn wait_for_job_state(
    db_url: &str,
    intent_id: &str,
    state: &str,
    timeout: Duration,
) -> Result<()> {
    let start = Instant::now();
    loop {
        match fetch_job_by_intent_id(db_url, intent_id).await {
            Ok(job) => {
                if job.state == state {
                    return Ok(());
                }
            }
            Err(err) => {
                // On slower CI hosts, the solver may not have inserted the job row yet.
                // Treat row-not-found as a poll miss, not a hard failure.
                let row_not_found = err
                    .chain()
                    .any(|e| e.to_string().contains("no rows returned by a query"));
                if !row_not_found {
                    return Err(err);
                }
            }
        }
        if start.elapsed() > timeout {
            anyhow::bail!("timed out waiting for job.state={state} for intent_id={intent_id}");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn wait_for_retry_recorded(
    db_url: &str,
    intent_id: &str,
    min_attempts: i32,
    timeout: Duration,
) -> Result<()> {
    let pool = sqlx::PgPool::connect(db_url).await?;
    let start = Instant::now();
    loop {
        let mut have_row = false;
        let mut attempts = 0;
        let mut last_error_present = false;

        match fetch_job_by_intent_id(db_url, intent_id).await {
            Ok(job) => {
                have_row = true;
                attempts = job.attempts;
                last_error_present = job.last_error.is_some();
            }
            Err(err) => {
                let row_not_found = err
                    .chain()
                    .any(|e| e.to_string().contains("no rows returned by a query"));
                if !row_not_found {
                    return Err(err);
                }
            }
        }

        let retry_in_future = sqlx::query_scalar::<_, bool>(
            "select next_retry_at > now() from solver.jobs where intent_id = decode($1,'hex')",
        )
        .bind(intent_id.trim_start_matches("0x"))
        .fetch_optional(&pool)
        .await?
        .unwrap_or(false);

        if have_row && attempts >= min_attempts && last_error_present && retry_in_future {
            return Ok(());
        }
        if start.elapsed() > timeout {
            anyhow::bail!(
                "timed out waiting for retryable error; retry_in_future={retry_in_future}"
            );
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_solver_tron_grpc_retries_transient_broadcast_failure_with_backoff() -> Result<()> {
    if !require_bins(&["docker", "anvil", "forge", "cast"]) {
        return Ok(());
    }

    cleanup_untron_e2e_containers().ok();

    // Start a private Tron network (tronbox/tre).
    let tron_tag = std::env::var("TRON_TRE_TAG").unwrap_or_else(|_| "1.0.4".to_string());
    let tron_name = format!("untron-e2e-tron-{}", find_free_port()?);
    let tron = GenericImage::new("tronbox/tre".to_string(), tron_tag)
        .with_exposed_port(9090.tcp())
        .with_exposed_port(50051.tcp())
        .with_exposed_port(50052.tcp())
        .with_wait_for(WaitFor::Nothing)
        .with_container_name(tron_name.clone())
        .start()
        .await
        .context("start tronbox/tre container")?;

    let tron_http_port = tron.get_host_port_ipv4(9090).await?;
    let tron_grpc_port = tron.get_host_port_ipv4(50051).await?;
    let tron_http_base = format!("http://127.0.0.1:{tron_http_port}");
    let _tron_grpc_url = format!("http://127.0.0.1:{tron_grpc_port}");

    wait_for_tronbox_admin(&tron_http_base, Duration::from_secs(240)).await?;
    let keys = wait_for_tronbox_accounts(&tron_http_base, Duration::from_secs(240)).await?;
    if keys.len() < 2 {
        anyhow::bail!("expected at least 2 tronbox accounts, got {}", keys.len());
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

    // Deploy hub contracts (use a no-sig reader; this test cares about retry behavior, not proof fidelity).
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

    // Start solver (Tron gRPC).
    let upstream: SocketAddr = format!("127.0.0.1:{tron_grpc_port}").parse().unwrap();
    let (proxied_tron_grpc_url, proxy_block_tx) = spawn_grpc_tcp_proxy(upstream).await?;

    let _solver = KillOnDrop::new(spawn_solver_tron_grpc_custom(
        &db_url,
        &postgrest_url,
        &rpc_url,
        &intents_addr,
        pk0,
        &proxied_tron_grpc_url,
        &tron_pk0,
        &tron_pk0,
        &tron_controller_address,
        "solver-tron-retry-backoff",
        "trx_transfer",
        &[],
    )?);

    // Solver migrates its own schema at startup.
    wait_for_solver_table(&db_url, "jobs", Duration::from_secs(30)).await?;

    // Wait until we're ready to broadcast (tron_prepared), then simulate a transient node outage.
    wait_for_job_state(
        &db_url,
        &intent_id,
        "tron_prepared",
        Duration::from_secs(180),
    )
    .await?;
    proxy_block_tx.send_replace(true);

    // The solver should record a retryable error with next_retry_at in the future.
    let attempts_before = fetch_job_by_intent_id(&db_url, &intent_id).await?.attempts;
    wait_for_retry_recorded(
        &db_url,
        &intent_id,
        attempts_before + 1,
        Duration::from_secs(60),
    )
    .await?;

    proxy_block_tx.send_replace(false);

    // The solver should recover and complete.
    let _rows = match wait_for_intents_solved_and_settled(&db_url, 1, Duration::from_secs(300))
        .await
    {
        Ok(rows) => rows,
        Err(e) => {
            let job = fetch_job_by_intent_id(&db_url, &intent_id).await?;
            eprintln!(
                "solver.jobs diagnostic: intent_id={} job_id={} state={} attempts={} next_retry_at={} leased_by={:?} tron_txid={:?} last_error={:?} claim_tx_hash={:?} prove_tx_hash={:?}",
                intent_id,
                job.job_id,
                job.state,
                job.attempts,
                job.next_retry_at,
                job.leased_by,
                job.tron_txid,
                job.last_error,
                job.claim_tx_hash,
                job.prove_tx_hash,
            );
            return Err(e);
        }
    };
    Ok(())
}
