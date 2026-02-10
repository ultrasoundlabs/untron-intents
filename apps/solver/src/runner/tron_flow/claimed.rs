use super::super::{
    JobCtx, SolverJob, b256_to_bytes32, decode_trigger_contract_and_selector,
    duration_hours_for_lock_period_blocks, ensure_delegate_reservation,
    lease, looks_like_tron_contract_failure, looks_like_tron_server_busy, retry,
};
use crate::{
    config::TronMode,
    db::{TronProofRow, TronSignedTxRow},
    tron_backend::TronExecution,
    types::IntentType,
};
use alloy::primitives::B256;
use alloy::sol_types::SolValue;
use anyhow::{Context, Result};
use std::time::Instant;

pub(crate) async fn process_claimed_state(
    ctx: &JobCtx,
    job: &SolverJob,
    id: B256,
    ty: IntentType,
) -> Result<()> {
    if ctx.cfg.tron.mode == TronMode::Mock {
        return process_claimed_state_mock(ctx, job, id, ty).await;
    }

    tracing::info!(id = %id, "preparing tron tx (persist signed bytes)");
    // TRX/USDT can optionally require consolidation (multi-key pre-txs). We persist the
    // whole plan as (pre txs + final tx), then broadcast them in order in tron_prepared.
    if matches!(ty, IntentType::TrxTransfer | IntentType::UsdtTransfer) {
        let plan = match ty {
            IntentType::TrxTransfer => {
                lease::with_lease_heartbeat(
                    ctx,
                    job.job_id,
                    ctx.tron.prepare_trx_transfer_plan(&job.intent_specs),
                )
                .await
                .context("prepare trx transfer plan")?
            }
            IntentType::UsdtTransfer => {
                lease::with_lease_heartbeat(
                    ctx,
                    job.job_id,
                    ctx.tron
                        .prepare_usdt_transfer_plan(ctx.hub.as_ref(), &job.intent_specs),
                )
                .await
                .context("prepare usdt transfer plan")?
            }
            _ => unreachable!(),
        };

        let pre_rows = plan
            .pre_txs
            .iter()
            .enumerate()
            .map(|(i, p)| TronSignedTxRow {
                step: format!("pre:{i:04}"),
                txid: p.txid,
                tx_bytes: p.tx_bytes.clone(),
                fee_limit_sun: p.fee_limit_sun,
                energy_required: p.energy_required,
                tx_size_bytes: p.tx_size_bytes,
            })
            .collect::<Vec<_>>();
        let final_row = TronSignedTxRow {
            step: "final".to_string(),
            txid: plan.final_tx.txid,
            tx_bytes: plan.final_tx.tx_bytes,
            fee_limit_sun: plan.final_tx.fee_limit_sun,
            energy_required: plan.final_tx.energy_required,
            tx_size_bytes: plan.final_tx.tx_size_bytes,
        };

        ctx.db
            .record_tron_plan(job.job_id, &ctx.instance_id, &pre_rows, &final_row)
            .await?;
        return Ok(());
    }

    // Delegate resource resell path: request a rental provider to broadcast the
    // onchain `DelegateResourceContract` and prove it once included.
    if ty == IntentType::DelegateResource
        && ctx.cfg.tron.delegate_resource_resell_enabled
        && process_delegate_resource_resell(ctx, job).await?
    {
        return Ok(());
    }

    let exec_res = match ty {
        IntentType::TriggerSmartContract => lease::with_lease_heartbeat(
            ctx,
            job.job_id,
            ctx.tron
                .prepare_trigger_smart_contract(ctx.hub.as_ref(), id, &job.intent_specs),
        )
        .await
        .context("prepare trigger smart contract"),
        IntentType::DelegateResource => {
            let pk = match ctx.db.get_delegate_reservation_for_job(job.job_id).await? {
                Some(r) => {
                    // Refresh TTL while in-flight.
                    let _ = ctx
                        .db
                        .upsert_delegate_reservation_for_job(
                            job.job_id,
                            &r.owner_address,
                            r.resource,
                            r.amount_sun,
                            i64::try_from(ctx.cfg.jobs.delegate_reservation_ttl_secs)
                                .unwrap_or(600),
                        )
                        .await;
                    ctx.tron
                        .private_key_for_owner(&r.owner_address)
                        .context("delegate reservation owner not in configured keys")?
                }
                None => ensure_delegate_reservation(ctx, job).await?,
            };
            lease::with_lease_heartbeat(
                ctx,
                job.job_id,
                ctx.tron.prepare_delegate_resource_with_key(
                    ctx.hub.as_ref(),
                    id,
                    pk,
                    &job.intent_specs,
                ),
            )
            .await
                .context("prepare delegate resource (reserved key)")
        }
        _ => unreachable!(),
    };

    let exec = match exec_res {
        Ok(v) => v,
        Err(err) => {
            handle_prepare_error(ctx, job, ty, &format!("{err:#}")).await?;
            return Ok(());
        }
    };

    match exec {
        TronExecution::ImmediateProof(_) => {
            anyhow::bail!("unexpected ImmediateProof in TRON_MODE=grpc")
        }
        TronExecution::PreparedTx(p) => {
            ctx.db
                .record_tron_prepared(
                    job.job_id,
                    &ctx.instance_id,
                    p.txid,
                    &p.tx_bytes,
                    p.fee_limit_sun,
                    p.energy_required,
                    p.tx_size_bytes,
                )
                .await?;
            Ok(())
        }
    }
}

