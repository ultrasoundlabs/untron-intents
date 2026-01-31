use anyhow::{Context, Result};
use e2e::{
    anvil::spawn_anvil,
    binaries::{cargo_build_indexer_bins, run_migrations},
    docker::{PostgresOptions, start_postgres},
    forge::{run_forge_build, run_forge_create_intents_forwarder, run_forge_create_untron_intents},
    postgres::wait_for_postgres,
    process::KillOnDrop,
    services::spawn_indexer,
    util::{find_free_port, require_bins},
};
use sqlx::Row;
use sqlx::{Connection, PgConnection};
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_forwarder_stream_ingests_bridgers_set() -> Result<()> {
    if !require_bins(&["docker", "anvil", "forge", "cast"]) {
        return Ok(());
    }

    let pg = start_postgres(PostgresOptions::default()).await?;
    let db_url = pg.db_url.clone();
    wait_for_postgres(&db_url, Duration::from_secs(30)).await?;

    cargo_build_indexer_bins()?;
    run_migrations(&db_url, true)?;

    // Anvil.
    let anvil_port = find_free_port()?;
    let rpc_url = format!("http://127.0.0.1:{anvil_port}");
    let _anvil = KillOnDrop::new(spawn_anvil(anvil_port)?);
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Deploy contracts.
    run_forge_build()?;
    let pk0 = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    let owner0 = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
    let pool = run_forge_create_untron_intents(&rpc_url, pk0, owner0)?;
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
        &pool,
        "forwarder",
        Some(&forwarders_chains),
    )?);

    // Emit BridgersSet.
    let bridger_a = "0x1111111111111111111111111111111111111111";
    let bridger_b = "0x2222222222222222222222222222222222222222";
    let status = std::process::Command::new("cast")
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
        .current_dir(e2e::util::repo_root())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .context("cast send setBridgers")?;
    if !status.success() {
        anyhow::bail!("cast send setBridgers failed");
    }

    // Wait for projection row.
    let mut conn = PgConnection::connect(&db_url).await?;
    let start = std::time::Instant::now();
    loop {
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

        if let Some(row) = row {
            let usdt: String = row.get("usdt_bridger");
            let usdc: String = row.get("usdc_bridger");
            if usdt == bridger_a.to_ascii_lowercase() && usdc == bridger_b.to_ascii_lowercase() {
                break;
            }
        }

        if start.elapsed() > Duration::from_secs(60) {
            anyhow::bail!(
                "timed out waiting for forwarder.bridgers_versions to reflect BridgersSet"
            );
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    Ok(())
}
