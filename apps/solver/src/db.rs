use anyhow::{Context, Result};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};

pub struct SolverDb {
    pool: PgPool,
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

    pub async fn migrate(&self) -> Result<()> {
        // The monorepo uses a single Postgres database for multiple components.
        // Indexer migrations already occupy `sqlx`'s default `_sqlx_migrations` table,
        // so we intentionally do *not* use sqlx's migrator here to avoid version collisions.
        //
        // Instead, keep solver's DB init idempotent.
        sqlx::query("create schema if not exists solver;")
            .execute(&self.pool)
            .await
            .context("create schema solver")?;
        sqlx::query(
            "create table if not exists solver.intent_runs ( \
                intent_id bytea primary key, \
                state text not null, \
                claim_tx_hash bytea, \
                prove_tx_hash bytea, \
                tron_txid bytea, \
                last_error text, \
                updated_at timestamptz not null default now(), \
                created_at timestamptz not null default now() \
            );",
        )
        .execute(&self.pool)
        .await
        .context("create table solver.intent_runs")?;
        sqlx::query(
            "create index if not exists intent_runs_state_idx on solver.intent_runs(state);",
        )
        .execute(&self.pool)
        .await
        .context("create index solver.intent_runs(state)")?;

        // Backfill columns for existing tables (create table above won't add columns).
        sqlx::query("alter table solver.intent_runs add column if not exists tron_txid bytea;")
            .execute(&self.pool)
            .await
            .context("alter table solver.intent_runs add tron_txid")?;
        Ok(())
    }

    pub async fn upsert_run_state(
        &self,
        intent_id: [u8; 32],
        state: &str,
        claim_tx_hash: Option<[u8; 32]>,
        prove_tx_hash: Option<[u8; 32]>,
        tron_txid: Option<[u8; 32]>,
        last_error: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "insert into solver.intent_runs(intent_id, state, claim_tx_hash, prove_tx_hash, tron_txid, last_error) \
             values ($1, $2, $3, $4, $5, $6) \
             on conflict (intent_id) do update set \
               state = excluded.state, \
               claim_tx_hash = coalesce(excluded.claim_tx_hash, solver.intent_runs.claim_tx_hash), \
               prove_tx_hash = coalesce(excluded.prove_tx_hash, solver.intent_runs.prove_tx_hash), \
               tron_txid = coalesce(excluded.tron_txid, solver.intent_runs.tron_txid), \
               last_error = excluded.last_error, \
               updated_at = now()",
        )
        .bind(intent_id.to_vec())
        .bind(state)
        .bind(claim_tx_hash.map(|h| h.to_vec()))
        .bind(prove_tx_hash.map(|h| h.to_vec()))
        .bind(tron_txid.map(|h| h.to_vec()))
        .bind(last_error)
        .execute(&self.pool)
        .await
        .context("upsert solver.intent_runs")?;
        Ok(())
    }

    pub async fn list_pending_proofs(&self, limit: i64) -> Result<Vec<([u8; 32], [u8; 32])>> {
        let rows = sqlx::query(
            "select intent_id, tron_txid \
             from solver.intent_runs \
             where state = 'tron_sent' and tron_txid is not null \
             order by updated_at asc \
             limit $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("list solver.intent_runs pending proofs")?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let intent_id: Vec<u8> = row.try_get("intent_id")?;
            let tron_txid: Vec<u8> = row.try_get("tron_txid")?;
            if intent_id.len() != 32 || tron_txid.len() != 32 {
                continue;
            }
            let mut a = [0u8; 32];
            a.copy_from_slice(&intent_id);
            let mut b = [0u8; 32];
            b.copy_from_slice(&tron_txid);
            out.push((a, b));
        }
        Ok(out)
    }
}
