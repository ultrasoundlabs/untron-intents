use super::{InventoryCheck, TRXTransferIntent, TronBackend, USDTTransferIntent, grpc};
use crate::config::TronMode;
use crate::hub::HubClient;
use alloy::sol_types::SolValue;
use anyhow::{Context, Result};

impl TronBackend {
    pub async fn can_fill_preclaim(
        &self,
        hub: &HubClient,
        ty: crate::types::IntentType,
        intent_specs: &[u8],
    ) -> Result<InventoryCheck> {
        if self.cfg.mode != TronMode::Grpc {
            return Ok(InventoryCheck {
                ok: true,
                reason: None,
                required_pre_txs: 0,
            });
        }
        if self.cfg.private_keys.is_empty() {
            return Ok(InventoryCheck {
                ok: false,
                reason: Some("no_tron_keys"),
                required_pre_txs: 0,
            });
        }
        if !matches!(
            ty,
            crate::types::IntentType::TrxTransfer | crate::types::IntentType::UsdtTransfer
        ) {
            return Ok(InventoryCheck {
                ok: true,
                reason: None,
                required_pre_txs: 0,
            });
        }

        // Quick inventory check (no signing): can any key fill, or can we consolidate within limits?
        let wallets = self
            .cfg
            .private_keys
            .iter()
            .copied()
            .map(|k| tron::TronWallet::new(k).context("init TronWallet"))
            .collect::<Result<Vec<_>>>()?;
        let addrs = wallets.iter().map(|w| w.address()).collect::<Vec<_>>();

        const BALANCE_RESERVE_SUN: i64 = 2_000_000;

        match ty {
            crate::types::IntentType::TrxTransfer => {
                let intent = TRXTransferIntent::abi_decode(intent_specs)
                    .context("abi_decode TRXTransferIntent")?;
                let amount_sun_i64 =
                    i64::try_from(intent.amountSun).context("amountSun out of i64 range")?;
                let balances =
                    grpc::fetch_trx_balances_sun(&self.cfg, &self.telemetry, &addrs).await?;
                if balances
                    .iter()
                    .any(|b| *b >= amount_sun_i64.saturating_add(BALANCE_RESERVE_SUN))
                {
                    return Ok(InventoryCheck {
                        ok: true,
                        reason: None,
                        required_pre_txs: 0,
                    });
                }
                if !self.jobs.consolidation_enabled {
                    return Ok(InventoryCheck {
                        ok: false,
                        reason: Some("consolidation_disabled"),
                        required_pre_txs: 0,
                    });
                }
                let max_pre_txs = usize::try_from(self.jobs.consolidation_max_pre_txs).unwrap_or(0);
                let Some(plan) = super::plan_trx_consolidation(
                    &balances,
                    amount_sun_i64.saturating_add(BALANCE_RESERVE_SUN),
                    max_pre_txs,
                )?
                else {
                    return Ok(InventoryCheck {
                        ok: false,
                        reason: Some("cannot_consolidate"),
                        required_pre_txs: 0,
                    });
                };

                if super::validate_trx_consolidation_caps(
                    &plan,
                    self.jobs.consolidation_max_total_trx_pull_sun,
                    self.jobs.consolidation_max_per_tx_trx_pull_sun,
                )
                .is_err()
                {
                    return Ok(InventoryCheck {
                        ok: false,
                        reason: Some("consolidation_caps"),
                        required_pre_txs: plan.transfers.len(),
                    });
                }

                Ok(InventoryCheck {
                    ok: true,
                    reason: None,
                    required_pre_txs: plan.transfers.len(),
                })
            }
            crate::types::IntentType::UsdtTransfer => {
                let intent = USDTTransferIntent::abi_decode(intent_specs)
                    .context("abi_decode USDTTransferIntent")?;
                let amount_u64 = u64::try_from(intent.amount).unwrap_or(u64::MAX);
                let tron_usdt = hub.v3_tron_usdt().await.context("load V3.tronUsdt")?;

                let token_balances = grpc::fetch_trc20_balances_u64(
                    &self.cfg,
                    &self.telemetry,
                    tron::TronAddress::from_evm(tron_usdt),
                    &addrs,
                )
                .await?;
                let trx_balances =
                    grpc::fetch_trx_balances_sun(&self.cfg, &self.telemetry, &addrs).await?;

                if token_balances.iter().enumerate().any(|(i, b)| {
                    *b >= amount_u64
                        && trx_balances.get(i).copied().unwrap_or(0) >= BALANCE_RESERVE_SUN
                }) {
                    return Ok(InventoryCheck {
                        ok: true,
                        reason: None,
                        required_pre_txs: 0,
                    });
                }
                if !self.jobs.consolidation_enabled {
                    return Ok(InventoryCheck {
                        ok: false,
                        reason: Some("consolidation_disabled"),
                        required_pre_txs: 0,
                    });
                }
                let max_pre_txs = usize::try_from(self.jobs.consolidation_max_pre_txs).unwrap_or(0);
                let Some(plan) =
                    super::plan_trc20_consolidation(&token_balances, amount_u64, max_pre_txs)?
                else {
                    return Ok(InventoryCheck {
                        ok: false,
                        reason: Some("cannot_consolidate"),
                        required_pre_txs: 0,
                    });
                };

                if super::validate_trc20_consolidation_caps(
                    &plan,
                    self.jobs.consolidation_max_total_usdt_pull_amount,
                    self.jobs.consolidation_max_per_tx_usdt_pull_amount,
                )
                .is_err()
                {
                    return Ok(InventoryCheck {
                        ok: false,
                        reason: Some("consolidation_caps"),
                        required_pre_txs: plan.transfers.len(),
                    });
                }

                Ok(InventoryCheck {
                    ok: true,
                    reason: None,
                    required_pre_txs: plan.transfers.len(),
                })
            }
            _ => Ok(InventoryCheck {
                ok: true,
                reason: None,
                required_pre_txs: 0,
            }),
        }
    }

