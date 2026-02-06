use super::{
    INTENT_CLAIM_DEPOSIT, JobCtx, SolverJob, b256_to_bytes32, ensure_delegate_reservation,
    finalize_after_prove, retry,
};
use crate::{
    config::{HubTxMode, TronMode},
    db::{HubUserOpKind, HubUserOpRow},
    hub::TronProof,
    types::{IntentType, JobState},
};
use alloy::primitives::{B256, U256};
use alloy::rpc::types::eth::erc4337::PackedUserOperation;
use alloy::sol_types::SolCall;
use anyhow::{Context, Result};
use std::{future::Future, time::Instant};

async fn enforce_claim_submission_preconditions(
    ctx: &JobCtx,
    job: &SolverJob,
    ty: IntentType,
) -> Result<bool> {
    if let Some(wait) = retry::enforce_claim_rate_limits(ctx, ty).await? {
        ctx.db
            .record_retryable_error(
                job.job_id,
                &ctx.instance_id,
                "claim_rate_limited",
                std::time::Duration::from_secs(u64::try_from(wait).unwrap_or(60)),
            )
            .await?;
        return Ok(true);
    }
    if ty == IntentType::DelegateResource
        && ctx.cfg.tron.mode == TronMode::Grpc
        && !ctx.cfg.tron.delegate_resource_resell_enabled
        && let Err(err) = ensure_delegate_reservation(ctx, job).await
    {
        let msg = format!("delegate reservation failed: {err:#}");
        ctx.db
            .record_retryable_error(
                job.job_id,
                &ctx.instance_id,
                &msg,
                retry::retry_delay(job.attempts),
            )
            .await?;
        return Ok(true);
    }
    Ok(false)
}

async fn delete_stale_prepared_userop_if_needed(
    ctx: &JobCtx,
    job: &SolverJob,
    kind: HubUserOpKind,
    row: &HubUserOpRow,
    deserialize_ctx: &'static str,
) -> Result<bool> {
    if row.userop_hash.is_none() && row.state == "prepared" {
        let u: PackedUserOperation =
            serde_json::from_str(&row.userop_json).context(deserialize_ctx)?;
        let chain_nonce = ctx.hub.safe4337_chain_nonce().await?;
        if u.nonce < chain_nonce {
            ctx.db
                .delete_hub_userop_prepared(job.job_id, &ctx.instance_id, kind)
                .await
                .ok();
            return Ok(true);
        }
    }
    Ok(false)
}

#[allow(clippy::too_many_arguments)]
async fn submit_safe4337_userop<F, Fut>(
    ctx: &JobCtx,
    job: &SolverJob,
    kind: HubUserOpKind,
    metric_name: &'static str,
    sem_ctx: &'static str,
    deserialize_ctx: &'static str,
    serialize_ctx: &'static str,
    build_userop_ctx: &'static str,
    build_userop: F,
    prepared_userop_json: Option<&str>,
) -> Result<bool>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<PackedUserOperation>>,
{
    let _permit = ctx.hub_userop_submit_sem.acquire().await.context(sem_ctx)?;

    let userop = if let Some(json) = prepared_userop_json {
        serde_json::from_str::<PackedUserOperation>(json).context(deserialize_ctx)?
    } else {
        let userop = build_userop().await.context(build_userop_ctx)?;
        let json = serde_json::to_string(&userop).context(serialize_ctx)?;
        ctx.db
            .insert_hub_userop_prepared(job.job_id, &ctx.instance_id, kind, &json)
            .await?;
        userop
    };

    let started = Instant::now();
    match ctx.hub.safe4337_send_userop(userop).await {
        Ok(userop_hash) => {
            ctx.telemetry.hub_userop_ok();
            ctx.telemetry
                .hub_submit_ms(metric_name, true, started.elapsed().as_millis() as u64);
            ctx.db
                .record_hub_userop_submitted(job.job_id, &ctx.instance_id, kind, &userop_hash)
                .await?;
            Ok(false)
        }
        Err(err) => {
            ctx.telemetry.hub_userop_err();
            ctx.telemetry
                .hub_submit_ms(metric_name, false, started.elapsed().as_millis() as u64);
            let msg = err.to_string();
            if msg.contains("AA25 invalid account nonce") {
                ctx.db
                    .delete_hub_userop_prepared(job.job_id, &ctx.instance_id, kind)
                    .await
                    .ok();
            }
            ctx.db
                .record_hub_userop_retryable_error(
                    job.job_id,
                    &ctx.instance_id,
                    kind,
                    &msg,
                    retry::retry_delay(job.attempts),
                )
                .await
                .ok();
            ctx.db
                .record_retryable_error(
                    job.job_id,
                    &ctx.instance_id,
                    &msg,
                    retry::retry_delay(job.attempts),
                )
                .await?;
            Ok(true)
        }
    }
}

