use super::*;

impl SolverDb {
    #[allow(clippy::too_many_arguments)]
    pub async fn record_tron_prepared(
        &self,
        job_id: i64,
        leased_by: &str,
        txid: [u8; 32],
        tx_bytes: &[u8],
        fee_limit_sun: Option<i64>,
        energy_required: Option<i64>,
        tx_size_bytes: Option<i64>,
    ) -> Result<()> {
        let expected_states = super::jobs::transitions::expected_state_binds("tron_prepared")?;
        let mut tx = self.pool.begin().await.context("begin tron_prepared tx")?;

        sqlx::query(
            "insert into solver.tron_signed_txs(txid, job_id, step, tx_bytes, fee_limit_sun, energy_required, tx_size_bytes, updated_at) \
             values ($1, $2, 'final', $3, $4, $5, $6, now()) \
             on conflict (txid) do update set \
                job_id = excluded.job_id, \
                step = excluded.step, \
                tx_bytes = excluded.tx_bytes, \
                fee_limit_sun = excluded.fee_limit_sun, \
                energy_required = excluded.energy_required, \
                tx_size_bytes = excluded.tx_size_bytes, \
                updated_at = now()",
        )
        .bind(txid.to_vec())
        .bind(job_id)
        .bind(tx_bytes)
        .bind(fee_limit_sun)
        .bind(energy_required)
        .bind(tx_size_bytes)
        .execute(&mut *tx)
        .await
        .context("upsert solver.tron_signed_txs")?;

        let n = sqlx::query(
            "update solver.jobs set state='tron_prepared', tron_txid=$1, updated_at=now() \
             where job_id=$2 and leased_by=$3 and lease_until >= now() \
               and state = any($4::text[])",
        )
        .bind(txid.to_vec())
        .bind(job_id)
        .bind(leased_by)
        .bind(expected_states)
        .execute(&mut *tx)
        .await
        .context("record tron_prepared")?
        .rows_affected();
        if n != 1 {
            anyhow::bail!("lost job lease for job_id={job_id}");
        }

        tx.commit().await.context("commit tron_prepared tx")?;
        Ok(())
    }

    pub async fn record_tron_plan(
        &self,
        job_id: i64,
        leased_by: &str,
        pre_txs: &[TronSignedTxRow],
        final_tx: &TronSignedTxRow,
    ) -> Result<()> {
        let expected_states = super::jobs::transitions::expected_state_binds("tron_prepared")?;
        let mut tx = self.pool.begin().await.context("begin tron_plan tx")?;

        // Replace any previous plan for this job idempotently.
        sqlx::query("delete from solver.tron_signed_txs where job_id = $1")
            .bind(job_id)
            .execute(&mut *tx)
            .await
            .context("delete solver.tron_signed_txs (job plan)")?;

        for row in pre_txs.iter().chain(std::iter::once(final_tx)) {
            sqlx::query(
                "insert into solver.tron_signed_txs(txid, job_id, step, tx_bytes, fee_limit_sun, energy_required, tx_size_bytes, updated_at) \
                 values ($1, $2, $3, $4, $5, $6, $7, now()) \
                 on conflict (txid) do update set \
                    job_id = excluded.job_id, \
                    step = excluded.step, \
                    tx_bytes = excluded.tx_bytes, \
                    fee_limit_sun = excluded.fee_limit_sun, \
                    energy_required = excluded.energy_required, \
                    tx_size_bytes = excluded.tx_size_bytes, \
                    updated_at = now()",
            )
            .bind(row.txid.to_vec())
            .bind(job_id)
            .bind(&row.step)
            .bind(&row.tx_bytes)
            .bind(row.fee_limit_sun)
            .bind(row.energy_required)
            .bind(row.tx_size_bytes)
            .execute(&mut *tx)
            .await
            .context("upsert solver.tron_signed_txs (plan row)")?;
        }

        let n = sqlx::query(
            "update solver.jobs set state='tron_prepared', tron_txid=$1, updated_at=now() \
             where job_id=$2 and leased_by=$3 and lease_until >= now() \
               and state = any($4::text[])",
        )
        .bind(final_tx.txid.to_vec())
        .bind(job_id)
        .bind(leased_by)
        .bind(expected_states)
        .execute(&mut *tx)
        .await
        .context("record tron_prepared (plan)")?
        .rows_affected();
        if n != 1 {
            anyhow::bail!("lost job lease for job_id={job_id}");
        }

        tx.commit().await.context("commit tron_plan tx")?;
        Ok(())
    }

