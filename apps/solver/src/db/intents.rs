use super::*;

impl SolverDb {
    pub async fn upsert_intent_emulation(
        &self,
        intent_id: [u8; 32],
        intent_type: i16,
        ok: bool,
        reason: Option<&str>,
        contract: Option<&[u8]>,
        selector: Option<&[u8]>,
    ) -> Result<()> {
        sqlx::query(
            "insert into solver.intent_emulations(intent_id, intent_type, ok, reason, contract, selector, checked_at, updated_at) \
             values ($1, $2, $3, $4, $5, $6, now(), now()) \
             on conflict (intent_id) do update set \
                intent_type = excluded.intent_type, \
                ok = excluded.ok, \
                reason = excluded.reason, \
                contract = excluded.contract, \
                selector = excluded.selector, \
                checked_at = now(), \
                updated_at = now()",
        )
        .bind(intent_id.to_vec())
        .bind(intent_type)
        .bind(ok)
        .bind(reason)
        .bind(contract.map(|b| b.to_vec()))
        .bind(selector.map(|b| b.to_vec()))
        .execute(&self.pool)
        .await
        .context("upsert solver.intent_emulations")?;
        Ok(())
    }

    pub async fn get_intent_emulation(
        &self,
        intent_id: [u8; 32],
    ) -> Result<Option<IntentEmulationRow>> {
        let row = sqlx::query(
            "select \
                ok, \
                reason, \
                contract, \
                selector, \
                extract(epoch from checked_at)::bigint as checked_at_unix \
             from solver.intent_emulations \
             where intent_id = $1",
        )
        .bind(intent_id.to_vec())
        .fetch_optional(&self.pool)
        .await
        .context("select solver.intent_emulations")?;

        let Some(row) = row else {
            return Ok(None);
        };

        Ok(Some(IntentEmulationRow {
            ok: row.try_get("ok")?,
            reason: row.try_get("reason")?,
            contract: row.try_get("contract")?,
            selector: row.try_get("selector")?,
            checked_at_unix: row.try_get("checked_at_unix")?,
        }))
    }

    pub async fn cleanup_expired_delegate_reservations(&self) -> Result<u64> {
        let n = sqlx::query("delete from solver.delegate_reservations where expires_at <= now()")
            .execute(&self.pool)
            .await
            .context("cleanup_expired_delegate_reservations")?
            .rows_affected();
        Ok(n)
    }

    pub async fn get_delegate_reservation_for_job(
        &self,
        job_id: i64,
    ) -> Result<Option<DelegateReservationRow>> {
        let row = sqlx::query(
            "select \
                owner_address, \
                resource, \
                amount_sun, \
                extract(epoch from (expires_at - now()))::bigint as expires_in_secs \
             from solver.delegate_reservations \
             where job_id = $1",
        )
        .bind(job_id)
        .fetch_optional(&self.pool)
        .await
        .context("select solver.delegate_reservations by job_id")?;

        let Some(row) = row else {
            return Ok(None);
        };

        let expires_in_secs: i64 = row.try_get("expires_in_secs")?;
        Ok(Some(DelegateReservationRow {
            owner_address: row.try_get("owner_address")?,
            resource: row.try_get("resource")?,
            amount_sun: row.try_get("amount_sun")?,
            expires_in_secs: expires_in_secs.max(0),
        }))
    }

    pub async fn upsert_delegate_reservation_for_job(
        &self,
        job_id: i64,
        owner_address: &[u8],
        resource: i16,
        amount_sun: i64,
        ttl_secs: i64,
    ) -> Result<()> {
        let ttl_secs = ttl_secs.max(1);
        sqlx::query(
            "insert into solver.delegate_reservations(job_id, owner_address, resource, amount_sun, expires_at, updated_at) \
             values ($1, $2, $3, $4, now() + make_interval(secs => $5), now()) \
             on conflict (job_id) do update set \
                owner_address = excluded.owner_address, \
                resource = excluded.resource, \
                amount_sun = excluded.amount_sun, \
                expires_at = excluded.expires_at, \
                updated_at = now()",
        )
        .bind(job_id)
        .bind(owner_address.to_vec())
        .bind(resource)
        .bind(amount_sun)
        .bind(ttl_secs)
        .execute(&self.pool)
        .await
        .context("upsert solver.delegate_reservations")?;
        Ok(())
    }

    pub async fn release_delegate_reservation_for_job(&self, job_id: i64) -> Result<()> {
        sqlx::query("delete from solver.delegate_reservations where job_id = $1")
            .bind(job_id)
            .execute(&self.pool)
            .await
            .context("delete solver.delegate_reservations by job_id")?;
        Ok(())
    }

    pub async fn sum_delegate_reserved_sun_by_owner(
        &self,
        resource: i16,
    ) -> Result<Vec<(Vec<u8>, i64)>> {
        let rows = sqlx::query(
            "select owner_address, sum(amount_sun)::bigint as reserved_sun \
             from solver.delegate_reservations \
             where expires_at > now() and resource = $1 \
             group by owner_address",
        )
        .bind(resource)
        .fetch_all(&self.pool)
        .await
        .context("sum_delegate_reserved_sun_by_owner")?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let owner: Vec<u8> = r.try_get("owner_address")?;
            let reserved: i64 = r.try_get("reserved_sun")?;
            out.push((owner, reserved));
        }
        Ok(out)
    }

    pub async fn upsert_intent_skip(
        &self,
        intent_id: [u8; 32],
        intent_type: i16,
        reason: &str,
        details_json: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "insert into solver.intent_skips(intent_id, intent_type, reason, details, skip_count) \
             values ($1, $2, $3, $4::jsonb, 1) \
             on conflict (intent_id) do update set \
               intent_type = excluded.intent_type, \
               reason = excluded.reason, \
               details = excluded.details, \
               skip_count = solver.intent_skips.skip_count + 1, \
               last_seen_at = now()",
        )
        .bind(intent_id.to_vec())
        .bind(intent_type)
        .bind(reason)
        .bind(details_json.unwrap_or("null"))
        .execute(&self.pool)
        .await
        .context("upsert solver.intent_skips")?;
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn intent_skip_summary(
        &self,
        since_secs: i64,
        limit: i64,
    ) -> Result<Vec<IntentSkipSummaryRow>> {
        let since_secs = since_secs.clamp(1, 365 * 24 * 3600);
        let limit = limit.clamp(1, 1_000);
        let rows = sqlx::query(
            "select \
                reason, \
                intent_type, \
                sum(skip_count)::bigint as skips, \
                extract(epoch from max(last_seen_at))::bigint as last_seen_unix \
             from solver.intent_skips \
             where last_seen_at > now() - make_interval(secs => $1) \
             group by reason, intent_type \
             order by skips desc, last_seen_unix desc \
             limit $2",
        )
        .bind(since_secs)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("intent_skip_summary")?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(IntentSkipSummaryRow {
                reason: r.try_get("reason")?,
                intent_type: r.try_get("intent_type")?,
                skips: r.try_get("skips")?,
                last_seen_unix: r.try_get("last_seen_unix")?,
            });
        }
        Ok(out)
    }
}
