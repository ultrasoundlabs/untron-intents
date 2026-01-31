use crate::{
    config::{JobConfig, TronConfig},
    hub::TronProof,
    metrics::SolverTelemetry,
};
use alloy::primitives::B256;
use alloy::sol_types::SolValue;
use anyhow::{Context, Result};
use tron::{TronAddress, TronGrpc, TronTxProofBuilder, TronWallet};

pub async fn broadcast_trx_transfer(
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    intent_specs: &[u8],
) -> Result<[u8; 32]> {
    let intent = super::TRXTransferIntent::abi_decode(intent_specs)
        .context("abi_decode TRXTransferIntent")?;

    let amount_sun_i64 = i64::try_from(intent.amountSun).context("amountSun out of i64 range")?;
    let to = TronAddress::from_evm(intent.to);

    let wallet = TronWallet::new(cfg.private_key).context("init TronWallet")?;
    let mut grpc = connect_grpc(cfg).await?;

    let started = std::time::Instant::now();
    let txid = wallet
        .broadcast_transfer_contract(&mut grpc, to, amount_sun_i64)
        .await
        .context("broadcast_transfer_contract")?;
    telemetry.tron_grpc_ms(
        "broadcast_transfer_contract",
        true,
        started.elapsed().as_millis() as u64,
    );

    Ok(txid)
}

pub async fn broadcast_delegate_resource(
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    intent_specs: &[u8],
) -> Result<[u8; 32]> {
    let intent = super::DelegateResourceIntent::abi_decode(intent_specs)
        .context("abi_decode DelegateResourceIntent")?;

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

    let wallet = TronWallet::new(cfg.private_key).context("init TronWallet")?;
    let mut grpc = connect_grpc(cfg).await?;

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
    telemetry.tron_grpc_ms(
        "broadcast_delegate_resource_contract",
        true,
        started.elapsed().as_millis() as u64,
    );

    Ok(txid)
}

pub async fn broadcast_usdt_transfer(
    hub: &crate::hub::HubClient,
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    intent_specs: &[u8],
) -> Result<[u8; 32]> {
    let intent = super::USDTTransferIntent::abi_decode(intent_specs)
        .context("abi_decode USDTTransferIntent")?;
    let tron_usdt = hub.v3_tron_usdt().await.context("load V3.tronUsdt")?;

    let wallet = TronWallet::new(cfg.private_key).context("init TronWallet")?;
    let mut grpc = connect_grpc(cfg).await?;

    let data = crate::abi::encode_trc20_transfer(intent.to, intent.amount);
    let fee_policy = tron::sender::FeePolicy {
        fee_limit_cap_sun: cfg.fee_limit_cap_sun,
        fee_limit_headroom_ppm: cfg.fee_limit_headroom_ppm,
    };

    let started = std::time::Instant::now();
    let txid = wallet
        .broadcast_trigger_smart_contract(
            &mut grpc,
            TronAddress::from_evm(tron_usdt),
            data,
            0,
            fee_policy,
        )
        .await
        .context("broadcast_trigger_smart_contract")?;
    telemetry.tron_grpc_ms(
        "broadcast_trigger_smart_contract",
        true,
        started.elapsed().as_millis() as u64,
    );

    Ok(txid)
}

pub async fn build_proof(cfg: &TronConfig, jobs: &JobConfig, txid: [u8; 32]) -> Result<TronProof> {
    let mut grpc = connect_grpc(cfg).await?;
    build_proof_with(&mut grpc, jobs, txid).await
}

async fn connect_grpc(cfg: &TronConfig) -> Result<TronGrpc> {
    TronGrpc::connect(&cfg.grpc_url, cfg.api_key.as_deref())
        .await
        .context("connect tron grpc")
}

async fn build_proof_with(
    grpc: &mut TronGrpc,
    jobs: &JobConfig,
    txid: [u8; 32],
) -> Result<TronProof> {
    let builder = TronTxProofBuilder::new(jobs.tron_finality_blocks);

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