    pub async fn list_tron_signed_txs_for_job(&self, job_id: i64) -> Result<Vec<TronSignedTxRow>> {
        let rows = sqlx::query(
            "select step, txid, tx_bytes, fee_limit_sun, energy_required, tx_size_bytes \
             from solver.tron_signed_txs \
             where job_id = $1 \
             order by (step = 'final')::int, step asc",
        )
        .bind(job_id)
        .fetch_all(&self.pool)
        .await
        .context("list solver.tron_signed_txs (job plan)")?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let txid: Vec<u8> = r.try_get("txid")?;
            let mut t = [0u8; 32];
            t.copy_from_slice(&txid);
            out.push(TronSignedTxRow {
                step: r.try_get("step")?,
                txid: t,
                tx_bytes: r.try_get("tx_bytes")?,
                fee_limit_sun: r.try_get("fee_limit_sun")?,
                energy_required: r.try_get("energy_required")?,
                tx_size_bytes: r.try_get("tx_size_bytes")?,
            });
        }
        Ok(out)
    }

    pub async fn load_tron_signed_tx_bytes(&self, txid: [u8; 32]) -> Result<Vec<u8>> {
        let row: Vec<u8> =
            sqlx::query_scalar("select tx_bytes from solver.tron_signed_txs where txid = $1")
                .bind(txid.to_vec())
                .fetch_one(&self.pool)
                .await
                .context("select solver.tron_signed_txs")?;
        Ok(row)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn upsert_tron_rental(
        &self,
        job_id: i64,
        provider: &str,
        resource: &str,
        receiver_evm: [u8; 20],
        balance_sun: i64,
        lock_period: i64,
        order_id: Option<&str>,
        txid: Option<[u8; 32]>,
        request_json: Option<&serde_json::Value>,
        response_json: Option<&serde_json::Value>,
    ) -> Result<()> {
        sqlx::query(
            "insert into solver.tron_rentals(job_id, provider, resource, receiver_evm, balance_sun, lock_period, order_id, txid, request_json, response_json, updated_at) \
             values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,now()) \
             on conflict (job_id) do update set \
                provider = excluded.provider, \
                resource = excluded.resource, \
                receiver_evm = excluded.receiver_evm, \
                balance_sun = excluded.balance_sun, \
                lock_period = excluded.lock_period, \
                order_id = excluded.order_id, \
                txid = excluded.txid, \
                request_json = excluded.request_json, \
                response_json = excluded.response_json, \
                updated_at = now()",
        )
        .bind(job_id)
        .bind(provider)
        .bind(resource)
        .bind(receiver_evm.to_vec())
        .bind(balance_sun)
        .bind(lock_period)
        .bind(order_id)
        .bind(txid.map(|t| t.to_vec()))
        .bind(request_json)
        .bind(response_json)
        .execute(&self.pool)
        .await
        .context("upsert solver.tron_rentals")?;
        Ok(())
    }

    pub async fn get_tron_rental_for_job(&self, job_id: i64) -> Result<Option<TronRentalRow>> {
        let row = sqlx::query(
            "select provider, resource, receiver_evm, balance_sun, lock_period, order_id, txid, request_json, response_json \
             from solver.tron_rentals where job_id = $1",
        )
        .bind(job_id)
        .fetch_optional(&self.pool)
        .await
        .context("select solver.tron_rentals")?;

        let Some(r) = row else {
            return Ok(None);
        };

        let recv: Vec<u8> = r.try_get("receiver_evm")?;
        let mut receiver_evm = [0u8; 20];
        receiver_evm.copy_from_slice(&recv);

        let txid: Option<Vec<u8>> = r.try_get("txid")?;
        let txid = txid.map(|v| {
            let mut t = [0u8; 32];
            t.copy_from_slice(&v);
            t
        });

        Ok(Some(TronRentalRow {
            provider: r.try_get("provider")?,
            resource: r.try_get("resource")?,
            receiver_evm,
            balance_sun: r.try_get("balance_sun")?,
            lock_period: r.try_get("lock_period")?,
            order_id: r.try_get("order_id")?,
            txid,
            request_json: r.try_get("request_json")?,
            response_json: r.try_get("response_json")?,
        }))
    }

    pub async fn rental_provider_is_frozen(&self, provider: &str) -> Result<Option<i64>> {
        let row = sqlx::query(
            "select extract(epoch from frozen_until)::bigint as until_unix \
             from solver.rental_provider_freezes \
             where provider = $1 and frozen_until is not null and frozen_until > now()",
        )
        .bind(provider)
        .fetch_optional(&self.pool)
        .await
        .context("select solver.rental_provider_freezes")?;

        Ok(row.map(|r| r.try_get::<i64, _>("until_unix").unwrap_or(0)))
    }

    pub async fn rental_provider_record_failure(
        &self,
        provider: &str,
        fail_window_secs: i64,
        freeze_secs: i64,
        threshold: i32,
        err: &str,
    ) -> Result<bool> {
        let mut tx = self
            .pool
            .begin()
            .await
            .context("begin rental_provider_record_failure")?;

        // Upsert row if missing.
        sqlx::query(
            "insert into solver.rental_provider_freezes(provider, frozen_until, fail_count, fail_window_start, last_error, updated_at) \
             values ($1, null, 0, now(), $2, now()) \
             on conflict (provider) do nothing",
        )
        .bind(provider)
        .bind(err)
        .execute(&mut *tx)
        .await
        .context("insert rental_provider_freezes")?;

        // If window expired, reset fail_count and window_start.
        let row = sqlx::query(
            "select fail_count, extract(epoch from fail_window_start)::bigint as window_unix \
             from solver.rental_provider_freezes where provider=$1 for update",
        )
        .bind(provider)
        .fetch_one(&mut *tx)
        .await
        .context("select rental_provider_freezes for update")?;
        let mut fail_count: i32 = row.try_get("fail_count")?;
        let window_unix: i64 = row.try_get("window_unix")?;
        let now_unix: i64 = sqlx::query_scalar("select extract(epoch from now())::bigint")
            .fetch_one(&mut *tx)
            .await
            .context("select now unix")?;
        if now_unix.saturating_sub(window_unix) > fail_window_secs.max(1) {
            fail_count = 0;
            sqlx::query(
                "update solver.rental_provider_freezes set fail_count=0, fail_window_start=now() \
                 where provider=$1",
            )
            .bind(provider)
            .execute(&mut *tx)
            .await
            .context("reset rental_provider_freezes window")?;
        }

        fail_count = fail_count.saturating_add(1);
        sqlx::query(
            "update solver.rental_provider_freezes set fail_count=$2, last_error=$3, updated_at=now() \
             where provider=$1",
        )
        .bind(provider)
        .bind(fail_count)
        .bind(err)
        .execute(&mut *tx)
        .await
        .context("update rental_provider_freezes failure")?;

        let froze_now = fail_count >= threshold.max(1) && freeze_secs > 0;
        if froze_now {
            sqlx::query(
                "update solver.rental_provider_freezes \
                 set frozen_until = now() + ($2::text || ' seconds')::interval, updated_at=now() \
                 where provider=$1",
            )
            .bind(provider)
            .bind(freeze_secs)
            .execute(&mut *tx)
            .await
            .context("freeze rental provider")?;
        }

        tx.commit()
            .await
            .context("commit rental_provider_record_failure")?;
        Ok(froze_now)
    }

    pub async fn rental_provider_record_success(&self, provider: &str) -> Result<()> {
        sqlx::query(
            "insert into solver.rental_provider_freezes(provider, frozen_until, fail_count, fail_window_start, last_error, updated_at) \
             values ($1, null, 0, now(), null, now()) \
             on conflict (provider) do update set \
                frozen_until = null, \
                fail_count = 0, \
                fail_window_start = now(), \
                last_error = null, \
                updated_at = now()",
        )
        .bind(provider)
        .execute(&self.pool)
        .await
        .context("record rental provider success")?;
        Ok(())
    }

    pub async fn upsert_tron_tx_costs(
        &self,
        job_id: i64,
        txid: [u8; 32],
        intent_type: Option<i16>,
        costs: &TronTxCostsRow,
    ) -> Result<()> {
        sqlx::query(
            "insert into solver.tron_tx_costs( \
                txid, job_id, intent_type, fee_sun, energy_usage_total, net_usage, energy_fee_sun, net_fee_sun, \
                block_number, block_timestamp, result_code, result_message, updated_at \
             ) values ( \
                $1, $2, $3, $4, $5, $6, $7, $8, \
                $9, $10, $11, $12, now() \
             ) \
             on conflict (txid) do update set \
                job_id = excluded.job_id, \
                intent_type = excluded.intent_type, \
                fee_sun = excluded.fee_sun, \
                energy_usage_total = excluded.energy_usage_total, \
                net_usage = excluded.net_usage, \
                energy_fee_sun = excluded.energy_fee_sun, \
                net_fee_sun = excluded.net_fee_sun, \
                block_number = excluded.block_number, \
                block_timestamp = excluded.block_timestamp, \
                result_code = excluded.result_code, \
                result_message = excluded.result_message, \
                updated_at = now()",
        )
        .bind(txid.to_vec())
        .bind(job_id)
        .bind(intent_type)
        .bind(costs.fee_sun)
        .bind(costs.energy_usage_total)
        .bind(costs.net_usage)
        .bind(costs.energy_fee_sun)
        .bind(costs.net_fee_sun)
        .bind(costs.block_number)
        .bind(costs.block_timestamp)
        .bind(costs.result_code)
        .bind(costs.result_message.as_deref())
        .execute(&self.pool)
        .await
        .context("upsert solver.tron_tx_costs")?;
        Ok(())
    }

    pub async fn tron_tx_costs_avg_fee_sun(
        &self,
        intent_type: i16,
        lookback: i64,
    ) -> Result<Option<i64>> {
        let lookback = lookback.clamp(1, 10_000);
        let v: Option<f64> = sqlx::query_scalar(
            "select avg(t.fee_sun)::float8 \
             from ( \
               select c.fee_sun \
               from solver.tron_tx_costs c \
               where c.fee_sun is not null \
                 and c.intent_type = $1 \
               order by c.updated_at desc \
               limit $2 \
             ) t",
        )
        .bind(intent_type)
        .bind(lookback)
        .fetch_optional(&self.pool)
        .await
        .context("avg solver.tron_tx_costs.fee_sun")?;

        Ok(v.and_then(|f| {
            if !f.is_finite() {
                None
            } else {
                Some(f.round() as i64)
            }
        }))
    }
}
