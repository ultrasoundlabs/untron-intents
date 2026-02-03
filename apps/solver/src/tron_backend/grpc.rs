use crate::{
    config::{JobConfig, TronConfig},
    hub::TronProof,
    metrics::SolverTelemetry,
};
use alloy::primitives::B256;
use alloy::sol_types::SolValue;
use anyhow::{Context, Result};
use prost::Message;
use tron::{TronAddress, TronGrpc, TronTxProofBuilder, TronWallet};

#[derive(Debug, Clone)]
pub struct PreparedTronTx {
    pub txid: [u8; 32],
    pub tx_bytes: Vec<u8>,
    pub fee_limit_sun: Option<i64>,
    pub energy_required: Option<i64>,
    pub tx_size_bytes: Option<i64>,
}

pub async fn fetch_transaction_info(
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    txid: [u8; 32],
) -> Result<tron::protocol::TransactionInfo> {
    let mut grpc = connect_grpc(cfg).await?;
    let started = std::time::Instant::now();
    let info = grpc
        .get_transaction_info_by_id(txid)
        .await
        .context("GetTransactionInfoById")?;
    telemetry.tron_grpc_ms(
        "get_transaction_info_by_id",
        true,
        started.elapsed().as_millis() as u64,
    );
    Ok(info)
}

pub async fn prepare_trx_transfer(
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    intent_specs: &[u8],
) -> Result<PreparedTronTx> {
    let intent = super::TRXTransferIntent::abi_decode(intent_specs)
        .context("abi_decode TRXTransferIntent")?;

    let amount_sun_i64 = i64::try_from(intent.amountSun).context("amountSun out of i64 range")?;
    let to = TronAddress::from_evm(intent.to);

    let wallet = TronWallet::new(cfg.private_key).context("init TronWallet")?;
    let mut grpc = connect_grpc(cfg).await?;

    let started = std::time::Instant::now();
    let signed = wallet
        .build_and_sign_transfer_contract(&mut grpc, to, amount_sun_i64)
        .await
        .context("build_and_sign_transfer_contract")?;
    telemetry.tron_grpc_ms(
        "build_and_sign_transfer_contract",
        true,
        started.elapsed().as_millis() as u64,
    );

    Ok(PreparedTronTx {
        txid: signed.txid,
        tx_bytes: signed.tx.encode_to_vec(),
        fee_limit_sun: Some(i64::try_from(signed.fee_limit_sun).unwrap_or(i64::MAX)),
        energy_required: Some(i64::try_from(signed.energy_required).unwrap_or(i64::MAX)),
        tx_size_bytes: Some(i64::try_from(signed.tx_size_bytes).unwrap_or(i64::MAX)),
    })
}

pub async fn prepare_trigger_smart_contract(
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    intent_specs: &[u8],
) -> Result<PreparedTronTx> {
    let intent = super::TriggerSmartContractIntent::abi_decode(intent_specs)
        .context("abi_decode TriggerSmartContractIntent")?;

    let to = TronAddress::from_evm(intent.to);
    let call_value_i64 =
        i64::try_from(intent.callValueSun).context("callValueSun out of i64 range")?;

    let wallet = TronWallet::new(cfg.private_key).context("init TronWallet")?;
    let mut grpc = connect_grpc(cfg).await?;

    if cfg.emulation_enabled {
        // Defensive: ensure the call is at least simulatable before we spend time broadcasting.
        emulate_trigger_smart_contract(
            &mut grpc,
            telemetry,
            &wallet,
            to,
            &intent.data,
            call_value_i64,
        )
        .await?;
    }

    let fee_policy = tron::sender::FeePolicy {
        fee_limit_cap_sun: cfg.fee_limit_cap_sun,
        fee_limit_headroom_ppm: cfg.fee_limit_headroom_ppm,
    };

    let started = std::time::Instant::now();
    let signed = wallet
        .build_and_sign_trigger_smart_contract(
            &mut grpc,
            to,
            intent.data.to_vec(),
            call_value_i64,
            fee_policy,
        )
        .await
        .context("build_and_sign_trigger_smart_contract")?;
    telemetry.tron_grpc_ms(
        "build_and_sign_trigger_smart_contract",
        true,
        started.elapsed().as_millis() as u64,
    );

    Ok(PreparedTronTx {
        txid: signed.txid,
        tx_bytes: signed.tx.encode_to_vec(),
        fee_limit_sun: Some(i64::try_from(signed.fee_limit_sun).unwrap_or(i64::MAX)),
        energy_required: Some(i64::try_from(signed.energy_required).unwrap_or(i64::MAX)),
        tx_size_bytes: Some(i64::try_from(signed.tx_size_bytes).unwrap_or(i64::MAX)),
    })
}

