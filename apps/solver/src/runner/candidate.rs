use super::{
    RentalQuoteDecision, ShouldAttemptDecision, Solver, b256_to_bytes32,
    decode_trigger_contract_and_selector, duration_hours_for_lock_period_blocks,
};
use crate::{
    config::TronMode,
    indexer::PoolOpenIntentRow,
    types::{IntentType, parse_b256, parse_hex_bytes},
};
use alloy::sol_types::SolValue;
use anyhow::Result;
use std::time::Instant;

impl Solver {
    async fn quote_best_energy_rental(
        &self,
        receiver: tron::TronAddress,
        balance_sun: u64,
        lock_period_blocks: u64,
        amount_units: u64,
        duration_hours: u64,
    ) -> Result<RentalQuoteDecision> {
        let mut recv = [0u8; 20];
        recv.copy_from_slice(receiver.evm().as_slice());

        let ctx_quote = tron::RentalContext {
            resource: tron::RentalResourceKind::Energy,
            amount: amount_units,
            lock_period: Some(lock_period_blocks),
            duration_hours: Some(duration_hours),
            balance_sun: Some(balance_sun),
            address_base58check: receiver.to_base58check(),
            address_hex41: format!("0x{}", hex::encode(receiver.prefixed_bytes())),
            address_evm_hex: format!("{:#x}", receiver.evm()),
            txid: None,
        };

        let mut best: Option<RentalQuoteDecision> = None;
        let mut last_err: Option<String> = None;

        for provider_cfg in &self.cfg.tron.energy_rental_providers {
            if provider_cfg.quote.is_none() {
                continue;
            }
            if self
                .db
                .rental_provider_is_frozen(&provider_cfg.name)
                .await?
                .is_some()
            {
                continue;
            }

            let provider = tron::JsonApiRentalProvider::new(provider_cfg.clone());
            let started = Instant::now();
            let res = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                provider.quote_with_rendered_request(&ctx_quote),
            )
            .await;
            let ms = started.elapsed().as_millis() as u64;

            match res {
                Ok(Ok((req, attempt))) => {
                    let ok = attempt.ok && attempt.cost_trx.is_some();
                    self.telemetry.rental_quote_ms(provider.name(), ok, ms);
                    if ok {
                        let cost_trx = attempt.cost_trx.unwrap_or(f64::INFINITY);
                        let resp = attempt.response_json.unwrap_or(serde_json::Value::Null);
                        let candidate = RentalQuoteDecision {
                            provider: provider.name().to_string(),
                            receiver_evm: recv,
                            balance_sun: i64::try_from(balance_sun).unwrap_or(i64::MAX),
                            lock_period: i64::try_from(lock_period_blocks).unwrap_or(i64::MAX),
                            amount_units,
                            duration_hours,
                            cost_trx,
                            rendered_request: req,
                            response_json: resp,
                        };
                        let choose = match &best {
                            None => true,
                            Some(b) => cost_trx < b.cost_trx,
                        };
                        if choose {
                            best = Some(candidate);
                        }
                        let _ = self
                            .db
                            .rental_provider_record_success(provider.name())
                            .await;
                    } else {
                        let msg = format!(
                            "ok={} cost_trx={:?} err={:?}",
                            attempt.ok, attempt.cost_trx, attempt.error
                        );
                        last_err = Some(format!("{}: {msg}", provider.name()));
                        self.telemetry.rental_quote_ms(provider.name(), false, ms);
                        let froze = self
                            .db
                            .rental_provider_record_failure(
                                provider.name(),
                                self.cfg.tron.rental_provider_fail_window_secs,
                                self.cfg.tron.rental_provider_freeze_secs,
                                self.cfg.tron.rental_provider_fail_threshold,
                                &msg,
                            )
                            .await;
                        if froze.unwrap_or(false) {
                            self.telemetry.rental_provider_frozen(provider.name());
                        }
                    }
                }
                Ok(Err(err)) => {
                    self.telemetry.rental_quote_ms(provider.name(), false, ms);
                    let msg = format!("{err:#}");
                    last_err = Some(format!("{}: {msg}", provider.name()));
                    let froze = self
                        .db
                        .rental_provider_record_failure(
                            provider.name(),
                            self.cfg.tron.rental_provider_fail_window_secs,
                            self.cfg.tron.rental_provider_freeze_secs,
                            self.cfg.tron.rental_provider_fail_threshold,
                            &msg,
                        )
                        .await;
                    if froze.unwrap_or(false) {
                        self.telemetry.rental_provider_frozen(provider.name());
                    }
                }
                Err(_) => {
                    self.telemetry.rental_quote_ms(provider.name(), false, ms);
                    let msg = "timeout".to_string();
                    last_err = Some(format!("{}: {msg}", provider.name()));
                    let froze = self
                        .db
                        .rental_provider_record_failure(
                            provider.name(),
                            self.cfg.tron.rental_provider_fail_window_secs,
                            self.cfg.tron.rental_provider_freeze_secs,
                            self.cfg.tron.rental_provider_fail_threshold,
                            &msg,
                        )
                        .await;
                    if froze.unwrap_or(false) {
                        self.telemetry.rental_provider_frozen(provider.name());
                    }
                }
            }
        }

