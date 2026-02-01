use crate::contracts::{ISafe, ISafeModuleSetup, ISafeProxyFactory};
use alloy::network::EthereumWallet;
use alloy::primitives::{Address, B256, Bytes, U256, keccak256};
use alloy::providers::{DynProvider, Provider, ProviderBuilder};
use alloy::rpc::client::{BuiltInConnectionString, RpcClient};
use alloy::rpc::types::eth::transaction::{TransactionInput, TransactionRequest};
use alloy::signers::local::PrivateKeySigner;
use alloy::sol_types::SolCall;
use anyhow::{Context, Result};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct SafeDeterministicDeploymentConfig {
    pub proxy_factory: Address,
    pub singleton: Address,
    pub module_setup: Address,
    pub salt_nonce: U256,
}

#[derive(Debug, Clone)]
pub struct Safe4337Config {
    pub entrypoint: Address,
    pub safe_4337_module: Address,
}

fn u256_to_be32(x: U256) -> [u8; 32] {
    x.to_be_bytes::<32>()
}

fn singleton_as_u256_be32(singleton: Address) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[12..].copy_from_slice(singleton.as_slice());
    out
}

async fn dyn_provider_for_rpc(rpc_url: &str) -> Result<DynProvider> {
    let transport = BuiltInConnectionString::connect(rpc_url)
        .await
        .with_context(|| format!("connect rpc: {rpc_url}"))?;
    let client = RpcClient::builder().transport(transport, false);
    let provider = ProviderBuilder::default().connect_client(client);
    Ok(DynProvider::new(provider))
}

async fn wallet_provider_for_rpc(rpc_url: &str, private_key: [u8; 32]) -> Result<DynProvider> {
    let signer =
        PrivateKeySigner::from_bytes(&private_key.into()).context("invalid private key")?;
    let wallet = EthereumWallet::from(signer);

    let transport = BuiltInConnectionString::connect(rpc_url)
        .await
        .with_context(|| format!("connect rpc: {rpc_url}"))?;
    let client = RpcClient::builder().transport(transport, false);
    let provider = ProviderBuilder::default()
        .wallet(wallet)
        .connect_client(client);
    Ok(DynProvider::new(provider))
}

pub fn build_safe_4337_initializer(
    owner: Address,
    cfg: &Safe4337Config,
    deploy: &SafeDeterministicDeploymentConfig,
) -> Bytes {
    let enable_4337 = ISafeModuleSetup::enableModulesCall {
        modules: vec![cfg.safe_4337_module],
    }
    .abi_encode();

    let initializer = ISafe::setupCall {
        owners: vec![owner],
        threshold: U256::from(1u64),
        to: deploy.module_setup,
        data: enable_4337.into(),
        fallbackHandler: cfg.safe_4337_module,
        paymentToken: Address::ZERO,
        payment: U256::ZERO,
        paymentReceiver: Address::ZERO,
    }
    .abi_encode();

    initializer.into()
}

pub async fn fetch_proxy_init_code_hash(
    provider: &DynProvider,
    proxy_factory: Address,
    singleton: Address,
) -> Result<B256> {
    // Try `proxyCreationCodehash(singleton)` first (cheapest to use if available).
    {
        let call = ISafeProxyFactory::proxyCreationCodehashCall { singleton };
        let data = call.abi_encode();
        let tx = TransactionRequest {
            to: Some(proxy_factory.into()),
            input: TransactionInput::new(data.into()),
            ..Default::default()
        };
        match provider.call(tx).await {
            Ok(out) => {
                let hash = ISafeProxyFactory::proxyCreationCodehashCall::abi_decode_returns(&out)
                    .context("decode proxyCreationCodehash return")?;
                return Ok(hash);
            }
            Err(_err) => {
                // Fall through; not all factories expose this.
            }
        }
    }

    // Fallback: fetch proxy creation code bytes and hash deploymentData = creationCode ++ uint256(uint160(singleton)).
    let call = ISafeProxyFactory::proxyCreationCodeCall {};
    let data = call.abi_encode();
    let tx = TransactionRequest {
        to: Some(proxy_factory.into()),
        input: TransactionInput::new(data.into()),
        ..Default::default()
    };
    let out = provider
        .call(tx)
        .await
        .context("eth_call proxyCreationCode (factory does not expose proxyCreationCodehash)")?;
    let creation_code: Bytes = ISafeProxyFactory::proxyCreationCodeCall::abi_decode_returns(&out)
        .context("decode proxyCreationCode return")?;

    let mut deployment_data = Vec::with_capacity(creation_code.len() + 32);
    deployment_data.extend_from_slice(creation_code.as_ref());
    deployment_data.extend_from_slice(&singleton_as_u256_be32(singleton));
    Ok(keccak256(deployment_data))
}

