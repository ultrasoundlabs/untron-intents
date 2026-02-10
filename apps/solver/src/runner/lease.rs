use super::{JobCtx, LEASE_FOR_SECS};
use anyhow::Result;
use std::future::Future;
use std::time::Duration;
use tokio::time::MissedTickBehavior;

const LEASE_RENEW_EVERY_SECS: u64 = 10;

pub(super) async fn renew_job_lease(ctx: &JobCtx, job_id: i64) -> Result<()> {
    ctx.db
        .renew_job_lease(
            job_id,
            &ctx.instance_id,
            Duration::from_secs(LEASE_FOR_SECS),
        )
        .await
}

pub(super) async fn with_lease_heartbeat<T, F>(ctx: &JobCtx, job_id: i64, fut: F) -> Result<T>
where
    F: Future<Output = Result<T>>,
{
    renew_job_lease(ctx, job_id).await?;

    let mut fut = Box::pin(fut);
    let mut lease_tick = tokio::time::interval(Duration::from_secs(LEASE_RENEW_EVERY_SECS));
    lease_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    lease_tick.tick().await;

    loop {
        tokio::select! {
            res = &mut fut => return res,
            _ = lease_tick.tick() => renew_job_lease(ctx, job_id).await?,
        }
    }
}