        best.ok_or_else(|| {
            anyhow::anyhow!(
                "{}",
                last_err
                    .unwrap_or_else(|| "no energy rental quote providers succeeded".to_string())
            )
        })
    }

    pub(super) async fn should_attempt(
        &mut self,
        row: &PoolOpenIntentRow,
    ) -> Result<ShouldAttemptDecision> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let ty = IntentType::from_i16(row.intent_type)?;
        let mut rental_quote: Option<RentalQuoteDecision> = None;
        let mut rental_cost_usd: f64 = 0.0;
        let mut delegate_resource_resell: bool = false;

        // Pre-claim inventory check for multi-key TRX/USDT: if we can't fill (and can't consolidate
        // within configured limits), skip before we spend the claim deposit.
        let mut required_pre_txs: usize = 0;
        if self.cfg.tron.mode == TronMode::Grpc
            && matches!(ty, IntentType::TrxTransfer | IntentType::UsdtTransfer)
            && (self.cfg.tron.private_keys.len() > 1 || self.cfg.jobs.consolidation_enabled)
        {
            let specs = parse_hex_bytes(&row.intent_specs)?;
            match self
                .tron
                .can_fill_preclaim(self.hub.as_ref(), ty, &specs)
                .await
            {
                Ok(inv) => {
                    required_pre_txs = inv.required_pre_txs;
                    if !inv.ok {
                        if let Ok(id) = parse_b256(&row.id) {
                            let details = serde_json::json!({
                                "reason": inv.reason,
                                "required_pre_txs": inv.required_pre_txs,
                            })
                            .to_string();
                            let _ = self
                                .db
                                .upsert_intent_skip(
                                    b256_to_bytes32(id),
                                    row.intent_type,
                                    inv.reason.unwrap_or("inventory_insufficient"),
                                    Some(&details),
                                )
                                .await;
                        }
                        return Ok(ShouldAttemptDecision {
                            ok: false,
                            rental_quote: None,
                        });
                    }
                }
                Err(err) => {
                    tracing::warn!(err = %err, "preclaim inventory check failed; continuing");
                }
            }
        }

        let hub_cost_usd = self.estimate_hub_cost_usd().await?;
        let tron_fee_usd_per_tx = self.estimate_tron_fee_usd(row.intent_type).await?;
        let tron_fee_usd = tron_fee_usd_per_tx * (1.0 + required_pre_txs as f64);

        // DelegateResource resell (ENERGY-only): quote rental providers before claim to ensure profitability.
        if self.cfg.tron.mode == TronMode::Grpc
            && ty == IntentType::DelegateResource
            && self.cfg.tron.delegate_resource_resell_enabled
        {
            let specs = parse_hex_bytes(&row.intent_specs)?;
            if let Ok(intent) = crate::tron_backend::DelegateResourceIntent::abi_decode(&specs) {
                // Only resell ENERGY. Bandwidth/TRON_POWER use the solver's own capacity and are gated separately.
                if intent.resource == 1 {
                    delegate_resource_resell = true;

                    let receiver = tron::TronAddress::from_evm(intent.receiver);
                    let balance_sun_u64 = u64::try_from(intent.balanceSun).unwrap_or(u64::MAX);
                    let lock_period_blocks = u64::try_from(intent.lockPeriod).unwrap_or(u64::MAX);

                    let duration_hours = duration_hours_for_lock_period_blocks(lock_period_blocks);
                    let totals = self.tron.energy_stake_totals().await?;
                    let amount_units = tron::resources::resource_units_for_min_trx_sun(
                        balance_sun_u64,
                        totals,
                        self.cfg.tron.resell_energy_headroom_ppm,
                    );

                    let need_profitability = self.cfg.policy.min_profit_usd > 0.0
                        || self.cfg.policy.require_priced_escrow;

                    match self
                        .quote_best_energy_rental(
                            receiver,
                            balance_sun_u64,
                            lock_period_blocks,
                            amount_units,
                            duration_hours,
                        )
                        .await
                    {
                        Ok(q) => {
                            if need_profitability {
                                let trx_usd = match self.pricing.trx_usd().await {
                                    Ok(v) => v,
                                    Err(err) => {
                                        tracing::warn!(err = %err, "trx_usd unavailable; skipping rental quote");
                                        if let Ok(id) = parse_b256(&row.id) {
                                            let _ = self
                                                .db
                                                .upsert_intent_skip(
                                                    b256_to_bytes32(id),
                                                    row.intent_type,
                                                    "rental_quote_no_price",
                                                    None,
                                                )
                                                .await;
                                        }
                                        return Ok(ShouldAttemptDecision {
                                            ok: false,
                                            rental_quote: None,
                                        });
                                    }
                                };
                                rental_cost_usd = q.cost_trx * trx_usd;
                            }
                            rental_quote = Some(q);
                        }
                        Err(err) => {
                            if need_profitability {
                                tracing::warn!(
                                    err = %err,
                                    "energy rental quote failed; skipping intent"
                                );
                                if let Ok(id) = parse_b256(&row.id) {
                                    let _ = self
                                        .db
                                        .upsert_intent_skip(
                                            b256_to_bytes32(id),
                                            row.intent_type,
                                            "rental_quote_failed",
                                            Some(&format!("{err:#}")),
                                        )
                                        .await;
                                }
                                return Ok(ShouldAttemptDecision {
                                    ok: false,
                                    rental_quote: None,
                                });
                            }
                        }
                    }
                }
            }
        }

        let eval = self
            .policy
            .evaluate_open_intent(
                row,
                now,
                &mut self.pricing,
                hub_cost_usd,
                tron_fee_usd + rental_cost_usd,
                delegate_resource_resell,
            )
            .await?;
        if !eval.allowed {
            if let Ok(id) = parse_b256(&row.id) {
                let _ = self
                    .db
                    .upsert_intent_skip(
                        b256_to_bytes32(id),
                        row.intent_type,
                        eval.reason.as_deref().unwrap_or("policy_reject"),
                        None,
                    )
                    .await;
            }
            if let Some(reason) = eval.reason.as_deref() {
                tracing::debug!(id = %row.id, intent_type = row.intent_type, reason, "skip intent");
            }
            return Ok(ShouldAttemptDecision {
                ok: false,
                rental_quote: None,
            });
        }

        // Dynamic breaker (if applicable).
        if let Some(b) = eval.breaker
            && self.is_breaker_active(b).await?
        {
            if let Ok(id) = parse_b256(&row.id) {
                let _ = self
                    .db
                    .upsert_intent_skip(
                        b256_to_bytes32(id),
                        row.intent_type,
                        "breaker_active",
                        None,
                    )
                    .await;
            }
            return Ok(ShouldAttemptDecision {
                ok: false,
                rental_quote: None,
            });
        }

        // Optional Tron emulation gating: avoid claiming intents we know will revert.
        if self.cfg.tron.emulation_enabled
            && self.cfg.tron.mode == TronMode::Grpc
            && matches!(
                ty,
                IntentType::TriggerSmartContract | IntentType::UsdtTransfer
            )
        {
            let specs = parse_hex_bytes(&row.intent_specs)?;
            let emu = self
                .tron
                .precheck_emulation(self.hub.as_ref(), ty, &specs)
                .await;
            if let Ok(id) = parse_b256(&row.id) {
                let (contract, selector) = match ty {
                    IntentType::TriggerSmartContract => {
                        decode_trigger_contract_and_selector(&specs)
                            .map(|(c, s)| (Some(c), s))
                            .unwrap_or((None, None))
                    }
                    _ => (None, None),
                };
                let contract_bytes = contract.map(|c| c.as_slice().to_vec());
                let selector_bytes = selector.map(|s| s.to_vec());
                let _ = self
                    .db
                    .upsert_intent_emulation(
                        b256_to_bytes32(id),
                        row.intent_type,
                        emu.ok,
                        emu.reason.as_deref(),
                        contract_bytes.as_deref(),
                        selector_bytes.as_deref(),
                    )
                    .await;
            }
            if !emu.ok {
                if let Ok(id) = parse_b256(&row.id) {
                    let _ = self
                        .db
                        .upsert_intent_skip(
                            b256_to_bytes32(id),
                            row.intent_type,
                            emu.reason.as_deref().unwrap_or("tron_emulation_failed"),
                            None,
                        )
                        .await;
                }
                tracing::debug!(
                    id = %row.id,
                    intent_type = row.intent_type,
                    reason = emu.reason.as_deref().unwrap_or("tron_emulation_failed"),
                    "skip intent (tron emulation)"
                );
                return Ok(ShouldAttemptDecision {
                    ok: false,
                    rental_quote: None,
                });
            }
        }

        // Best-effort capacity check for resource delegation: avoid claiming intents we cannot fill
        // because we don't have enough staked TRX for the requested resource.
        if self.cfg.tron.mode == TronMode::Grpc
            && row.intent_type == IntentType::DelegateResource as i16
            && !delegate_resource_resell
        {
            let specs = parse_hex_bytes(&row.intent_specs)?;
            if let Ok(intent) = crate::tron_backend::DelegateResourceIntent::abi_decode(&specs) {
                let rc = match intent.resource {
                    0 => tron::protocol::ResourceCode::Bandwidth,
                    1 => tron::protocol::ResourceCode::Energy,
                    2 => tron::protocol::ResourceCode::TronPower,
                    _ => tron::protocol::ResourceCode::Energy,
                };

                let needed = i64::try_from(intent.balanceSun).unwrap_or(i64::MAX);
                let by_key = match self.tron.delegate_available_sun_by_key(rc).await {
                    Ok(v) => v,
                    Err(err) => {
                        tracing::warn!(err = %err, "delegate capacity check failed; continuing");
                        return Ok(ShouldAttemptDecision {
                            ok: true,
                            rental_quote,
                        });
                    }
                };
                let reserved = match self
                    .db
                    .sum_delegate_reserved_sun_by_owner(i16::from(intent.resource))
                    .await
                {
                    Ok(v) => v,
                    Err(err) => {
                        tracing::warn!(err = %err, "delegate reservation sum failed; continuing");
                        return Ok(ShouldAttemptDecision {
                            ok: true,
                            rental_quote,
                        });
                    }
                };
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
                    owners.push(hex::encode(&owner));
                    avail.push(*a);
                    resv.push(r);
                }

                if crate::tron_backend::select_delegate_executor_index(&avail, &resv, needed)
                    .is_none()
                {
                    if let Ok(id) = parse_b256(&row.id) {
                        let details = serde_json::json!({
                            "needed_sun": needed,
                            "resource": intent.resource,
                            "owners": owners,
                            "available_sun": avail,
                            "reserved_sun": resv,
                        })
                        .to_string();
                        let _ = self
                            .db
                            .upsert_intent_skip(
                                b256_to_bytes32(id),
                                row.intent_type,
                                "delegate_capacity_insufficient",
                                Some(&details),
                            )
                            .await;
                    }
                    return Ok(ShouldAttemptDecision {
                        ok: false,
                        rental_quote: None,
                    });
                }
            }
        }

        Ok(ShouldAttemptDecision {
            ok: true,
            rental_quote,
        })
    }
}
