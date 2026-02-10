use super::{JobCtx, SolverJob};
use crate::types::IntentType;
use anyhow::Result;

pub(super) fn retry_delay(attempts: i32) -> std::time::Duration {
    // Exponential backoff with caps. This is intentionally simple and centralized.
    let shift = u32::try_from(attempts.clamp(0, 10)).unwrap_or(0);
    let base = 1u64.checked_shl(shift).unwrap_or(u64::MAX);
    std::time::Duration::from_secs(base.min(300))
}

pub(super) async fn record_fatal(ctx: &JobCtx, job: &SolverJob, msg: &str) -> Result<()> {
    ctx.db
        .record_fatal_error(job.job_id, &ctx.instance_id, msg)
        .await?;
    let _ = ctx
        .db
        .release_delegate_reservation_for_job(job.job_id)
        .await;

    if ctx.cfg.jobs.global_pause_fatal_threshold > 0 {
        let window = i64::try_from(ctx.cfg.jobs.global_pause_window_secs).unwrap_or(300);
        let n = ctx.db.count_recent_fatal_errors(window).await.unwrap_or(0);
        if n >= i64::try_from(ctx.cfg.jobs.global_pause_fatal_threshold).unwrap_or(i64::MAX) {
            let secs = i64::try_from(ctx.cfg.jobs.global_pause_duration_secs).unwrap_or(300);
            let reason = format!("auto_pause_fatal_threshold_exceeded n={n}");
            let _ = ctx.db.set_global_pause_for_secs(secs, &reason).await;
        }
    }

    Ok(())
}

pub(super) async fn enforce_claim_rate_limits(ctx: &JobCtx, ty: IntentType) -> Result<Option<i64>> {
    if let Some(wait) = ctx
        .db
        .rate_limit_claim_per_minute(
            "claim:global",
            ctx.cfg.jobs.rate_limit_claims_per_minute_global,
        )
        .await?
    {
        ctx.telemetry.claim_rate_limited("claim:global");
        return Ok(Some(wait));
    }

    let (k, limit) = match ty {
        IntentType::TrxTransfer => (
            "claim:trx_transfer",
            ctx.cfg.jobs.rate_limit_claims_per_minute_trx_transfer,
        ),
        IntentType::UsdtTransfer => (
            "claim:usdt_transfer",
            ctx.cfg.jobs.rate_limit_claims_per_minute_usdt_transfer,
        ),
        IntentType::DelegateResource => (
            "claim:delegate_resource",
            ctx.cfg.jobs.rate_limit_claims_per_minute_delegate_resource,
        ),
        IntentType::TriggerSmartContract => (
            "claim:trigger_smart_contract",
            ctx.cfg
                .jobs
                .rate_limit_claims_per_minute_trigger_smart_contract,
        ),
    };
    if let Some(wait) = ctx.db.rate_limit_claim_per_minute(k, limit).await? {
        ctx.telemetry.claim_rate_limited(k);
        return Ok(Some(wait));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::retry_delay;

    #[test]
    fn retry_delay_grows_monotonically_until_cap() {
        let mut prev = std::time::Duration::from_secs(0);
        for attempts in 0..=16 {
            let d = retry_delay(attempts);
            assert!(
                d >= prev,
                "retry_delay regressed at attempts={attempts}: prev={prev:?} next={d:?}"
            );
            assert!(d <= std::time::Duration::from_secs(300));
            prev = d;
        }
    }

    #[test]
    fn retry_delay_caps_at_five_minutes_after_ten_attempts() {
        for attempts in [10, 11, 20, i32::MAX] {
            assert_eq!(retry_delay(attempts), std::time::Duration::from_secs(300));
        }
    }

    #[test]
    fn retry_delay_clamps_negative_attempts_to_initial_backoff() {
        assert_eq!(retry_delay(-1), std::time::Duration::from_secs(1));
        assert_eq!(retry_delay(i32::MIN), std::time::Duration::from_secs(1));
    }
}