async fn record_userop_poll_retryable(
    ctx: &JobCtx,
    job: &SolverJob,
    kind: HubUserOpKind,
    msg: &str,
) -> Result<()> {
    ctx.db
        .record_hub_userop_retryable_error(
            job.job_id,
            &ctx.instance_id,
            kind,
            msg,
            retry::retry_delay(job.attempts),
        )
        .await
        .ok();
    ctx.db
        .record_retryable_error(
            job.job_id,
            &ctx.instance_id,
            msg,
            retry::retry_delay(job.attempts),
        )
        .await
}

pub(super) async fn process_ready_state(
    ctx: &JobCtx,
    job: &SolverJob,
    id: B256,
    ty: IntentType,
) -> Result<()> {
    tracing::info!(id = %id, intent_type = job.intent_type, "claiming intent");
    if let Some((secs_left, reason)) = ctx.db.global_pause_active().await? {
        ctx.telemetry.global_paused();
        let msg = format!(
            "global_pause: {}",
            reason.unwrap_or_else(|| "paused".to_string())
        );
        ctx.db
            .record_retryable_error(
                job.job_id,
                &ctx.instance_id,
                &msg,
                std::time::Duration::from_secs(u64::try_from(secs_left).unwrap_or(1)),
            )
            .await?;
        return Ok(());
    }
    // Ensure claim deposit can be pulled. We do this here (rather than at startup) so:
    // - the solver starts even if AA infrastructure is temporarily down
    // - retries are controlled by the job state machine
    let usdt = match ctx.hub.pool_usdt().await {
        Ok(v) => v,
        Err(err) => {
            let msg = format!("pool_usdt failed: {err:#}");
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
    if let Err(err) = ctx
        .hub
        .ensure_erc20_allowance(
            usdt,
            ctx.hub.pool_address(),
            U256::from(INTENT_CLAIM_DEPOSIT),
        )
        .await
    {
        let msg = format!("ensure_erc20_allowance failed: {err:#}");
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
    match ctx.cfg.hub.tx_mode {
        HubTxMode::Eoa => {
            if let Some(wait) = retry::enforce_claim_rate_limits(ctx, ty).await? {
                ctx.db
                    .record_retryable_error(
                        job.job_id,
                        &ctx.instance_id,
                        "claim_rate_limited",
                        std::time::Duration::from_secs(u64::try_from(wait).unwrap_or(60)),
                    )
                    .await?;
                return Ok(());
            }
            if ty == IntentType::DelegateResource
                && ctx.cfg.tron.mode == TronMode::Grpc
                && !ctx.cfg.tron.delegate_resource_resell_enabled
                && let Err(err) = ensure_delegate_reservation(ctx, job).await
            {
                let msg = format!("delegate reservation failed: {err:#}");
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

            match ctx.hub.claim_intent(id).await {
                Ok(receipt) => {
                    ctx.db
                        .record_claim(
                            job.job_id,
                            &ctx.instance_id,
                            b256_to_bytes32(receipt.transaction_hash),
                        )
                        .await?;
                    ctx.telemetry
                        .job_state_transition(job.intent_type, "ready", "claimed");
                    Ok(())
                }
                Err(err) => {
                    let msg = err.to_string();
                    // If already claimed, don't keep retrying.
                    if msg.contains("AlreadyClaimed") {
                        ctx.telemetry.job_state_transition(
                            job.intent_type,
                            "ready",
                            "failed_fatal",
                        );
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
                    Ok(())
                }
            }
        }
        HubTxMode::Safe4337 => {
            let kind = HubUserOpKind::Claim;
            let mut row = ctx.db.get_hub_userop(job.job_id, kind).await?;
            if let Some(r) = row.as_ref() {
                // If we've already included it, we should have advanced state.
                if r.state == "included" {
                    return Ok(());
                }
                if delete_stale_prepared_userop_if_needed(
                    ctx,
                    job,
                    kind,
                    r,
                    "deserialize claim userop",
                )
                .await?
                {
                    row = None;
                }
            }

            let prepared_userop_json = row
                .as_ref()
                .filter(|r| r.userop_hash.is_none() && r.state == "prepared")
                .map(|r| r.userop_json.as_str());
            if row.is_none() || prepared_userop_json.is_some() {
                if enforce_claim_submission_preconditions(ctx, job, ty).await? {
                    return Ok(());
                }
                let should_retry = submit_safe4337_userop(
                    ctx,
                    job,
                    kind,
                    "claim_userop",
                    "acquire hub_userop_submit_sem (claim)",
                    "deserialize claim userop",
                    "serialize claim userop",
                    "build claimIntent userop",
                    || async {
                        let call = crate::hub::IUntronIntents::claimIntentCall { id };
                        ctx.hub
                            .safe4337_build_call_userop(ctx.hub.pool_address(), call.abi_encode())
                            .await
                    },
                    prepared_userop_json,
                )
                .await?;
                if should_retry {
                    return Ok(());
                }
            }

            // Poll receipt if we have a userop hash.
            let row = ctx.db.get_hub_userop(job.job_id, kind).await?;
            let Some(r) = row else {
                return Ok(());
            };
            let Some(userop_hash) = r.userop_hash.clone() else {
                return Ok(());
            };

            match ctx.hub.safe4337_get_userop_receipt(&userop_hash).await {
                Ok(Some(receipt)) => {
                    let Some(tx_hash) = receipt.tx_hash else {
                        return Ok(());
                    };
                    let success = receipt.success.unwrap_or(false);
                    let receipt_json =
                        serde_json::to_string(&receipt.raw).unwrap_or_else(|_| "{}".to_string());
                    ctx.db
                        .record_hub_userop_included(
                            job.job_id,
                            &ctx.instance_id,
                            kind,
                            b256_to_bytes32(tx_hash),
                            receipt.block_number.map(|n| n as i64),
                            success,
                            receipt.actual_gas_cost_wei,
                            receipt.actual_gas_used,
                            &receipt_json,
                        )
                        .await?;
                    if success {
                        ctx.db
                            .record_claim(job.job_id, &ctx.instance_id, b256_to_bytes32(tx_hash))
                            .await?;
                        ctx.telemetry
                            .job_state_transition(job.intent_type, "ready", "claimed");
                    } else {
                        let msg = format!(
                            "claim userop failed: {:?}",
                            receipt.reason.unwrap_or(serde_json::Value::Null)
                        );
                        ctx.db
                            .record_hub_userop_fatal_error(job.job_id, &ctx.instance_id, kind, &msg)
                            .await
                            .ok();
                        retry::record_fatal(ctx, job, &msg).await?;
                    }
                    Ok(())
                }
                Ok(None) => Ok(()),
                Err(err) => {
                    let msg = err.to_string();
                    record_userop_poll_retryable(ctx, job, kind, &msg).await?;
                    Ok(())
                }
            }
        }
    }
}

pub(super) async fn process_proof_built_state(
    ctx: &JobCtx,
    job: &SolverJob,
    id: B256,
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
    let proof = ctx.db.load_tron_proof(txid).await?;
    let tron = TronProof {
        blocks: std::array::from_fn(|i| proof.blocks[i].clone()),
        encoded_tx: proof.encoded_tx,
        proof: proof
            .proof
            .into_iter()
            .map(|b| B256::from_slice(&b))
            .collect(),
        index: crate::types::parse_u256_dec(&proof.index_dec).unwrap_or(U256::ZERO),
    };
    tracing::info!(id = %id, "submitting proveIntentFill");
    match ctx.cfg.hub.tx_mode {
        HubTxMode::Eoa => match ctx.hub.prove_intent_fill(id, tron).await {
            Ok(receipt) => {
                ctx.db
                    .record_prove(
                        job.job_id,
                        &ctx.instance_id,
                        b256_to_bytes32(receipt.transaction_hash),
                    )
                    .await?;
                ctx.telemetry
                    .job_state_transition(job.intent_type, "proof_built", "proved");
                let _ = finalize_after_prove(ctx, job).await;
                Ok(())
            }
            Err(err) => {
                let msg = err.to_string();
                ctx.db
                    .record_retryable_error(
                        job.job_id,
                        &ctx.instance_id,
                        &msg,
                        retry::retry_delay(job.attempts),
                    )
                    .await?;
                Ok(())
            }
        },
        HubTxMode::Safe4337 => {
            let kind = HubUserOpKind::Prove;
            let mut row = ctx.db.get_hub_userop(job.job_id, kind).await?;
            if let Some(r) = row.as_ref() {
                if r.state == "included" {
                    return Ok(());
                }
                if delete_stale_prepared_userop_if_needed(
                    ctx,
                    job,
                    kind,
                    r,
                    "deserialize prove userop",
                )
                .await?
                {
                    row = None;
                }
            }

            let prepared_userop_json = row
                .as_ref()
                .filter(|r| r.userop_hash.is_none() && r.state == "prepared")
                .map(|r| r.userop_json.as_str());
            if row.is_none() || prepared_userop_json.is_some() {
                let tron = tron.clone();
                let should_retry = submit_safe4337_userop(
                    ctx,
                    job,
                    kind,
                    "prove_userop",
                    "acquire hub_userop_submit_sem (prove)",
                    "deserialize prove userop",
                    "serialize prove userop",
                    "build proveIntentFill userop",
                    || async move {
                        let call = crate::hub::IUntronIntents::proveIntentFillCall {
                            id,
                            blocks: tron.blocks.map(alloy::primitives::Bytes::from),
                            encodedTx: tron.encoded_tx.into(),
                            proof: tron.proof,
                            index: tron.index,
                        };
                        ctx.hub
                            .safe4337_build_call_userop(ctx.hub.pool_address(), call.abi_encode())
                            .await
                    },
                    prepared_userop_json,
                )
                .await?;
                if should_retry {
                    return Ok(());
                }
            }

            let row = ctx.db.get_hub_userop(job.job_id, kind).await?;
            let Some(r) = row else {
                return Ok(());
            };
            let Some(userop_hash) = r.userop_hash.clone() else {
                return Ok(());
            };

            match ctx.hub.safe4337_get_userop_receipt(&userop_hash).await {
                Ok(Some(receipt)) => {
                    let Some(tx_hash) = receipt.tx_hash else {
                        return Ok(());
                    };
                    let success = receipt.success.unwrap_or(false);
                    let receipt_json =
                        serde_json::to_string(&receipt.raw).unwrap_or_else(|_| "{}".to_string());
                    ctx.db
                        .record_hub_userop_included(
                            job.job_id,
                            &ctx.instance_id,
                            kind,
                            b256_to_bytes32(tx_hash),
                            receipt.block_number.map(|n| n as i64),
                            success,
                            receipt.actual_gas_cost_wei,
                            receipt.actual_gas_used,
                            &receipt_json,
                        )
                        .await?;
                    if success {
                        ctx.db
                            .record_prove(job.job_id, &ctx.instance_id, b256_to_bytes32(tx_hash))
                            .await?;
                        ctx.telemetry.job_state_transition(
                            job.intent_type,
                            "proof_built",
                            "proved",
                        );
                        let _ = finalize_after_prove(ctx, job).await;
                    } else {
                        let msg = format!(
                            "prove userop failed: {:?}",
                            receipt.reason.unwrap_or(serde_json::Value::Null)
                        );
                        ctx.db
                            .record_hub_userop_fatal_error(job.job_id, &ctx.instance_id, kind, &msg)
                            .await
                            .ok();
                        retry::record_fatal(ctx, job, &msg).await?;
                    }
                    Ok(())
                }
                Ok(None) => Ok(()),
                Err(err) => {
                    let msg = err.to_string();
                    record_userop_poll_retryable(ctx, job, kind, &msg).await?;
                    Ok(())
                }
            }
        }
    }
}

pub(super) async fn process_proved_state(
    ctx: &JobCtx,
    job: &SolverJob,
    state: JobState,
) -> Result<()> {
    // A proved intent may not be paid immediately (virtual receiver intents), or may require
    // a separate settle. We use indexer truth to decide when we're really "done".
    let intent_id_hex = format!("0x{}", hex::encode(job.intent_id));
    match ctx.indexer.fetch_intent(&intent_id_hex).await {
        Ok(Some(row)) => {
            let from_state: &'static str = match state {
                JobState::Proved => JobState::Proved.as_db_str(),
                JobState::ProvedWaitingFunding => JobState::ProvedWaitingFunding.as_db_str(),
                JobState::ProvedWaitingSettlement => JobState::ProvedWaitingSettlement.as_db_str(),
                _ => JobState::Proved.as_db_str(),
            };
            if row.closed {
                ctx.db.record_done(job.job_id, &ctx.instance_id).await?;
                ctx.telemetry
                    .job_state_transition(job.intent_type, from_state, "done");
                return Ok(());
            }
            if row.solved && row.funded && row.settled {
                ctx.db.record_done(job.job_id, &ctx.instance_id).await?;
                ctx.telemetry
                    .job_state_transition(job.intent_type, from_state, "done");
                return Ok(());
            }
            if row.solved && !row.funded {
                if state != JobState::ProvedWaitingFunding {
                    ctx.db
                        .record_job_state(job.job_id, &ctx.instance_id, "proved_waiting_funding")
                        .await?;
                    ctx.telemetry.job_state_transition(
                        job.intent_type,
                        from_state,
                        "proved_waiting_funding",
                    );
                }
                return Ok(());
            }
            if row.solved && row.funded && !row.settled {
                if state != JobState::ProvedWaitingSettlement {
                    ctx.db
                        .record_job_state(job.job_id, &ctx.instance_id, "proved_waiting_settlement")
                        .await?;
                    ctx.telemetry.job_state_transition(
                        job.intent_type,
                        from_state,
                        "proved_waiting_settlement",
                    );
                }
                return Ok(());
            }
            Ok(())
        }
        Ok(None) => Ok(()),
        Err(err) => {
            tracing::warn!(err = %err, "failed to query pool_intents for proved job; keeping state");
            Ok(())
        }
    }
}
