use crate::config::TronConfig;
use anyhow::{Context, Result};
use tron::TronGrpc;

mod emulate;
mod fetch;
mod prepare;
mod proof;

pub(super) use emulate::{emulate_trigger_smart_contract_intent, emulate_usdt_transfer_intent};
pub(super) use fetch::{
    delegated_resource_available_sun, fetch_account, fetch_energy_stake_totals,
    fetch_net_stake_totals, fetch_transaction_info, fetch_trc20_balances_u64,
    fetch_trx_balances_sun,
};
pub(super) use prepare::{
    build_trc20_transfer, build_trx_transfer, prepare_delegate_resource,
    prepare_delegate_resource_with_key, prepare_trigger_smart_contract, prepare_trx_transfer,
    prepare_trx_transfer_with_key, prepare_usdt_transfer, prepare_usdt_transfer_with_key,
};
pub(super) use proof::{broadcast_signed_tx, build_proof, tx_is_known};

#[derive(Debug, Clone)]
pub(super) struct PreparedTronTx {
    pub txid: [u8; 32],
    pub tx_bytes: Vec<u8>,
    pub fee_limit_sun: Option<i64>,
    pub energy_required: Option<i64>,
    pub tx_size_bytes: Option<i64>,
}

pub(super) async fn connect_grpc(cfg: &TronConfig) -> Result<TronGrpc> {
    TronGrpc::connect(&cfg.grpc_url, cfg.api_key.as_deref())
        .await
        .context("connect tron grpc")
}
