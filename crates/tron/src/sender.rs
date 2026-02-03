use super::grpc::TronGrpc;
use super::protocol::{
    CreateSmartContract, DelegateResourceContract, FreezeBalanceV2Contract, SmartContract,
    Transaction, TransferContract, TriggerSmartContract,
};
use super::resources::{parse_chain_fees, quote_fee_limit_sun};
use super::{TronAddress, TronWallet};
use anyhow::{Context, Result};
use prost::Message;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy)]
pub struct FeePolicy {
    /// Cap (sun) applied after headroom.
    pub fee_limit_cap_sun: u64,
    /// Extra headroom applied as parts-per-million.
    pub fee_limit_headroom_ppm: u64,
}

impl FeePolicy {
    pub fn apply(&self, base: u64) -> u64 {
        let headroom = base.saturating_mul(self.fee_limit_headroom_ppm.min(1_000_000)) / 1_000_000;
        base.saturating_add(headroom).min(self.fee_limit_cap_sun)
    }
}

#[derive(Debug, Clone)]
pub struct SignedTronTx {
    pub tx: Transaction,
    /// `sha256(raw_data_bytes)`.
    pub txid: [u8; 32],
    pub fee_limit_sun: u64,
    pub energy_required: u64,
    pub tx_size_bytes: u64,
}

impl TronWallet {
    /// Builds and signs a contract deployment (`CreateSmartContract`) tx.
    ///
    /// Notes:
    /// - This uses the node's `DeployContract` to construct the tx skeleton (ref block bytes/hash/etc).
    /// - `fee_limit_sun` is applied locally at signing time (the node tx skeleton may omit it).
    pub async fn build_and_sign_deploy_contract(
        &self,
        grpc: &mut TronGrpc,
        name: &str,
        bytecode: Vec<u8>,
        fee_limit_sun: i64,
        consume_user_resource_percent: i64,
        origin_energy_limit: i64,
    ) -> Result<SignedTronTx> {
        let owner = self.address.prefixed_bytes().to_vec();

        let tx_ext = grpc
            .deploy_contract(CreateSmartContract {
                owner_address: owner.clone(),
                new_contract: Some(SmartContract {
                    origin_address: owner,
                    name: name.to_string(),
                    bytecode,
                    consume_user_resource_percent,
                    origin_energy_limit,
                    ..Default::default()
                }),
                call_token_value: 0,
                token_id: 0,
            })
            .await
            .context("deploy_contract")?;

        if tx_ext.transaction.is_none() {
            let msg = tx_ext
                .result
                .as_ref()
                .map(|r| String::from_utf8_lossy(&r.message).into_owned())
                .unwrap_or_else(|| "<missing>".to_string());
            let ok = tx_ext.result.as_ref().map(|r| r.result);
            anyhow::bail!("node returned no transaction for DeployContract: ok={ok:?} msg={msg}");
        }

        let mut tx = tx_ext.transaction.context("node returned no transaction")?;
        let raw = tx.raw_data.take().context("node returned no raw_data")?;
        let (signed, txid, tx_size) =
            self.sign_raw_with_fee_limit(raw, tx.ret.clone(), fee_limit_sun)?;

        Ok(SignedTronTx {
            tx: signed,
            txid,
            fee_limit_sun: u64::try_from(fee_limit_sun.max(0)).unwrap_or(u64::MAX),
            energy_required: 0,
            tx_size_bytes: tx_size,
        })
    }

