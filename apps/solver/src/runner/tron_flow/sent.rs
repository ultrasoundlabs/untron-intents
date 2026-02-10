use super::super::{
    JobCtx, SolverJob, decode_trigger_contract_and_selector, lease, retry,
};
use crate::{config::TronMode, db::TronProofRow, db::TronTxCostsRow, types::IntentType};
use alloy::primitives::B256;
use anyhow::Result;
use std::time::Instant;

pub(crate) async fn process_tron_sent_state(
    ctx: &JobCtx,
    job: &SolverJob,
    id: B256,
    ty: IntentType,
) -> Result<()> {
    let Some(txid) = job.tron_txid else {
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
    tracing::info!(id = %id, "building tron proof");
    let started = Instant::now();
    let tron = match lease::with_lease_heartbeat(ctx, job.job_id, ctx.tron.build_proof(txid)).await
    {
        Ok(v) => v,
        Err(err) => {
            ctx.telemetry
                .tron_proof_ms(false, started.elapsed().as_millis() as u64);
            let msg = err.to_string();
            if msg.contains("tron_tx_failed:") {
                // If simulation was enabled and this tx still failed onchain, treat this as
                // especially suspicious and apply a stronger breaker backoff.
                if matches!(
                    ty,
                    IntentType::TriggerSmartContract | IntentType::UsdtTransfer
                ) {
                    let (contract, selector) = match ty {
                        IntentType::TriggerSmartContract => {
                            decode_trigger_contract_and_selector(&job.intent_specs)
                                .unwrap_or((alloy::primitives::Address::ZERO, None))
                        }
                        IntentType::UsdtTransfer => {
                            let contract = ctx
                                .hub
                                .v3_tron_usdt()
                                .await
                                .unwrap_or(alloy::primitives::Address::ZERO);
                            (contract, Some([0xa9, 0x05, 0x9c, 0xbb]))
                        }
                        _ => (alloy::primitives::Address::ZERO, None),
                    };

                    if contract != alloy::primitives::Address::ZERO {
                        let mut mismatch = false;
                        if ctx.cfg.tron.emulation_enabled
                            && ctx.cfg.tron.mode == TronMode::Grpc
                            && let Ok(Some(emu)) = ctx.db.get_intent_emulation(job.intent_id).await
                        {
                            mismatch = emu.ok;
                        }
                        if mismatch {
                            ctx.telemetry.emulation_mismatch();
                        }

                        let weight: i32 = if mismatch {
                            i32::try_from(ctx.cfg.jobs.breaker_mismatch_penalty)
                                .unwrap_or(2)
                                .max(1)
                        } else {
                            1
                        };
                        let err_msg = if mismatch {
                            format!("onchain_fail_after_emulation_ok: {msg}")
                        } else {
                            msg.clone()
                        };
                        let _ = ctx
                            .db
                            .breaker_record_failure_weighted(contract, selector, &err_msg, weight)
                            .await;
                    }
                }

                retry::record_fatal(ctx, job, &msg).await?;
                return Ok(());
            }
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
    };
    ctx.telemetry
        .tron_proof_ms(true, started.elapsed().as_millis() as u64);
    let proof_row = TronProofRow {
        blocks: tron.blocks.into_iter().collect(),
        encoded_tx: tron.encoded_tx,
        proof: tron
            .proof
            .into_iter()
            .map(|b| b.as_slice().to_vec())
            .collect(),
        index_dec: tron.index.to_string(),
    };
    ctx.db.save_tron_proof(txid, &proof_row).await?;

    if let Ok(Some(info)) = ctx.tron.fetch_transaction_info(txid).await {
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
            result_message: Some(String::from_utf8_lossy(&info.res_message).into_owned()),
        };
        let _ = ctx
            .db
            .upsert_tron_tx_costs(job.job_id, txid, Some(job.intent_type), &costs)
            .await;
    }

    ctx.db
        .record_proof_built(job.job_id, &ctx.instance_id)
        .await?;
    ctx.telemetry
        .job_state_transition(job.intent_type, "tron_sent", "proof_built");
    Ok(())
}
