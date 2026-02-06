use super::{
    DelegateResourceIntent, TRXTransferIntent, TriggerSmartContractIntent, TronExecution,
    USDTTransferIntent, empty_proof, evm_to_tron_raw21, tron_sender_from_privkey_or_fallback,
};
use crate::{
    abi::encode_trc20_transfer,
    config::TronConfig,
    hub::{DelegateResourceContract, HubClient, TransferContract, TriggerSmartContract},
};
use alloy::primitives::{B256, U256, keccak256};
use alloy::sol_types::SolValue;
use anyhow::{Context, Result};

pub async fn execute_trx_transfer(
    hub: &HubClient,
    cfg: &TronConfig,
    intent_id: B256,
    intent_specs: &[u8],
) -> Result<TronExecution> {
    let reader = cfg
        .mock_reader_address
        .context("missing TRON_MOCK_READER_ADDRESS")?;
    let intent =
        TRXTransferIntent::abi_decode(intent_specs).context("abi_decode TRXTransferIntent")?;
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

    Ok(TronExecution::ImmediateProof(Box::new(empty_proof())))
}

pub async fn execute_trigger_smart_contract(
    hub: &HubClient,
    cfg: &TronConfig,
    intent_id: B256,
    intent_specs: &[u8],
) -> Result<TronExecution> {
    let reader = cfg
        .mock_reader_address
        .context("missing TRON_MOCK_READER_ADDRESS")?;
    let intent = TriggerSmartContractIntent::abi_decode(intent_specs)
        .context("abi_decode TriggerSmartContractIntent")?;
    let tx_id = keccak256([intent_id.as_slice(), b":trigger"].concat());

    let call = TriggerSmartContract {
        txId: tx_id,
        tronBlockNumber: U256::from(10u64),
        tronBlockTimestamp: 10u32,
        senderTron: tron_sender_from_privkey_or_fallback(cfg.private_key, hub),
        toTron: evm_to_tron_raw21(intent.to),
        callValueSun: intent.callValueSun,
        data: intent.data,
    };

    hub.mock_set_trigger_tx(reader, call)
        .await
        .context("mock setTx")?;

    Ok(TronExecution::ImmediateProof(Box::new(empty_proof())))
}

pub async fn execute_delegate_resource(
    hub: &HubClient,
    cfg: &TronConfig,
    intent_id: B256,
    intent_specs: &[u8],
) -> Result<TronExecution> {
    let reader = cfg
        .mock_reader_address
        .context("missing TRON_MOCK_READER_ADDRESS")?;
    let intent =
        DelegateResourceIntent::abi_decode(intent_specs).context("abi_decode DelegateResource")?;
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

    Ok(TronExecution::ImmediateProof(Box::new(empty_proof())))
}

pub async fn execute_usdt_transfer(
    hub: &HubClient,
    cfg: &TronConfig,
    intent_id: B256,
    intent_specs: &[u8],
) -> Result<TronExecution> {
    let reader = cfg
        .mock_reader_address
        .context("missing TRON_MOCK_READER_ADDRESS")?;
    let intent = USDTTransferIntent::abi_decode(intent_specs).context("abi_decode USDT")?;

    let tron_usdt = hub.v3_tron_usdt().await.context("load V3.tronUsdt")?;
    let tx_id = keccak256([intent_id.as_slice(), b":usdt"].concat());

    let data = encode_trc20_transfer(intent.to, intent.amount);
    let call = TriggerSmartContract {
        txId: tx_id,
        tronBlockNumber: U256::from(3u64),
        tronBlockTimestamp: 3u32,
        senderTron: tron_sender_from_privkey_or_fallback(cfg.private_key, hub),
        toTron: evm_to_tron_raw21(tron_usdt),
        callValueSun: U256::ZERO,
        data: data.into(),
    };

    hub.mock_set_trigger_tx(reader, call)
        .await
        .context("mock setTx")?;

    Ok(TronExecution::ImmediateProof(Box::new(empty_proof())))
}
