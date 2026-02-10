use super::*;

fn classify_transition_reject_reason(
    expected_states: &[String],
    current_state: Option<&str>,
    current_leased_by: Option<&str>,
    expected_leased_by: &str,
    lease_valid: Option<bool>,
    job_exists: bool,
) -> &'static str {
    if !job_exists {
        return "job_not_found";
    }

    let Some(cs) = current_state else {
        return "unknown_conflict";
    };
    if !expected_states.iter().any(|s| s == cs) {
        return "state_mismatch";
    }
    if current_leased_by != Some(expected_leased_by) {
        return "lease_owner_mismatch";
    }
    if lease_valid == Some(false) {
        return "lease_expired";
    }
    "unknown_conflict"
}

impl SolverDb {
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

    pub async fn job_id_for_intent(&self, intent_id: [u8; 32]) -> Result<Option<i64>> {
        let v: Option<i64> =
            sqlx::query_scalar("select job_id from solver.jobs where intent_id = $1")
                .bind(intent_id.to_vec())
                .fetch_optional(&self.pool)
                .await
                .context("select solver.jobs.job_id by intent_id")?;
        Ok(v)
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
                    state in ( \
                        'ready', \
                        'claimed', \
                        'tron_prepared', \
                        'tron_sent', \
                        'proof_built', \
                        'proved', \
                        'proved_waiting_funding', \
                        'proved_waiting_settlement' \
                    ) \
                    and next_retry_at <= now() \
                    and ( \
                        (lease_until is null or lease_until < now()) \
                        or (leased_by = $2 and lease_until >= now()) \
                    ) \
                order by \
                    (case when state in ('claimed', 'tron_prepared', 'tron_sent', 'proof_built') then 0 else 1 end) asc, \
                    claim_window_expires_at asc nulls last, \
                    job_id asc \
                limit $1 \
                for update skip locked \
            ) \
            update solver.jobs j set \
                leased_by = $2, \
                lease_until = now() + make_interval(secs => $3), \
                updated_at = now() \
            from cte \
            where j.job_id = cte.job_id \
            returning j.job_id, j.intent_id, j.intent_type, j.intent_specs, j.deadline, \
                      extract(epoch from j.claim_window_expires_at)::bigint as claim_window_expires_at_unix, \
                      j.state, j.attempts, j.tron_txid",
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
                claim_window_expires_at_unix: row.try_get("claim_window_expires_at_unix")?,
                state: row.try_get("state")?,
                attempts: row.try_get("attempts")?,
                tron_txid,
            });
        }
        Ok(out)
    }

    pub async fn renew_job_lease(
        &self,
        job_id: i64,
        leased_by: &str,
        lease_for: Duration,
    ) -> Result<()> {
        let secs: i64 = lease_for.as_secs().try_into().unwrap_or(60);
        let n = sqlx::query(
            "update solver.jobs set \
                lease_until = now() + make_interval(secs => $1), \
                updated_at = now() \
             where job_id = $2 and leased_by = $3 and lease_until >= now() \
               and state not in ('done', 'failed_fatal')",
        )
        .bind(secs)
        .bind(job_id)
        .bind(leased_by)
        .execute(&self.pool)
        .await
        .context("renew solver.jobs lease")?
        .rows_affected();
        if n != 1 {
            anyhow::bail!("lost job lease for job_id={job_id}");
        }
        Ok(())
    }

    pub(super) async fn update_job_state_from(
        &self,
        job_id: i64,
        leased_by: &str,
        state: &str,
        expected_states: &[&str],
    ) -> Result<()> {
        let expected_states: Vec<String> =
            expected_states.iter().map(|s| (*s).to_string()).collect();
        let n = sqlx::query(
            "update solver.jobs set state = $1, updated_at = now() \
             where job_id = $2 and leased_by = $3 and lease_until >= now() \
               and state = any($4::text[])",
        )
        .bind(state)
        .bind(job_id)
        .bind(leased_by)
        .bind(&expected_states)
        .execute(&self.pool)
        .await
        .context("update solver.jobs state")?
        .rows_affected();
        if n != 1 {
            let diag = sqlx::query(
                "select state, leased_by, (lease_until >= now()) as lease_valid \
                 from solver.jobs where job_id = $1",
            )
            .bind(job_id)
            .fetch_optional(&self.pool)
            .await
            .ok()
            .flatten();
            let job_exists = diag.is_some();

            let mut current_state: Option<String> = None;
            let mut current_leased_by: Option<String> = None;
            let mut lease_valid: Option<bool> = None;

            if let Some(row) = diag {
                current_state = row.try_get("state").ok();
                current_leased_by = row.try_get("leased_by").ok();
                lease_valid = row.try_get("lease_valid").ok();
            }
            let reason = classify_transition_reject_reason(
                &expected_states,
                current_state.as_deref(),
                current_leased_by.as_deref(),
                leased_by,
                lease_valid,
                job_exists,
            );

            anyhow::bail!(
                "[transition_reject:{reason}] rejected state transition for job_id={job_id}: expected one of {:?} -> {} (current_state={:?}, leased_by={:?}, lease_valid={:?})",
                expected_states,
                state,
                current_state,
                current_leased_by,
                lease_valid
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::classify_transition_reject_reason;

    #[test]
    fn classify_transition_reject_reason_cases() {
        let expected = vec!["claimed".to_string(), "tron_prepared".to_string()];

        assert_eq!(
            classify_transition_reject_reason(
                &expected,
                Some("done"),
                Some("solver-a"),
                "solver-a",
                Some(true),
                true,
            ),
            "state_mismatch"
        );

        assert_eq!(
            classify_transition_reject_reason(
                &expected,
                Some("claimed"),
                Some("solver-b"),
                "solver-a",
                Some(true),
                true,
            ),
            "lease_owner_mismatch"
        );

        assert_eq!(
            classify_transition_reject_reason(
                &expected,
                Some("claimed"),
                Some("solver-a"),
                "solver-a",
                Some(false),
                true,
            ),
            "lease_expired"
        );

        assert_eq!(
            classify_transition_reject_reason(
                &expected,
                Some("claimed"),
                Some("solver-a"),
                "solver-a",
                Some(true),
                true,
            ),
            "unknown_conflict"
        );

        assert_eq!(
            classify_transition_reject_reason(&expected, None, None, "solver-a", None, false,),
            "job_not_found"
        );
    }
}
