use super::protocol::TriggerSmartContract;
use super::{address::TronAddress, grpc::TronGrpc};
use alloy::primitives::{Address, FixedBytes, U256, keccak256};
use anyhow::{Context, Result};
use k256::ecdsa::SigningKey;

#[derive(Debug, Clone)]
pub struct BroadcastedTronTx {
    pub txid: [u8; 32],
    pub result: crate::protocol::Return,
}

impl BroadcastedTronTx {
    pub fn ok(&self) -> bool {
        self.result.result
    }

    pub fn message_lossy(&self) -> String {
        String::from_utf8_lossy(&self.result.message).into_owned()
    }

    pub fn message_hex(&self) -> String {
        hex::encode(&self.result.message)
    }
}

pub struct TronWallet {
    pub(crate) key: SigningKey,
    pub(crate) address: TronAddress,
}

impl TronWallet {
    pub fn new(private_key: [u8; 32]) -> Result<Self> {
        let key = SigningKey::from_slice(&private_key).context("invalid TRON private key")?;
        let address = tron_address_from_signing_key(&key);
        Ok(Self { key, address })
    }

    pub fn address(&self) -> TronAddress {
        self.address
    }

    pub async fn broadcast_trigger_smart_contract(
        &self,
        grpc: &mut TronGrpc,
        contract: TronAddress,
        data: Vec<u8>,
        call_value_sun: i64,
        fee_policy: crate::sender::FeePolicy,
    ) -> Result<[u8; 32]> {
        let res = self
            .broadcast_trigger_smart_contract_result(
                grpc,
                contract,
                data,
                call_value_sun,
                fee_policy,
            )
            .await?;

        if !res.ok() {
            anyhow::bail!(
                "broadcast failed: msg_hex=0x{}, msg_utf8={}",
                res.message_hex(),
                res.message_lossy()
            );
        }

        Ok(res.txid)
    }

    pub async fn broadcast_transfer_contract(
        &self,
        grpc: &mut TronGrpc,
        to: TronAddress,
        amount_sun: i64,
    ) -> Result<[u8; 32]> {
        let res = self
            .broadcast_transfer_contract_result(grpc, to, amount_sun)
            .await?;

        if !res.ok() {
            anyhow::bail!(
                "broadcast failed: msg_hex=0x{}, msg_utf8={}",
                res.message_hex(),
                res.message_lossy()
            );
        }

        Ok(res.txid)
    }

    pub async fn broadcast_transfer_contract_result(
        &self,
        grpc: &mut TronGrpc,
        to: TronAddress,
        amount_sun: i64,
    ) -> Result<BroadcastedTronTx> {
        let signed = self
            .build_and_sign_transfer_contract(grpc, to, amount_sun)
            .await
            .context("build_and_sign_transfer_contract")?;

        let ret = grpc
            .broadcast_transaction(signed.tx)
            .await
            .context("broadcast_transaction")?;

        Ok(BroadcastedTronTx {
            txid: signed.txid,
            result: ret,
        })
    }

    pub async fn broadcast_delegate_resource_contract(
        &self,
        grpc: &mut TronGrpc,
        receiver: TronAddress,
        resource: crate::protocol::ResourceCode,
        balance_sun: i64,
        lock: bool,
        lock_period: i64,
    ) -> Result<[u8; 32]> {
        let res = self
            .broadcast_delegate_resource_contract_result(
                grpc,
                receiver,
                resource,
                balance_sun,
                lock,
                lock_period,
            )
            .await?;

        if !res.ok() {
            anyhow::bail!(
                "broadcast failed: msg_hex=0x{}, msg_utf8={}",
                res.message_hex(),
                res.message_lossy()
            );
        }

        Ok(res.txid)
    }

    pub async fn broadcast_freeze_balance_v2(
        &self,
        grpc: &mut TronGrpc,
        frozen_balance_sun: i64,
        resource: crate::protocol::ResourceCode,
    ) -> Result<[u8; 32]> {
        let res = self
            .broadcast_freeze_balance_v2_result(grpc, frozen_balance_sun, resource)
            .await?;

        if !res.ok() {
            anyhow::bail!(
                "broadcast failed: msg_hex=0x{}, msg_utf8={}",
                res.message_hex(),
                res.message_lossy()
            );
        }

        Ok(res.txid)
    }

