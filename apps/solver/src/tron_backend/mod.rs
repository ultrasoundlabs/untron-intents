use crate::{
    config::{JobConfig, TronConfig, TronMode},
    hub::HubClient,
    metrics::SolverTelemetry,
};
use alloy::primitives::{B256, FixedBytes, U256};
use anyhow::{Context, Result};

mod grpc;
mod mock;

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
