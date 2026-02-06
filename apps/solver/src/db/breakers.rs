use super::*;

impl SolverDb {
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
        self.breaker_record_failure_weighted(contract, selector, error, 1)
            .await
    }

    pub async fn breaker_record_failure_weighted(
        &self,
        contract: Address,
        selector: Option<[u8; 4]>,
        error: &str,
        weight: i32,
    ) -> Result<(i32, i64)> {
        let weight = weight.clamp(1, 100);
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

        let next = current.unwrap_or(0).saturating_add(weight);
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

pub(super) fn breaker_backoff_secs(fail_count: i32) -> i64 {
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
