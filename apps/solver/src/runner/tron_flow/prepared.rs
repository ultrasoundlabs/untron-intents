use super::super::{JobCtx, LEASE_FOR_SECS, SolverJob, retry};
use crate::{
    db::{TronSignedTxRow, TronTxCostsRow},
    types::IntentType,
};
use anyhow::{Context, Result};
use std::time::Instant;

pub(crate) async fn process_tron_prepared_state(
    ctx: &JobCtx,
    job: &SolverJob,
    ty: IntentType,
) -> Result<()> {
    let Some(final_txid) = job.tron_txid else {
        ctx.db
            .record_retryable_error(
                job.job_id,
                &ctx.instance_id,
                "missing tron_txid",
                retry::retry_delay(job.attempts),
            )
            .await?;
        return Ok(());
    };

    let plan = ctx.db.list_tron_signed_txs_for_job(job.job_id).await?;
    let txs = if plan.is_empty() {
        vec![TronSignedTxRow {
            step: "final".to_string(),
            txid: final_txid,
            tx_bytes: ctx.db.load_tron_signed_tx_bytes(final_txid).await?,
            fee_limit_sun: None,
            energy_required: None,
            tx_size_bytes: None,
        }]
    } else {
        plan
    };

    for row in &txs {
        ctx.db
            .renew_job_lease(
                job.job_id,
                &ctx.instance_id,
                std::time::Duration::from_secs(LEASE_FOR_SECS),
            )
            .await?;

        // If already included, skip.
        let included = match ctx.tron.fetch_transaction_info(row.txid).await {
            Ok(Some(info)) => info.block_number > 0,
            _ => false,
        };
        if included {
            continue;
        }

        // If already known onchain (pending), don't double-broadcast.
        if ctx.tron.tx_is_known(row.txid).await {
            continue;
        }

        let _permit = ctx
            .tron_broadcast_sem
            .clone()
            .acquire_owned()
            .await
            .context("acquire tron_broadcast_sem")?;
        let started = Instant::now();
        let res = ctx.tron.broadcast_signed_tx(&row.tx_bytes).await;
        let ms = started.elapsed().as_millis() as u64;
        match res {
            Ok(()) => {
                ctx.telemetry.tron_tx_ok();
                ctx.telemetry.tron_broadcast_ms(true, ms);
            }
            Err(err) => {
                ctx.telemetry.tron_tx_err();
                ctx.telemetry.tron_broadcast_ms(false, ms);
                let msg = err.to_string();
                ctx.db
                    .record_retryable_error(
                        job.job_id,
                        &ctx.instance_id,
                        &msg,
                        retry::retry_delay(job.attempts),
                    )
                    .await?;
                return Ok(());
            }
        }

        // Wait until included so subsequent steps are reliably funded.
        let started = Instant::now();
        let mut next_lease_refresh = started + std::time::Duration::from_secs(10);
        loop {
            if Instant::now() >= next_lease_refresh {
                ctx.db
                    .renew_job_lease(
                        job.job_id,
                        &ctx.instance_id,
                        std::time::Duration::from_secs(LEASE_FOR_SECS),
                    )
                    .await?;
                next_lease_refresh = Instant::now() + std::time::Duration::from_secs(10);
            }
            if started.elapsed() > std::time::Duration::from_secs(60) {
                ctx.db
                    .record_retryable_error(
                        job.job_id,
                        &ctx.instance_id,
                        "tron tx inclusion timeout",
                        retry::retry_delay(job.attempts),
                    )
                    .await?;
                return Ok(());
            }
            match ctx.tron.fetch_transaction_info(row.txid).await {
                Ok(Some(info)) if info.block_number > 0 => {
                    let receipt = info.receipt.as_ref();
                    let costs = TronTxCostsRow {
                        fee_sun: Some(info.fee),
                        energy_usage_total: receipt.map(|r| r.energy_usage_total),
                        net_usage: receipt.map(|r| r.net_usage),
                        energy_fee_sun: receipt.map(|r| r.energy_fee),
                        net_fee_sun: receipt.map(|r| r.net_fee),
                        block_number: Some(info.block_number),
                        block_timestamp: Some(info.block_time_stamp),
                        result_code: Some(info.result),
                        result_message: Some(
                            String::from_utf8_lossy(&info.res_message).into_owned(),
                        ),
                    };
                    let _ = ctx
                        .db
                        .upsert_tron_tx_costs(job.job_id, row.txid, Some(job.intent_type), &costs)
                        .await;
                    break;
                }
                _ => {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }
    }

    if ty == IntentType::DelegateResource {
        let _ = ctx
            .db
            .release_delegate_reservation_for_job(job.job_id)
            .await;
    }

    ctx.db
        .record_tron_txid(job.job_id, &ctx.instance_id, final_txid)
        .await?;
    Ok(())
}
