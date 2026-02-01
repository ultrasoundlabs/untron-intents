use alloy::primitives::{Address, FixedBytes, U256};
use alloy::sol_types::{Eip712Domain, SolStruct};
use anyhow::{Context, Result};
use k256::ecdsa::SigningKey;

use crate::contracts::SafeOp;
use crate::packing::{ensure_u48, pack_init_code, pack_paymaster_and_data, u48_be_bytes};
use alloy::rpc::types::eth::erc4337::PackedUserOperation;

pub(crate) fn safeop_digest(
    chain_id: u64,
    safe_4337_module: Address,
    entry_point: Address,
    op: &PackedUserOperation,
) -> Result<FixedBytes<32>> {
    let domain = Eip712Domain::new(
        None,
        None,
        Some(U256::from(chain_id)),
        Some(safe_4337_module),
        None,
    );

    let init_code = pack_init_code(op.factory, op.factory_data.as_ref())?;
    let paymaster_and_data = pack_paymaster_and_data(
        op.paymaster,
        op.paymaster_verification_gas_limit,
        op.paymaster_post_op_gas_limit,
        op.paymaster_data.as_ref(),
    )?;

    let verification_gas_limit = u128::try_from(op.verification_gas_limit)
        .context("verificationGasLimit overflows uint128")?;
    let call_gas_limit =
        u128::try_from(op.call_gas_limit).context("callGasLimit overflows uint128")?;
    let max_priority_fee = u128::try_from(op.max_priority_fee_per_gas)
        .context("maxPriorityFeePerGas overflows uint128")?;
    let max_fee = u128::try_from(op.max_fee_per_gas).context("maxFeePerGas overflows uint128")?;

    let valid_after: u64 = 0;
    let valid_until: u64 = 0;
    ensure_u48(valid_after, "validAfter")?;
    ensure_u48(valid_until, "validUntil")?;

    let safeop = SafeOp {
        safe: op.sender,
        nonce: op.nonce,
        initCode: init_code.into(),
        callData: op.call_data.clone(),
        verificationGasLimit: verification_gas_limit,
        callGasLimit: call_gas_limit,
        preVerificationGas: op.pre_verification_gas,
        maxPriorityFeePerGas: max_priority_fee,
        maxFeePerGas: max_fee,
        paymasterAndData: paymaster_and_data.into(),
        validAfter: alloy::primitives::Uint::<48, 1>::from(valid_after),
        validUntil: alloy::primitives::Uint::<48, 1>::from(valid_until),
        entryPoint: entry_point,
    };

    Ok(safeop.eip712_signing_hash(&domain))
}

pub(crate) fn sign_userop_with_key(
    owner_key: &SigningKey,
    chain_id: u64,
    safe_4337_module: Address,
    entry_point: Address,
    op: &PackedUserOperation,
) -> Result<Vec<u8>> {
    let valid_after: u64 = 0;
    let valid_until: u64 = 0;
    ensure_u48(valid_after, "validAfter")?;
    ensure_u48(valid_until, "validUntil")?;

    let digest = safeop_digest(chain_id, safe_4337_module, entry_point, op)?;

    let (sig, recid) = owner_key
        .sign_prehash_recoverable(digest.as_slice())
        .context("sign SafeOp digest")?;

    let mut sig65 = sig.to_bytes().to_vec();
    sig65.push(recid.to_byte() + 27);

    let mut out = Vec::with_capacity(12 + sig65.len());
    out.extend_from_slice(&u48_be_bytes(valid_after));
    out.extend_from_slice(&u48_be_bytes(valid_until));
    out.extend_from_slice(&sig65);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::Bytes;
    use k256::ecdsa::VerifyingKey;
    use k256::ecdsa::signature::hazmat::PrehashVerifier;

    #[test]
    fn sign_userop_prefix_and_verifies() {
        let owner_key = SigningKey::from_slice(&[7u8; 32]).unwrap();
        let verify_key = VerifyingKey::from(&owner_key);

        let chain_id = 10u64;
        let module = Address::repeat_byte(0x11);
        let entry = Address::repeat_byte(0x22);

        let op = PackedUserOperation {
            sender: Address::repeat_byte(0x33),
            nonce: U256::from(1u64),
            factory: None,
            factory_data: None,
            call_data: Bytes::from(vec![1, 2, 3]),
            call_gas_limit: U256::from(100_000u64),
            verification_gas_limit: U256::from(200_000u64),
            pre_verification_gas: U256::from(30_000u64),
            max_fee_per_gas: U256::from(1u64),
            max_priority_fee_per_gas: U256::from(1u64),
            paymaster: None,
            paymaster_verification_gas_limit: None,
            paymaster_post_op_gas_limit: None,
            paymaster_data: None,
            signature: Bytes::new(),
        };

        let sig = sign_userop_with_key(&owner_key, chain_id, module, entry, &op).unwrap();
        assert_eq!(sig.len(), 12 + 65);
        assert_eq!(&sig[0..12], &[0u8; 12]);

        let digest = safeop_digest(chain_id, module, entry, &op).unwrap();
        let sig64 = k256::ecdsa::Signature::from_slice(&sig[12..12 + 64]).unwrap();
        verify_key
            .verify_prehash(digest.as_slice(), &sig64)
            .unwrap();
    }
}
