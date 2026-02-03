use anyhow::Result;
use e2e::{
    anvil::spawn_anvil,
    binaries::{cargo_build_indexer_bins, cargo_build_solver_bin, run_migrations},
    cast::{run_cast_create_trx_transfer_intent, run_cast_mint_mock_erc20},
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
    services::{spawn_indexer, spawn_solver_mock_custom},
    util::{find_free_port, require_bins},
};
use sqlx::Row;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
struct JobMini {
    intent_id: String,
    state: String,
    last_error: Option<String>,
    claim_tx_hash: Option<String>,
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

async fn fetch_job_minis(db_url: &str, intent_ids: &[String]) -> Result<Vec<JobMini>> {
    let pool = sqlx::PgPool::connect(db_url).await?;
    let mut out = Vec::with_capacity(intent_ids.len());
    for id in intent_ids {
        let row = sqlx::query(
            "select \
               state, \
               last_error, \
               encode(claim_tx_hash, 'hex') as claim_tx_hash_hex \
             from solver.jobs \
             where intent_id = decode($1, 'hex')",
        )
        .bind(id.trim_start_matches("0x"))
        .fetch_optional(&pool)
        .await?;
        if let Some(row) = row {
            let claim_hex: Option<String> = row.get("claim_tx_hash_hex");
            out.push(JobMini {
                intent_id: id.clone(),
                state: row.get("state"),
                last_error: row.get("last_error"),
                claim_tx_hash: claim_hex.map(|h| format!("0x{h}")),
            });
        }
    }
    Ok(out)
}

async fn wait_for_rate_limit_effect(db_url: &str, intent_ids: &[String], timeout: Duration) -> Result<()> {
    let start = Instant::now();
    loop {
        let jobs = fetch_job_minis(db_url, intent_ids).await?;
        if jobs.len() >= 3 {
            let mut claimed = 0;
            let mut rate_limited = 0;
            for j in &jobs {
                if j.claim_tx_hash.is_some() || j.state != "ready" {
                    claimed += 1;
                }
                if j.last_error.as_deref() == Some("claim_rate_limited") && j.state == "ready" {
                    rate_limited += 1;
                }
            }
            if claimed >= 1 && rate_limited >= 1 {
                return Ok(());
            }
        }

        if start.elapsed() > timeout {
            anyhow::bail!("timed out waiting for claim rate limit effect; jobs={jobs:?}");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_solver_rate_limits_trx_transfer_claims_mock_tron() -> Result<()> {
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
    let _anvil = KillOnDrop::new(spawn_anvil(anvil_port)?);
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Deploy contracts.
    run_forge_build()?;
    let pk0 = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    let owner0 = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
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

    // Create three TRX transfer intents.
    let to = "0x00000000000000000000000000000000000000aa";
    let _ = run_cast_create_trx_transfer_intent(&rpc_url, pk0, &intents_addr, to, "1234", 1)?;
    let _ = run_cast_create_trx_transfer_intent(&rpc_url, pk0, &intents_addr, to, "5678", 1)?;
    let _ = run_cast_create_trx_transfer_intent(&rpc_url, pk0, &intents_addr, to, "9999", 1)?;
    wait_for_pool_current_intents_count(&db_url, 3, Duration::from_secs(45)).await?;

    let intent_ids = fetch_current_intents(&db_url)
        .await?
        .into_iter()
        .map(|r| r.id)
        .collect::<Vec<_>>();
    if intent_ids.len() != 3 {
        anyhow::bail!("expected three intents, got {}", intent_ids.len());
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

    // Start solver with a tight per-type claim rate limit.
    let _solver = KillOnDrop::new(spawn_solver_mock_custom(
        &db_url,
        &postgrest_url,
        &rpc_url,
        &intents_addr,
        pk0,
        &mock_reader,
        "solver-rate-limit",
        "trx_transfer",
        &[("SOLVER_RATE_LIMIT_CLAIMS_PER_MINUTE_TRX_TRANSFER", "1")],
    )?);

    wait_for_solver_table(&db_url, "jobs", Duration::from_secs(30)).await?;
    wait_for_rate_limit_effect(&db_url, &intent_ids, Duration::from_secs(30)).await?;

    Ok(())
}
