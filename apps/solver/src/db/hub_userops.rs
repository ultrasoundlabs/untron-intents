use super::*;

impl SolverDb {
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
                block_number, \
                success, \
                receipt::text as receipt_json, \
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
            block_number: row.try_get("block_number")?,
            success: row.try_get("success")?,
            receipt_json: row.try_get("receipt_json")?,
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

    #[allow(clippy::too_many_arguments)]
    pub async fn record_hub_userop_included(
        &self,
        job_id: i64,
        leased_by: &str,
        kind: HubUserOpKind,
        tx_hash: [u8; 32],
        block_number: Option<i64>,
        success: bool,
        actual_gas_cost_wei: Option<U256>,
        actual_gas_used: Option<U256>,
        receipt_json: &str,
    ) -> Result<()> {
        let actual_gas_cost_wei = actual_gas_cost_wei.map(|v| v.to_string());
        let actual_gas_used = actual_gas_used.map(|v| v.to_string());
        let n = sqlx::query(
            "update solver.hub_userops u set \
                tx_hash=$1, \
                block_number=coalesce($2, u.block_number), \
                success=$3, \
                actual_gas_cost_wei=coalesce($4::numeric, u.actual_gas_cost_wei), \
                actual_gas_used=coalesce($5::numeric, u.actual_gas_used), \
                receipt=$6::jsonb, \
                state='included', \
                updated_at=now() \
             from solver.jobs j \
             where u.job_id=j.job_id \
               and u.kind=$7::solver.userop_kind \
               and j.job_id=$8 and j.leased_by=$9 and j.lease_until >= now()",
        )
        .bind(tx_hash.to_vec())
        .bind(block_number)
        .bind(success)
        .bind(actual_gas_cost_wei)
        .bind(actual_gas_used)
        .bind(receipt_json)
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

    pub async fn hub_userop_avg_actual_gas_cost_wei(
        &self,
        kind: HubUserOpKind,
        lookback: i64,
    ) -> Result<Option<U256>> {
        let lookback = lookback.clamp(1, 10_000);
        let rows = sqlx::query(
            "select actual_gas_cost_wei::text as v \
             from solver.hub_userops \
             where state='included' \
               and kind=$1::solver.userop_kind \
               and actual_gas_cost_wei is not null \
             order by updated_at desc \
             limit $2",
        )
        .bind(kind.as_str())
        .bind(lookback)
        .fetch_all(&self.pool)
        .await
        .context("select solver.hub_userops actual_gas_cost_wei")?;

        if rows.is_empty() {
            return Ok(None);
        }

        let mut sum = U256::ZERO;
        let mut n: u64 = 0;
        for row in rows {
            let s: String = row.try_get("v")?;
            let v: U256 = s
                .parse()
                .with_context(|| format!("parse actual_gas_cost_wei numeric: {s}"))?;
            sum = sum.saturating_add(v);
            n = n.saturating_add(1);
        }
        if n == 0 {
            return Ok(None);
        }
        Ok(Some(sum / U256::from(n)))
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
}