    /// Builds and signs a FreezeBalanceV2 tx (Stake 2.0).
    pub async fn build_and_sign_freeze_balance_v2(
        &self,
        grpc: &mut TronGrpc,
        frozen_balance_sun: i64,
        resource: super::protocol::ResourceCode,
    ) -> Result<SignedTronTx> {
        let owner = self.address.prefixed_bytes().to_vec();

        let tx_ext = grpc
            .freeze_balance_v2(FreezeBalanceV2Contract {
                owner_address: owner,
                frozen_balance: frozen_balance_sun,
                resource: resource as i32,
            })
            .await
            .context("freeze_balance_v2")?;

        if tx_ext.transaction.is_none() {
            let msg = tx_ext
                .result
                .as_ref()
                .map(|r| String::from_utf8_lossy(&r.message).into_owned())
                .unwrap_or_else(|| "<missing>".to_string());
            let ok = tx_ext.result.as_ref().map(|r| r.result);
            anyhow::bail!("node returned no transaction for FreezeBalanceV2: ok={ok:?} msg={msg}");
        }

        let mut tx = tx_ext.transaction.context("node returned no transaction")?;
        let raw = tx.raw_data.take().context("node returned no raw_data")?;

        let (signed, txid, tx_size) = self.sign_raw_with_fee_limit(raw, tx.ret.clone(), 0)?;

        Ok(SignedTronTx {
            tx: signed,
            txid,
            fee_limit_sun: 0,
            energy_required: 0,
            tx_size_bytes: tx_size,
        })
    }

    /// Builds and signs a native TRX transfer (`TransferContract`) tx.
    pub async fn build_and_sign_transfer_contract(
        &self,
        grpc: &mut TronGrpc,
        to: TronAddress,
        amount_sun: i64,
    ) -> Result<SignedTronTx> {
        let owner = self.address.prefixed_bytes().to_vec();
        let to_addr = to.prefixed_bytes().to_vec();

        let mut tx = grpc
            .create_transfer_transaction(TransferContract {
                owner_address: owner,
                to_address: to_addr,
                amount: amount_sun,
            })
            .await
            .context("create_transfer_transaction")?;
        let raw = tx.raw_data.take().context("node returned no raw_data")?;

        let (signed, txid, tx_size) = self.sign_raw_with_fee_limit(raw, tx.ret.clone(), 0)?;

        Ok(SignedTronTx {
            tx: signed,
            txid,
            fee_limit_sun: 0,
            energy_required: 0,
            tx_size_bytes: tx_size,
        })
    }

    /// Builds and signs a resource delegation (`DelegateResourceContract`) tx.
    pub async fn build_and_sign_delegate_resource_contract(
        &self,
        grpc: &mut TronGrpc,
        receiver: TronAddress,
        resource: super::protocol::ResourceCode,
        balance_sun: i64,
        lock: bool,
        lock_period: i64,
    ) -> Result<SignedTronTx> {
        let owner = self.address.prefixed_bytes().to_vec();
        let receiver_address = receiver.prefixed_bytes().to_vec();

        let tx_ext = grpc
            .delegate_resource(DelegateResourceContract {
                owner_address: owner,
                resource: resource as i32,
                balance: balance_sun,
                receiver_address,
                lock,
                lock_period,
            })
            .await
            .context("delegate_resource")?;

        if tx_ext.transaction.is_none() {
            let msg = tx_ext
                .result
                .as_ref()
                .map(|r| String::from_utf8_lossy(&r.message).into_owned())
                .unwrap_or_else(|| "<missing>".to_string());
            let ok = tx_ext.result.as_ref().map(|r| r.result);
            anyhow::bail!("node returned no transaction for DelegateResource: ok={ok:?} msg={msg}");
        }

        let mut tx = tx_ext.transaction.context("node returned no transaction")?;
        let raw = tx.raw_data.take().context("node returned no raw_data")?;

        let (signed, txid, tx_size) = self.sign_raw_with_fee_limit(raw, tx.ret.clone(), 0)?;

        Ok(SignedTronTx {
            tx: signed,
            txid,
            fee_limit_sun: 0,
            energy_required: 0,
            tx_size_bytes: tx_size,
        })
    }

