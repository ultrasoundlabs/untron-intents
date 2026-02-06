use anyhow::{Context, Result};
use e2e::{
    anvil::spawn_anvil,
    binaries::{cargo_build_indexer_bins, cargo_build_solver_bin, run_migrations},
    cast::{
        cast_abi_encode, run_cast_create_trigger_smart_contract_intent, run_cast_mint_mock_erc20,
    },
    docker::{PostgresOptions, PostgrestOptions, start_postgres, start_postgrest},
    docker_cleanup::cleanup_untron_e2e_containers,
    forge::{
        run_forge_build, run_forge_create_mock_erc20, run_forge_create_mock_tron_tx_reader,
        run_forge_create_mock_untron_v3, run_forge_create_untron_intents_with_args,
    },
    http::wait_for_http_ok,
    pool_db::{fetch_current_intents, wait_for_pool_current_intents_count},
    postgres::{configure_postgrest_roles, wait_for_postgres},
    process::KillOnDrop,
    services::{spawn_indexer, spawn_solver_tron_grpc_custom},
    solver_db::fetch_job_by_intent_id,
    tronbox::{decode_hex32, wait_for_tronbox_accounts, wait_for_tronbox_admin},
    util::{find_free_port, require_bins},
};
use sqlx::Row;
use std::time::{Duration, Instant};
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};

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

