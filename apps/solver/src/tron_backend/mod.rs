use crate::{
    config::{JobConfig, TronConfig, TronMode},
    hub::HubClient,
    metrics::SolverTelemetry,
};
use alloy::primitives::{B256, FixedBytes, U256};
use alloy::sol_types::SolValue;
use anyhow::{Context, Result};

mod grpc;
mod mock;
mod planner;

use planner::{plan_trc20_consolidation, plan_trx_consolidation};

alloy::sol! {
    struct TriggerSmartContractIntent {
        address to;
        uint256 callValueSun;
        bytes data;
    }

    struct TRXTransferIntent {
        address to;
        uint256 amountSun;
    }

    struct DelegateResourceIntent {
        address receiver;
        uint8 resource;
        uint256 balanceSun;
        uint256 lockPeriod;
    }

    struct USDTTransferIntent {
        address to;
        uint256 amount;
    }
}

#[derive(Clone)]
pub struct TronBackend {
    cfg: TronConfig,
    jobs: JobConfig,
    telemetry: SolverTelemetry,
}

#[derive(Debug, Clone)]
pub struct TronPreparedTx {
    pub txid: [u8; 32],
    pub tx_bytes: Vec<u8>,
    pub fee_limit_sun: Option<i64>,
    pub energy_required: Option<i64>,
    pub tx_size_bytes: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct TronPreparedPlan {
    pub pre_txs: Vec<TronPreparedTx>,
    pub final_tx: TronPreparedTx,
}

#[derive(Debug, Clone)]
pub struct InventoryCheck {
    pub ok: bool,
    pub reason: Option<&'static str>,
    pub required_pre_txs: usize,
}

pub enum TronExecution {
    ImmediateProof(crate::hub::TronProof),
    PreparedTx(TronPreparedTx),
}

#[derive(Debug, Clone)]
pub struct EmulationCheck {
    pub ok: bool,
    pub reason: Option<String>,
}

impl TronBackend {
    pub fn new(cfg: TronConfig, jobs: JobConfig, telemetry: SolverTelemetry) -> Self {
        Self {
            cfg,
            jobs,
            telemetry,
        }
    }

    pub async fn prepare_trx_transfer(
        &self,
        hub: &HubClient,
        intent_id: B256,
        intent_specs: &[u8],
    ) -> Result<TronExecution> {
        match self.cfg.mode {
            TronMode::Mock => {
                mock::execute_trx_transfer(hub, &self.cfg, intent_id, intent_specs).await
            }
            TronMode::Grpc => {
                let p = grpc::prepare_trx_transfer(&self.cfg, &self.telemetry, intent_specs)
                    .await
                    .context("grpc prepare transfer")?;
                Ok(TronExecution::PreparedTx(TronPreparedTx {
                    txid: p.txid,
                    tx_bytes: p.tx_bytes,
                    fee_limit_sun: p.fee_limit_sun,
                    energy_required: p.energy_required,
                    tx_size_bytes: p.tx_size_bytes,
                }))
            }
        }
    }

    pub async fn prepare_trigger_smart_contract(
        &self,
        hub: &HubClient,
        intent_id: B256,
        intent_specs: &[u8],
    ) -> Result<TronExecution> {
        match self.cfg.mode {
            TronMode::Mock => {
                mock::execute_trigger_smart_contract(hub, &self.cfg, intent_id, intent_specs).await
            }
            TronMode::Grpc => {
                let p =
                    grpc::prepare_trigger_smart_contract(&self.cfg, &self.telemetry, intent_specs)
                        .await
                        .context("grpc prepare trigger_smart_contract")?;
                Ok(TronExecution::PreparedTx(TronPreparedTx {
                    txid: p.txid,
                    tx_bytes: p.tx_bytes,
                    fee_limit_sun: p.fee_limit_sun,
                    energy_required: p.energy_required,
                    tx_size_bytes: p.tx_size_bytes,
                }))
            }
        }
    }

