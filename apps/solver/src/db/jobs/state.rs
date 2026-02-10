use super::*;
use crate::types::JobState;

impl SolverDb {
    pub async fn record_claim(
        &self,
        job_id: i64,
        leased_by: &str,
        claim_tx_hash: [u8; 32],
    ) -> Result<()> {
        let expected_states = super::transitions::expected_state_binds_for(JobState::Claimed);
        let n = sqlx::query(
            "update solver.jobs set state='claimed', claim_tx_hash=$1, updated_at=now() \
             where job_id=$2 and leased_by=$3 and lease_until >= now() \
               and state = any($4::text[])",
        )
        .bind(claim_tx_hash.to_vec())
        .bind(job_id)
        .bind(leased_by)
        .bind(expected_states)
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
        let expected_states = super::transitions::expected_state_binds_for(JobState::TronSent);
        let n = sqlx::query(
            "update solver.jobs set state='tron_sent', tron_txid=$1, updated_at=now() \
             where job_id=$2 and leased_by=$3 and lease_until >= now() \
               and state = any($4::text[])",
        )
        .bind(tron_txid.to_vec())
        .bind(job_id)
        .bind(leased_by)
        .bind(expected_states)
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
        self.update_job_state_from(
            job_id,
            leased_by,
            JobState::ProofBuilt.as_db_str(),
            super::transitions::expected_previous_state_names_for(JobState::ProofBuilt),
        )
        .await
    }

    pub async fn record_prove(
        &self,
        job_id: i64,
        leased_by: &str,
        prove_tx_hash: [u8; 32],
    ) -> Result<()> {
        let expected_states = super::transitions::expected_state_binds_for(JobState::Proved);
        let n = sqlx::query(
            "update solver.jobs set state='proved', prove_tx_hash=$1, updated_at=now() \
             where job_id=$2 and leased_by=$3 and lease_until >= now() \
               and state = any($4::text[])",
        )
        .bind(prove_tx_hash.to_vec())
        .bind(job_id)
        .bind(leased_by)
        .bind(expected_states)
        .execute(&self.pool)
        .await
        .context("record prove")?
        .rows_affected();
        if n != 1 {
            anyhow::bail!("lost job lease for job_id={job_id}");
        }
        Ok(())
    }

    pub async fn record_job_state(
        &self,
        job_id: i64,
        leased_by: &str,
        state: JobState,
    ) -> Result<()> {
        let expected = super::transitions::expected_previous_state_names_for(state);
        self.update_job_state_from(job_id, leased_by, state.as_db_str(), expected)
            .await
    }

    pub async fn record_done(&self, job_id: i64, leased_by: &str) -> Result<()> {
        self.record_job_state(job_id, leased_by, JobState::Done).await
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
             where job_id=$3 and leased_by=$4 \
               and state not in ('done', 'failed_fatal')",
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
             where job_id=$2 and leased_by=$3 \
               and state <> 'done'",
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

    pub async fn global_pause_active(&self) -> Result<Option<(i64, Option<String>)>> {
        let row = sqlx::query(
            "select \
                extract(epoch from (pause_until - now()))::bigint as secs_left, \
                reason \
             from solver.global_pause \
             where id = 1 and pause_until > now()",
        )
        .fetch_optional(&self.pool)
        .await
        .context("select solver.global_pause")?;

        let Some(row) = row else {
            return Ok(None);
        };

        let secs_left: i64 = row.try_get("secs_left")?;
        let reason: Option<String> = row.try_get("reason")?;
        Ok(Some((secs_left.max(1), reason)))
    }

    pub async fn set_global_pause_for_secs(&self, secs: i64, reason: &str) -> Result<()> {
        let secs = secs.max(1);
        sqlx::query(
            "insert into solver.global_pause(id, pause_until, reason, updated_at) \
             values (1, now() + make_interval(secs => $1), $2, now()) \
             on conflict (id) do update set \
                pause_until = excluded.pause_until, \
                reason = excluded.reason, \
                updated_at = now()",
        )
        .bind(secs)
        .bind(reason)
        .execute(&self.pool)
        .await
        .context("upsert solver.global_pause")?;
        Ok(())
    }

    pub async fn count_recent_fatal_errors(&self, window_secs: i64) -> Result<i64> {
        let window_secs = window_secs.max(1);
        let row = sqlx::query(
            "select count(*)::bigint as n \
             from solver.jobs \
             where state = 'failed_fatal' \
               and updated_at > now() - make_interval(secs => $1)",
        )
        .bind(window_secs)
        .fetch_one(&self.pool)
        .await
        .context("count_recent_fatal_errors")?;
        Ok(row.try_get::<i64, _>("n")?)
    }

    pub async fn rate_limit_claim_per_minute(&self, key: &str, limit: u64) -> Result<Option<i64>> {
        if limit == 0 {
            return Ok(None);
        }
        let limit_i64: i64 = limit.try_into().unwrap_or(i64::MAX);
        let row = sqlx::query(
            "with upsert as ( \
                insert into solver.rate_limits(key, window_start, count, updated_at) \
                values ($1, date_trunc('minute', now()), 1, now()) \
                on conflict (key, window_start) do update set \
                    count = solver.rate_limits.count + 1, \
                    updated_at = now() \
                returning count \
             ), wait as ( \
                select extract(epoch from (date_trunc('minute', now()) + interval '1 minute' - now()))::bigint as wait_secs \
             ) \
             select \
                (select count from upsert) as count, \
                (select wait_secs from wait) as wait_secs",
        )
        .bind(key)
        .fetch_one(&self.pool)
        .await
        .context("rate_limit_claim_per_minute")?;
        let count: i64 = row.try_get("count")?;
        let wait_secs: i64 = row.try_get("wait_secs")?;
        if count > limit_i64 {
            return Ok(Some(wait_secs.max(1)));
        }
        Ok(None)
    }
}