    /// Returns the staked-but-not-yet-delegated TRX (in SUN) available to delegate for `resource`.
    ///
    /// This is a *best-effort safety check* meant to avoid claiming intents we cannot satisfy due
    /// to insufficient staked inventory. It is not a perfect reservation system.
    #[allow(dead_code)]
    pub async fn delegated_resource_available_sun(
        &self,
        resource: tron::protocol::ResourceCode,
    ) -> Result<Option<i64>> {
        match self.cfg.mode {
            TronMode::Mock => Ok(None),
            TronMode::Grpc => {
                let wallet = tron::TronWallet::new(self.cfg.private_key)
                    .context("init TronWallet (capacity check)")?;
                let account = grpc::fetch_account(&self.cfg, &self.telemetry, wallet.address())
                    .await
                    .context("fetch Tron account")?;
                Ok(Some(grpc::delegated_resource_available_sun(
                    &account, resource,
                )))
            }
        }
    }

    pub fn tron_key_addresses(&self) -> Result<Vec<tron::TronAddress>> {
        let keys = if !self.cfg.private_keys.is_empty() {
            self.cfg.private_keys.clone()
        } else if self.cfg.private_key != [0u8; 32] {
            vec![self.cfg.private_key]
        } else {
            Vec::new()
        };

        let mut out = Vec::with_capacity(keys.len());
        for pk in keys {
            let w = tron::TronWallet::new(pk).context("init TronWallet")?;
            out.push(w.address());
        }
        Ok(out)
    }

    pub fn private_key_for_owner(&self, owner_address_prefixed: &[u8]) -> Option<[u8; 32]> {
        for pk in &self.cfg.private_keys {
            if let Ok(w) = tron::TronWallet::new(*pk)
                && w.address().prefixed_bytes().as_slice() == owner_address_prefixed
            {
                return Some(*pk);
            }
        }
        if let Ok(w) = tron::TronWallet::new(self.cfg.private_key)
            && w.address().prefixed_bytes().as_slice() == owner_address_prefixed
        {
            return Some(self.cfg.private_key);
        }
        None
    }

    pub async fn delegate_available_sun_by_key(
        &self,
        resource: tron::protocol::ResourceCode,
    ) -> Result<Vec<(tron::TronAddress, i64)>> {
        if self.cfg.mode != TronMode::Grpc {
            return Ok(Vec::new());
        }

        let addrs = self.tron_key_addresses().context("tron_key_addresses")?;
        let mut out = Vec::with_capacity(addrs.len());
        for a in addrs {
            let account = grpc::fetch_account(&self.cfg, &self.telemetry, a)
                .await
                .context("fetch_account")?;
            out.push((
                a,
                grpc::delegated_resource_available_sun(&account, resource),
            ));
        }
        Ok(out)
    }
}