pub async fn prepare_delegate_resource(
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    intent_specs: &[u8],
) -> Result<PreparedTronTx> {
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
    let signed = wallet
        .build_and_sign_delegate_resource_contract(
            &mut grpc,
            receiver,
            rc,
            balance_sun_i64,
            true,
            lock_period_i64,
        )
        .await
        .context("build_and_sign_delegate_resource_contract")?;
    telemetry.tron_grpc_ms(
        "build_and_sign_delegate_resource_contract",
        true,
        started.elapsed().as_millis() as u64,
    );

    Ok(PreparedTronTx {
        txid: signed.txid,
        tx_bytes: signed.tx.encode_to_vec(),
        fee_limit_sun: Some(i64::try_from(signed.fee_limit_sun).unwrap_or(i64::MAX)),
        energy_required: Some(i64::try_from(signed.energy_required).unwrap_or(i64::MAX)),
        tx_size_bytes: Some(i64::try_from(signed.tx_size_bytes).unwrap_or(i64::MAX)),
    })
}

pub async fn prepare_usdt_transfer(
    hub: &crate::hub::HubClient,
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    intent_specs: &[u8],
) -> Result<PreparedTronTx> {
    let intent = super::USDTTransferIntent::abi_decode(intent_specs)
        .context("abi_decode USDTTransferIntent")?;
    let tron_usdt = hub.v3_tron_usdt().await.context("load V3.tronUsdt")?;

    let wallet = TronWallet::new(cfg.private_key).context("init TronWallet")?;
    let mut grpc = connect_grpc(cfg).await?;

    let data = crate::abi::encode_trc20_transfer(intent.to, intent.amount);
    if cfg.emulation_enabled {
        emulate_trigger_smart_contract(
            &mut grpc,
            telemetry,
            &wallet,
            TronAddress::from_evm(tron_usdt),
            &alloy::primitives::Bytes::from(data.as_slice().to_vec()),
            0,
        )
        .await?;
    }
    let fee_policy = tron::sender::FeePolicy {
        fee_limit_cap_sun: cfg.fee_limit_cap_sun,
        fee_limit_headroom_ppm: cfg.fee_limit_headroom_ppm,
    };

    let started = std::time::Instant::now();
    let signed = wallet
        .build_and_sign_trigger_smart_contract(
            &mut grpc,
            TronAddress::from_evm(tron_usdt),
            data,
            0,
            fee_policy,
        )
        .await
        .context("build_and_sign_trigger_smart_contract")?;
    telemetry.tron_grpc_ms(
        "build_and_sign_trigger_smart_contract",
        true,
        started.elapsed().as_millis() as u64,
    );

    Ok(PreparedTronTx {
        txid: signed.txid,
        tx_bytes: signed.tx.encode_to_vec(),
        fee_limit_sun: Some(i64::try_from(signed.fee_limit_sun).unwrap_or(i64::MAX)),
        energy_required: Some(i64::try_from(signed.energy_required).unwrap_or(i64::MAX)),
        tx_size_bytes: Some(i64::try_from(signed.tx_size_bytes).unwrap_or(i64::MAX)),
    })
}

pub async fn emulate_trigger_smart_contract_intent(
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    intent_specs: &[u8],
) -> Result<i64> {
    let intent = super::TriggerSmartContractIntent::abi_decode(intent_specs)
        .context("abi_decode TriggerSmartContractIntent")?;
    let to = TronAddress::from_evm(intent.to);
    let call_value_i64 =
        i64::try_from(intent.callValueSun).context("callValueSun out of i64 range")?;

    let wallet = TronWallet::new(cfg.private_key).context("init TronWallet")?;
    let mut grpc = connect_grpc(cfg).await?;
    emulate_trigger_smart_contract(
        &mut grpc,
        telemetry,
        &wallet,
        to,
        &intent.data,
        call_value_i64,
    )
    .await
}

pub async fn emulate_usdt_transfer_intent(
    hub: &crate::hub::HubClient,
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    intent_specs: &[u8],
) -> Result<i64> {
    let intent = super::USDTTransferIntent::abi_decode(intent_specs)
        .context("abi_decode USDTTransferIntent")?;
    let tron_usdt = hub.v3_tron_usdt().await.context("load V3.tronUsdt")?;
    let data = crate::abi::encode_trc20_transfer(intent.to, intent.amount);

    let wallet = TronWallet::new(cfg.private_key).context("init TronWallet")?;
    let mut grpc = connect_grpc(cfg).await?;
    emulate_trigger_smart_contract(
        &mut grpc,
        telemetry,
        &wallet,
        TronAddress::from_evm(tron_usdt),
        &alloy::primitives::Bytes::from(data),
        0,
    )
    .await
}

pub async fn build_proof(cfg: &TronConfig, jobs: &JobConfig, txid: [u8; 32]) -> Result<TronProof> {
    let mut grpc = connect_grpc(cfg).await?;
    build_proof_with(&mut grpc, jobs, txid).await
}

