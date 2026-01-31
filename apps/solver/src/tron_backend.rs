use crate::{
    config::{JobConfig, TronConfig, TronMode},
    hub::{DelegateResourceContract, HubClient, TransferContract, TronProof},
    metrics::SolverTelemetry,
};
use alloy::primitives::{Address, B256, FixedBytes, U256, keccak256};
use alloy::sol_types::SolValue;
use anyhow::{Context, Result};
use tron::{TronAddress, TronGrpc, TronTxProofBuilder, TronWallet};

alloy::sol! {
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
}

#[derive(Clone)]
pub struct TronBackend {
    cfg: TronConfig,
    jobs: JobConfig,
    telemetry: SolverTelemetry,
}

pub enum TronExecution {
    ImmediateProof(TronProof),
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
                let reader = self
                    .cfg
                    .mock_reader_address
                    .context("missing TRON_MOCK_READER_ADDRESS")?;
                let intent = TRXTransferIntent::abi_decode(intent_specs)
                    .context("abi_decode TRXTransferIntent")?;
                let tx_id = keccak256([intent_id.as_slice(), b":trx"].concat());

                let transfer = TransferContract {
                    txId: tx_id,
                    tronBlockNumber: U256::from(1u64),
                    tronBlockTimestamp: 1u32,
                    senderTron: evm_to_tron_raw21(hub.solver_address()),
                    toTron: evm_to_tron_raw21(intent.to),
                    amountSun: intent.amountSun,
                };
                hub.mock_set_transfer_tx(reader, transfer)
                    .await
                    .context("mock setTransferTx")?;

                Ok(TronExecution::ImmediateProof(empty_proof()))
            }
            TronMode::Grpc => self
                .broadcast_trx_transfer_grpc(intent_specs)
                .await
                .map(|txid| TronExecution::BroadcastedTx { txid })
                .context("grpc transfer"),
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
                let reader = self
                    .cfg
                    .mock_reader_address
                    .context("missing TRON_MOCK_READER_ADDRESS")?;
                let intent = DelegateResourceIntent::abi_decode(intent_specs)
                    .context("abi_decode DelegateResourceIntent")?;
                let tx_id = keccak256([intent_id.as_slice(), b":delegate"].concat());

                let delegation = DelegateResourceContract {
                    txId: tx_id,
                    tronBlockNumber: U256::from(2u64),
                    balanceSun: intent.balanceSun,
                    lockPeriod: intent.lockPeriod,
                    ownerTron: evm_to_tron_raw21(hub.solver_address()),
                    receiverTron: evm_to_tron_raw21(intent.receiver),
                    tronBlockTimestamp: 2u32,
                    resource: intent.resource,
                    lock: true,
                };
                hub.mock_set_delegate_resource_tx(reader, delegation)
                    .await
                    .context("mock setDelegateResourceTx")?;

                Ok(TronExecution::ImmediateProof(empty_proof()))
            }
            TronMode::Grpc => self
                .broadcast_delegate_resource_grpc(intent_specs)
                .await
                .map(|txid| TronExecution::BroadcastedTx { txid })
                .context("grpc delegate"),
        }
    }
}

impl TronBackend {
    pub async fn build_proof(&self, txid: [u8; 32]) -> Result<TronProof> {
        let mut grpc = self.connect_grpc().await?;
        self.build_proof_with(&mut grpc, txid).await
    }

    async fn broadcast_trx_transfer_grpc(&self, intent_specs: &[u8]) -> Result<[u8; 32]> {
        let intent = TRXTransferIntent::abi_decode(intent_specs).context("abi_decode")?;

        let amount_sun_i64 =
            i64::try_from(intent.amountSun).context("amountSun out of i64 range")?;
        let to = TronAddress::from_evm(intent.to);

        let wallet = TronWallet::new(self.cfg.private_key).context("init TronWallet")?;
        let mut grpc = self.connect_grpc().await?;

        let started = std::time::Instant::now();
        let txid = wallet
            .broadcast_transfer_contract(&mut grpc, to, amount_sun_i64)
            .await
            .context("broadcast_transfer_contract")?;
        self.telemetry.tron_grpc_ms(
            "broadcast_transfer_contract",
            true,
            started.elapsed().as_millis() as u64,
        );

        Ok(txid)
    }

    async fn broadcast_delegate_resource_grpc(&self, intent_specs: &[u8]) -> Result<[u8; 32]> {
        let intent = DelegateResourceIntent::abi_decode(intent_specs).context("abi_decode")?;

        let balance_sun_i64 =
            i64::try_from(intent.balanceSun).context("balanceSun out of i64 range")?;
        let lock_period_i64 =
            i64::try_from(intent.lockPeriod).context("lockPeriod out of i64 range")?;

        let rc = match intent.resource {
            0 => tron::protocol::ResourceCode::Bandwidth,
            1 => tron::protocol::ResourceCode::Energy,
            2 => tron::protocol::ResourceCode::TronPower,
            other => anyhow::bail!("unsupported DelegateResourceIntent.resource: {other}"),
        };

        let receiver = TronAddress::from_evm(intent.receiver);

        let wallet = TronWallet::new(self.cfg.private_key).context("init TronWallet")?;
        let mut grpc = self.connect_grpc().await?;

        let started = std::time::Instant::now();
        let txid = wallet
            .broadcast_delegate_resource_contract(
                &mut grpc,
                receiver,
                rc,
                balance_sun_i64,
                true,
                lock_period_i64,
            )
            .await
            .context("broadcast_delegate_resource_contract")?;
        self.telemetry.tron_grpc_ms(
            "broadcast_delegate_resource_contract",
            true,
            started.elapsed().as_millis() as u64,
        );

        Ok(txid)
    }

    async fn connect_grpc(&self) -> Result<TronGrpc> {
        TronGrpc::connect(&self.cfg.grpc_url, self.cfg.api_key.as_deref())
            .await
            .context("connect tron grpc")
    }

    async fn build_proof_with(&self, grpc: &mut TronGrpc, txid: [u8; 32]) -> Result<TronProof> {
        let builder = TronTxProofBuilder::new(self.jobs.tron_finality_blocks);

        // `build` already checks finality. We retry here to avoid making callers implement
        // their own polling loops.
        let start = std::time::Instant::now();
        loop {
            match builder.build(grpc, txid).await {
                Ok(bundle) => {
                    let proof = bundle
                        .proof
                        .into_iter()
                        .map(|p| B256::from_slice(p.as_slice()))
                        .collect::<Vec<_>>();

                    return Ok(TronProof {
                        blocks: bundle.blocks,
                        encoded_tx: bundle.encoded_tx,
                        proof,
                        index: bundle.index,
                    });
                }
                Err(err) => {
                    if start.elapsed() > std::time::Duration::from_secs(180) {
                        return Err(err).context("build tron proof (timeout)");
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
            }
        }
    }
}

fn empty_proof() -> TronProof {
    TronProof {
        blocks: std::array::from_fn(|_| Vec::new()),
        encoded_tx: Vec::new(),
        proof: Vec::new(),
        index: U256::ZERO,
    }
}

fn evm_to_tron_raw21(a: Address) -> FixedBytes<21> {
    let mut out = [0u8; 21];
    out[0] = 0x41;
    out[1..].copy_from_slice(a.as_slice());
    FixedBytes::from(out)
}
