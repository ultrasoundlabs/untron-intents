use anyhow::{Context, Result};
use e2e::{
    anvil::spawn_anvil,
    binaries::{cargo_build_indexer_bins, run_migrations},
    cast::{run_cast_create_intent, run_cast_rpc},
    docker::{PostgresOptions, start_postgres},
    forge::{run_forge_build, run_forge_create_untron_intents},
    pool_db::{
        CurrentIntentRow, wait_for_current_intent_match, wait_for_pool_current_intents_count,
    },
    postgres::wait_for_postgres,
    process::KillOnDrop,
    services::spawn_indexer,
    util::{find_free_port, require_bins},
};
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_indexer_ingests_pool_intent_created() -> Result<()> {
    if !require_bins(&["docker", "anvil", "forge", "cast"]) {
        return Ok(());
    }

    let pg = start_postgres(PostgresOptions::default()).await?;
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

    let deadline = run_cast_create_intent(&rpc_url, pk0, &intents_addr, 1)?;

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
    if !require_bins(&["docker", "anvil", "forge", "cast"]) {
        return Ok(());
    }

    let pg = start_postgres(PostgresOptions::default()).await?;
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

    // Snapshot, emit an intent, then revert and emit another intent.
    let snapshot_id = run_cast_rpc(&rpc_url, "evm_snapshot", &[])?;
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

    // The original #1 should be gone and replaced by #2.
    wait_for_pool_current_intents_count(&db_url, 1, Duration::from_secs(45)).await?;
    wait_for_current_intent_match(&db_url, &expected2, Duration::from_secs(45)).await?;

    Ok(())
}
