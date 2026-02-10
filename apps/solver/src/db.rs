use alloy::primitives::{Address, U256};
use alloy::rpc::types::eth::erc4337::PackedUserOperation;
use anyhow::{Context, Result};
use sqlx::{Acquire, Executor, PgPool, Postgres, Row, postgres::PgPoolOptions};
use std::time::Duration;

mod breakers;
mod hub_userops;
mod intents;
mod jobs;
mod migrations;
mod proofs;
mod tron;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SolverJob {
    pub job_id: i64,
    pub intent_id: [u8; 32],
    pub intent_type: i16,
    pub intent_specs: Vec<u8>,
    pub deadline: i64,
    pub claim_window_expires_at_unix: Option<i64>,
    pub state: String,
    pub attempts: i32,
    pub tron_txid: Option<[u8; 32]>,
}

#[derive(Clone)]
pub struct SolverDb {
    pool: PgPool,
}

#[derive(Debug, Clone)]
pub struct TronTxCostsRow {
    pub fee_sun: Option<i64>,
    pub energy_usage_total: Option<i64>,
    pub net_usage: Option<i64>,
    pub energy_fee_sun: Option<i64>,
    pub net_fee_sun: Option<i64>,
    pub block_number: Option<i64>,
    pub block_timestamp: Option<i64>,
    pub result_code: Option<i32>,
    pub result_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TronSignedTxRow {
    pub step: String,
    pub txid: [u8; 32],
    pub tx_bytes: Vec<u8>,
    pub fee_limit_sun: Option<i64>,
    pub energy_required: Option<i64>,
    pub tx_size_bytes: Option<i64>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TronRentalRow {
    pub provider: String,
    pub resource: String,
    pub receiver_evm: [u8; 20],
    pub balance_sun: i64,
    pub lock_period: i64,
    pub order_id: Option<String>,
    pub txid: Option<[u8; 32]>,
    pub request_json: Option<serde_json::Value>,
    pub response_json: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct IntentSkipSummaryRow {
    pub reason: String,
    pub intent_type: Option<i16>,
    pub skips: i64,
    pub last_seen_unix: i64,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct IntentEmulationRow {
    pub ok: bool,
    pub reason: Option<String>,
    pub contract: Option<Vec<u8>>,
    pub selector: Option<Vec<u8>>,
    pub checked_at_unix: Option<i64>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DelegateReservationRow {
    pub owner_address: Vec<u8>,
    pub resource: i16,
    pub amount_sun: i64,
    pub expires_in_secs: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubUserOpKind {
    Claim,
    Prove,
}

impl HubUserOpKind {
    pub fn as_str(self) -> &'static str {
        match self {
            HubUserOpKind::Claim => "claim",
            HubUserOpKind::Prove => "prove",
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct HubUserOpRow {
    pub userop_id: i64,
    pub state: String,
    pub userop_json: String,
    pub userop_hash: Option<String>,
    pub tx_hash: Option<[u8; 32]>,
    pub block_number: Option<i64>,
    pub success: Option<bool>,
    pub receipt_json: Option<String>,
    pub attempts: i32,
}

#[derive(Debug, Clone)]
pub struct TronProofRow {
    pub blocks: Vec<Vec<u8>>,
    pub encoded_tx: Vec<u8>,
    pub proof: Vec<Vec<u8>>,
    pub index_dec: String,
}

impl SolverDb {
    pub async fn connect(db_url: &str, max_connections: u32) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .connect(db_url)
            .await
            .context("connect SOLVER_DB_URL")?;
        Ok(Self { pool })
    }
}
