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

const CLAIM_WINDOW_SECS: i64 = 120;

fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

async fn refresh_claim_window_expires_at(ctx: &JobCtx, job: &SolverJob, id: B256) -> Result<()> {
    let fallback = now_unix_secs().saturating_add(CLAIM_WINDOW_SECS);
    let expires_at_unix = match ctx.hub.intent_solver_claimed_at(id).await {
        Ok((solver, claimed_at)) if solver == ctx.hub.solver_address() && claimed_at > 0 => {
            i64::try_from(claimed_at)
                .unwrap_or(i64::MAX)
                .saturating_add(CLAIM_WINDOW_SECS)
        }
        Ok((solver, claimed_at)) => {
            tracing::warn!(
                id = %id,
                solver = ?solver,
                claimed_at,
                "unable to confirm onchain claim timestamp for this solver; using fallback expiry"
            );
            fallback
        }
        Err(err) => {
            tracing::warn!(
                id = %id,
                err = %err,
                "failed to fetch onchain claim timestamp; using fallback expiry"
            );
            fallback
        }
    };
    ctx.db
        .set_claim_window_expires_at(job.job_id, &ctx.instance_id, Some(expires_at_unix))
        .await?;
    Ok(())
}

async fn enforce_claim_submission_preconditions(
    ctx: &JobCtx,
    job: &SolverJob,
    ty: IntentType,
) -> Result<bool> {
    if ctx.cfg.hub.tx_mode == HubTxMode::Safe4337 {
        let safe4337_max_claimed_unproved_jobs = i64::try_from(
            ctx.cfg.jobs.safe4337_max_claimed_unproved_jobs,
        )
        .unwrap_or(i64::MAX)
        .max(1);
        let claimed_unproved = ctx.db.count_claimed_unproved_jobs().await?;
        if claimed_unproved >= safe4337_max_claimed_unproved_jobs {
            ctx.db
                .record_retryable_error(
                    job.job_id,
                    &ctx.instance_id,
                    "claim_window_backpressure",
                    std::time::Duration::from_secs(5),
                )
                .await?;
            return Ok(true);
        }
    }
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

fn should_rebuild_prepared_userop_on_submit_error(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("aa25 invalid account nonce")
        || m.contains("invalid account nonce")
        || m.contains("nonce too low")
        || m.contains("nonce too high")
        || m.contains("replacement transaction underpriced")
        || m.contains("underpriced")
        || m.contains("fee too low")
        || m.contains("max fee per gas less than block base fee")
        || m.contains("maxfeepergas")
        || m.contains("maxpriorityfeepergas")
        || (m.contains("aa3") && m.contains("paymaster"))
        || (m.contains("paymaster")
            && (m.contains("expired") || m.contains("deposit") || m.contains("stake")))
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
            if should_rebuild_prepared_userop_on_submit_error(&msg) {
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

#[cfg(test)]
mod tests {
    use super::should_rebuild_prepared_userop_on_submit_error;

    #[test]
    fn rebuild_userop_on_nonce_errors() {
        assert!(should_rebuild_prepared_userop_on_submit_error(
            "AA25 invalid account nonce",
        ));
        assert!(should_rebuild_prepared_userop_on_submit_error(
            "nonce too low",
        ));
    }

    #[test]
    fn rebuild_userop_on_fee_errors() {
        assert!(should_rebuild_prepared_userop_on_submit_error(
            "replacement transaction underpriced",
        ));
        assert!(should_rebuild_prepared_userop_on_submit_error(
            "max fee per gas less than block base fee",
        ));
    }

    #[test]
    fn rebuild_userop_on_paymaster_errors() {
        assert!(should_rebuild_prepared_userop_on_submit_error(
            "AA31 paymaster deposit too low",
        ));
    }

    #[test]
    fn keep_prepared_userop_for_non_rebuild_errors() {
        assert!(!should_rebuild_prepared_userop_on_submit_error(
            "insufficient funds for transfer",
        ));
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

fn included_userop_failure_message(prefix: &str, row: &HubUserOpRow) -> String {
    match row.receipt_json.as_deref() {
        Some(receipt) => format!("{prefix}: {receipt}"),
        None => prefix.to_string(),
    }
}

async fn reconcile_included_claim_userop(
    ctx: &JobCtx,
    job: &SolverJob,
    id: B256,
    row: &HubUserOpRow,
) -> Result<()> {
    if !row.success.unwrap_or(false) {
        let msg = included_userop_failure_message("claim userop failed", row);
        retry::record_fatal(ctx, job, &msg).await?;
        return Ok(());
    }

    let Some(tx_hash) = row.tx_hash else {
        ctx.db
            .record_retryable_error(
                job.job_id,
                &ctx.instance_id,
                "claim userop included without tx_hash",
                retry::retry_delay(job.attempts),
            )
            .await?;
        return Ok(());
    };

    ctx.db
        .record_claim(job.job_id, &ctx.instance_id, tx_hash)
        .await?;
    refresh_claim_window_expires_at(ctx, job, id).await?;
    ctx.telemetry
        .job_state_transition(job.intent_type, "ready", "claimed");
    Ok(())
}

async fn reconcile_included_prove_userop(
    ctx: &JobCtx,
    job: &SolverJob,
    row: &HubUserOpRow,
) -> Result<()> {
    if !row.success.unwrap_or(false) {
        let msg = included_userop_failure_message("prove userop failed", row);
        retry::record_fatal(ctx, job, &msg).await?;
        return Ok(());
    }

    let Some(tx_hash) = row.tx_hash else {
        ctx.db
            .record_retryable_error(
                job.job_id,
                &ctx.instance_id,
                "prove userop included without tx_hash",
                retry::retry_delay(job.attempts),
            )
            .await?;
        return Ok(());
    };

    ctx.db
        .record_prove(job.job_id, &ctx.instance_id, tx_hash)
        .await?;
    ctx.telemetry
        .job_state_transition(job.intent_type, "proof_built", "proved");
    let _ = finalize_after_prove(ctx, job).await;
    Ok(())
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
                    refresh_claim_window_expires_at(ctx, job, id).await?;
                    ctx.telemetry
                        .job_state_transition(job.intent_type, "ready", "claimed");
                    Ok(())
                }
                Err(err) => {
                    let msg = err.to_string();
                    if msg.contains("AlreadyClaimed") {
                        match ctx.hub.intent_solver(id).await {
                            Ok(solver) if solver == ctx.hub.solver_address() => {
                                ctx.db
                                    .record_job_state(
                                        job.job_id,
                                        &ctx.instance_id,
                                        JobState::Claimed,
                                    )
                                    .await?;
                                refresh_claim_window_expires_at(ctx, job, id).await?;
                                ctx.telemetry.job_state_transition(
                                    job.intent_type,
                                    "ready",
                                    "claimed",
                                );
                                return Ok(());
                            }
                            Ok(_) => {
                                ctx.telemetry.job_state_transition(
                                    job.intent_type,
                                    "ready",
                                    "failed_fatal",
                                );
                                retry::record_fatal(ctx, job, &msg).await?;
                                return Ok(());
                            }
                            Err(err) => {
                                let reconcile_msg =
                                    format!("already claimed; failed to reconcile solver: {err:#}");
                                ctx.db
                                    .record_retryable_error(
                                        job.job_id,
                                        &ctx.instance_id,
                                        &reconcile_msg,
                                        retry::retry_delay(job.attempts),
                                    )
                                    .await?;
                                return Ok(());
                            }
                        }
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
                // Recover from partial persistence: included userop without advanced job state.
                if r.state == "included" {
                    reconcile_included_claim_userop(ctx, job, id, r).await?;
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
                        refresh_claim_window_expires_at(ctx, job, id).await?;
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
                match ctx.hub.intent_status(id).await {
                    Ok(status) if status.solved => {
                        ctx.db
                            .record_job_state(job.job_id, &ctx.instance_id, JobState::Proved)
                            .await?;
                        ctx.telemetry.job_state_transition(
                            job.intent_type,
                            "proof_built",
                            "proved",
                        );
                        let _ = finalize_after_prove(ctx, job).await;
                        Ok(())
                    }
                    Ok(_) => {
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
                    Err(status_err) => {
                        let combined =
                            format!("prove failed: {msg}; intent_status failed: {status_err:#}");
                        ctx.db
                            .record_retryable_error(
                                job.job_id,
                                &ctx.instance_id,
                                &combined,
                                retry::retry_delay(job.attempts),
                            )
                            .await?;
                        Ok(())
                    }
                }
            }
        },
        HubTxMode::Safe4337 => {
            let kind = HubUserOpKind::Prove;
            let mut row = ctx.db.get_hub_userop(job.job_id, kind).await?;
            if let Some(r) = row.as_ref() {
                // Recover from partial persistence: included userop without advanced job state.
                if r.state == "included" {
                    reconcile_included_prove_userop(ctx, job, r).await?;
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
                        .record_job_state(
                            job.job_id,
                            &ctx.instance_id,
                            JobState::ProvedWaitingFunding,
                        )
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
                        .record_job_state(
                            job.job_id,
                            &ctx.instance_id,
                            JobState::ProvedWaitingSettlement,
                        )
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
