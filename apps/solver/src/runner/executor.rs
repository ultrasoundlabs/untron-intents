use super::{JobCtx, job::process_job};
use crate::{db::SolverJob, types::IntentType};
use tokio::task::JoinSet;

pub(super) async fn execute_leased_jobs(ctx: JobCtx, jobs: Vec<SolverJob>) {
    let mut set = JoinSet::new();
    for job in jobs {
        let ctx = ctx.clone();
        set.spawn(async move {
            let intent_type = job.intent_type;
            let telemetry = ctx.telemetry.clone();
            let ty = match IntentType::from_i16(job.intent_type) {
                Ok(v) => v,
                Err(err) => {
                    tracing::warn!(err = %err, "unknown intent type in job");
                    return;
                }
            };
            let _permit = match ctx.job_type_sems.for_intent_type(ty).acquire_owned().await {
                Ok(p) => p,
                Err(err) => {
                    tracing::warn!(err = %err, "failed to acquire job type permit");
                    return;
                }
            };
            if let Err(err) = process_job(ctx, job).await {
                let reason = classify_job_error(&err);
                telemetry.job_failure_reason(intent_type, reason);
                tracing::warn!(err = %err, "job failed");
            }
        });
    }
    while let Some(res) = set.join_next().await {
        if let Err(err) = res {
            tracing::warn!(err = %err, "job task panicked");
        }
    }
}

fn classify_job_error(err: &anyhow::Error) -> &'static str {
    if chain_contains(err, "[transition_reject:state_mismatch]") {
        return "transition_state_mismatch";
    }
    if chain_contains(err, "[transition_reject:lease_expired]") {
        return "transition_lease_expired";
    }
    if chain_contains(err, "[transition_reject:lease_owner_mismatch]") {
        return "transition_lease_owner_mismatch";
    }
    if chain_contains(err, "[transition_reject:job_not_found]") {
        return "transition_job_not_found";
    }
    if chain_contains(err, "lost job lease") {
        return "lost_job_lease";
    }
    if chain_contains(err, "delegate_capacity_insufficient") {
        return "delegate_capacity_insufficient";
    }
    if chain_contains(err, "global solver pause active") {
        return "global_pause";
    }
    if chain_contains(err, "indexer lag too high") {
        return "indexer_lag";
    }
    "other"
}

fn chain_contains(err: &anyhow::Error, needle: &str) -> bool {
    err.chain().any(|cause| cause.to_string().contains(needle))
}

#[cfg(test)]
mod tests {
    use super::classify_job_error;
    use anyhow::Context;

    #[test]
    fn classify_job_error_maps_transition_reasons() {
        let err = anyhow::anyhow!("[transition_reject:lease_expired] rejected transition");
        assert_eq!(classify_job_error(&err), "transition_lease_expired");

        let err = anyhow::anyhow!("[transition_reject:state_mismatch] rejected transition");
        assert_eq!(classify_job_error(&err), "transition_state_mismatch");
    }

    #[test]
    fn classify_job_error_uses_chain_messages() {
        let err: anyhow::Error = Err::<(), _>(anyhow::anyhow!("lost job lease for job_id=7"))
            .context("outer context")
            .unwrap_err();
        assert_eq!(classify_job_error(&err), "lost_job_lease");
    }
}