pub fn predict_safe_proxy_address_create2(
    proxy_factory: Address,
    initializer: &Bytes,
    salt_nonce: U256,
    proxy_init_code_hash: B256,
) -> Address {
    // salt = keccak256( keccak256(initializer) || saltNonce )  (matches SafeProxyFactory.createProxyWithNonce)
    // Note: some factories support createChainSpecificProxyWithNonce which also includes chainId in the salt.
    // We currently deploy via createProxyWithNonce for maximal compatibility.
    let init_hash: B256 = keccak256(initializer.as_ref());
    let mut salt_preimage = Vec::with_capacity(64);
    salt_preimage.extend_from_slice(init_hash.as_slice());
    salt_preimage.extend_from_slice(&u256_to_be32(salt_nonce));
    let salt: B256 = keccak256(&salt_preimage);

    // create2 = keccak256(0xff || factory || salt || init_code_hash)[12..]
    let mut buf = Vec::with_capacity(1 + 20 + 32 + 32);
    buf.push(0xff);
    buf.extend_from_slice(proxy_factory.as_slice());
    buf.extend_from_slice(salt.as_slice());
    buf.extend_from_slice(proxy_init_code_hash.as_slice());

    let h: B256 = keccak256(&buf);
    Address::from_slice(&h.as_slice()[12..])
}

