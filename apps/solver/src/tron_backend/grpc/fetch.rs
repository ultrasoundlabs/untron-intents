use super::connect_grpc;
use crate::{config::TronConfig, metrics::SolverTelemetry};
use anyhow::{Context, Result};
use tron::TronAddress;

pub(crate) async fn fetch_account(
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    address: TronAddress,
) -> Result<tron::protocol::Account> {
    let mut grpc = connect_grpc(cfg).await?;
    let started = std::time::Instant::now();
    let account = grpc
        .get_account(address.prefixed_bytes().to_vec())
        .await
        .context("GetAccount")?;
    telemetry.tron_grpc_ms("get_account", true, started.elapsed().as_millis() as u64);
    Ok(account)
}

pub(crate) async fn fetch_energy_stake_totals(
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    address: TronAddress,
) -> Result<tron::resources::ResourceStakeTotals> {
    let mut grpc = connect_grpc(cfg).await?;
    let started = std::time::Instant::now();
    let msg = grpc
        .get_account_resource(address.prefixed_bytes().to_vec())
        .await
        .context("GetAccountResource")?;
    telemetry.tron_grpc_ms(
        "get_account_resource",
        true,
        started.elapsed().as_millis() as u64,
    );
    tron::resources::parse_energy_stake_totals(&msg).context("parse_energy_stake_totals")
}

#[allow(dead_code)]
pub(crate) async fn fetch_net_stake_totals(
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    address: TronAddress,
) -> Result<tron::resources::ResourceStakeTotals> {
    let mut grpc = connect_grpc(cfg).await?;
    let started = std::time::Instant::now();
    let msg = grpc
        .get_account_resource(address.prefixed_bytes().to_vec())
        .await
        .context("GetAccountResource")?;
    telemetry.tron_grpc_ms(
        "get_account_resource",
        true,
        started.elapsed().as_millis() as u64,
    );
    tron::resources::parse_net_stake_totals(&msg).context("parse_net_stake_totals")
}

#[allow(dead_code)]
pub(crate) async fn fetch_trx_balance_sun(
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    address: TronAddress,
) -> Result<i64> {
    Ok(fetch_account(cfg, telemetry, address).await?.balance)
}

pub(crate) async fn fetch_trx_balances_sun(
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    addresses: &[TronAddress],
) -> Result<Vec<i64>> {
    let mut grpc = connect_grpc(cfg).await?;
    let mut out = Vec::with_capacity(addresses.len());
    for a in addresses {
        let started = std::time::Instant::now();
        let account = grpc
            .get_account(a.prefixed_bytes().to_vec())
            .await
            .context("GetAccount")?;
        telemetry.tron_grpc_ms("get_account", true, started.elapsed().as_millis() as u64);
        out.push(account.balance);
    }
    Ok(out)
}

#[allow(dead_code)]
pub(crate) async fn fetch_trc20_balance_u64(
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    token: TronAddress,
    owner: TronAddress,
) -> Result<u64> {
    let mut grpc = connect_grpc(cfg).await?;
    let msg = tron::protocol::TriggerSmartContract {
        owner_address: owner.prefixed_bytes().to_vec(),
        contract_address: token.prefixed_bytes().to_vec(),
        data: crate::abi::encode_trc20_balance_of(owner.evm()),
        ..Default::default()
    };

    let started = std::time::Instant::now();
    let res = grpc
        .trigger_constant_contract(msg)
        .await
        .context("TriggerConstantContract(balanceOf)")?;
    telemetry.tron_grpc_ms(
        "trigger_constant_contract_balance_of",
        true,
        started.elapsed().as_millis() as u64,
    );

    let Some(first) = res.constant_result.first() else {
        return Ok(0);
    };
    // EVM ABI uint256 in big-endian (32 bytes). Some nodes may return shorter byte arrays.
    let mut buf = [0u8; 32];
    if first.len() >= 32 {
        buf.copy_from_slice(&first[first.len() - 32..]);
    } else {
        buf[32 - first.len()..].copy_from_slice(first);
    }
    let v = alloy::primitives::U256::from_be_bytes(buf);
    Ok(u64::try_from(v).unwrap_or(u64::MAX))
}

pub(crate) async fn fetch_trc20_balances_u64(
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    token: TronAddress,
    owners: &[TronAddress],
) -> Result<Vec<u64>> {
    let mut grpc = connect_grpc(cfg).await?;
    let mut out = Vec::with_capacity(owners.len());
    for o in owners {
        let msg = tron::protocol::TriggerSmartContract {
            owner_address: o.prefixed_bytes().to_vec(),
            contract_address: token.prefixed_bytes().to_vec(),
            data: crate::abi::encode_trc20_balance_of(o.evm()),
            ..Default::default()
        };
        let started = std::time::Instant::now();
        let res = grpc
            .trigger_constant_contract(msg)
            .await
            .context("TriggerConstantContract(balanceOf)")?;
        telemetry.tron_grpc_ms(
            "trigger_constant_contract_balance_of",
            true,
            started.elapsed().as_millis() as u64,
        );
        let Some(first) = res.constant_result.first() else {
            out.push(0);
            continue;
        };
        let mut buf = [0u8; 32];
        if first.len() >= 32 {
            buf.copy_from_slice(&first[first.len() - 32..]);
        } else {
            buf[32 - first.len()..].copy_from_slice(first);
        }
        let v = alloy::primitives::U256::from_be_bytes(buf);
        out.push(u64::try_from(v).unwrap_or(u64::MAX));
    }
    Ok(out)
}

pub(crate) fn delegated_resource_available_sun(
    account: &tron::protocol::Account,
    resource: tron::protocol::ResourceCode,
) -> i64 {
    let mut staked: i64 = 0;
    for f in &account.frozen_v2 {
        if tron::protocol::ResourceCode::try_from(f.r#type).ok() == Some(resource) {
            staked = staked.saturating_add(f.amount);
        }
    }

    let delegated: i64 = match resource {
        tron::protocol::ResourceCode::Energy => account
            .account_resource
            .as_ref()
            .map(|r| r.delegated_frozen_v2_balance_for_energy)
            .unwrap_or(0),
        tron::protocol::ResourceCode::Bandwidth => {
            account.delegated_frozen_v2_balance_for_bandwidth
        }
        tron::protocol::ResourceCode::TronPower => 0,
    };

    staked.saturating_sub(delegated).max(0)
}

pub(crate) async fn fetch_transaction_info(
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