    pub async fn broadcast_freeze_balance_v2_result(
        &self,
        grpc: &mut TronGrpc,
        frozen_balance_sun: i64,
        resource: crate::protocol::ResourceCode,
    ) -> Result<BroadcastedTronTx> {
        let signed = self
            .build_and_sign_freeze_balance_v2(grpc, frozen_balance_sun, resource)
            .await
            .context("build_and_sign_freeze_balance_v2")?;

        let ret = grpc
            .broadcast_transaction(signed.tx)
            .await
            .context("broadcast_transaction")?;

        Ok(BroadcastedTronTx {
            txid: signed.txid,
            result: ret,
        })
    }

    pub async fn broadcast_delegate_resource_contract_result(
        &self,
        grpc: &mut TronGrpc,
        receiver: TronAddress,
        resource: crate::protocol::ResourceCode,
        balance_sun: i64,
        lock: bool,
        lock_period: i64,
    ) -> Result<BroadcastedTronTx> {
        let signed = self
            .build_and_sign_delegate_resource_contract(
                grpc,
                receiver,
                resource,
                balance_sun,
                lock,
                lock_period,
            )
            .await
            .context("build_and_sign_delegate_resource_contract")?;

        let ret = grpc
            .broadcast_transaction(signed.tx)
            .await
            .context("broadcast_transaction")?;

        Ok(BroadcastedTronTx {
            txid: signed.txid,
            result: ret,
        })
    }

    pub async fn broadcast_trigger_smart_contract_result(
        &self,
        grpc: &mut TronGrpc,
        contract: TronAddress,
        data: Vec<u8>,
        call_value_sun: i64,
        fee_policy: crate::sender::FeePolicy,
    ) -> Result<BroadcastedTronTx> {
        let account = grpc
            .get_account(self.address.prefixed_bytes().to_vec())
            .await
            .context("get Tron account")?;

        let signed = self
            .build_and_sign_trigger_smart_contract(grpc, contract, data, call_value_sun, fee_policy)
            .await
            .context("build_and_sign_trigger_smart_contract")?;

        let fee_limit_i64 = i64::try_from(signed.fee_limit_sun).unwrap_or(i64::MAX);
        let balance = account.balance;
        if balance < fee_limit_i64 {
            anyhow::bail!(
                "insufficient TRX for fee_limit: balance={} sun, fee_limit={} sun",
                balance,
                fee_limit_i64
            );
        }

        let ret = grpc
            .broadcast_transaction(signed.tx)
            .await
            .context("broadcast_transaction")?;

        Ok(BroadcastedTronTx {
            txid: signed.txid,
            result: ret,
        })
    }
}

fn tron_address_from_signing_key(key: &SigningKey) -> TronAddress {
    let public_key = key.verifying_key().to_encoded_point(false);
    let public_key_bytes = public_key.as_bytes();
    let hash = keccak256(&public_key_bytes[1..]);
    let addr20 = Address::from_slice(&hash[12..]);
    TronAddress::from_evm(addr20)
}

// ===== ABI helpers (EVM ABI, used by Tron TriggerSmartContract) =====

pub async fn trc20_balance_of(
    grpc: &mut TronGrpc,
    token_contract: TronAddress,
    owner: TronAddress,
    caller: TronAddress,
) -> Result<U256> {
    let selector = keccak256("balanceOf(address)".as_bytes());
    let mut data = Vec::with_capacity(4 + 32);
    data.extend_from_slice(&selector[..4]);
    data.extend_from_slice(&encode_address(owner.evm()));

    let tx_ext = grpc
        .trigger_constant_contract(TriggerSmartContract {
            owner_address: caller.prefixed_bytes().to_vec(),
            contract_address: token_contract.prefixed_bytes().to_vec(),
            call_value: 0,
            data,
            call_token_value: 0,
            token_id: 0,
        })
        .await
        .context("trigger_constant_contract(balanceOf)")?;

    let out = tx_ext
        .constant_result
        .first()
        .context("missing constant_result")?;
    if out.len() != 32 {
        anyhow::bail!("unexpected balanceOf output length: {}", out.len());
    }
    Ok(U256::from_be_slice(out))
}

pub fn encode_is_event_chain_tip(tip: FixedBytes<32>) -> Vec<u8> {
    let selector = keccak256("isEventChainTip(bytes32)".as_bytes());
    let mut out = Vec::with_capacity(4 + 32);
    out.extend_from_slice(&selector[..4]);
    out.extend_from_slice(tip.as_slice());
    out
}

pub fn encode_pull_from_receivers(token: Address, receiver_salts: &[FixedBytes<32>]) -> Vec<u8> {
    let selector = keccak256("pullFromReceivers(address,bytes32[])".as_bytes());

    let mut out = Vec::new();
    out.extend_from_slice(&selector[..4]);

    // head: (address, offset)
    out.extend_from_slice(&encode_address(token));
    out.extend_from_slice(&encode_u256(U256::from(64u64))); // dynamic tail starts after 2*32 bytes

    // tail: array
    out.extend_from_slice(&encode_u256(U256::from(receiver_salts.len())));
    for salt in receiver_salts {
        out.extend_from_slice(salt.as_slice());
    }

    out
}