pub async fn ensure_safe_deployed(
    rpc_url: &str,
    chain_id: u64,
    owner_private_key: [u8; 32],
    safe_4337: &Safe4337Config,
    deploy: &SafeDeterministicDeploymentConfig,
) -> Result<Address> {
    let read = dyn_provider_for_rpc(rpc_url).await?;
    let wallet = wallet_provider_for_rpc(rpc_url, owner_private_key).await?;

    let signer =
        PrivateKeySigner::from_bytes(&owner_private_key.into()).context("invalid private key")?;
    let owner = signer.address();

    let initializer = build_safe_4337_initializer(owner, safe_4337, deploy);
    let init_code_hash =
        fetch_proxy_init_code_hash(&read, deploy.proxy_factory, deploy.singleton)
            .await
            .with_context(|| {
                "safe factory does not expose proxyCreationCode/hash; set HUB_SAFE_ADDRESS explicitly or use a SafeProxyFactory that exposes proxyCreationCode() or proxyCreationCodehash(address)".to_string()
            })?;
    let predicted = predict_safe_proxy_address_create2(
        deploy.proxy_factory,
        &initializer,
        deploy.salt_nonce,
        init_code_hash,
    );

    let code = read
        .get_code_at(predicted)
        .await
        .context("eth_getCode predicted safe")?;
    if !code.is_empty() {
        return Ok(predicted);
    }

    let calldata = ISafeProxyFactory::createProxyWithNonceCall {
        singleton: deploy.singleton,
        initializer: initializer.clone(),
        saltNonce: deploy.salt_nonce,
    }
    .abi_encode();

    let tx = TransactionRequest {
        from: Some(owner),
        chain_id: Some(chain_id),
        to: Some(deploy.proxy_factory.into()),
        input: TransactionInput::new(calldata.into()),
        ..Default::default()
    };

    // Alloy does not auto-fill these for wallet sends; set them explicitly.
    let nonce = read
        .get_transaction_count(owner)
        .await
        .context("eth_getTransactionCount")?;

    let gas_price: u128 = read.get_gas_price().await.context("eth_gasPrice")?;
    let priority: u128 = read
        .get_max_priority_fee_per_gas()
        .await
        .unwrap_or(gas_price / 10);

    let gas_limit: u64 = read
        .estimate_gas(tx.clone())
        .await
        .context("eth_estimateGas createProxyWithNonce")?;
    let gas_limit = gas_limit.saturating_mul(12).saturating_div(10).max(50_000);

    let tx = TransactionRequest {
        nonce: Some(nonce),
        gas: Some(gas_limit),
        max_fee_per_gas: Some(gas_price),
        max_priority_fee_per_gas: Some(priority),
        ..tx
    };

    let pending = wallet
        .send_transaction(tx)
        .await
        .context("send createProxyWithNonce tx")?;

    let _receipt = pending
        .with_timeout(Some(Duration::from_secs(60)))
        .get_receipt()
        .await
        .context("wait createProxyWithNonce receipt")?;

    let code = read
        .get_code_at(predicted)
        .await
        .context("eth_getCode predicted safe post-deploy")?;
    if code.is_empty() {
        anyhow::bail!("safe deployment tx mined but no code at predicted address");
    }

    Ok(predicted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::hex;

    #[test]
    fn initializer_decodes_to_expected_setup_params() {
        let owner = Address::repeat_byte(0x11);
        let entry = Address::repeat_byte(0x22);
        let module = Address::repeat_byte(0x33);
        let module_setup = Address::repeat_byte(0x44);

        let safe_4337 = Safe4337Config {
            entrypoint: entry,
            safe_4337_module: module,
        };
        let deploy = SafeDeterministicDeploymentConfig {
            proxy_factory: Address::repeat_byte(0x55),
            singleton: Address::repeat_byte(0x66),
            module_setup,
            salt_nonce: U256::ZERO,
        };

        let initializer = build_safe_4337_initializer(owner, &safe_4337, &deploy);
        let decoded = ISafe::setupCall::abi_decode(initializer.as_ref()).unwrap();
        assert_eq!(decoded.owners, vec![owner]);
        assert_eq!(decoded.threshold, U256::from(1u64));
        assert_eq!(decoded.to, module_setup);
        assert_eq!(decoded.fallbackHandler, module);
        assert_eq!(decoded.paymentToken, Address::ZERO);
        assert_eq!(decoded.payment, U256::ZERO);
        assert_eq!(decoded.paymentReceiver, Address::ZERO);

        let enable =
            ISafeModuleSetup::enableModulesCall::abi_decode(decoded.data.as_ref()).unwrap();
        assert_eq!(enable.modules, vec![module]);
    }

    #[test]
    fn create_proxy_with_nonce_calldata_matches_selector() {
        let singleton = Address::repeat_byte(0x66);
        let initializer = Bytes::from(hex!("010203"));
        let salt_nonce = U256::ZERO;
        let calldata = ISafeProxyFactory::createProxyWithNonceCall {
            singleton,
            initializer: initializer.clone().into(),
            saltNonce: salt_nonce,
        }
        .abi_encode();
        // Selector of createProxyWithNonce(address,bytes,uint256).
        assert_eq!(&calldata[0..4], &hex!("1688f0b9"));
    }

    #[test]
    fn singleton_u256_encoding_places_address_in_low_20_bytes() {
        let a: Address = "0x1111111111111111111111111111111111111111"
            .parse()
            .unwrap();
        let out = singleton_as_u256_be32(a);
        assert_eq!(&out[0..12], &[0u8; 12]);
        assert_eq!(&out[12..], a.as_slice());
    }
}
