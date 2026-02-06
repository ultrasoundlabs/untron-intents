use super::{PreparedTronTx, connect_grpc, emulate::emulate_trigger_smart_contract};
use crate::{config::TronConfig, metrics::SolverTelemetry};
use alloy::sol_types::SolValue;
use anyhow::{Context, Result};
use prost::Message;
use std::time::Duration;
use tron::{TronAddress, TronWallet};

pub(crate) async fn prepare_trx_transfer(
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    intent_specs: &[u8],
) -> Result<PreparedTronTx> {
    let intent = super::super::TRXTransferIntent::abi_decode(intent_specs)
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

pub(crate) async fn prepare_trx_transfer_with_key(
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    private_key: [u8; 32],
    intent_specs: &[u8],
) -> Result<PreparedTronTx> {
    let intent = super::super::TRXTransferIntent::abi_decode(intent_specs)
        .context("abi_decode TRXTransferIntent")?;
    let amount_sun_i64 = i64::try_from(intent.amountSun).context("amountSun out of i64 range")?;
    let to = TronAddress::from_evm(intent.to);
    build_trx_transfer(cfg, telemetry, private_key, to, amount_sun_i64).await
}

pub(crate) async fn build_trx_transfer(
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    private_key: [u8; 32],
    to: TronAddress,
    amount_sun: i64,
) -> Result<PreparedTronTx> {
    let wallet = TronWallet::new(private_key).context("init TronWallet")?;
    let mut grpc = connect_grpc(cfg).await?;

    let started = std::time::Instant::now();
    let signed = wallet
        .build_and_sign_transfer_contract(&mut grpc, to, amount_sun)
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

pub(crate) async fn prepare_trigger_smart_contract(
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    intent_specs: &[u8],
) -> Result<PreparedTronTx> {
    let intent = super::super::TriggerSmartContractIntent::abi_decode(intent_specs)
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

    maybe_attempt_energy_rental(cfg, wallet.address(), signed.energy_required, signed.txid).await;

    Ok(PreparedTronTx {
        txid: signed.txid,
        tx_bytes: signed.tx.encode_to_vec(),
        fee_limit_sun: Some(i64::try_from(signed.fee_limit_sun).unwrap_or(i64::MAX)),
        energy_required: Some(i64::try_from(signed.energy_required).unwrap_or(i64::MAX)),
        tx_size_bytes: Some(i64::try_from(signed.tx_size_bytes).unwrap_or(i64::MAX)),
    })
}

pub(crate) async fn build_trc20_transfer(
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    private_key: [u8; 32],
    token: TronAddress,
    to: TronAddress,
    amount: u64,
) -> Result<PreparedTronTx> {
    let wallet = TronWallet::new(private_key).context("init TronWallet")?;
    let mut grpc = connect_grpc(cfg).await?;

    let data = crate::abi::encode_trc20_transfer(to.evm(), alloy::primitives::U256::from(amount));
    let fee_policy = tron::sender::FeePolicy {
        fee_limit_cap_sun: cfg.fee_limit_cap_sun,
        fee_limit_headroom_ppm: cfg.fee_limit_headroom_ppm,
    };

    let started = std::time::Instant::now();
    let signed = wallet
        .build_and_sign_trigger_smart_contract(&mut grpc, token, data, 0, fee_policy)
        .await
        .context("build_and_sign_trigger_smart_contract")?;
    telemetry.tron_grpc_ms(
        "build_and_sign_trigger_smart_contract",
        true,
        started.elapsed().as_millis() as u64,
    );

    maybe_attempt_energy_rental(cfg, wallet.address(), signed.energy_required, signed.txid).await;

    Ok(PreparedTronTx {
        txid: signed.txid,
        tx_bytes: signed.tx.encode_to_vec(),
        fee_limit_sun: Some(i64::try_from(signed.fee_limit_sun).unwrap_or(i64::MAX)),
        energy_required: Some(i64::try_from(signed.energy_required).unwrap_or(i64::MAX)),
        tx_size_bytes: Some(i64::try_from(signed.tx_size_bytes).unwrap_or(i64::MAX)),
    })
}

async fn maybe_attempt_energy_rental(
    cfg: &TronConfig,
    owner: TronAddress,
    energy_required: u64,
    txid: [u8; 32],
) {
    if cfg.energy_rental_providers.is_empty() || energy_required == 0 {
        return;
    }

    let ctx = tron::rental::RentalContext {
        resource: tron::rental::RentalResourceKind::Energy,
        amount: energy_required,
        lock_period: None,
        duration_hours: None,
        balance_sun: None,
        address_base58check: owner.to_base58check(),
        address_hex41: format!("0x{}", hex::encode(owner.prefixed_bytes())),
        address_evm_hex: format!("{:#x}", owner.evm()),
        txid: Some(format!("0x{}", hex::encode(txid))),
    };

    for p in &cfg.energy_rental_providers {
        let provider = tron::rental::JsonApiRentalProvider::new(p.clone());
        let res = tokio::time::timeout(Duration::from_secs(2), provider.rent(&ctx)).await;
        match res {
            Ok(Ok(attempt)) => {
                if attempt.ok {
                    tracing::info!(
                        provider = %attempt.provider,
                        order_id = attempt.order_id.as_deref().unwrap_or(""),
                        "energy rental requested"
                    );
                    break;
                }
                tracing::warn!(
                    provider = %attempt.provider,
                    error = attempt.error.as_deref().unwrap_or(""),
                    "energy rental request failed"
                );
            }
            Ok(Err(err)) => {
                tracing::warn!(
                    provider = %provider.name(),
                    error = %err,
                    "energy rental request error"
                );
            }
            Err(_) => {
                tracing::warn!(
                    provider = %provider.name(),
                    "energy rental request timed out"
                );
            }
        }
    }
}

pub(crate) async fn prepare_delegate_resource(
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    intent_specs: &[u8],
) -> Result<PreparedTronTx> {
    prepare_delegate_resource_with_key(cfg, telemetry, cfg.private_key, intent_specs).await
}

pub(crate) async fn prepare_delegate_resource_with_key(
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    private_key: [u8; 32],
    intent_specs: &[u8],
) -> Result<PreparedTronTx> {
    let intent = super::super::DelegateResourceIntent::abi_decode(intent_specs)
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

    let wallet = TronWallet::new(private_key).context("init TronWallet")?;
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

pub(crate) async fn prepare_usdt_transfer(
    hub: &crate::hub::HubClient,
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    intent_specs: &[u8],
) -> Result<PreparedTronTx> {
    let intent = super::super::USDTTransferIntent::abi_decode(intent_specs)
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

    maybe_attempt_energy_rental(cfg, wallet.address(), signed.energy_required, signed.txid).await;

    Ok(PreparedTronTx {
        txid: signed.txid,
        tx_bytes: signed.tx.encode_to_vec(),
        fee_limit_sun: Some(i64::try_from(signed.fee_limit_sun).unwrap_or(i64::MAX)),
        energy_required: Some(i64::try_from(signed.energy_required).unwrap_or(i64::MAX)),
        tx_size_bytes: Some(i64::try_from(signed.tx_size_bytes).unwrap_or(i64::MAX)),
    })
}

pub(crate) async fn prepare_usdt_transfer_with_key(
    hub: &crate::hub::HubClient,
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    private_key: [u8; 32],
    intent_specs: &[u8],
) -> Result<PreparedTronTx> {
    let intent = super::super::USDTTransferIntent::abi_decode(intent_specs)
        .context("abi_decode USDTTransferIntent")?;
    let tron_usdt = hub.v3_tron_usdt().await.context("load V3.tronUsdt")?;

    let wallet = TronWallet::new(private_key).context("init TronWallet")?;
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

    maybe_attempt_energy_rental(cfg, wallet.address(), signed.energy_required, signed.txid).await;

    Ok(PreparedTronTx {
        txid: signed.txid,
        tx_bytes: signed.tx.encode_to_vec(),
        fee_limit_sun: Some(i64::try_from(signed.fee_limit_sun).unwrap_or(i64::MAX)),
        energy_required: Some(i64::try_from(signed.energy_required).unwrap_or(i64::MAX)),
        tx_size_bytes: Some(i64::try_from(signed.tx_size_bytes).unwrap_or(i64::MAX)),
    })
}
