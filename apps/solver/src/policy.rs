use crate::{
    config::PolicyConfig,
    indexer::PoolOpenIntentRow,
    pricing::Pricing,
    tron_backend::{
        DelegateResourceIntent, TRXTransferIntent, TriggerSmartContractIntent, USDTTransferIntent,
    },
    types::IntentType,
};
use alloy::primitives::{Address, U256};
use alloy::sol_types::SolValue;
use anyhow::{Context, Result};

const TRON_BLOCK_TIME_SECS: u64 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BreakerQuery {
    pub contract: Address,
    pub selector: Option<[u8; 4]>,
}

#[derive(Debug, Clone)]
pub struct PolicyEvaluation {
    pub allowed: bool,
    pub reason: Option<String>,
    pub breaker: Option<BreakerQuery>,
}

#[derive(Clone)]
pub struct PolicyEngine {
    cfg: PolicyConfig,
}

struct StaticCheckOutcome {
    breaker: Option<BreakerQuery>,
    reject_reason: Option<String>,
}

impl PolicyEngine {
    pub fn new(cfg: PolicyConfig) -> Self {
        Self { cfg }
    }

    pub async fn evaluate_open_intent(
        &self,
        row: &PoolOpenIntentRow,
        now_unix_secs: i64,
        pricing: &mut Pricing,
        hub_cost_usd: f64,
        tron_fee_usd: f64,
        delegate_resource_resell: bool,
    ) -> Result<PolicyEvaluation> {
        let mut eval = PolicyEvaluation {
            allowed: false,
            reason: None,
            breaker: None,
        };

        if row.closed || row.solved {
            eval.reason = Some("closed_or_solved".to_string());
            return Ok(eval);
        }
        if !row.funded {
            eval.reason = Some("not_funded".to_string());
            return Ok(eval);
        }
        if row.solver.is_some() {
            eval.reason = Some("already_claimed".to_string());
            return Ok(eval);
        }

        let slack = i64::try_from(self.cfg.min_deadline_slack_secs).unwrap_or(i64::MAX);
        if row.deadline.saturating_sub(now_unix_secs) < slack {
            eval.reason = Some("deadline_slack".to_string());
            return Ok(eval);
        }

        let ty = IntentType::from_i16(row.intent_type)?;
        if !self.cfg.enabled_intent_types.contains(&ty) {
            eval.reason = Some("intent_type_disabled".to_string());
            return Ok(eval);
        }

        // Static per-type checks (caps + trigger allow/deny/selector guards).
        let static_eval = self.static_intent_checks(ty, &row.intent_specs)?;
        if let Some(reason) = static_eval.reject_reason {
            eval.reason = Some(reason);
            return Ok(eval);
        }
        eval.breaker = static_eval.breaker;

        // Best-effort profitability gating.
        if let Some(reason) = self
            .profitability_check(
                row,
                ty,
                pricing,
                hub_cost_usd,
                tron_fee_usd,
                delegate_resource_resell,
            )
            .await
            .context("profitability_check")?
        {
            eval.reason = Some(reason);
            return Ok(eval);
        }

        eval.allowed = true;
        Ok(eval)
    }

