use anyhow::{Context, Result};
use e2e::{
    anvil::spawn_anvil,
    binaries::{cargo_build_indexer_bins, run_migrations},
    cast::run_cast_create_intent,
    docker::{PostgresOptions, PostgrestOptions, start_postgres, start_postgrest},
    docker_cleanup::cleanup_untron_e2e_containers,
    forge::{run_forge_build, run_forge_create_untron_intents},
    http::{http_get_json, wait_for_http_ok},
    pool_db::{assert_multi_intent_ordering, wait_for_pool_current_intents_count},
    postgres::{configure_postgrest_roles, wait_for_postgres},
    process::KillOnDrop,
    services::spawn_indexer,
    util::{find_free_port, require_bins},
};
use serde_json::Value;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_postgrest_pool_intents_smoke() -> Result<()> {
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
    run_migrations(&db_url, true)?;

    // PostgREST roles.
    let pgrst_pw = "pgrst_auth_pw";
    configure_postgrest_roles(&db_url, pgrst_pw).await?;

    // Anvil + pool + indexer.
    let anvil_port = find_free_port()?;
    let rpc_url = format!("http://127.0.0.1:{anvil_port}");
    let mut anvil = KillOnDrop::new(spawn_anvil(anvil_port)?);
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

    // Reduce DB load during PostgREST startup.
    indexer.kill_now();
    anvil.kill_now();

    let pgrst = start_postgrest(PostgrestOptions {
        network,
        container_name: Some(format!("untron-e2e-pgrst-{}", find_free_port()?)),
        db_uri: format!("postgres://pgrst_authenticator:{pgrst_pw}@{pg_name}:5432/untron"),
        ..Default::default()
    })
    .await?;
    let postgrest_url = pgrst.base_url.clone();
    wait_for_http_ok(&format!("{postgrest_url}/health"), Duration::from_secs(30)).await?;

    let url = format!("{postgrest_url}/pool_intents?order=valid_from_seq.desc&limit=1");
    let json = http_get_json(&url).await.context("GET pool_intents")?;
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
    if !require_bins(&["docker", "anvil", "forge", "cast"]) {
        return Ok(());
    }

    cleanup_untron_e2e_containers().ok();

    let pg = start_postgres(PostgresOptions {
        container_name: Some(format!("untron-e2e-pg-{}", find_free_port()?)),
        ..Default::default()
    })
    .await?;
    let db_url = pg.db_url.clone();
    wait_for_postgres(&db_url, Duration::from_secs(30)).await?;

    cargo_build_indexer_bins()?;
    run_migrations(&db_url, true)?;

    // Anvil + pool.
    let anvil_port = find_free_port()?;
    let rpc_url = format!("http://127.0.0.1:{anvil_port}");
    let _anvil = KillOnDrop::new(spawn_anvil(anvil_port)?);
    tokio::time::sleep(Duration::from_millis(500)).await;

    run_forge_build()?;
    let pk0 = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    let owner0 = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
    let intents_addr = run_forge_create_untron_intents(&rpc_url, pk0, owner0)?;

    let _indexer = KillOnDrop::new(spawn_indexer(
        &db_url,
        &rpc_url,
        &intents_addr,
        "pool",
        None,
    )?);

    let _ = run_cast_create_intent(&rpc_url, pk0, &intents_addr, 1)?;
    let _ = run_cast_create_intent(&rpc_url, pk0, &intents_addr, 2)?;
    let _ = run_cast_create_intent(&rpc_url, pk0, &intents_addr, 3)?;

    wait_for_pool_current_intents_count(&db_url, 3, Duration::from_secs(30)).await?;
    assert_multi_intent_ordering(&db_url, 3).await?;

    Ok(())
}
