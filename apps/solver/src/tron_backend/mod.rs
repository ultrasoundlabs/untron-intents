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

pub enum TronExecution {
    ImmediateProof(crate::hub::TronProof),
    BroadcastedTx { txid: [u8; 32] },
}

impl TronBackend {
    pub fn new(cfg: TronConfig, jobs: JobConfig, telemetry: SolverTelemetry) -> Self {
        Self {
            cfg,
            jobs,
            telemetry,
        }
    }

    pub async fn execute_trx_transfer(
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
                let txid = grpc::broadcast_trx_transfer(&self.cfg, &self.telemetry, intent_specs)
                    .await
                    .context("grpc transfer")?;
                Ok(TronExecution::BroadcastedTx { txid })
            }
        }
    }

    pub async fn execute_trigger_smart_contract(
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
                let txid = grpc::broadcast_trigger_smart_contract(
                    &self.cfg,
                    &self.telemetry,
                    intent_specs,
                )
                .await
                .context("grpc trigger_smart_contract")?;
                Ok(TronExecution::BroadcastedTx { txid })
            }
        }
    }

    pub async fn execute_delegate_resource(
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
                let txid =
                    grpc::broadcast_delegate_resource(&self.cfg, &self.telemetry, intent_specs)
                        .await
                        .context("grpc delegate")?;
                Ok(TronExecution::BroadcastedTx { txid })
            }
        }
    }

    pub async fn execute_usdt_transfer(
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
                let txid =
                    grpc::broadcast_usdt_transfer(hub, &self.cfg, &self.telemetry, intent_specs)
                        .await
                        .context("grpc usdt transfer")?;
                Ok(TronExecution::BroadcastedTx { txid })
            }
        }
    }

    pub async fn build_proof(&self, txid: [u8; 32]) -> Result<crate::hub::TronProof> {
        match self.cfg.mode {
            TronMode::Mock => anyhow::bail!("build_proof is not available in TRON_MODE=mock"),
            TronMode::Grpc => grpc::build_proof(&self.cfg, &self.jobs, txid).await,
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
