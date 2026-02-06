use crate::{
    config::{JobConfig, TronConfig, TronMode},
    hub::HubClient,
    metrics::SolverTelemetry,
};
use alloy::primitives::B256;
use alloy::sol_types::SolValue;
use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tron::resources::ResourceStakeTotals;

mod grpc;
mod inventory;
mod mock;
mod planner;
mod utils;

use planner::{plan_trc20_consolidation, plan_trx_consolidation};
pub use utils::select_delegate_executor_index;
use utils::{
    empty_proof, evm_to_tron_raw21, tron_sender_from_privkey_or_fallback,
    validate_trc20_consolidation_caps, validate_trx_consolidation_caps,
};

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
    stake_totals_cache: Arc<RwLock<StakeTotalsCache>>,
}

#[derive(Debug, Clone)]
struct StakeTotalsCache {
    energy: Option<CachedTotals>,
    net: Option<CachedTotals>,
}

#[derive(Debug, Clone)]
struct CachedTotals {
    fetched_at: Instant,
    totals: ResourceStakeTotals,
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
enum ResourceStakeTotalsKind {
    Energy,
    Net,
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
    ImmediateProof(Box<crate::hub::TronProof>),
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
            stake_totals_cache: Arc::new(RwLock::new(StakeTotalsCache {
                energy: None,
                net: None,
            })),
        }
    }

    pub async fn energy_stake_totals(&self) -> Result<ResourceStakeTotals> {
        if self.cfg.mode != TronMode::Grpc {
            anyhow::bail!("energy_stake_totals is only available in TRON_MODE=grpc");
        }
        if let Some(cached) = self
            .get_cached_stake_totals(ResourceStakeTotalsKind::Energy)
            .await
        {
            return Ok(cached);
        }
        let wallet = tron::TronWallet::new(self.cfg.private_key).context("init TronWallet")?;
        let totals = grpc::fetch_energy_stake_totals(&self.cfg, &self.telemetry, wallet.address())
            .await
            .context("fetch_energy_stake_totals")?;
        self.put_cached_stake_totals(ResourceStakeTotalsKind::Energy, totals)
            .await;
        Ok(totals)
    }

    #[allow(dead_code)]
    pub async fn net_stake_totals(&self) -> Result<ResourceStakeTotals> {
        if self.cfg.mode != TronMode::Grpc {
            anyhow::bail!("net_stake_totals is only available in TRON_MODE=grpc");
        }
        if let Some(cached) = self
            .get_cached_stake_totals(ResourceStakeTotalsKind::Net)
            .await
        {
            return Ok(cached);
        }
        let wallet = tron::TronWallet::new(self.cfg.private_key).context("init TronWallet")?;
        let totals = grpc::fetch_net_stake_totals(&self.cfg, &self.telemetry, wallet.address())
            .await
            .context("fetch_net_stake_totals")?;
        self.put_cached_stake_totals(ResourceStakeTotalsKind::Net, totals)
            .await;
        Ok(totals)
    }

    async fn get_cached_stake_totals(
        &self,
        kind: ResourceStakeTotalsKind,
    ) -> Option<ResourceStakeTotals> {
        let ttl = std::time::Duration::from_secs(self.cfg.stake_totals_cache_ttl_secs.max(1));
        let cache = self.stake_totals_cache.read().await;
        let entry = match kind {
            ResourceStakeTotalsKind::Energy => cache.energy.as_ref(),
            ResourceStakeTotalsKind::Net => cache.net.as_ref(),
        }?;

        if entry.fetched_at.elapsed() <= ttl {
            Some(entry.totals)
        } else {
            None
        }
    }

    async fn put_cached_stake_totals(
        &self,
        kind: ResourceStakeTotalsKind,
        totals: ResourceStakeTotals,
    ) {
        let mut cache = self.stake_totals_cache.write().await;
        let entry = CachedTotals {
            fetched_at: Instant::now(),
            totals,
        };
        match kind {
            ResourceStakeTotalsKind::Energy => cache.energy = Some(entry),
            ResourceStakeTotalsKind::Net => cache.net = Some(entry),
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

    pub async fn prepare_delegate_resource_with_key(
        &self,
        hub: &HubClient,
        intent_id: B256,
        private_key: [u8; 32],
        intent_specs: &[u8],
    ) -> Result<TronExecution> {
        match self.cfg.mode {
            TronMode::Mock => {
                mock::execute_delegate_resource(hub, &self.cfg, intent_id, intent_specs).await
            }
            TronMode::Grpc => {
                let p = grpc::prepare_delegate_resource_with_key(
                    &self.cfg,
                    &self.telemetry,
                    private_key,
                    intent_specs,
                )
                .await
                .context("grpc prepare delegate (with key)")?;
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
