use super::{
    JobCtx, SolverJob, b256_to_bytes32, decode_trigger_contract_and_selector,
    duration_hours_for_lock_period_blocks, ensure_delegate_reservation,
    looks_like_tron_contract_failure, looks_like_tron_server_busy, retry,
};
use crate::{
    config::TronMode,
    db::{TronProofRow, TronSignedTxRow, TronTxCostsRow},
    tron_backend::TronExecution,
    types::IntentType,
};
use alloy::primitives::B256;
use alloy::sol_types::SolValue;
use anyhow::{Context, Result};
use std::time::Instant;

pub(super) async fn process_claimed_state(
    ctx: &JobCtx,
    job: &SolverJob,
    id: B256,
    ty: IntentType,
) -> Result<()> {
    if ctx.cfg.tron.mode == TronMode::Mock {
        tracing::info!(id = %id, "executing tron tx (mock)");
        let exec_res = match ty {
            IntentType::TriggerSmartContract => ctx
                .tron
                .prepare_trigger_smart_contract(ctx.hub.as_ref(), id, &job.intent_specs)
                .await
                .context("execute trigger smart contract"),
            IntentType::TrxTransfer => ctx
                .tron
                .prepare_trx_transfer(ctx.hub.as_ref(), id, &job.intent_specs)
                .await
                .context("execute trx transfer"),
            IntentType::UsdtTransfer => ctx
                .tron
                .prepare_usdt_transfer(ctx.hub.as_ref(), id, &job.intent_specs)
                .await
                .context("execute usdt transfer"),
            IntentType::DelegateResource => ctx
                .tron
                .prepare_delegate_resource(ctx.hub.as_ref(), id, &job.intent_specs)
                .await
                .context("execute delegate resource"),
        };

        let exec = match exec_res {
            Ok(v) => v,
            Err(err) => {
                let msg = format!("{err:#}");

                // If this is a trigger smart contract, record a breaker on likely-deterministic failures.
                if ty == IntentType::TriggerSmartContract
                    && !looks_like_tron_server_busy(&msg)
                    && looks_like_tron_contract_failure(&msg)
                    && let Some((contract, selector)) =
                        decode_trigger_contract_and_selector(&job.intent_specs)
                {
                    let _ = ctx
                        .db
                        .breaker_record_failure(contract, selector, &msg)
                        .await;
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
                let txid = job.tron_txid.unwrap_or_else(|| {
                    b256_to_bytes32(alloy::primitives::keccak256(id.as_slice()))
                });
                ctx.db.save_tron_proof(txid, &proof_row).await?;
                ctx.db
                    .record_tron_txid(job.job_id, &ctx.instance_id, txid)
                    .await?;
                ctx.db
                    .record_proof_built(job.job_id, &ctx.instance_id)
                    .await?;
                Ok(())
            }
            TronExecution::PreparedTx(_) => {
                anyhow::bail!("unexpected PreparedTx in TRON_MODE=mock")
            }
        }
    } else {
        tracing::info!(id = %id, "preparing tron tx (persist signed bytes)");
        // TRX/USDT can optionally require consolidation (multi-key pre-txs). We persist the
        // whole plan as (pre txs + final tx), then broadcast them in order in tron_prepared.
        if matches!(ty, IntentType::TrxTransfer | IntentType::UsdtTransfer) {
            let plan = match ty {
                IntentType::TrxTransfer => ctx
                    .tron
                    .prepare_trx_transfer_plan(&job.intent_specs)
                    .await
                    .context("prepare trx transfer plan")?,
                IntentType::UsdtTransfer => ctx
                    .tron
                    .prepare_usdt_transfer_plan(ctx.hub.as_ref(), &job.intent_specs)
                    .await
                    .context("prepare usdt transfer plan")?,
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
        if ty == IntentType::DelegateResource && ctx.cfg.tron.delegate_resource_resell_enabled {
            let intent = crate::tron_backend::DelegateResourceIntent::abi_decode(&job.intent_specs)
                .context("decode DelegateResourceIntent")?;
            // Resell ENERGY only; bandwidth/TRON_POWER use the solver's own capacity.
            if intent.resource == 1 {
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
                    let mut chosen: Option<(tron::RenderedJsonApiRequest, tron::RentalAttempt)> =
                        None;

                    let mut providers = ctx.cfg.tron.energy_rental_providers.clone();
                    if let Some(p) = preferred {
                        providers.sort_by_key(|c| if c.name == p { 0 } else { 1 });
                    }
                    for p in &providers {
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
                                let _ =
                                    ctx.db.rental_provider_record_success(provider.name()).await;
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
                        let msg = last_err
                            .unwrap_or_else(|| "no energy rental providers succeeded".to_string());
                        ctx.db
                            .record_retryable_error(
                                job.job_id,
                                &ctx.instance_id,
                                &msg,
                                retry::retry_delay(job.attempts),
                            )
                            .await?;
                        return Ok(());
                    };

                    let txid_hex = attempt.txid.as_ref().unwrap();
                    let bytes = hex::decode(txid_hex.trim_start_matches("0x"))
                        .context("decode rental txid hex")?;
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
                return Ok(());
            }
        }

        let exec_res = match ty {
            IntentType::TriggerSmartContract => ctx
                .tron
                .prepare_trigger_smart_contract(ctx.hub.as_ref(), id, &job.intent_specs)
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
                ctx.tron
                    .prepare_delegate_resource_with_key(ctx.hub.as_ref(), id, pk, &job.intent_specs)
                    .await
                    .context("prepare delegate resource (reserved key)")
            }
            _ => unreachable!(),
        };

        let exec = match exec_res {
            Ok(v) => v,
            Err(err) => {
                let msg = format!("{err:#}");
                if ty == IntentType::TriggerSmartContract
                    && !looks_like_tron_server_busy(&msg)
                    && looks_like_tron_contract_failure(&msg)
                    && let Some((contract, selector)) =
                        decode_trigger_contract_and_selector(&job.intent_specs)
                {
                    let _ = ctx
                        .db
                        .breaker_record_failure(contract, selector, &msg)
                        .await;
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
}

pub(super) async fn process_tron_prepared_state(
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
        loop {
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

pub(super) async fn process_tron_sent_state(
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
    let tron = match ctx.tron.build_proof(txid).await {
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