async fn wait_for_intent_skip_reason(
    db_url: &str,
    intent_id_hex: &str,
    expected_reason: &str,
    timeout: Duration,
) -> Result<()> {
    let pool = sqlx::PgPool::connect(db_url).await?;
    let start = Instant::now();
    loop {
        let row = sqlx::query(
            "select reason \
             from solver.intent_skips \
             where intent_id = decode($1, 'hex')",
        )
        .bind(intent_id_hex.trim_start_matches("0x"))
        .fetch_optional(&pool)
        .await?;

        if let Some(row) = row {
            let reason: String = row.get("reason");
            if reason == expected_reason {
                return Ok(());
            }
            anyhow::bail!("unexpected intent skip reason: got={reason} expected={expected_reason}");
        }

        if start.elapsed() > timeout {
            anyhow::bail!(
                "timed out waiting for solver.intent_skips for intent_id={intent_id_hex}"
            );
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn wait_for_no_job_for_intent(
    db_url: &str,
    intent_id_hex: &str,
    timeout: Duration,
) -> Result<()> {
    let pool = sqlx::PgPool::connect(db_url).await?;
    let start = Instant::now();
    loop {
        let n: i64 = sqlx::query_scalar(
            "select count(*)::bigint \
             from solver.jobs \
             where intent_id = decode($1, 'hex')",
        )
        .bind(intent_id_hex.trim_start_matches("0x"))
        .fetch_one(&pool)
        .await?;
        if n == 0 {
            return Ok(());
        }
        if start.elapsed() > timeout {
            anyhow::bail!("expected no solver.jobs row for intent_id={intent_id_hex}, got {n}");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn wait_for_breaker_active(
    db_url: &str,
    contract_hex: &str,
    selector_hex: &str,
    timeout: Duration,
) -> Result<()> {
    let pool = sqlx::PgPool::connect(db_url).await?;
    let start = Instant::now();
    loop {
        let row = sqlx::query(
            "select fail_count, cooldown_until > now() as active \
             from solver.circuit_breakers \
             where contract = decode($1, 'hex') \
               and (selector is null or selector = decode($2, 'hex'))",
        )
        .bind(contract_hex.trim_start_matches("0x"))
        .bind(selector_hex.trim_start_matches("0x"))
        .fetch_optional(&pool)
        .await?;

        if let Some(row) = row {
            let fail_count: i32 = row.get("fail_count");
            let active: bool = row.get("active");
            if active && fail_count >= 1 {
                return Ok(());
            }
        }

        if start.elapsed() > timeout {
            anyhow::bail!(
                "timed out waiting for breaker activation for contract={contract_hex} selector={selector_hex}"
            );
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_solver_trigger_breaker_suppresses_claims_tron_grpc() -> Result<()> {
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
        anyhow::bail!("expected at least 1 tronbox account");
    }
    let tron_pk0 = keys[0].clone();
    let tron_wallet0 = tron::TronWallet::new(decode_hex32(&tron_pk0)?).context("tron wallet0")?;
    let tron_controller_address = tron_wallet0.address().to_base58check();

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
    let mock_reader = run_forge_create_mock_tron_tx_reader(&rpc_url, pk0)?;

    // Use a non-existent Tron contract address (will deterministically fail TriggerSmartContract preparation).
    let trigger_target = "0x00000000000000000000000000000000000000bb";

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

    // Ensure the solver schema (including `solver.circuit_breakers`) is migrated before assertions.
    let mut migrator = KillOnDrop::new(spawn_solver_tron_grpc_custom(
        &db_url,
        &postgrest_url,
        &rpc_url,
        &intents_addr,
        pk0,
        &tron_grpc_url,
        &tron_pk0,
        &tron_pk0,
        &tron_controller_address,
        "solver-trigger-breaker-migrate",
        "trigger_smart_contract",
        &[
            ("SOLVER_TRIGGER_CONTRACT_ALLOWLIST_CSV", trigger_target),
            ("SOLVER_TRON_EMULATION_ENABLED", "false"),
            ("INDEXER_MAX_HEAD_LAG_BLOCKS", "1000000"),
        ],
    )?);
    wait_for_solver_table(&db_url, "circuit_breakers", Duration::from_secs(30)).await?;
    wait_for_solver_table(&db_url, "intent_skips", Duration::from_secs(30)).await?;
    wait_for_solver_table(&db_url, "jobs", Duration::from_secs(30)).await?;
    migrator.kill_now();

    let selector = "0xdeadbeef";
    // Same selector as `selector` (first 4 bytes), but slightly different calldata to avoid de-dupe concerns.
    let selector_variant = "0xdeadbeef11";
    let expected_specs = cast_abi_encode(
        "f((address,uint256,bytes))",
        &[&format!("({trigger_target},0,{selector})")],
    )?;

    let _ = run_cast_create_trigger_smart_contract_intent(
        &rpc_url,
        pk0,
        &intents_addr,
        trigger_target,
        "0",
        selector,
        1,
    )?;
    wait_for_pool_current_intents_count(&db_url, 1, Duration::from_secs(45)).await?;

    let (intent_id, intent_specs) = {
        let rows = fetch_current_intents(&db_url).await?;
        let r = rows
            .first()
            .context("expected one indexed intent in pool.intent_versions")?;
        (r.id.clone(), r.row.intent_specs.clone())
    };
    if intent_specs != expected_specs {
        anyhow::bail!("unexpected intent_specs; expected={expected_specs} got={intent_specs}");
    }

    // Start solver configured to only fill TriggerSmartContract.
    // Tron emulation is disabled so the intent is claimed and then fails during Tron tx preparation, which
    // should record a breaker row.
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
        "solver-trigger-breaker-1",
        "trigger_smart_contract",
        &[
            ("SOLVER_TRIGGER_CONTRACT_ALLOWLIST_CSV", trigger_target),
            ("SOLVER_TRON_EMULATION_ENABLED", "false"),
            ("INDEXER_MAX_HEAD_LAG_BLOCKS", "1000000"),
        ],
    )?);

    if let Err(e) =
        wait_for_breaker_active(&db_url, trigger_target, selector, Duration::from_secs(180)).await
    {
        let job = fetch_job_by_intent_id(&db_url, &intent_id).await.ok();
        let skip_reason: Option<String> = {
            let pool = sqlx::PgPool::connect(&db_url).await.ok();
            if let Some(pool) = pool {
                sqlx::query_scalar(
                    "select reason \
                     from solver.intent_skips \
                     where intent_id = decode($1, 'hex')",
                )
                .bind(intent_id.trim_start_matches("0x"))
                .fetch_optional(&pool)
                .await
                .ok()
                .flatten()
            } else {
                None
            }
        };
        eprintln!(
            "breaker activation timed out: intent_id={} job={:?} skip_reason={:?}",
            intent_id, job, skip_reason
        );
        return Err(e);
    }

    // Restart solver: breaker should still suppress a new intent with the same (contract, selector).
    solver.kill_now();
    let _ = run_cast_create_trigger_smart_contract_intent(
        &rpc_url,
        pk0,
        &intents_addr,
        trigger_target,
        "0",
        selector_variant,
        1,
    )?;
    wait_for_pool_current_intents_count(&db_url, 2, Duration::from_secs(45)).await?;
    let intent_id2 = fetch_current_intents(&db_url).await?[1].id.clone();

    let _solver2 = KillOnDrop::new(spawn_solver_tron_grpc_custom(
        &db_url,
        &postgrest_url,
        &rpc_url,
        &intents_addr,
        pk0,
        &tron_grpc_url,
        &tron_pk0,
        &tron_pk0,
        &tron_controller_address,
        "solver-trigger-breaker-2",
        "trigger_smart_contract",
        &[
            ("SOLVER_TRIGGER_CONTRACT_ALLOWLIST_CSV", trigger_target),
            ("SOLVER_TRON_EMULATION_ENABLED", "false"),
            ("INDEXER_MAX_HEAD_LAG_BLOCKS", "1000000"),
        ],
    )?);

    wait_for_intent_skip_reason(
        &db_url,
        &intent_id2,
        "breaker_active",
        Duration::from_secs(30),
    )
    .await?;
    wait_for_no_job_for_intent(&db_url, &intent_id2, Duration::from_secs(2)).await?;

    Ok(())
}