    pub async fn prepare_delegate_resource(
        &self,
        hub: &HubClient,
        intent_id: B256,
        intent_specs: &[u8],
    ) -> Result<TronExecution> {
        match self.cfg.mode {
            TronMode::Mock => {
                mock::execute_delegate_resource(hub, &self.cfg, intent_id, intent_specs).await
            }
            TronMode::Grpc => {
                let p = grpc::prepare_delegate_resource(&self.cfg, &self.telemetry, intent_specs)
                    .await
                    .context("grpc prepare delegate")?;
                Ok(TronExecution::PreparedTx(TronPreparedTx {
                    txid: p.txid,
                    tx_bytes: p.tx_bytes,
                    fee_limit_sun: p.fee_limit_sun,
                    energy_required: p.energy_required,
                    tx_size_bytes: p.tx_size_bytes,
                }))
            }
        }
    }

    pub async fn prepare_trx_transfer_plan(&self, intent_specs: &[u8]) -> Result<TronPreparedPlan> {
        if self.cfg.mode != TronMode::Grpc {
            anyhow::bail!("prepare_trx_transfer_plan is only available in TRON_MODE=grpc");
        }
        if self.cfg.private_keys.is_empty() {
            anyhow::bail!("no tron private keys configured");
        }

        let intent =
            TRXTransferIntent::abi_decode(intent_specs).context("abi_decode TRXTransferIntent")?;
        let amount_sun_i64 =
            i64::try_from(intent.amountSun).context("amountSun out of i64 range")?;

        let wallets = self
            .cfg
            .private_keys
            .iter()
            .copied()
            .map(|k| tron::TronWallet::new(k).context("init TronWallet"))
            .collect::<Result<Vec<_>>>()?;
        let addrs = wallets.iter().map(|w| w.address()).collect::<Vec<_>>();
        let balances = grpc::fetch_trx_balances_sun(&self.cfg, &self.telemetry, &addrs)
            .await
            .context("fetch_trx_balances_sun")?;

        // Reserve some TRX for fees.
        const BALANCE_RESERVE_SUN: i64 = 2_000_000;
        let mut best: Option<usize> = None;
        for (i, b) in balances.iter().enumerate() {
            if *b >= amount_sun_i64.saturating_add(BALANCE_RESERVE_SUN) {
                best = Some(i);
                break;
            }
        }
        if let Some(executor_index) = best {
            let p = grpc::prepare_trx_transfer_with_key(
                &self.cfg,
                &self.telemetry,
                self.cfg.private_keys[executor_index],
                intent_specs,
            )
            .await?;
            return Ok(TronPreparedPlan {
                pre_txs: Vec::new(),
                final_tx: TronPreparedTx {
                    txid: p.txid,
                    tx_bytes: p.tx_bytes,
                    fee_limit_sun: p.fee_limit_sun,
                    energy_required: p.energy_required,
                    tx_size_bytes: p.tx_size_bytes,
                },
            });
        }

        if !self.jobs.consolidation_enabled {
            anyhow::bail!("insufficient TRX balance (and consolidation disabled)");
        }

        let max_pre_txs = usize::try_from(self.jobs.consolidation_max_pre_txs).unwrap_or(0);
        let Some(plan) =
            plan_trx_consolidation(&balances, amount_sun_i64 + BALANCE_RESERVE_SUN, max_pre_txs)?
        else {
            anyhow::bail!("insufficient TRX balance (cannot consolidate within limits)");
        };

        validate_trx_consolidation_caps(
            &plan,
            self.jobs.consolidation_max_total_trx_pull_sun,
            self.jobs.consolidation_max_per_tx_trx_pull_sun,
        )?;

        let executor = wallets[plan.executor_index].address();
        let mut pre_txs = Vec::with_capacity(plan.transfers.len());
        for (from_idx, amt) in plan.transfers {
            let p = grpc::build_trx_transfer(
                &self.cfg,
                &self.telemetry,
                self.cfg.private_keys[from_idx],
                executor,
                amt,
            )
            .await?;
            pre_txs.push(TronPreparedTx {
                txid: p.txid,
                tx_bytes: p.tx_bytes,
                fee_limit_sun: p.fee_limit_sun,
                energy_required: p.energy_required,
                tx_size_bytes: p.tx_size_bytes,
            });
        }

        let p = grpc::prepare_trx_transfer_with_key(
            &self.cfg,
            &self.telemetry,
            self.cfg.private_keys[plan.executor_index],
            intent_specs,
        )
        .await?;

        Ok(TronPreparedPlan {
            pre_txs,
            final_tx: TronPreparedTx {
                txid: p.txid,
                tx_bytes: p.tx_bytes,
                fee_limit_sun: p.fee_limit_sun,
                energy_required: p.energy_required,
                tx_size_bytes: p.tx_size_bytes,
            },
        })
    }