    /// Builds and signs a TriggerSmartContract tx with a fee limit derived from chain parameters.
    ///
    /// Important nuance:
    /// - Even if energy is "rented"/delegated, many nodes still require the account to have enough
    ///   TRX balance to cover `fee_limit` as a worst-case bound. We therefore compute fee_limit as:
    ///   `energy_required * getEnergyFee + tx_size_bytes * getTransactionFee`, plus headroom and cap.
    pub async fn build_and_sign_trigger_smart_contract(
        &self,
        grpc: &mut TronGrpc,
        contract: TronAddress,
        data: Vec<u8>,
        call_value_sun: i64,
        fee_policy: FeePolicy,
    ) -> Result<SignedTronTx> {
        let chain_params = grpc.get_chain_parameters().await?;
        let fees = parse_chain_fees(&chain_params)?;

        let owner = self.address.prefixed_bytes().to_vec();
        let contract_addr = contract.prefixed_bytes().to_vec();

        let energy_required_i64 = grpc
            .estimate_energy(TriggerSmartContract {
                owner_address: owner.clone(),
                contract_address: contract_addr.clone(),
                call_value: call_value_sun,
                data: data.clone(),
                call_token_value: 0,
                token_id: 0,
            })
            .await?
            .energy_required;
        let mut energy_required =
            u64::try_from(energy_required_i64).context("energy_required out of range")?;
        // Some private Tron networks return `energy_required=0` even for state-changing calls.
        // A too-small derived fee_limit causes nodes to reject txs with "Not enough energy".
        if energy_required == 0 {
            energy_required = 50_000;
        }

        // Ask node to build the tx skeleton (ref block bytes/hash/etc).
        let tx_ext = grpc
            .trigger_contract(TriggerSmartContract {
                owner_address: owner,
                contract_address: contract_addr,
                call_value: call_value_sun,
                data,
                call_token_value: 0,
                token_id: 0,
            })
            .await
            .context("trigger_contract")?;

        let mut tx = tx_ext.transaction.context("node returned no transaction")?;
        let raw = tx.raw_data.take().context("node returned no raw_data")?;

        // Two-pass sizing to account for fee_limit varint size in raw_data (affects tx size/bandwidth fee).
        let (_signed0, _txid0, tx_size0) =
            self.sign_raw_with_fee_limit(raw.clone(), tx.ret.clone(), 0)?;

        let base0 = quote_fee_limit_sun(energy_required, tx_size0, fees);
        let fee_limit0 = fee_policy.apply(base0);

        let (signed1, txid1, tx_size1) = self.sign_raw_with_fee_limit(
            raw.clone(),
            tx.ret.clone(),
            i64::try_from(fee_limit0).context("fee_limit_sun out of range")?,
        )?;

        let base1 = quote_fee_limit_sun(energy_required, tx_size1, fees);
        let fee_limit1 = fee_policy.apply(base1);

        let (tx_final, txid_final, tx_size_final, fee_limit_final) = if fee_limit1 == fee_limit0 {
            (signed1, txid1, tx_size1, fee_limit1)
        } else {
            let (signed2, txid2, tx_size2) = self.sign_raw_with_fee_limit(
                raw,
                tx.ret,
                i64::try_from(fee_limit1).context("fee_limit_sun out of range")?,
            )?;
            (signed2, txid2, tx_size2, fee_limit1)
        };

        Ok(SignedTronTx {
            tx: tx_final,
            txid: txid_final,
            fee_limit_sun: fee_limit_final,
            energy_required,
            tx_size_bytes: tx_size_final,
        })
    }

    fn sign_raw_with_fee_limit(
        &self,
        mut raw: super::protocol::transaction::Raw,
        ret: Vec<super::protocol::transaction::Result>,
        fee_limit_sun: i64,
    ) -> Result<(Transaction, [u8; 32], u64)> {
        raw.fee_limit = fee_limit_sun.max(0);

        let raw_bytes = raw.encode_to_vec();
        let txid = Sha256::digest(&raw_bytes);

        let (rec_sig, recid) = self
            .key
            .clone()
            .sign_digest_recoverable(Sha256::new_with_prefix(&raw_bytes))
            .context("sign Tron tx")?;

        let mut sig65 = rec_sig.to_bytes().to_vec();
        sig65.push(recid.to_byte() + 27);

        let signed = Transaction {
            raw_data: Some(raw),
            signature: vec![sig65],
            ret,
        };

        let size = u64::try_from(signed.encode_to_vec().len()).unwrap_or(u64::MAX);

        let mut out = [0u8; 32];
        out.copy_from_slice(&txid);
        Ok((signed, out, size))
    }
}