    fn static_intent_checks(
        &self,
        ty: IntentType,
        intent_specs_hex: &str,
    ) -> Result<StaticCheckOutcome> {
        let specs = match crate::types::parse_hex_bytes(intent_specs_hex) {
            Ok(v) => v,
            Err(_) => {
                return Ok(StaticCheckOutcome {
                    breaker: None,
                    reject_reason: Some("bad_intent_specs".to_string()),
                });
            }
        };

        match ty {
            IntentType::TriggerSmartContract => {
                let intent = TriggerSmartContractIntent::abi_decode(&specs)
                    .context("decode TriggerSmartContractIntent")
                    .map_err(|err| anyhow::anyhow!("trigger decode failed: {err:#}"))?;

                if let Some(max) = self.cfg.max_trigger_call_value_sun {
                    let v = match u256_to_u64_checked(intent.callValueSun) {
                        Ok(v) => v,
                        Err(_) => {
                            return Ok(StaticCheckOutcome {
                                breaker: None,
                                reject_reason: Some("trigger_call_value_range".to_string()),
                            });
                        }
                    };
                    if v > max {
                        return Ok(StaticCheckOutcome {
                            breaker: None,
                            reject_reason: Some("trigger_call_value_cap".to_string()),
                        });
                    }
                }

                let data = intent.data.as_ref();
                if let Some(max_len) = self.cfg.max_trigger_calldata_len
                    && data.len() > max_len as usize
                {
                    return Ok(StaticCheckOutcome {
                        breaker: None,
                        reject_reason: Some("trigger_calldata_len".to_string()),
                    });
                }

                let selector = selector4(data);
                if selector.is_none() && !self.cfg.trigger_allow_fallback_calls {
                    return Ok(StaticCheckOutcome {
                        breaker: None,
                        reject_reason: Some("trigger_selector_missing".to_string()),
                    });
                }

                if !self.is_trigger_contract_allowed(intent.to) {
                    return Ok(StaticCheckOutcome {
                        breaker: None,
                        reject_reason: Some("trigger_contract_not_allowed".to_string()),
                    });
                }

                if let Some(sel) = selector
                    && self.cfg.trigger_selector_denylist.contains(&sel)
                {
                    return Ok(StaticCheckOutcome {
                        breaker: None,
                        reject_reason: Some("trigger_selector_denied".to_string()),
                    });
                }

                Ok(StaticCheckOutcome {
                    breaker: Some(BreakerQuery {
                        contract: intent.to,
                        selector,
                    }),
                    reject_reason: None,
                })
            }
            IntentType::TrxTransfer => {
                let intent = match TRXTransferIntent::abi_decode(&specs) {
                    Ok(v) => v,
                    Err(_) => {
                        return Ok(StaticCheckOutcome {
                            breaker: None,
                            reject_reason: Some("trx_decode_failed".to_string()),
                        });
                    }
                };
                if let Some(max) = self.cfg.max_trx_transfer_sun {
                    let v = match u256_to_u64_checked(intent.amountSun) {
                        Ok(v) => v,
                        Err(_) => {
                            return Ok(StaticCheckOutcome {
                                breaker: None,
                                reject_reason: Some("trx_amount_range".to_string()),
                            });
                        }
                    };
                    if v > max {
                        return Ok(StaticCheckOutcome {
                            breaker: None,
                            reject_reason: Some("trx_amount_cap".to_string()),
                        });
                    }
                }
                Ok(StaticCheckOutcome {
                    breaker: None,
                    reject_reason: None,
                })
            }
            IntentType::UsdtTransfer => {
                let intent = match USDTTransferIntent::abi_decode(&specs) {
                    Ok(v) => v,
                    Err(_) => {
                        return Ok(StaticCheckOutcome {
                            breaker: None,
                            reject_reason: Some("usdt_decode_failed".to_string()),
                        });
                    }
                };
                if let Some(max) = self.cfg.max_usdt_transfer_amount {
                    let v = match u256_to_u64_checked(intent.amount) {
                        Ok(v) => v,
                        Err(_) => {
                            return Ok(StaticCheckOutcome {
                                breaker: None,
                                reject_reason: Some("usdt_amount_range".to_string()),
                            });
                        }
                    };
                    if v > max {
                        return Ok(StaticCheckOutcome {
                            breaker: None,
                            reject_reason: Some("usdt_amount_cap".to_string()),
                        });
                    }
                }
                Ok(StaticCheckOutcome {
                    breaker: None,
                    reject_reason: None,
                })
            }
            IntentType::DelegateResource => {
                let intent = match DelegateResourceIntent::abi_decode(&specs) {
                    Ok(v) => v,
                    Err(_) => {
                        return Ok(StaticCheckOutcome {
                            breaker: None,
                            reject_reason: Some("delegate_decode_failed".to_string()),
                        });
                    }
                };
                if let Some(max) = self.cfg.max_delegate_balance_sun {
                    let v = match u256_to_u64_checked(intent.balanceSun) {
                        Ok(v) => v,
                        Err(_) => {
                            return Ok(StaticCheckOutcome {
                                breaker: None,
                                reject_reason: Some("delegate_balance_range".to_string()),
                            });
                        }
                    };
                    if v > max {
                        return Ok(StaticCheckOutcome {
                            breaker: None,
                            reject_reason: Some("delegate_balance_cap".to_string()),
                        });
                    }
                }
                if let Some(max) = self.cfg.max_delegate_lock_period_secs {
                    let v_blocks = match u256_to_u64_checked(intent.lockPeriod) {
                        Ok(v) => v,
                        Err(_) => {
                            return Ok(StaticCheckOutcome {
                                breaker: None,
                                reject_reason: Some("delegate_lock_range".to_string()),
                            });
                        }
                    };
                    let v_secs = lock_period_secs_from_blocks(v_blocks);
                    if v_secs > max {
                        return Ok(StaticCheckOutcome {
                            breaker: None,
                            reject_reason: Some("delegate_lock_cap".to_string()),
                        });
                    }
                }
                Ok(StaticCheckOutcome {
                    breaker: None,
                    reject_reason: None,
                })
            }
        }
    }