    pub async fn prepare_usdt_transfer(
        &self,
        hub: &HubClient,
        intent_id: B256,
        intent_specs: &[u8],
    ) -> Result<TronExecution> {
        match self.cfg.mode {
            TronMode::Mock => {
                mock::execute_usdt_transfer(hub, &self.cfg, intent_id, intent_specs).await
            }
            TronMode::Grpc => {
                let p = grpc::prepare_usdt_transfer(hub, &self.cfg, &self.telemetry, intent_specs)
                    .await
                    .context("grpc prepare usdt transfer")?;
                Ok(TronExecution::PreparedTx(TronPreparedTx {
                    txid: p.txid,
                    tx_bytes: p.tx_bytes,
                    fee_limit_sun: p.fee_limit_sun,
                    energy_required: p.energy_required,
                    tx_size_bytes: p.tx_size_bytes,
                }))
            }
        }
    }

    pub async fn prepare_usdt_transfer_plan(
        &self,
        hub: &HubClient,
        intent_specs: &[u8],
    ) -> Result<TronPreparedPlan> {
        if self.cfg.mode != TronMode::Grpc {
            anyhow::bail!("prepare_usdt_transfer_plan is only available in TRON_MODE=grpc");
        }
        if self.cfg.private_keys.is_empty() {
            anyhow::bail!("no tron private keys configured");
        }

        let intent = USDTTransferIntent::abi_decode(intent_specs)
            .context("abi_decode USDTTransferIntent")?;
        let amount_u64 = u64::try_from(intent.amount).unwrap_or(u64::MAX);
        let tron_usdt = hub.v3_tron_usdt().await.context("load V3.tronUsdt")?;

        let wallets = self
            .cfg
            .private_keys
            .iter()
            .copied()
            .map(|k| tron::TronWallet::new(k).context("init TronWallet"))
            .collect::<Result<Vec<_>>>()?;
        let addrs = wallets.iter().map(|w| w.address()).collect::<Vec<_>>();
        let token_balances = grpc::fetch_trc20_balances_u64(
            &self.cfg,
            &self.telemetry,
            tron::TronAddress::from_evm(tron_usdt),
            &addrs,
        )
        .await
        .context("fetch usdt balances")?;
        let trx_balances = grpc::fetch_trx_balances_sun(&self.cfg, &self.telemetry, &addrs)
            .await
            .context("fetch trx balances")?;

        const BALANCE_RESERVE_SUN: i64 = 2_000_000;
        let mut best: Option<usize> = None;
        for (i, b) in token_balances.iter().enumerate() {
            if *b >= amount_u64 && trx_balances.get(i).copied().unwrap_or(0) >= BALANCE_RESERVE_SUN
            {
                best = Some(i);
                break;
            }
        }
        if let Some(executor_index) = best {
            let p = grpc::prepare_usdt_transfer_with_key(
                hub,
                &self.cfg,
                &self.telemetry,
                self.cfg.private_keys[executor_index],
                intent_specs,
            )
            .await?;
            return Ok(TronPreparedPlan {
                pre_txs: Vec::new(),
                final_tx: TronPreparedTx {
                    txid: p.txid,
                    tx_bytes: p.tx_bytes,
                    fee_limit_sun: p.fee_limit_sun,
                    energy_required: p.energy_required,
                    tx_size_bytes: p.tx_size_bytes,
                },
            });
        }

        if !self.jobs.consolidation_enabled {
            anyhow::bail!("insufficient USDT balance (and consolidation disabled)");
        }
        let max_pre_txs = usize::try_from(self.jobs.consolidation_max_pre_txs).unwrap_or(0);
        let Some(plan) = plan_trc20_consolidation(&token_balances, amount_u64, max_pre_txs)? else {
            anyhow::bail!("insufficient USDT balance (cannot consolidate within limits)");
        };

        validate_trc20_consolidation_caps(
            &plan,
            self.jobs.consolidation_max_total_usdt_pull_amount,
            self.jobs.consolidation_max_per_tx_usdt_pull_amount,
        )?;

        let executor = wallets[plan.executor_index].address();
        let mut pre_txs = Vec::with_capacity(plan.transfers.len());
        for (from_idx, amt) in plan.transfers {
            let p = grpc::build_trc20_transfer(
                &self.cfg,
                &self.telemetry,
                self.cfg.private_keys[from_idx],
                tron::TronAddress::from_evm(tron_usdt),
                executor,
                amt,
            )
            .await?;
            pre_txs.push(TronPreparedTx {
                txid: p.txid,
                tx_bytes: p.tx_bytes,
                fee_limit_sun: p.fee_limit_sun,
                energy_required: p.energy_required,
                tx_size_bytes: p.tx_size_bytes,
            });
        }

        let p = grpc::prepare_usdt_transfer_with_key(
            hub,
            &self.cfg,
            &self.telemetry,
            self.cfg.private_keys[plan.executor_index],
            intent_specs,
        )
        .await?;

        Ok(TronPreparedPlan {
            pre_txs,
            final_tx: TronPreparedTx {
                txid: p.txid,
                tx_bytes: p.tx_bytes,
                fee_limit_sun: p.fee_limit_sun,
                energy_required: p.energy_required,
                tx_size_bytes: p.tx_size_bytes,
            },
        })
    }

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
                let Some(plan) = plan_trx_consolidation(
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

                if validate_trx_consolidation_caps(
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
                    plan_trc20_consolidation(&token_balances, amount_u64, max_pre_txs)?
                else {
                    return Ok(InventoryCheck {
                        ok: false,
                        reason: Some("cannot_consolidate"),
                        required_pre_txs: 0,
                    });
                };

                if validate_trc20_consolidation_caps(
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

    pub async fn precheck_emulation(
        &self,
        hub: &HubClient,
        ty: crate::types::IntentType,
        intent_specs: &[u8],
    ) -> EmulationCheck {
        if self.cfg.mode != TronMode::Grpc || !self.cfg.emulation_enabled {
            return EmulationCheck {
                ok: true,
                reason: None,
            };
        }

        let res = match ty {
            crate::types::IntentType::TriggerSmartContract => {
                grpc::emulate_trigger_smart_contract_intent(
                    &self.cfg,
                    &self.telemetry,
                    intent_specs,
                )
                .await
                .map(|_| ())
            }
            crate::types::IntentType::UsdtTransfer => {
                grpc::emulate_usdt_transfer_intent(hub, &self.cfg, &self.telemetry, intent_specs)
                    .await
                    .map(|_| ())
            }
            _ => Ok(()),
        };

        match res {
            Ok(()) => EmulationCheck {
                ok: true,
                reason: None,
            },
            Err(err) => {
                let msg = err.to_string();
                if msg.contains("emulation_revert:") {
                    return EmulationCheck {
                        ok: false,
                        reason: Some("tron_emulation_revert".to_string()),
                    };
                }
                tracing::warn!(
                    err = %err,
                    "tron emulation check failed; continuing without gating"
                );
                EmulationCheck {
                    ok: true,
                    reason: None,
                }
            }
        }
    }

    /// Returns the staked-but-not-yet-delegated TRX (in SUN) available to delegate for `resource`.
    ///
    /// This is a *best-effort safety check* meant to avoid claiming intents we cannot satisfy due
    /// to insufficient staked inventory. It is not a perfect reservation system.
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

    pub async fn build_proof(&self, txid: [u8; 32]) -> Result<crate::hub::TronProof> {
        match self.cfg.mode {
            TronMode::Mock => anyhow::bail!("build_proof is not available in TRON_MODE=mock"),
            TronMode::Grpc => grpc::build_proof(&self.cfg, &self.jobs, txid).await,
        }
    }

    pub async fn tx_is_known(&self, txid: [u8; 32]) -> bool {
        match self.cfg.mode {
            TronMode::Mock => false,
            TronMode::Grpc => grpc::tx_is_known(&self.cfg, &self.telemetry, txid).await,
        }
    }

    pub async fn broadcast_signed_tx(&self, tx_bytes: &[u8]) -> Result<()> {
        match self.cfg.mode {
            TronMode::Mock => {
                anyhow::bail!("broadcast_signed_tx is not available in TRON_MODE=mock")
            }
            TronMode::Grpc => grpc::broadcast_signed_tx(&self.cfg, &self.telemetry, tx_bytes).await,
        }
    }

    pub async fn fetch_transaction_info(
        &self,
        txid: [u8; 32],
    ) -> Result<Option<tron::protocol::TransactionInfo>> {
        match self.cfg.mode {
            TronMode::Mock => Ok(None),
            TronMode::Grpc => Ok(Some(
                grpc::fetch_transaction_info(&self.cfg, &self.telemetry, txid).await?,
            )),
        }
    }
}

pub(super) fn empty_proof() -> crate::hub::TronProof {
    crate::hub::TronProof {
        blocks: std::array::from_fn(|_| Vec::new()),
        encoded_tx: Vec::new(),
        proof: Vec::new(),
        index: U256::ZERO,
    }
}

pub(super) fn evm_to_tron_raw21(a: alloy::primitives::Address) -> FixedBytes<21> {
    let mut out = [0u8; 21];
    out[0] = 0x41;
    out[1..].copy_from_slice(a.as_slice());
    FixedBytes::from(out)
}

pub(super) fn tron_sender_from_privkey_or_fallback(
    tron_pk: [u8; 32],
    hub: &HubClient,
) -> FixedBytes<21> {
    if tron_pk != [0u8; 32] {
        if let Ok(w) = tron::TronWallet::new(tron_pk) {
            let b = w.address().prefixed_bytes();
            return FixedBytes::from_slice(&b);
        }
    }
    evm_to_tron_raw21(hub.solver_address())
}

fn validate_trx_consolidation_caps(
    plan: &planner::TrxConsolidationPlan,
    max_total_pull_sun: u64,
    max_per_tx_pull_sun: u64,
) -> Result<()> {
    let total: i64 = plan.transfers.iter().map(|(_, a)| *a).sum();
    if max_total_pull_sun > 0 && total > i64::try_from(max_total_pull_sun).unwrap_or(i64::MAX) {
        anyhow::bail!("consolidation max_total_trx_pull_sun exceeded");
    }
    if max_per_tx_pull_sun > 0 {
        let cap = i64::try_from(max_per_tx_pull_sun).unwrap_or(i64::MAX);
        if plan.transfers.iter().any(|(_, a)| *a > cap) {
            anyhow::bail!("consolidation max_per_tx_trx_pull_sun exceeded");
        }
    }
    Ok(())
}

fn validate_trc20_consolidation_caps(
    plan: &planner::Trc20ConsolidationPlan,
    max_total_pull_amount: u64,
    max_per_tx_pull_amount: u64,
) -> Result<()> {
    let total: u64 = plan.transfers.iter().map(|(_, a)| *a).sum();
    if max_total_pull_amount > 0 && total > max_total_pull_amount {
        anyhow::bail!("consolidation max_total_usdt_pull_amount exceeded");
    }
    if max_per_tx_pull_amount > 0
        && plan
            .transfers
            .iter()
            .any(|(_, a)| *a > max_per_tx_pull_amount)
    {
        anyhow::bail!("consolidation max_per_tx_usdt_pull_amount exceeded");
    }
    Ok(())
}