pub async fn tx_is_known(cfg: &TronConfig, telemetry: &SolverTelemetry, txid: [u8; 32]) -> bool {
    let mut grpc = match connect_grpc(cfg).await {
        Ok(v) => v,
        Err(_) => return false,
    };
    let started = std::time::Instant::now();
    let res = grpc.get_transaction_info_by_id(txid).await;
    let ok = match res {
        Ok(info) => {
            // Some nodes return an "empty" TransactionInfo for unknown txids. Treat a tx as
            // known only if we see either:
            // - the id field populated and equal to the requested txid, or
            // - a non-zero block_number (confirmed).
            let id_matches = info.id.len() == 32 && info.id.as_slice() == txid;
            let confirmed = info.block_number > 0;
            id_matches || confirmed
        }
        Err(_) => false,
    };
    telemetry.tron_grpc_ms(
        "get_transaction_info_by_id",
        ok,
        started.elapsed().as_millis() as u64,
    );
    ok
}

pub async fn broadcast_signed_tx(
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    tx_bytes: &[u8],
) -> Result<()> {
    let tx = tron::protocol::Transaction::decode(tx_bytes).context("decode signed tx bytes")?;
    let mut grpc = connect_grpc(cfg).await?;
    let started = std::time::Instant::now();
    let ret = grpc
        .broadcast_transaction(tx)
        .await
        .context("broadcast_transaction")?;
    telemetry.tron_grpc_ms(
        "broadcast_transaction",
        ret.result,
        started.elapsed().as_millis() as u64,
    );
    if !ret.result {
        // Re-broadcasts are expected after restarts (or if state was updated after a broadcast
        // but before persisting). Treat "duplicate" responses as success.
        let msg_utf8 = String::from_utf8_lossy(&ret.message).to_string();
        let msg_upper = msg_utf8.to_ascii_uppercase();
        if msg_upper.contains("DUP") || msg_upper.contains("EXISTS") {
            return Ok(());
        }
        anyhow::bail!(
            "broadcast failed: msg_hex=0x{}, msg_utf8={}",
            hex::encode(&ret.message),
            msg_utf8
        );
    }
    Ok(())
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
                let info = grpc
                    .get_transaction_info_by_id(txid)
                    .await
                    .context("get_transaction_info_by_id (post-finality)")?;
                let failed = info.block_number > 0
                    && info.result == tron::protocol::transaction_info::Code::Failed as i32;
                if failed {
                    let msg = String::from_utf8_lossy(&info.res_message).into_owned();
                    anyhow::bail!("tron_tx_failed: {msg}");
                }

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

async fn emulate_trigger_smart_contract(
    grpc: &mut TronGrpc,
    telemetry: &SolverTelemetry,
    wallet: &TronWallet,
    contract: TronAddress,
    data: &alloy::primitives::Bytes,
    call_value_sun: i64,
) -> Result<i64> {
    let msg = tron::protocol::TriggerSmartContract {
        owner_address: wallet.address().prefixed_bytes().to_vec(),
        contract_address: contract.prefixed_bytes().to_vec(),
        call_value: call_value_sun,
        data: data.to_vec(),
        call_token_value: 0,
        token_id: 0,
    };

    let started = std::time::Instant::now();
    let est = grpc.estimate_energy(msg).await.context("EstimateEnergy")?;
    let ok = est.result.as_ref().map(|r| r.result).unwrap_or(false);
    telemetry.tron_grpc_ms("estimate_energy", ok, started.elapsed().as_millis() as u64);

    let Some(ret) = est.result else {
        anyhow::bail!("emulation_failed: missing result");
    };
    if !ret.result {
        let msg_utf8 = String::from_utf8_lossy(&ret.message).into_owned();
        match tron::protocol::r#return::ResponseCode::try_from(ret.code) {
            Ok(tron::protocol::r#return::ResponseCode::ContractValidateError)
            | Ok(tron::protocol::r#return::ResponseCode::ContractExeError) => {
                anyhow::bail!(
                    "emulation_revert: code={} msg_hex=0x{} msg_utf8={}",
                    ret.code,
                    hex::encode(&ret.message),
                    msg_utf8
                );
            }
            _ => {
                anyhow::bail!(
                    "emulation_failed: code={} msg_hex=0x{} msg_utf8={}",
                    ret.code,
                    hex::encode(&ret.message),
                    msg_utf8
                );
            }
        }
    }

    Ok(est.energy_required)
}

#[cfg(test)]
mod tests {
    #[test]
    fn emulation_revert_marker_is_stable() {
        let msg = anyhow::anyhow!("emulation_revert: code=3 msg_hex=0x00 msg_utf8=oops");
        assert!(msg.to_string().contains("emulation_revert:"));
    }
}