    fn is_trigger_contract_allowed(&self, contract: Address) -> bool {
        if !self.cfg.trigger_contract_allowlist.is_empty() {
            return self.cfg.trigger_contract_allowlist.contains(&contract);
        }
        if self.cfg.trigger_contract_denylist.contains(&contract) {
            return false;
        }
        true
    }

    async fn profitability_check(
        &self,
        row: &PoolOpenIntentRow,
        ty: IntentType,
        pricing: &mut Pricing,
        hub_cost_usd: f64,
        tron_fee_usd: f64,
        delegate_resource_resell: bool,
    ) -> Result<Option<String>> {
        if self.cfg.min_profit_usd <= 0.0 && !self.cfg.require_priced_escrow {
            return Ok(None);
        }

        let escrow_token: Address = row.escrow_token.parse().unwrap_or_default();
        let priced = self.cfg.allowed_escrow_tokens.contains(&escrow_token);
        if !priced && self.cfg.require_priced_escrow {
            return Ok(Some("escrow_token_unpriced".to_string()));
        }
        if !priced || self.cfg.min_profit_usd <= 0.0 {
            return Ok(None);
        }

        let escrow_amount = crate::types::parse_u256_dec(&row.escrow_amount).unwrap_or(U256::ZERO);
        // For MVP we treat allowed escrow tokens as 6-decimal $1 stables.
        let revenue_usd = (escrow_amount.to_string().parse::<f64>().unwrap_or(0.0)) / 1e6;

        let trx_usd = pricing.trx_usd().await.unwrap_or(0.0);
        let cost_usd = if ty == IntentType::DelegateResource && delegate_resource_resell {
            0.0
        } else {
            match estimate_cost_usd(&self.cfg, ty, &row.intent_specs, trx_usd) {
                Ok(v) => v,
                Err(_) => return Ok(Some("cost_estimate_failed".to_string())),
            }
        };
        let profit = revenue_usd - cost_usd - hub_cost_usd - tron_fee_usd;
        if profit < self.cfg.min_profit_usd {
            return Ok(Some("unprofitable".to_string()));
        }

        Ok(None)
    }
}

pub fn selector4(data: &[u8]) -> Option<[u8; 4]> {
    if data.len() < 4 {
        return None;
    }
    let mut out = [0u8; 4];
    out.copy_from_slice(&data[..4]);
    Some(out)
}

