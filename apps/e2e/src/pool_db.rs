use anyhow::Result;
use sqlx::{Connection, PgConnection, Row};
use std::collections::HashSet;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct CurrentIntentRow {
    pub creator: String,
    pub intent_type: i16,
    pub escrow_token: String,
    pub escrow_amount: String,
    pub refund_beneficiary: String,
    pub deadline: i64,
    pub intent_specs: String,
    pub solver: Option<String>,
    pub solver_claimed_at: Option<i64>,
    pub tron_tx_id: Option<String>,
    pub tron_block_number: Option<i64>,
    pub solved: bool,
    pub funded: bool,
    pub settled: bool,
    pub closed: bool,
}

#[derive(Debug, Clone)]
pub struct CurrentIntentRowWithId {
    pub id: String,
    pub row: CurrentIntentRow,
}

pub async fn fetch_current_intents(db_url: &str) -> Result<Vec<CurrentIntentRowWithId>> {
    let mut conn = PgConnection::connect(db_url).await?;
    let rows = sqlx::query(
        "select \
           id as id_hex, \
           lower(creator) as creator, \
           intent_type, \
           lower(escrow_token) as escrow_token, \
           escrow_amount::text as escrow_amount, \
           lower(refund_beneficiary) as refund_beneficiary, \
           deadline, \
           intent_specs, \
           lower(solver) as solver, \
           solver_claimed_at, \
           tron_tx_id, \
           tron_block_number, \
           solved, funded, settled, closed \
         from pool.intent_versions \
         where valid_to_seq is null \
         order by valid_from_seq asc",
    )
    .fetch_all(&mut conn)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let id: String = r.get("id_hex");
        out.push(CurrentIntentRowWithId {
            id,
            row: CurrentIntentRow {
                creator: r.get::<String, _>("creator"),
                intent_type: r.get::<i16, _>("intent_type"),
                escrow_token: r.get::<String, _>("escrow_token"),
                escrow_amount: r.get::<String, _>("escrow_amount"),
                refund_beneficiary: r.get::<String, _>("refund_beneficiary"),
                deadline: r.get::<i64, _>("deadline"),
                intent_specs: r.get::<String, _>("intent_specs"),
                solver: r.get::<Option<String>, _>("solver"),
                solver_claimed_at: r.get::<Option<i64>, _>("solver_claimed_at"),
                tron_tx_id: r.get::<Option<String>, _>("tron_tx_id"),
                tron_block_number: r.get::<Option<i64>, _>("tron_block_number"),
                solved: r.get::<bool, _>("solved"),
                funded: r.get::<bool, _>("funded"),
                settled: r.get::<bool, _>("settled"),
                closed: r.get::<bool, _>("closed"),
            },
        });
    }
    Ok(out)
}

pub async fn fetch_pool_current_intents_count(db_url: &str) -> Result<i64> {
    let mut conn = PgConnection::connect(db_url).await?;
    let row = sqlx::query(
        "select count(*)::bigint as c \
         from pool.intent_versions \
         where valid_to_seq is null",
    )
    .fetch_one(&mut conn)
    .await?;
    Ok(row.get::<i64, _>("c"))
}

pub async fn wait_for_pool_current_intents_count(
    db_url: &str,
    expected_count: i64,
    timeout: Duration,
) -> Result<()> {
    let start = Instant::now();
    loop {
        let c = fetch_pool_current_intents_count(db_url).await?;
        if c == expected_count {
            return Ok(());
        }
        if start.elapsed() > timeout {
            anyhow::bail!("timed out waiting for pool intents count {expected_count}, got {c}");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

pub async fn wait_for_current_intent_match(
    db_url: &str,
    expected: &CurrentIntentRow,
    timeout: Duration,
) -> Result<()> {
    let start = Instant::now();
    loop {
        let rows = fetch_current_intents(db_url).await?;
        if let Some(last) = rows.last() {
            if &last.row == expected {
                return Ok(());
            }
        }
        if start.elapsed() > timeout {
            anyhow::bail!("timed out waiting for expected intent row; expected={expected:?}");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

pub async fn wait_for_intents_solved_and_settled(
    db_url: &str,
    expected_count: usize,
    timeout: Duration,
) -> Result<Vec<CurrentIntentRowWithId>> {
    let start = Instant::now();
    loop {
        let rows = fetch_current_intents(db_url).await?;
        if rows.len() == expected_count
            && rows.iter().all(|r| r.row.solver.is_some())
            && rows.iter().all(|r| r.row.solved)
            && rows.iter().all(|r| r.row.funded)
            && rows.iter().all(|r| r.row.settled)
            && rows.iter().all(|r| r.row.tron_tx_id.is_some())
            && rows.iter().all(|r| r.row.tron_block_number.is_some())
        {
            return Ok(rows);
        }
        if start.elapsed() > timeout {
            anyhow::bail!("timed out waiting for intents to be solved+settled; rows={rows:?}");
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

pub async fn assert_multi_intent_ordering(db_url: &str, expected_count: usize) -> Result<()> {
    let mut conn = PgConnection::connect(db_url).await?;
    let rows = sqlx::query(
        "select id, valid_from_seq, escrow_amount::text as escrow_amount \
         from pool.intent_versions \
         where valid_to_seq is null \
         order by valid_from_seq asc",
    )
    .fetch_all(&mut conn)
    .await?;

    if rows.len() != expected_count {
        anyhow::bail!(
            "expected {expected_count} current intents, got {}",
            rows.len()
        );
    }

    let mut ids = HashSet::new();
    let mut prev_seq: Option<i64> = None;
    for r in rows {
        let id: String = r.get("id");
        let seq: i64 = r.get("valid_from_seq");
        if !ids.insert(id) {
            anyhow::bail!("expected unique intent ids, got duplicate");
        }
        if let Some(p) = prev_seq {
            if seq <= p {
                anyhow::bail!("expected increasing valid_from_seq, got {p} then {seq}");
            }
        }
        prev_seq = Some(seq);
    }
    Ok(())
}

impl PartialEq for CurrentIntentRow {
    fn eq(&self, other: &Self) -> bool {
        self.creator == other.creator
            && self.intent_type == other.intent_type
            && self.escrow_token == other.escrow_token
            && self.escrow_amount == other.escrow_amount
            && self.refund_beneficiary == other.refund_beneficiary
            && self.deadline == other.deadline
            && self.intent_specs == other.intent_specs
            && self.solver == other.solver
            && self.solver_claimed_at == other.solver_claimed_at
            && self.tron_tx_id == other.tron_tx_id
            && self.tron_block_number == other.tron_block_number
            && self.solved == other.solved
            && self.funded == other.funded
            && self.settled == other.settled
            && self.closed == other.closed
    }
}
