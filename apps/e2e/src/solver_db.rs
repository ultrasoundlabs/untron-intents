use anyhow::{Context, Result};
use sqlx::{PgPool, Row};

#[derive(Debug, Clone)]
pub struct SolverJobRow {
    pub job_id: i64,
    pub intent_id: String,
    pub state: String,
    pub attempts: i32,
    pub next_retry_at: String,
    pub last_error: Option<String>,
    pub leased_by: Option<String>,
    pub claim_tx_hash: Option<String>,
    pub prove_tx_hash: Option<String>,
    pub tron_txid: Option<String>,
}

pub async fn fetch_job_by_intent_id(db_url: &str, intent_id_hex: &str) -> Result<SolverJobRow> {
    let pool = PgPool::connect(db_url).await.context("connect db")?;
    let bytes = hex::decode(intent_id_hex.trim_start_matches("0x")).context("decode intent_id")?;
    let row = sqlx::query(
        "select job_id, intent_id, state, attempts, next_retry_at::text as next_retry_at, last_error, \
                leased_by, claim_tx_hash, prove_tx_hash, tron_txid \
         from solver.jobs where intent_id = $1",
    )
    .bind(bytes)
    .fetch_one(&pool)
    .await
    .context("select solver.jobs")?;

    let intent_id: Vec<u8> = row.try_get("intent_id")?;
    let claim: Option<Vec<u8>> = row.try_get("claim_tx_hash")?;
    let prove: Option<Vec<u8>> = row.try_get("prove_tx_hash")?;
    let tron_txid: Option<Vec<u8>> = row.try_get("tron_txid")?;

    Ok(SolverJobRow {
        job_id: row.try_get("job_id")?,
        intent_id: format!("0x{}", hex::encode(intent_id)),
        state: row.try_get("state")?,
        attempts: row.try_get("attempts")?,
        next_retry_at: row.try_get("next_retry_at")?,
        last_error: row.try_get("last_error")?,
        leased_by: row.try_get("leased_by")?,
        claim_tx_hash: claim.map(|v| format!("0x{}", hex::encode(v))),
        prove_tx_hash: prove.map(|v| format!("0x{}", hex::encode(v))),
        tron_txid: tron_txid.map(|v| format!("0x{}", hex::encode(v))),
    })
}

#[derive(Debug, Clone)]
pub struct HubUserOpRow {
    pub kind: String,
    pub state: String,
    pub userop_hash: Option<String>,
    pub tx_hash: Option<String>,
    pub block_number: Option<i64>,
    pub success: Option<bool>,
    pub receipt_json: Option<String>,
}

pub async fn fetch_hub_userop(db_url: &str, job_id: i64, kind: &str) -> Result<HubUserOpRow> {
    let pool = PgPool::connect(db_url).await.context("connect db")?;
    let row = sqlx::query(
        "select kind::text as kind, state::text as state, userop_hash, tx_hash, block_number, success, receipt::text as receipt_json \
         from solver.hub_userops where job_id = $1 and kind = $2",
    )
    .bind(job_id)
    .bind(kind)
    .fetch_one(&pool)
    .await
    .context("select solver.hub_userops")?;

    let tx_hash: Option<Vec<u8>> = row.try_get("tx_hash")?;
    let tx_hash = tx_hash.map(|v| format!("0x{}", hex::encode(v)));

    Ok(HubUserOpRow {
        kind: row.try_get("kind")?,
        state: row.try_get("state")?,
        userop_hash: row.try_get("userop_hash")?,
        tx_hash,
        block_number: row.try_get("block_number")?,
        success: row.try_get("success")?,
        receipt_json: row.try_get("receipt_json")?,
    })
}
