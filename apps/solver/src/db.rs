use anyhow::{Context, Result};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use std::time::Duration;

const MIGRATIONS: &[(i32, &str)] = &[
    (1, include_str!("../db/migrations/0001_schema.sql")),
    (2, include_str!("../db/migrations/0002_jobs.sql")),
    (3, include_str!("../db/migrations/0003_tron_proofs.sql")),
];

#[derive(Debug, Clone)]
pub struct SolverJob {
    pub job_id: i64,
    pub intent_id: [u8; 32],
    pub intent_type: i16,
    pub intent_specs: Vec<u8>,
    pub deadline: i64,
    pub state: String,
    pub attempts: i32,
    pub tron_txid: Option<[u8; 32]>,
}

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
        // Prevent concurrent migrations when multiple solver processes start at once.
        // Even `create schema if not exists` can race at the catalog level.
        const MIGRATION_LOCK_KEY: i64 = 0x554E_5452_4F4E_534C; // "UNTRONSL"
        sqlx::query("select pg_advisory_lock($1)")
            .bind(MIGRATION_LOCK_KEY)
            .execute(&self.pool)
            .await
            .context("acquire solver migration lock")?;

        let res: Result<()> = async {
            // Ensure schema and migration table exist before trying to read them.
            exec_sql_batch_pool(&self.pool, MIGRATIONS[0].1)
                .await
                .context("apply solver schema bootstrap (v1)")?;

            for (version, sql) in MIGRATIONS {
                if *version == 1 {
                    continue;
                }
                let applied: Option<i32> = sqlx::query_scalar(
                    "select version from solver.schema_migrations where version = $1",
                )
                .bind(*version)
                .fetch_optional(&self.pool)
                .await
                .context("read solver.schema_migrations")?;

                if applied.is_some() {
                    continue;
                }

                let mut tx = self.pool.begin().await.context("begin migration tx")?;
                exec_sql_batch_tx(&mut tx, sql)
                    .await
                    .with_context(|| format!("apply solver migration v{version}"))?;
                sqlx::query("insert into solver.schema_migrations(version) values ($1)")
                    .bind(*version)
                    .execute(&mut *tx)
                    .await
                    .context("insert solver.schema_migrations")?;
                tx.commit().await.context("commit migration tx")?;
            }
            Ok(())
        }
        .await;

        // Best-effort unlock.
        let _ = sqlx::query("select pg_advisory_unlock($1)")
            .bind(MIGRATION_LOCK_KEY)
            .execute(&self.pool)
            .await;

        res
    }

    pub async fn insert_job_if_new(
        &self,
        intent_id: [u8; 32],
        intent_type: i16,
        intent_specs: &[u8],
        deadline: i64,
    ) -> Result<()> {
        sqlx::query(
            "insert into solver.jobs(intent_id, intent_type, intent_specs, deadline, state) \
             values ($1, $2, $3, $4, 'ready') \
             on conflict (intent_id) do nothing",
        )
        .bind(intent_id.to_vec())
        .bind(intent_type)
        .bind(intent_specs)
        .bind(deadline)
        .execute(&self.pool)
        .await
        .context("insert solver.jobs")?;
        Ok(())
    }

    pub async fn lease_jobs(
        &self,
        leased_by: &str,
        lease_for: Duration,
        limit: i64,
    ) -> Result<Vec<SolverJob>> {
        let secs: i64 = lease_for.as_secs().try_into().unwrap_or(60);
        let rows = sqlx::query(
            "with cte as ( \
                select job_id \
                from solver.jobs \
                where \
                    state in ('ready', 'claimed', 'tron_sent', 'proof_built') \
                    and next_retry_at <= now() \
                    and (lease_until is null or lease_until < now()) \
                order by job_id asc \
                limit $1 \
                for update skip locked \
            ) \
            update solver.jobs j set \
                leased_by = $2, \
                lease_until = now() + make_interval(secs => $3), \
                updated_at = now() \
            from cte \
            where j.job_id = cte.job_id \
            returning j.job_id, j.intent_id, j.intent_type, j.intent_specs, j.deadline, j.state, j.attempts, j.tron_txid",
        )
        .bind(limit)
        .bind(leased_by)
        .bind(secs)
        .fetch_all(&self.pool)
        .await
        .context("lease solver.jobs")?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let job_id: i64 = row.try_get("job_id")?;
            let intent_id: Vec<u8> = row.try_get("intent_id")?;
            let tron_txid: Option<Vec<u8>> = row.try_get("tron_txid")?;
            let mut iid = [0u8; 32];
            iid.copy_from_slice(&intent_id);
            let tron_txid = tron_txid.and_then(|v| {
                if v.len() != 32 {
                    return None;
                }
                let mut out = [0u8; 32];
                out.copy_from_slice(&v);
                Some(out)
            });
            out.push(SolverJob {
                job_id,
                intent_id: iid,
                intent_type: row.try_get("intent_type")?,
                intent_specs: row.try_get("intent_specs")?,
                deadline: row.try_get("deadline")?,
                state: row.try_get("state")?,
                attempts: row.try_get("attempts")?,
                tron_txid,
            });
        }
        Ok(out)
    }

    pub async fn update_job_state(&self, job_id: i64, leased_by: &str, state: &str) -> Result<()> {
        let n = sqlx::query(
            "update solver.jobs set state = $1, updated_at = now() \
             where job_id = $2 and leased_by = $3 and lease_until >= now()",
        )
        .bind(state)
        .bind(job_id)
        .bind(leased_by)
        .execute(&self.pool)
        .await
        .context("update solver.jobs state")?
        .rows_affected();
        if n != 1 {
            anyhow::bail!("lost job lease for job_id={job_id}");
        }
        Ok(())
    }

    pub async fn record_claim(
        &self,
        job_id: i64,
        leased_by: &str,
        claim_tx_hash: [u8; 32],
    ) -> Result<()> {
        let n = sqlx::query(
            "update solver.jobs set state='claimed', claim_tx_hash=$1, updated_at=now() \
             where job_id=$2 and leased_by=$3 and lease_until >= now()",
        )
        .bind(claim_tx_hash.to_vec())
        .bind(job_id)
        .bind(leased_by)
        .execute(&self.pool)
        .await
        .context("record claim")?
        .rows_affected();
        if n != 1 {
            anyhow::bail!("lost job lease for job_id={job_id}");
        }
        Ok(())
    }

    pub async fn record_tron_txid(
        &self,
        job_id: i64,
        leased_by: &str,
        tron_txid: [u8; 32],
    ) -> Result<()> {
        let n = sqlx::query(
            "update solver.jobs set state='tron_sent', tron_txid=$1, updated_at=now() \
             where job_id=$2 and leased_by=$3 and lease_until >= now()",
        )
        .bind(tron_txid.to_vec())
        .bind(job_id)
        .bind(leased_by)
        .execute(&self.pool)
        .await
        .context("record tron txid")?
        .rows_affected();
        if n != 1 {
            anyhow::bail!("lost job lease for job_id={job_id}");
        }
        Ok(())
    }

    pub async fn record_proof_built(&self, job_id: i64, leased_by: &str) -> Result<()> {
        self.update_job_state(job_id, leased_by, "proof_built")
            .await
    }

    pub async fn record_prove(
        &self,
        job_id: i64,
        leased_by: &str,
        prove_tx_hash: [u8; 32],
    ) -> Result<()> {
        let n = sqlx::query(
            "update solver.jobs set state='done', prove_tx_hash=$1, updated_at=now() \
             where job_id=$2 and leased_by=$3 and lease_until >= now()",
        )
        .bind(prove_tx_hash.to_vec())
        .bind(job_id)
        .bind(leased_by)
        .execute(&self.pool)
        .await
        .context("record prove")?
        .rows_affected();
        if n != 1 {
            anyhow::bail!("lost job lease for job_id={job_id}");
        }
        Ok(())
    }

    pub async fn record_retryable_error(
        &self,
        job_id: i64,
        leased_by: &str,
        err: &str,
        next_retry_in: Duration,
    ) -> Result<()> {
        let secs: i64 = next_retry_in.as_secs().try_into().unwrap_or(1);
        let n = sqlx::query(
            "update solver.jobs set \
                attempts = attempts + 1, \
                last_error = $1, \
                next_retry_at = now() + make_interval(secs => $2), \
                lease_until = now(), \
                updated_at = now() \
             where job_id=$3 and leased_by=$4",
        )
        .bind(err)
        .bind(secs)
        .bind(job_id)
        .bind(leased_by)
        .execute(&self.pool)
        .await
        .context("record retryable error")?
        .rows_affected();
        if n != 1 {
            anyhow::bail!("lost job lease for job_id={job_id}");
        }
        Ok(())
    }

    pub async fn record_fatal_error(&self, job_id: i64, leased_by: &str, err: &str) -> Result<()> {
        let n = sqlx::query(
            "update solver.jobs set \
                state = 'failed_fatal', \
                last_error = $1, \
                lease_until = now(), \
                updated_at = now() \
             where job_id=$2 and leased_by=$3",
        )
        .bind(err)
        .bind(job_id)
        .bind(leased_by)
        .execute(&self.pool)
        .await
        .context("record fatal error")?
        .rows_affected();
        if n != 1 {
            anyhow::bail!("lost job lease for job_id={job_id}");
        }
        Ok(())
    }

    pub async fn save_tron_proof(&self, txid: [u8; 32], proof: &TronProofRow) -> Result<()> {
        sqlx::query(
            "insert into solver.tron_proofs(txid, blocks, encoded_tx, proof, index_dec) \
             values ($1, $2, $3, $4, $5) \
             on conflict (txid) do update set \
               blocks = excluded.blocks, \
               encoded_tx = excluded.encoded_tx, \
               proof = excluded.proof, \
               index_dec = excluded.index_dec",
        )
        .bind(txid.to_vec())
        .bind(&proof.blocks)
        .bind(&proof.encoded_tx)
        .bind(&proof.proof)
        .bind(&proof.index_dec)
        .execute(&self.pool)
        .await
        .context("save solver.tron_proofs")?;
        Ok(())
    }

    pub async fn load_tron_proof(&self, txid: [u8; 32]) -> Result<TronProofRow> {
        let row = sqlx::query(
            "select blocks, encoded_tx, proof, index_dec from solver.tron_proofs where txid = $1",
        )
        .bind(txid.to_vec())
        .fetch_one(&self.pool)
        .await
        .context("load solver.tron_proofs")?;
        Ok(TronProofRow {
            blocks: row.try_get("blocks")?,
            encoded_tx: row.try_get("encoded_tx")?,
            proof: row.try_get("proof")?,
            index_dec: row.try_get("index_dec")?,
        })
    }
}

async fn exec_sql_batch_pool(pool: &PgPool, sql: &str) -> Result<()> {
    for stmt in sql.split(';') {
        let s = stmt.trim();
        if s.is_empty() {
            continue;
        }
        sqlx::query(s).execute(pool).await.with_context(|| {
            format!(
                "execute migration statement: {}",
                s.lines().next().unwrap_or("")
            )
        })?;
    }
    Ok(())
}

async fn exec_sql_batch_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    sql: &str,
) -> Result<()> {
    for stmt in sql.split(';') {
        let s = stmt.trim();
        if s.is_empty() {
            continue;
        }
        sqlx::query(s).execute(&mut **tx).await.with_context(|| {
            format!(
                "execute migration statement: {}",
                s.lines().next().unwrap_or("")
            )
        })?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct TronProofRow {
    pub blocks: Vec<Vec<u8>>,
    pub encoded_tx: Vec<u8>,
    pub proof: Vec<Vec<u8>>,
    pub index_dec: String,
}
