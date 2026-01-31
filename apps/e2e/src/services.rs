use crate::process::null_stdio;
use crate::util::repo_root;
use anyhow::{Context, Result};
use std::process::{Child, Command, Stdio};

pub fn spawn_indexer(
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
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    null_stdio(&mut cmd);
    if let Some(json) = forwarders_chains_json {
        cmd.env("FORWARDERS_CHAINS", json);
    }
    cmd.spawn().context("spawn indexer")
}

pub fn spawn_solver_mock(
    db_url: &str,
    postgrest_url: &str,
    rpc_url: &str,
    pool_contract: &str,
    solver_private_key: &str,
    mock_reader: &str,
) -> Result<Child> {
    let root = repo_root();
    let mut cmd = Command::new(root.join("target/debug/solver"));
    cmd.current_dir(&root)
        .env("SOLVER_DB_URL", db_url)
        .env("INDEXER_API_BASE_URL", postgrest_url)
        .env("HUB_RPC_URL", rpc_url)
        .env("HUB_POOL_ADDRESS", pool_contract)
        .env("HUB_TX_MODE", "eoa")
        .env("HUB_SIGNER_PRIVATE_KEY_HEX", solver_private_key)
        .env("TRON_MODE", "mock")
        .env("TRON_MOCK_READER_ADDRESS", mock_reader)
        .env(
            "SOLVER_ENABLED_INTENT_TYPES",
            "trx_transfer,delegate_resource,usdt_transfer",
        )
        .env("SOLVER_TICK_INTERVAL_SECS", "1")
        .env("RUST_LOG", "info")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    null_stdio(&mut cmd);
    cmd.spawn().context("spawn solver (mock)")
}

pub fn spawn_solver_tron_grpc(
    db_url: &str,
    postgrest_url: &str,
    rpc_url: &str,
    pool_contract: &str,
    hub_solver_private_key: &str,
    tron_grpc_url: &str,
    tron_private_key_hex: &str,
    tron_controller_address: &str,
) -> Result<Child> {
    let root = repo_root();
    let mut cmd = Command::new(root.join("target/debug/solver"));
    cmd.current_dir(&root)
        .env("SOLVER_DB_URL", db_url)
        .env("INDEXER_API_BASE_URL", postgrest_url)
        .env("HUB_RPC_URL", rpc_url)
        .env("HUB_POOL_ADDRESS", pool_contract)
        .env("HUB_TX_MODE", "eoa")
        .env("HUB_SIGNER_PRIVATE_KEY_HEX", hub_solver_private_key)
        .env("TRON_MODE", "grpc")
        .env("TRON_GRPC_URL", tron_grpc_url)
        .env("TRON_PRIVATE_KEY_HEX", tron_private_key_hex)
        .env("TRON_CONTROLLER_ADDRESS", tron_controller_address)
        .env("SOLVER_TICK_INTERVAL_SECS", "1")
        .env("RUST_LOG", "info")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    null_stdio(&mut cmd);
    cmd.spawn().context("spawn solver (tron grpc)")
}