pub fn encode_rebalance_usdt(rebalancer: Address, in_amount: U256) -> Vec<u8> {
    let selector = keccak256("rebalanceUsdt(address,uint256)".as_bytes());
    let mut out = Vec::with_capacity(4 + 32 + 32);
    out.extend_from_slice(&selector[..4]);
    out.extend_from_slice(&encode_address(rebalancer));
    out.extend_from_slice(&encode_u256(in_amount));
    out
}

fn encode_address(addr: Address) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[12..].copy_from_slice(addr.as_slice());
    out
}

fn encode_u256(v: U256) -> [u8; 32] {
    v.to_be_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use k256::ecdsa::signature::DigestVerifier;
    use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
    use prost::Message;
    use sha2::{Digest, Sha256};

    #[test]
    fn encode_is_event_chain_tip_layout() {
        let tip = FixedBytes::from_slice(&[0x11u8; 32]);
        let out = encode_is_event_chain_tip(tip);

        let selector = keccak256("isEventChainTip(bytes32)".as_bytes());
        assert_eq!(&out[..4], &selector[..4]);
        assert_eq!(&out[4..], tip.as_slice());
    }

    #[test]
    fn encode_rebalance_usdt_layout() {
        let rebalancer = Address::from_slice(&[0x22u8; 20]);
        let amount = U256::from(123_456_789u64);
        let out = encode_rebalance_usdt(rebalancer, amount);

        let selector = keccak256("rebalanceUsdt(address,uint256)".as_bytes());
        assert_eq!(&out[..4], &selector[..4]);

        let mut addr_word = [0u8; 32];
        addr_word[12..].copy_from_slice(rebalancer.as_slice());
        assert_eq!(&out[4..36], &addr_word);
        let amt_word: [u8; 32] = amount.to_be_bytes();
        assert_eq!(&out[36..68], &amt_word);
    }

    #[test]
    fn encode_pull_from_receivers_layout() {
        let token = Address::from_slice(&[0x44u8; 20]);
        let s1 = FixedBytes::from_slice(&[0x01u8; 32]);
        let s2 = FixedBytes::from_slice(&[0x02u8; 32]);

        let out = encode_pull_from_receivers(token, &[s1, s2]);

        let selector = keccak256("pullFromReceivers(address,bytes32[])".as_bytes());
        assert_eq!(&out[..4], &selector[..4]);

        let mut token_word = [0u8; 32];
        token_word[12..].copy_from_slice(token.as_slice());
        assert_eq!(&out[4..36], &token_word);

        let mut offset_word = [0u8; 32];
        offset_word[31] = 64;
        assert_eq!(&out[36..68], &offset_word);

        let mut len_word = [0u8; 32];
        len_word[31] = 2;
        assert_eq!(&out[68..100], &len_word);
        assert_eq!(&out[100..132], s1.as_slice());
        assert_eq!(&out[132..164], s2.as_slice());
        assert_eq!(out.len(), 164);
    }

    #[test]
    fn tx_signing_is_over_raw_data_sha256() {
        let private_key = [0x11u8; 32];
        let key = SigningKey::from_slice(&private_key).unwrap();

        // Build a minimal-but-non-empty raw tx so encoding is deterministic.
        let raw = crate::protocol::transaction::Raw {
            timestamp: 1,
            expiration: 2,
            fee_limit: 3,
            ..Default::default()
        };
        let raw_bytes = raw.encode_to_vec();
        let digest = Sha256::new_with_prefix(&raw_bytes);

        let (rec_sig, recid) = key.sign_digest_recoverable(digest.clone()).unwrap();
        let mut sig65 = rec_sig.to_bytes().to_vec();
        sig65.push(recid.to_byte() + 27);

        // txId is sha256(raw_data bytes)
        let txid = Sha256::digest(&raw_bytes);
        assert_eq!(txid.len(), 32);

        // Verify/recover.
        let sig = Signature::try_from(&sig65[..64]).unwrap();
        let v = sig65[64] - 27;
        let recid2 = RecoveryId::try_from(v).unwrap();
        let recovered = VerifyingKey::recover_from_digest(digest.clone(), &sig, recid2).unwrap();
        assert_eq!(recovered, *key.verifying_key(), "recovered key mismatch");
        recovered.verify_digest(digest, &sig).unwrap();
    }
}
