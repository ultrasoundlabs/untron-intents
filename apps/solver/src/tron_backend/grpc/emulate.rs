use super::connect_grpc;
use crate::{config::TronConfig, metrics::SolverTelemetry};
use alloy::sol_types::SolValue;
use anyhow::{Context, Result};
use tron::{TronAddress, TronGrpc, TronWallet};

pub(crate) async fn emulate_trigger_smart_contract_intent(
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    intent_specs: &[u8],
) -> Result<i64> {
    let intent = super::super::TriggerSmartContractIntent::abi_decode(intent_specs)
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

pub(crate) async fn emulate_usdt_transfer_intent(
    hub: &crate::hub::HubClient,
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    intent_specs: &[u8],
) -> Result<i64> {
    let intent = super::super::USDTTransferIntent::abi_decode(intent_specs)
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

pub(crate) async fn emulate_trigger_smart_contract(
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