async fn process_claimed_state_mock(
    ctx: &JobCtx,
    job: &SolverJob,
    id: B256,
    ty: IntentType,
) -> Result<()> {
    tracing::info!(id = %id, "executing tron tx (mock)");
    let exec_res = match ty {
        IntentType::TriggerSmartContract => lease::with_lease_heartbeat(
            ctx,
            job.job_id,
            ctx.tron
                .prepare_trigger_smart_contract(ctx.hub.as_ref(), id, &job.intent_specs),
        )
        .await
        .context("execute trigger smart contract"),
        IntentType::TrxTransfer => lease::with_lease_heartbeat(
            ctx,
            job.job_id,
            ctx.tron.prepare_trx_transfer(ctx.hub.as_ref(), id, &job.intent_specs),
        )
        .await
        .context("execute trx transfer"),
        IntentType::UsdtTransfer => lease::with_lease_heartbeat(
            ctx,
            job.job_id,
            ctx.tron
                .prepare_usdt_transfer(ctx.hub.as_ref(), id, &job.intent_specs),
        )
        .await
        .context("execute usdt transfer"),
        IntentType::DelegateResource => lease::with_lease_heartbeat(
            ctx,
            job.job_id,
            ctx.tron
                .prepare_delegate_resource(ctx.hub.as_ref(), id, &job.intent_specs),
        )
        .await
        .context("execute delegate resource"),
    };

    let exec = match exec_res {
        Ok(v) => v,
        Err(err) => {
            handle_prepare_error(ctx, job, ty, &format!("{err:#}")).await?;
            return Ok(());
        }
    };

    match exec {
        TronExecution::ImmediateProof(tron) => {
            // Store proof first; submit prove in the next tick (restart-safe).
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
            let txid = job
                .tron_txid
                .unwrap_or_else(|| b256_to_bytes32(alloy::primitives::keccak256(id.as_slice())));
            ctx.db.save_tron_proof(txid, &proof_row).await?;
            ctx.db
                .record_tron_txid(job.job_id, &ctx.instance_id, txid)
                .await?;
            ctx.db
                .record_proof_built(job.job_id, &ctx.instance_id)
                .await?;
            Ok(())
        }
        TronExecution::PreparedTx(_) => anyhow::bail!("unexpected PreparedTx in TRON_MODE=mock"),
    }
}

