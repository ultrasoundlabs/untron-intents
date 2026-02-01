use alloy::{
    primitives::Address,
    providers::{DynProvider, Provider, ProviderBuilder},
    rpc::client::{BuiltInConnectionString, RpcClient},
};
use alloy_provider::ext::Erc4337Api;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::time::Duration;

use alloy::rpc::types::eth::erc4337::PackedUserOperation;
use alloy::rpc::types::eth::erc4337::SendUserOperationResponse;

use crate::packing::redact_url;

const RPC_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub(crate) struct BundlerPool {
    urls: Vec<String>,
    providers: Vec<DynProvider>,
    next_idx: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UserOperationGasEstimationV07 {
    pub pre_verification_gas: alloy::primitives::U256,
    pub verification_gas_limit: alloy::primitives::U256,
    pub call_gas_limit: alloy::primitives::U256,
    pub paymaster_verification_gas_limit: alloy::primitives::U256,
    pub paymaster_post_op_gas_limit: alloy::primitives::U256,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendUserOperationResponseObj {
    user_op_hash: alloy::primitives::Bytes,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SendUserOperationResponseAny {
    // Canonical ERC-4337 response shape:
    //   { "userOpHash": "0x..." }
    Obj(SendUserOperationResponseObj),
    // Some bundlers (incl. Pimlico) return just the hash as a string.
    Hash(alloy::primitives::Bytes),
}

impl From<SendUserOperationResponseAny> for SendUserOperationResponse {
    fn from(v: SendUserOperationResponseAny) -> Self {
        match v {
            SendUserOperationResponseAny::Obj(v) => SendUserOperationResponse {
                user_op_hash: v.user_op_hash,
            },
            SendUserOperationResponseAny::Hash(user_op_hash) => {
                SendUserOperationResponse { user_op_hash }
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EstimateV07 {
    pre_verification_gas: alloy::primitives::U256,
    verification_gas_limit: alloy::primitives::U256,
    call_gas_limit: alloy::primitives::U256,
    #[serde(default)]
    paymaster_verification_gas_limit: alloy::primitives::U256,
    #[serde(default)]
    paymaster_post_op_gas_limit: alloy::primitives::U256,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EstimateV06 {
    pre_verification_gas: alloy::primitives::U256,
    verification_gas: alloy::primitives::U256,
    paymaster_verification_gas: alloy::primitives::U256,
    paymaster_post_op_gas: alloy::primitives::U256,
    call_gas_limit: alloy::primitives::U256,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum EstimateAny {
    V07(EstimateV07),
    V06(EstimateV06),
}

impl From<EstimateAny> for UserOperationGasEstimationV07 {
    fn from(v: EstimateAny) -> Self {
        match v {
            EstimateAny::V07(v) => Self {
                pre_verification_gas: v.pre_verification_gas,
                verification_gas_limit: v.verification_gas_limit,
                call_gas_limit: v.call_gas_limit,
                paymaster_verification_gas_limit: v.paymaster_verification_gas_limit,
                paymaster_post_op_gas_limit: v.paymaster_post_op_gas_limit,
            },
            EstimateAny::V06(v) => Self {
                pre_verification_gas: v.pre_verification_gas,
                verification_gas_limit: v.verification_gas,
                call_gas_limit: v.call_gas_limit,
                paymaster_verification_gas_limit: v.paymaster_verification_gas,
                paymaster_post_op_gas_limit: v.paymaster_post_op_gas,
            },
        }
    }
}

impl BundlerPool {
    pub(crate) async fn new(urls: Vec<String>) -> Result<Self> {
        if urls.is_empty() {
            anyhow::bail!("bundler urls must be non-empty");
        }

        let mut providers = Vec::with_capacity(urls.len());
        for url in &urls {
            let transport =
                tokio::time::timeout(RPC_TIMEOUT, BuiltInConnectionString::connect(url))
                    .await
                    .map_err(|_| {
                        anyhow::anyhow!("timed out connecting bundler rpc: {}", redact_url(url))
                    })?
                    .with_context(|| format!("connect bundler rpc: {}", redact_url(url)))?;
            let client = RpcClient::builder().transport(transport, false);
            let provider = ProviderBuilder::default().connect_client(client);
            providers.push(DynProvider::new(provider));
        }

        Ok(Self {
            urls,
            providers,
            next_idx: 0,
        })
    }

    fn mark_success(&mut self, idx: usize) {
        if !self.providers.is_empty() {
            self.next_idx = (idx + 1) % self.providers.len();
        }
    }

    pub(crate) async fn estimate_user_operation_gas(
        &mut self,
        user_op: &PackedUserOperation,
        entry_point: Address,
    ) -> Result<UserOperationGasEstimationV07> {
        let mut last_err: Option<anyhow::Error> = None;

        let order = rotate_order(self.next_idx, self.providers.len());
        for idx in order {
            let url = &self.urls[idx];
            let url = redact_url(url);
            let provider = &self.providers[idx];

            // Use a tolerant response type: many bundlers return v0.7 field names
            // (`verificationGasLimit`, `paymasterVerificationGasLimit`, ...), but alloy's
            // `UserOperationGasEstimation` type currently models v0.6 response names.
            let fut = provider.raw_request(
                "eth_estimateUserOperationGas".into(),
                (user_op.clone(), entry_point),
            );

            match tokio::time::timeout(RPC_TIMEOUT, fut).await {
                Ok(Ok(v)) => {
                    let v: EstimateAny = v;
                    self.mark_success(idx);
                    return Ok(v.into());
                }
                Ok(Err(err)) => {
                    let err = anyhow::Error::new(err).context("eth_estimateUserOperationGas");
                    tracing::warn!(
                        bundler = %url,
                        err = %format!("{err:#}"),
                        "bundler rpc failed"
                    );
                    last_err = Some(err);
                }
                Err(_) => {
                    let err = anyhow::anyhow!("timed out");
                    tracing::warn!(
                        bundler = %url,
                        err = %err,
                        "bundler rpc timed out (eth_estimateUserOperationGas)"
                    );
                    last_err = Some(err);
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            anyhow::anyhow!("all bundlers failed for eth_estimateUserOperationGas")
        }))
    }

    pub(crate) async fn send_user_operation(
        &mut self,
        user_op: &PackedUserOperation,
        entry_point: Address,
    ) -> Result<SendUserOperationResponse> {
        let mut last_err: Option<anyhow::Error> = None;

        let order = rotate_order(self.next_idx, self.providers.len());
        for idx in order {
            let url = &self.urls[idx];
            let url = redact_url(url);
            let provider = &self.providers[idx];

            // Use a tolerant response type: some bundlers return `{ userOpHash }`, others return
            // the hash string directly.
            let fut = provider.raw_request(
                "eth_sendUserOperation".into(),
                (user_op.clone(), entry_point),
            );

            match tokio::time::timeout(RPC_TIMEOUT, fut).await {
                Ok(Ok(v)) => {
                    let v: SendUserOperationResponseAny = v;
                    let v: SendUserOperationResponse = v.into();
                    self.mark_success(idx);
                    return Ok(v);
                }
                Ok(Err(err)) => {
                    let err = anyhow::Error::new(err).context("eth_sendUserOperation");
                    tracing::warn!(
                        bundler = %url,
                        err = %format!("{err:#}"),
                        "bundler rpc failed"
                    );
                    last_err = Some(err);
                }
                Err(_) => {
                    let err = anyhow::anyhow!("timed out");
                    tracing::warn!(
                        bundler = %url,
                        err = %err,
                        "bundler rpc timed out (eth_sendUserOperation)"
                    );
                    last_err = Some(err);
                }
            }
        }

        Err(last_err
            .unwrap_or_else(|| anyhow::anyhow!("all bundlers failed for eth_sendUserOperation")))
    }

    pub(crate) async fn supported_entry_points(&mut self) -> Result<Vec<Address>> {
        let mut last_err: Option<anyhow::Error> = None;

        let order = rotate_order(self.next_idx, self.providers.len());
        for idx in order {
            let url = &self.urls[idx];
            let url = redact_url(url);
            let provider = &self.providers[idx];

            let fut = provider.supported_entry_points();
            match tokio::time::timeout(RPC_TIMEOUT, fut).await {
                Ok(Ok(v)) => {
                    self.mark_success(idx);
                    return Ok(v);
                }
                Ok(Err(err)) => {
                    let err = anyhow::Error::new(err).context("eth_supportedEntryPoints");
                    tracing::warn!(
                        bundler = %url,
                        err = %format!("{err:#}"),
                        "bundler rpc failed"
                    );
                    last_err = Some(err);
                }
                Err(_) => {
                    let err = anyhow::anyhow!("timed out");
                    tracing::warn!(
                        bundler = %url,
                        err = %err,
                        "bundler rpc timed out (eth_supportedEntryPoints)"
                    );
                    last_err = Some(err);
                }
            }
        }

        Err(last_err
            .unwrap_or_else(|| anyhow::anyhow!("all bundlers failed for eth_supportedEntryPoints")))
    }
}

fn rotate_order(start_idx: usize, len: usize) -> Vec<usize> {
    if len == 0 {
        return Vec::new();
    }
    let start = start_idx % len;
    (0..len).map(|o| (start + o) % len).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_rotates_from_next_idx() {
        assert_eq!(rotate_order(0, 3), vec![0, 1, 2]);
        assert_eq!(rotate_order(2, 3), vec![2, 0, 1]);
        assert_eq!(rotate_order(5, 3), vec![2, 0, 1]);
    }

    #[test]
    fn send_user_operation_response_accepts_object_or_hash_string() {
        let obj = r#"{"userOpHash":"0x1234"}"#;
        let v: SendUserOperationResponseAny = serde_json::from_str(obj).unwrap();
        let r: SendUserOperationResponse = v.into();
        assert_eq!(r.user_op_hash.as_ref(), &[0x12, 0x34]);

        let hash = r#""0x1234""#;
        let v: SendUserOperationResponseAny = serde_json::from_str(hash).unwrap();
        let r: SendUserOperationResponse = v.into();
        assert_eq!(r.user_op_hash.as_ref(), &[0x12, 0x34]);
    }
}
