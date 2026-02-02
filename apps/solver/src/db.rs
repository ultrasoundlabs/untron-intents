use alloy::primitives::{Address, U256};
use alloy::rpc::types::eth::erc4337::PackedUserOperation;
use anyhow::{Context, Result};
use sqlx::{Acquire, Executor, PgPool, Postgres, Row, postgres::PgPoolOptions};
use std::time::Duration;

const MIGRATIONS: &[(i32, &str)] = &[
    (1, include_str!("../db/migrations/0001_schema.sql")),
    (2, include_str!("../db/migrations/0002_jobs.sql")),
    (3, include_str!("../db/migrations/0003_tron_proofs.sql")),
    (4, include_str!("../db/migrations/0004_hub_userops.sql")),
    (
        5,
        include_str!("../db/migrations/0005_circuit_breakers.sql"),
    ),
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
pub struct HubUserOpRow {
    pub userop_id: i64,
    pub state: String,
    pub userop_json: String,
    pub userop_hash: Option<String>,
    pub tx_hash: Option<[u8; 32]>,
    pub success: Option<bool>,
    pub attempts: i32,
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
        //
        // IMPORTANT: advisory locks are per-session/connection. We must run the entire migration
        // sequence on a single connection, otherwise we might:
        // - acquire the lock on connection A
        // - run migrations on connection B
        // - "unlock" on connection C (leading to a warning and leaving the original lock held)
        const MIGRATION_LOCK_KEY: i64 = 0x554E_5452_4F4E_534C; // "UNTRONSL"
        let mut conn = self
            .pool
            .acquire()
            .await
            .context("acquire connection for solver migrations")?;

        sqlx::query("select pg_advisory_lock($1)")
            .bind(MIGRATION_LOCK_KEY)
            .execute(&mut *conn)
            .await
            .context("acquire solver migration lock")?;

        let res: Result<()> = async {
            // Ensure schema and migration table exist before trying to read them.
            exec_sql_batch(&mut *conn, MIGRATIONS[0].1)
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
                .fetch_optional(&mut *conn)
                .await
                .context("read solver.schema_migrations")?;

                if applied.is_some() {
                    continue;
                }

                let mut tx = conn.begin().await.context("begin migration tx")?;
                exec_sql_batch(&mut *tx, sql)
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

        // Best-effort unlock (same connection that acquired it).
        let _ = sqlx::query("select pg_advisory_unlock($1)")
            .bind(MIGRATION_LOCK_KEY)
            .execute(&mut *conn)
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
                    and ( \
                        (lease_until is null or lease_until < now()) \
                        or (leased_by = $2 and lease_until >= now()) \
                    ) \
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

    pub async fn get_hub_userop(
        &self,
        job_id: i64,
        kind: HubUserOpKind,
    ) -> Result<Option<HubUserOpRow>> {
        let row = sqlx::query(
            "select \
                userop_id, \
                state::text as state, \
                userop::text as userop_json, \
                userop_hash, \
                tx_hash, \
                success, \
                attempts \
             from solver.hub_userops \
             where job_id=$1 and kind::text=$2",
        )
        .bind(job_id)
        .bind(kind.as_str())
        .fetch_optional(&self.pool)
        .await
        .context("select solver.hub_userops")?;

        let Some(row) = row else {
            return Ok(None);
        };

        let tx_hash: Option<Vec<u8>> = row.try_get("tx_hash")?;
        let tx_hash = tx_hash.and_then(|v| {
            if v.len() != 32 {
                return None;
            }
            let mut out = [0u8; 32];
            out.copy_from_slice(&v);
            Some(out)
        });

        Ok(Some(HubUserOpRow {
            userop_id: row.try_get("userop_id")?,
            state: row.try_get("state")?,
            userop_json: row.try_get("userop_json")?,
            userop_hash: row.try_get("userop_hash")?,
            tx_hash,
            success: row.try_get("success")?,
            attempts: row.try_get("attempts")?,
        }))
    }

    pub async fn insert_hub_userop_prepared(
        &self,
        job_id: i64,
        leased_by: &str,
        kind: HubUserOpKind,
        userop_json: &str,
    ) -> Result<()> {
        let n = sqlx::query(
            "insert into solver.hub_userops(job_id, kind, userop, state) \
             select j.job_id, $1::solver.userop_kind, $2::jsonb, 'prepared' \
             from solver.jobs j \
             where j.job_id=$3 and j.leased_by=$4 and j.lease_until >= now() \
             on conflict (job_id, kind) do nothing",
        )
        .bind(kind.as_str())
        .bind(userop_json)
        .bind(job_id)
        .bind(leased_by)
        .execute(&self.pool)
        .await
        .context("insert solver.hub_userops prepared")?
        .rows_affected();

        if n > 1 {
            anyhow::bail!("unexpected rows_affected inserting hub_userops: {n}");
        }
        Ok(())
    }

    pub async fn record_hub_userop_submitted(
        &self,
        job_id: i64,
        leased_by: &str,
        kind: HubUserOpKind,
        userop_hash: &str,
    ) -> Result<()> {
        let n = sqlx::query(
            "update solver.hub_userops u set \
                userop_hash = coalesce(u.userop_hash, $1), \
                state = 'submitted', \
                updated_at = now() \
             from solver.jobs j \
             where u.job_id=j.job_id \
               and u.kind=$2::solver.userop_kind \
               and j.job_id=$3 and j.leased_by=$4 and j.lease_until >= now()",
        )
        .bind(userop_hash)
        .bind(kind.as_str())
        .bind(job_id)
        .bind(leased_by)
        .execute(&self.pool)
        .await
        .context("update solver.hub_userops submitted")?
        .rows_affected();

        if n != 1 {
            anyhow::bail!("lost job lease for job_id={job_id}");
        }
        Ok(())
    }

    pub async fn record_hub_userop_included(
        &self,
        job_id: i64,
        leased_by: &str,
        kind: HubUserOpKind,
        tx_hash: [u8; 32],
        success: bool,
    ) -> Result<()> {
        let n = sqlx::query(
            "update solver.hub_userops u set \
                tx_hash=$1, \
                success=$2, \
                state='included', \
                updated_at=now() \
             from solver.jobs j \
             where u.job_id=j.job_id \
               and u.kind=$3::solver.userop_kind \
               and j.job_id=$4 and j.leased_by=$5 and j.lease_until >= now()",
        )
        .bind(tx_hash.to_vec())
        .bind(success)
        .bind(kind.as_str())
        .bind(job_id)
        .bind(leased_by)
        .execute(&self.pool)
        .await
        .context("update solver.hub_userops included")?
        .rows_affected();

        if n != 1 {
            anyhow::bail!("lost job lease for job_id={job_id}");
        }
        Ok(())
    }

    pub async fn record_hub_userop_retryable_error(
        &self,
        job_id: i64,
        leased_by: &str,
        kind: HubUserOpKind,
        msg: &str,
        retry_in: Duration,
    ) -> Result<()> {
        let secs: i64 = retry_in.as_secs().try_into().unwrap_or(60);
        let n = sqlx::query(
            "update solver.hub_userops u set \
                attempts = u.attempts + 1, \
                last_error = $1, \
                next_retry_at = now() + make_interval(secs => $2), \
                updated_at = now() \
             from solver.jobs j \
             where u.job_id=j.job_id \
               and u.kind=$3::solver.userop_kind \
               and j.job_id=$4 and j.leased_by=$5 and j.lease_until >= now()",
        )
        .bind(msg)
        .bind(secs)
        .bind(kind.as_str())
        .bind(job_id)
        .bind(leased_by)
        .execute(&self.pool)
        .await
        .context("update solver.hub_userops retryable error")?
        .rows_affected();

        if n != 1 {
            anyhow::bail!("lost job lease for job_id={job_id}");
        }
        Ok(())
    }

    pub async fn record_hub_userop_fatal_error(
        &self,
        job_id: i64,
        leased_by: &str,
        kind: HubUserOpKind,
        msg: &str,
    ) -> Result<()> {
        let n = sqlx::query(
            "update solver.hub_userops u set \
                state='failed_fatal', \
                last_error=$1, \
                updated_at=now() \
             from solver.jobs j \
             where u.job_id=j.job_id \
               and u.kind=$2::solver.userop_kind \
               and j.job_id=$3 and j.leased_by=$4 and j.lease_until >= now()",
        )
        .bind(msg)
        .bind(kind.as_str())
        .bind(job_id)
        .bind(leased_by)
        .execute(&self.pool)
        .await
        .context("update solver.hub_userops fatal error")?
        .rows_affected();

        if n != 1 {
            anyhow::bail!("lost job lease for job_id={job_id}");
        }
        Ok(())
    }

    pub async fn delete_hub_userop_prepared(
        &self,
        job_id: i64,
        leased_by: &str,
        kind: HubUserOpKind,
    ) -> Result<()> {
        // Only delete local "prepared but not submitted" artifacts. This lets us rebuild with a
        // fresh nonce in cases where the original op is stale (AA25).
        sqlx::query(
            "delete from solver.hub_userops u \
             using solver.jobs j \
             where u.job_id=j.job_id \
               and u.job_id=$1 \
               and u.kind=$2::solver.userop_kind \
               and u.state='prepared' \
               and u.userop_hash is null \
               and j.leased_by=$3 \
               and j.lease_until >= now()",
        )
        .bind(job_id)
        .bind(kind.as_str())
        .bind(leased_by)
        .execute(&self.pool)
        .await
        .context("delete solver.hub_userops prepared")?;
        Ok(())
    }

    /// Computes a nonce floor for Safe4337 userops based on our persisted "submitted but not yet
    /// included" userops for a given sender.
    ///
    /// This is important after restarts: `EntryPoint.getNonce` only reflects included ops, but
    /// bundlers may already have accepted pending ops and will reject re-using the same nonce
    /// (`AA25 invalid account nonce`).
    pub async fn hub_userop_nonce_floor_for_sender(&self, sender: Address) -> Result<Option<U256>> {
        let rows = sqlx::query(
            "select userop::text as userop_json \
             from solver.hub_userops \
             where state='submitted' and tx_hash is null",
        )
        .fetch_all(&self.pool)
        .await
        .context("select solver.hub_userops (nonce floor)")?;

        let mut max_nonce: Option<U256> = None;
        for row in rows {
            let json: String = row.try_get("userop_json")?;
            let op: PackedUserOperation =
                serde_json::from_str(&json).context("deserialize hub userop json")?;
            if op.sender != sender {
                continue;
            }
            max_nonce = Some(match max_nonce {
                Some(cur) => cur.max(op.nonce),
                None => op.nonce,
            });
        }

        Ok(max_nonce.map(|n| n.saturating_add(U256::from(1u64))))
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

    pub async fn breaker_is_active(
        &self,
        contract: Address,
        selector: Option<[u8; 4]>,
    ) -> Result<bool> {
        let selector = selector.map(|s| s.to_vec());
        let active: bool = sqlx::query_scalar(
            "select exists( \
                select 1 from solver.circuit_breakers \
                where contract = $1 \
                  and cooldown_until > now() \
                  and (selector is null or selector = $2) \
            )",
        )
        .bind(contract.as_slice())
        .bind(selector)
        .fetch_one(&self.pool)
        .await
        .context("breaker_is_active")?;
        Ok(active)
    }

    pub async fn breaker_record_failure(
        &self,
        contract: Address,
        selector: Option<[u8; 4]>,
        error: &str,
    ) -> Result<(i32, i64)> {
        let selector = selector.map(|s| s.to_vec());
        let mut tx = self.pool.begin().await.context("begin breaker tx")?;

        let current: Option<i32> = sqlx::query_scalar(
            "select fail_count from solver.circuit_breakers \
             where contract = $1 and selector is not distinct from $2",
        )
        .bind(contract.as_slice())
        .bind(&selector)
        .fetch_optional(&mut *tx)
        .await
        .context("select breaker fail_count")?;

        let next = current.unwrap_or(0).saturating_add(1);
        let cooldown_secs = breaker_backoff_secs(next);

        sqlx::query(
            "insert into solver.circuit_breakers(contract, selector, fail_count, cooldown_until, last_error, updated_at) \
             values ($1, $2, $3, now() + make_interval(secs => $4), $5, now()) \
             on conflict (contract, selector) do update set \
                fail_count = excluded.fail_count, \
                cooldown_until = excluded.cooldown_until, \
                last_error = excluded.last_error, \
                updated_at = now()",
        )
        .bind(contract.as_slice())
        .bind(selector)
        .bind(next)
        .bind(cooldown_secs)
        .bind(error)
        .execute(&mut *tx)
        .await
        .context("upsert solver.circuit_breakers")?;

        tx.commit().await.context("commit breaker tx")?;
        Ok((next, cooldown_secs))
    }
}

fn breaker_backoff_secs(fail_count: i32) -> i64 {
    match fail_count {
        0 => 0,
        1 => 60,
        2 => 300,
        3 => 1800,
        4 => 21600,
        _ => 86400,
    }
}

#[cfg(test)]
mod breaker_tests {
    use super::breaker_backoff_secs;

    #[test]
    fn breaker_backoff_schedule_is_stable() {
        assert_eq!(breaker_backoff_secs(0), 0);
        assert_eq!(breaker_backoff_secs(1), 60);
        assert_eq!(breaker_backoff_secs(2), 300);
        assert_eq!(breaker_backoff_secs(3), 1800);
        assert_eq!(breaker_backoff_secs(4), 21600);
        assert_eq!(breaker_backoff_secs(5), 86400);
        assert_eq!(breaker_backoff_secs(100), 86400);
    }
}

async fn exec_sql_batch<E>(exec: &mut E, sql: &str) -> Result<()>
where
    for<'c> &'c mut E: Executor<'c, Database = Postgres>,
{
    for stmt in sql.split(';') {
        let s = stmt.trim();
        if s.is_empty() {
            continue;
        }
        sqlx::query(s).execute(&mut *exec).await.with_context(|| {
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