async fn process_delegate_resource_resell(ctx: &JobCtx, job: &SolverJob) -> Result<bool> {
    let intent = crate::tron_backend::DelegateResourceIntent::abi_decode(&job.intent_specs)
        .context("decode DelegateResourceIntent")?;
    // Resell ENERGY only; bandwidth/TRON_POWER use the solver's own capacity.
    if intent.resource != 1 {
        return Ok(false);
    }

    let existing = ctx.db.get_tron_rental_for_job(job.job_id).await?;
    let txid = if let Some(r) = existing.as_ref().and_then(|r| r.txid) {
        r
    } else {
        let receiver = tron::TronAddress::from_evm(intent.receiver);
        let balance_sun_i64 =
            i64::try_from(intent.balanceSun).context("balanceSun out of i64 range")?;
        let lock_period_i64 =
            i64::try_from(intent.lockPeriod).context("lockPeriod out of i64 range")?;

        let mut recv = [0u8; 20];
        recv.copy_from_slice(intent.receiver.as_slice());

        let totals = ctx.tron.energy_stake_totals().await?;
        let units = tron::resources::resource_units_for_min_trx_sun(
            u64::try_from(balance_sun_i64.max(0)).unwrap_or(0),
            totals,
            ctx.cfg.tron.resell_energy_headroom_ppm,
        );
        let duration_hours = duration_hours_for_lock_period_blocks(
            u64::try_from(lock_period_i64.max(0)).unwrap_or(0),
        );

        // Prefer the pre-quoted provider (if present).
        let preferred = existing.as_ref().map(|r| r.provider.as_str());
        let ctx_rent = tron::RentalContext {
            resource: tron::RentalResourceKind::Energy,
            amount: units,
            lock_period: Some(u64::try_from(lock_period_i64.max(0)).unwrap_or(0)),
            duration_hours: Some(duration_hours),
            balance_sun: Some(u64::try_from(balance_sun_i64.max(0)).unwrap_or(0)),
            address_base58check: receiver.to_base58check(),
            address_hex41: format!("0x{}", hex::encode(receiver.prefixed_bytes())),
            address_evm_hex: format!("{:#x}", receiver.evm()),
            txid: None,
        };

        let mut last_err: Option<String> = None;
        let mut chosen: Option<(tron::RenderedJsonApiRequest, tron::RentalAttempt)> = None;

        let mut providers = ctx.cfg.tron.energy_rental_providers.clone();
        if let Some(p) = preferred {
            providers.sort_by_key(|c| if c.name == p { 0 } else { 1 });
        }
        for p in &providers {
            let _ = lease::renew_job_lease(ctx, job.job_id).await;
            let provider = tron::JsonApiRentalProvider::new(p.clone());
            if ctx
                .db
                .rental_provider_is_frozen(provider.name())
                .await?
                .is_some()
            {
                continue;
            }

            let started = Instant::now();
            let res = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                provider.rent_with_rendered_request(&ctx_rent),
            )
            .await;
            let ms = started.elapsed().as_millis() as u64;

            match res {
                Ok(Ok((req, attempt))) if attempt.ok && attempt.txid.is_some() => {
                    ctx.telemetry.rental_order_ms(provider.name(), true, ms);
                    chosen = Some((req, attempt));
                    let _ = ctx.db.rental_provider_record_success(provider.name()).await;
                    break;
                }
                Ok(Ok((_req, attempt))) => {
                    ctx.telemetry.rental_order_ms(provider.name(), false, ms);
                    let msg = format!(
                        "ok={} txid={:?} err={:?}",
                        attempt.ok, attempt.txid, attempt.error
                    );
                    last_err = Some(format!("{}: {msg}", provider.name()));
                    let froze = ctx
                        .db
                        .rental_provider_record_failure(
                            provider.name(),
                            ctx.cfg.tron.rental_provider_fail_window_secs,
                            ctx.cfg.tron.rental_provider_freeze_secs,
                            ctx.cfg.tron.rental_provider_fail_threshold,
                            &msg,
                        )
                        .await;
                    if froze.unwrap_or(false) {
                        ctx.telemetry.rental_provider_frozen(provider.name());
                    }
                }
                Ok(Err(err)) => {
                    ctx.telemetry.rental_order_ms(provider.name(), false, ms);
                    let msg = format!("{err:#}");
                    last_err = Some(format!("{}: {msg}", provider.name()));
                    let froze = ctx
                        .db
                        .rental_provider_record_failure(
                            provider.name(),
                            ctx.cfg.tron.rental_provider_fail_window_secs,
                            ctx.cfg.tron.rental_provider_freeze_secs,
                            ctx.cfg.tron.rental_provider_fail_threshold,
                            &msg,
                        )
                        .await;
                    if froze.unwrap_or(false) {
                        ctx.telemetry.rental_provider_frozen(provider.name());
                    }
                }
                Err(_) => {
                    ctx.telemetry.rental_order_ms(provider.name(), false, ms);
                    let msg = "timeout".to_string();
                    last_err = Some(format!("{}: {msg}", provider.name()));
                    let froze = ctx
                        .db
                        .rental_provider_record_failure(
                            provider.name(),
                            ctx.cfg.tron.rental_provider_fail_window_secs,
                            ctx.cfg.tron.rental_provider_freeze_secs,
                            ctx.cfg.tron.rental_provider_fail_threshold,
                            &msg,
                        )
                        .await;
                    if froze.unwrap_or(false) {
                        ctx.telemetry.rental_provider_frozen(provider.name());
                    }
                }
            }
        }

        let Some((rendered_req, attempt)) = chosen else {
            let msg =
                last_err.unwrap_or_else(|| "no energy rental providers succeeded".to_string());
            ctx.db
                .record_retryable_error(
                    job.job_id,
                    &ctx.instance_id,
                    &msg,
                    retry::retry_delay(job.attempts),
                )
                .await?;
            return Ok(true);
        };

        let txid_hex = attempt.txid.as_ref().unwrap();
        let bytes =
            hex::decode(txid_hex.trim_start_matches("0x")).context("decode rental txid hex")?;
        if bytes.len() != 32 {
            anyhow::bail!("rental txid is not 32 bytes: {txid_hex}");
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&bytes);

        let mut request_json = existing
            .as_ref()
            .and_then(|r| r.request_json.clone())
            .unwrap_or_else(|| serde_json::json!({}));
        if !request_json.is_object() {
            request_json = serde_json::json!({});
        }
        request_json["order"] =
            serde_json::to_value(&rendered_req).unwrap_or(serde_json::Value::Null);
        request_json["order_meta"] = serde_json::json!({
            "duration_hours": duration_hours,
            "amount_units": units,
        });

        let mut response_json = existing
            .as_ref()
            .and_then(|r| r.response_json.clone())
            .unwrap_or_else(|| serde_json::json!({}));
        if !response_json.is_object() {
            response_json = serde_json::json!({});
        }
        response_json["order"] = attempt
            .response_json
            .clone()
            .unwrap_or(serde_json::Value::Null);

        // Update rental row with txid once known.
        ctx.db
            .upsert_tron_rental(
                job.job_id,
                &attempt.provider,
                "energy",
                recv,
                balance_sun_i64,
                lock_period_i64,
                attempt.order_id.as_deref(),
                Some(out),
                Some(&request_json),
                Some(&response_json),
            )
            .await
            .ok();

        out
    };

    ctx.db
        .record_tron_txid(job.job_id, &ctx.instance_id, txid)
        .await?;
    Ok(true)
}

async fn handle_prepare_error(
    ctx: &JobCtx,
    job: &SolverJob,
    ty: IntentType,
    msg: &str,
) -> Result<()> {
    // If this is a trigger smart contract, record a breaker on likely-deterministic failures.
    if ty == IntentType::TriggerSmartContract
        && !looks_like_tron_server_busy(msg)
        && looks_like_tron_contract_failure(msg)
        && let Some((contract, selector)) = decode_trigger_contract_and_selector(&job.intent_specs)
    {
        let _ = ctx.db.breaker_record_failure(contract, selector, msg).await;
    }

    ctx.db
        .record_retryable_error(
            job.job_id,
            &ctx.instance_id,
            msg,
            retry::retry_delay(job.attempts),
        )
        .await?;
    Ok(())
}
