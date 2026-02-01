use anyhow::{Context, Result};
use sqlx::{PgPool, Row};

#[derive(Debug, Clone)]
pub struct SolverJobRow {
    pub intent_id: String,
    pub state: String,
    pub leased_by: Option<String>,
    pub claim_tx_hash: Option<String>,
    pub prove_tx_hash: Option<String>,
}

pub async fn fetch_job_by_intent_id(db_url: &str, intent_id_hex: &str) -> Result<SolverJobRow> {
    let pool = PgPool::connect(db_url).await.context("connect db")?;
    let bytes = hex::decode(intent_id_hex.trim_start_matches("0x")).context("decode intent_id")?;
    let row = sqlx::query(
        "select intent_id, state, leased_by, claim_tx_hash, prove_tx_hash \
         from solver.jobs where intent_id = $1",
    )
    .bind(bytes)
    .fetch_one(&pool)
    .await
    .context("select solver.jobs")?;

    let intent_id: Vec<u8> = row.try_get("intent_id")?;
    let claim: Option<Vec<u8>> = row.try_get("claim_tx_hash")?;
    let prove: Option<Vec<u8>> = row.try_get("prove_tx_hash")?;

    Ok(SolverJobRow {
        intent_id: format!("0x{}", hex::encode(intent_id)),
        state: row.try_get("state")?,
        leased_by: row.try_get("leased_by")?,
        claim_tx_hash: claim.map(|v| format!("0x{}", hex::encode(v))),
        prove_tx_hash: prove.map(|v| format!("0x{}", hex::encode(v))),
    })
}
