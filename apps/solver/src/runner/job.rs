use super::{JobCtx, SolverJob, hub_flow, retry, tron_flow};
use crate::types::{IntentType, JobState};
use alloy::primitives::B256;
use alloy::sol_types::SolValue;
use anyhow::{Context, Result};

pub(super) async fn ensure_delegate_reservation(ctx: &JobCtx, job: &SolverJob) -> Result<[u8; 32]> {
    let intent = crate::tron_backend::DelegateResourceIntent::abi_decode(&job.intent_specs)
        .context("decode DelegateResourceIntent")?;
    let needed = i64::try_from(intent.balanceSun).unwrap_or(i64::MAX);
    let resource_i16 = i16::from(intent.resource);
    let rc = match intent.resource {
        0 => tron::protocol::ResourceCode::Bandwidth,
        1 => tron::protocol::ResourceCode::Energy,
        2 => tron::protocol::ResourceCode::TronPower,
        other => anyhow::bail!("unsupported DelegateResourceIntent.resource: {other}"),
    };

    // If already reserved, just refresh TTL and return the chosen key.
    if let Some(existing) = ctx.db.get_delegate_reservation_for_job(job.job_id).await? {
        ctx.db
            .upsert_delegate_reservation_for_job(
                job.job_id,
                &existing.owner_address,
                existing.resource,
                existing.amount_sun,
                i64::try_from(ctx.cfg.jobs.delegate_reservation_ttl_secs).unwrap_or(600),
            )
            .await?;
        return ctx
            .tron
            .private_key_for_owner(&existing.owner_address)
            .context("delegate reservation owner not in configured keys");
    }

    let by_key = ctx.tron.delegate_available_sun_by_key(rc).await?;
    let reserved = ctx
        .db
        .sum_delegate_reserved_sun_by_owner(resource_i16)
        .await?;
    let mut reserved_map = std::collections::HashMap::<Vec<u8>, i64>::new();
    for (owner, amt) in reserved {
        reserved_map.insert(owner, amt);
    }

    let mut owners = Vec::with_capacity(by_key.len());
    let mut avail = Vec::with_capacity(by_key.len());
    let mut resv = Vec::with_capacity(by_key.len());
    for (addr, a) in &by_key {
        let owner = addr.prefixed_bytes().to_vec();
        let r = *reserved_map.get(&owner).unwrap_or(&0);
        owners.push(owner);
        avail.push(*a);
        resv.push(r);
    }

    let Some(idx) = crate::tron_backend::select_delegate_executor_index(&avail, &resv, needed)
    else {
        ctx.telemetry.delegate_reservation_conflict();
        anyhow::bail!("delegate_capacity_insufficient");
    };

    let owner = owners[idx].clone();
    ctx.db
        .upsert_delegate_reservation_for_job(
            job.job_id,
            &owner,
            resource_i16,
            needed,
            i64::try_from(ctx.cfg.jobs.delegate_reservation_ttl_secs).unwrap_or(600),
        )
        .await?;
    ctx.tron
        .private_key_for_owner(&owner)
        .context("delegate reservation owner not in configured keys")
}

pub(super) async fn finalize_after_prove(ctx: &JobCtx, job: &SolverJob) -> Result<()> {
    let id = B256::from_slice(&job.intent_id);
    // After proving, the hub state is the source of truth. Poll briefly so downstream tooling/tests
    // don't depend on "one extra solver tick".
    for _ in 0..25 {
        match ctx.hub.intent_status(id).await {
            Ok(status) => {
                if status.closed || (status.solved && status.funded && status.settled) {
                    ctx.db.record_done(job.job_id, &ctx.instance_id).await?;
                    ctx.telemetry
                        .job_state_transition(job.intent_type, "proved", "done");
                    return Ok(());
                }
                if status.solved && !status.funded {
                    ctx.db
                        .record_job_state(
                            job.job_id,
                            &ctx.instance_id,
                            JobState::ProvedWaitingFunding,
                        )
                        .await?;
                    ctx.telemetry.job_state_transition(
                        job.intent_type,
                        "proved",
                        "proved_waiting_funding",
                    );
                    return Ok(());
                }
                if status.solved && status.funded && !status.settled {
                    ctx.db
                        .record_job_state(
                            job.job_id,
                            &ctx.instance_id,
                            JobState::ProvedWaitingSettlement,
                        )
                        .await?;
                    ctx.telemetry.job_state_transition(
                        job.intent_type,
                        "proved",
                        "proved_waiting_settlement",
                    );
                    return Ok(());
                }
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                continue;
            }
            Err(_) => {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                continue;
            }
        }
    }
    Ok(())
}

pub(super) async fn process_job(ctx: JobCtx, job: SolverJob) -> Result<()> {
    let id = B256::from_slice(&job.intent_id);
    let ty = IntentType::from_i16(job.intent_type)?;
    let state = match JobState::parse(&job.state) {
        Ok(state) => state,
        Err(_) => {
            ctx.telemetry
                .job_state_transition(job.intent_type, "unknown", "failed_fatal");
            retry::record_fatal(&ctx, &job, &format!("unknown job state: {}", job.state)).await?;
            return Ok(());
        }
    };

    match state {
        JobState::Ready => hub_flow::process_ready_state(&ctx, &job, id, ty).await,
        JobState::Claimed => tron_flow::process_claimed_state(&ctx, &job, id, ty).await,
        JobState::TronPrepared => tron_flow::process_tron_prepared_state(&ctx, &job, ty).await,
        JobState::TronSent => tron_flow::process_tron_sent_state(&ctx, &job, id, ty).await,
        JobState::ProofBuilt => hub_flow::process_proof_built_state(&ctx, &job, id).await,
        JobState::Proved | JobState::ProvedWaitingFunding | JobState::ProvedWaitingSettlement => {
            hub_flow::process_proved_state(&ctx, &job, state).await
        }
        JobState::Done | JobState::FailedFatal => Ok(()),
    }
}

pub(super) fn b256_to_bytes32(v: B256) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(v.as_slice());
    out
}

pub(super) fn duration_hours_for_lock_period_blocks(lock_period_blocks: u64) -> u64 {
    let secs = lock_period_blocks.saturating_mul(3);
    let hours = secs.saturating_add(3599) / 3600;
    hours.max(1)
}

pub(super) fn looks_like_tron_server_busy(msg: &str) -> bool {
    msg.contains("SERVER_BUSY")
}

pub(super) fn looks_like_tron_contract_failure(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("revert")
        || m.contains("contract_validate_error")
        || m.contains("contract validate error")
        || m.contains("out_of_energy")
        || m.contains("out of energy")
        || m.contains("validate")
}

pub(super) fn decode_trigger_contract_and_selector(
    intent_specs: &[u8],
) -> Option<(alloy::primitives::Address, Option<[u8; 4]>)> {
    use alloy::sol_types::SolValue;

    let intent = crate::tron_backend::TriggerSmartContractIntent::abi_decode(intent_specs).ok()?;
    let data = intent.data.as_ref();
    let selector = if data.len() >= 4 {
        let mut out = [0u8; 4];
        out.copy_from_slice(&data[..4]);
        Some(out)
    } else {
        None
    };
    Some((intent.to, selector))
}