fn u256_to_u64_checked(v: U256) -> Result<u64> {
    v.try_into()
        .map_err(|_| anyhow::anyhow!("u256 out of u64 range"))
}

fn lock_period_secs_from_blocks(lock_period_blocks: u64) -> u64 {
    lock_period_blocks.saturating_mul(TRON_BLOCK_TIME_SECS)
}

pub fn estimate_cost_usd(
    cfg: &PolicyConfig,
    ty: IntentType,
    intent_specs_hex: &str,
    trx_usd: f64,
) -> Result<f64> {
    let specs = crate::types::parse_hex_bytes(intent_specs_hex)?;
    let cost = match ty {
        IntentType::TriggerSmartContract => {
            let intent = TriggerSmartContractIntent::abi_decode(&specs)
                .context("decode TriggerSmartContractIntent")?;
            let sun: f64 = intent.callValueSun.to_string().parse().unwrap_or(0.0);
            (sun / 1e6) * trx_usd
        }
        IntentType::TrxTransfer => {
            let intent =
                TRXTransferIntent::abi_decode(&specs).context("decode TRXTransferIntent")?;
            let sun: f64 = intent.amountSun.to_string().parse().unwrap_or(0.0);
            (sun / 1e6) * trx_usd
        }
        IntentType::UsdtTransfer => {
            let intent =
                USDTTransferIntent::abi_decode(&specs).context("decode USDTTransferIntent")?;
            let amt: f64 = intent.amount.to_string().parse().unwrap_or(0.0);
            amt / 1e6
        }
        IntentType::DelegateResource => {
            let intent = DelegateResourceIntent::abi_decode(&specs)
                .context("decode DelegateResourceIntent")?;
            let sun: f64 = intent.balanceSun.to_string().parse().unwrap_or(0.0);
            let lock_blocks = u256_to_u64_checked(intent.lockPeriod).unwrap_or(u64::MAX);
            let principal_usd = (sun / 1e6) * trx_usd;
            let day_frac = lock_period_secs_from_blocks(lock_blocks) as f64 / 86400.0;

            principal_usd * (cfg.capital_lock_ppm_per_day as f64 / 1e6) * day_frac
        }
    };
    Ok(cost)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PolicyConfig;
    use crate::indexer::PoolOpenIntentRow;
    use crate::pricing::{Pricing, PricingConfig};
    use alloy::primitives::{Bytes, U256};

    fn cfg() -> PolicyConfig {
        PolicyConfig {
            enabled_intent_types: vec![IntentType::TriggerSmartContract],
            min_deadline_slack_secs: 0,
            min_profit_usd: 0.0,
            hub_cost_usd: 0.0,
            hub_cost_history_lookback: 50,
            hub_cost_headroom_ppm: 0,
            tron_fee_usd: 0.0,
            tron_fee_history_lookback: 50,
            tron_fee_headroom_ppm: 0,
            capital_lock_ppm_per_day: 0,
            require_priced_escrow: false,
            allowed_escrow_tokens: vec![],
            trigger_contract_allowlist: vec![],
            trigger_contract_denylist: vec![],
            trigger_selector_denylist: vec![],
            trigger_allow_fallback_calls: false,
            max_trx_transfer_sun: None,
            max_usdt_transfer_amount: None,
            max_delegate_balance_sun: None,
            max_delegate_lock_period_secs: None,
            max_trigger_call_value_sun: None,
            max_trigger_calldata_len: None,
        }
    }

    fn row_for(intent_type: IntentType, intent_specs: Vec<u8>, deadline: i64) -> PoolOpenIntentRow {
        PoolOpenIntentRow {
            id: format!("0x{}", "11".repeat(32)),
            intent_type: intent_type as i16,
            intent_specs: format!("0x{}", hex::encode(intent_specs)),
            escrow_token: Address::ZERO.to_string(),
            escrow_amount: "0".to_string(),
            solver: None,
            deadline,
            solved: false,
            funded: true,
            settled: false,
            closed: false,
        }
    }

    #[test]
    fn selector4_extracts_first_4_bytes() {
        assert_eq!(selector4(&[]), None);
        assert_eq!(selector4(&[1, 2, 3]), None);
        assert_eq!(selector4(&[1, 2, 3, 4]), Some([1, 2, 3, 4]));
        assert_eq!(selector4(&[1, 2, 3, 4, 5]), Some([1, 2, 3, 4]));
    }

    #[test]
    fn trigger_allowlist_blocks_unlisted_contracts() {
        let mut c = cfg();
        let allowed: Address = "0x00000000000000000000000000000000000000aa"
            .parse()
            .unwrap();
        c.trigger_contract_allowlist = vec![allowed];
        let p = PolicyEngine::new(c);

        assert!(p.is_trigger_contract_allowed(allowed));
        assert!(!p.is_trigger_contract_allowed(Address::ZERO));
    }

    #[test]
    fn trigger_denylist_blocks_even_when_no_allowlist() {
        let mut c = cfg();
        let denied: Address = "0x00000000000000000000000000000000000000bb"
            .parse()
            .unwrap();
        c.trigger_contract_denylist = vec![denied];
        let p = PolicyEngine::new(c);

        assert!(!p.is_trigger_contract_allowed(denied));
        assert!(p.is_trigger_contract_allowed(Address::ZERO));
    }

    #[test]
    fn delegate_lock_cost_scales_with_lock_period_and_principal() {
        let mut c = cfg();
        c.capital_lock_ppm_per_day = 100_000; // 10% / day (intentionally large for test)

        let intent = DelegateResourceIntent {
            receiver: Address::ZERO,
            resource: 1,
            balanceSun: U256::from(1_000_000u64), // 1 TRX (1e6 sun)
            lockPeriod: U256::from(28_800u64),    // 1 day in Tron blocks
        };
        let specs_hex = format!("0x{}", hex::encode(intent.abi_encode()));
        let cost = estimate_cost_usd(&c, IntentType::DelegateResource, &specs_hex, 0.5).unwrap();
        // principal = $0.50; 10%/day => $0.05
        assert!((cost - 0.05).abs() < 1e-9, "cost={cost}");
    }

    #[tokio::test]
    async fn delegate_lock_cap_treats_lock_period_as_blocks() {
        let mut c = cfg();
        c.enabled_intent_types = vec![IntentType::DelegateResource];
        c.max_delegate_lock_period_secs = Some(86_400); // 1 day in seconds

        let intent = DelegateResourceIntent {
            receiver: Address::ZERO,
            resource: 1,
            balanceSun: U256::from(1_000_000u64),
            lockPeriod: U256::from(28_801u64), // 1 day + 1 block
        };

        let row = row_for(IntentType::DelegateResource, intent.abi_encode(), 2_000_000);
        let mut pricing = Pricing::new(PricingConfig {
            trx_usd_override: Some(0.3),
            trx_usd_ttl: std::time::Duration::from_secs(60),
            trx_usd_url: "http://example.invalid".to_string(),
            eth_usd_override: Some(2_000.0),
            eth_usd_ttl: std::time::Duration::from_secs(60),
            eth_usd_url: "http://example.invalid".to_string(),
        });

        let eval = PolicyEngine::new(c)
            .evaluate_open_intent(&row, 1_000_000, &mut pricing, 0.0, 0.0, false)
            .await
            .unwrap();
        assert!(!eval.allowed);
        assert_eq!(eval.reason.as_deref(), Some("delegate_lock_cap"));
    }

    #[tokio::test]
    async fn trigger_selector_denylist_rejects() {
        let to: Address = "0x00000000000000000000000000000000000000aa"
            .parse()
            .unwrap();
        let intent = TriggerSmartContractIntent {
            to,
            callValueSun: U256::ZERO,
            data: Bytes::from(vec![0x09, 0x5e, 0xa7, 0xb3, 0x00]),
        };

        let mut c = cfg();
        c.trigger_contract_allowlist = vec![to];
        c.trigger_selector_denylist = vec![[0x09, 0x5e, 0xa7, 0xb3]];
        let p = PolicyEngine::new(c);

        let now = 1_000_000i64;
        let row = row_for(
            IntentType::TriggerSmartContract,
            intent.abi_encode(),
            now + 10_000,
        );
        let mut pricing = Pricing::new(PricingConfig {
            trx_usd_override: Some(0.3),
            trx_usd_ttl: std::time::Duration::from_secs(60),
            trx_usd_url: "http://example.invalid".to_string(),
            eth_usd_override: Some(2_000.0),
            eth_usd_ttl: std::time::Duration::from_secs(60),
            eth_usd_url: "http://example.invalid".to_string(),
        });

        let eval = p
            .evaluate_open_intent(&row, now, &mut pricing, 0.0, 0.0, false)
            .await
            .unwrap();
        assert!(!eval.allowed);
        assert_eq!(eval.reason.as_deref(), Some("trigger_selector_denied"));
    }

    #[tokio::test]
    async fn trigger_fallback_calls_rejected_by_default() {
        let to: Address = "0x00000000000000000000000000000000000000aa"
            .parse()
            .unwrap();
        let intent = TriggerSmartContractIntent {
            to,
            callValueSun: U256::ZERO,
            data: Bytes::from(vec![]),
        };

        let mut c = cfg();
        c.trigger_contract_allowlist = vec![to];
        let p = PolicyEngine::new(c);

        let now = 1_000_000i64;
        let row = row_for(
            IntentType::TriggerSmartContract,
            intent.abi_encode(),
            now + 10_000,
        );
        let mut pricing = Pricing::new(PricingConfig {
            trx_usd_override: Some(0.3),
            trx_usd_ttl: std::time::Duration::from_secs(60),
            trx_usd_url: "http://example.invalid".to_string(),
            eth_usd_override: Some(2_000.0),
            eth_usd_ttl: std::time::Duration::from_secs(60),
            eth_usd_url: "http://example.invalid".to_string(),
        });

        let eval = p
            .evaluate_open_intent(&row, now, &mut pricing, 0.0, 0.0, false)
            .await
            .unwrap();
        assert!(!eval.allowed);
        assert_eq!(eval.reason.as_deref(), Some("trigger_selector_missing"));
    }

    #[tokio::test]
    async fn profitability_subtracts_hub_cost_override() {
        let mut c = cfg();
        c.enabled_intent_types = vec![IntentType::UsdtTransfer];
        c.min_profit_usd = 0.1;
        c.require_priced_escrow = true;
        c.allowed_escrow_tokens = vec![Address::ZERO];
        let p = PolicyEngine::new(c);

        let now = 1_000_000i64;
        let intent = USDTTransferIntent {
            to: Address::ZERO,
            amount: U256::ZERO,
        };

        let mut row = row_for(IntentType::UsdtTransfer, intent.abi_encode(), now + 10_000);
        row.escrow_amount = "1000000".to_string(); // $1.00
        row.escrow_token = Address::ZERO.to_string();

        let mut pricing = Pricing::new(PricingConfig {
            trx_usd_override: Some(0.3),
            trx_usd_ttl: std::time::Duration::from_secs(60),
            trx_usd_url: "http://example.invalid".to_string(),
            eth_usd_override: Some(2_000.0),
            eth_usd_ttl: std::time::Duration::from_secs(60),
            eth_usd_url: "http://example.invalid".to_string(),
        });

        // $1.00 revenue - $0.00 tron - $0.95 hub = $0.05 profit < $0.10
        let eval = p
            .evaluate_open_intent(&row, now, &mut pricing, 0.95, 0.0, false)
            .await
            .unwrap();
        assert!(!eval.allowed);
        assert_eq!(eval.reason.as_deref(), Some("unprofitable"));
    }
}
