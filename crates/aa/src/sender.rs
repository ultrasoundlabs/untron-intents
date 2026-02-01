use crate::bundler_pool::BundlerPool;
use crate::contracts::{IEntryPointDeposits, IEntryPointNonces, Safe4337Module};
use crate::packing::{add_gas_buffer, hex_bytes0x};
use crate::paymaster::{PaymasterPool, PaymasterService};
use crate::safe::{Safe4337Config, SafeDeterministicDeploymentConfig, ensure_safe_deployed};
use crate::signing::sign_userop_with_key;
use alloy::sol_types::SolCall;
use alloy::{
    primitives::{Address, Bytes, U256},
    providers::{DynProvider, Provider, ProviderBuilder},
    rpc::client::{BuiltInConnectionString, RpcClient},
};
use anyhow::{Context, Result};
use k256::ecdsa::SigningKey;

use alloy::rpc::types::eth::erc4337::PackedUserOperation;

const GAS_BUFFER_PCT: u64 = 10;
// Many bundlers assume a non-trivial priority fee for EIP-1559 style chains.
// Keep this conservative but non-zero to avoid "too low fee" rejections in local e2e.
const MIN_PRIORITY_FEE_WEI: u128 = 1_000_000_000; // 1 gwei

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymasterFinalizationMode {
    SkipIfStubFinal,
    AlwaysFetchFinal,
}

#[derive(Debug, Clone)]
pub struct Safe4337UserOpSenderOptions {
    pub check_bundler_entrypoints: bool,
    pub paymaster_finalization: PaymasterFinalizationMode,
}

impl Default for Safe4337UserOpSenderOptions {
    fn default() -> Self {
        Self {
            check_bundler_entrypoints: false,
            paymaster_finalization: PaymasterFinalizationMode::AlwaysFetchFinal,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Safe4337UserOpSenderConfig {
    pub rpc_url: String,
    pub chain_id: Option<u64>,
    pub entrypoint: Address,
    pub safe: Option<Address>,
    pub safe_4337_module: Address,
    pub safe_deployment: Option<SafeDeterministicDeploymentConfig>,
    pub bundler_urls: Vec<String>,
    pub owner_private_key: [u8; 32],
    pub paymasters: Vec<PaymasterService>,
    pub options: Safe4337UserOpSenderOptions,
}

pub struct Safe4337UserOpSender {
    cfg: Safe4337UserOpSenderConfig,
    provider: DynProvider,
    chain_id: u64,
    owner_key: SigningKey,
    safe: Address,
    bundlers: BundlerPool,
    paymasters: Option<PaymasterPool>,
    cached_nonce: Option<U256>,
}

#[derive(Debug, Clone)]
pub struct Safe4337UserOpSubmission {
    pub userop_hash: String,
    pub nonce: U256,
}

impl Safe4337UserOpSender {
    pub async fn new(cfg: Safe4337UserOpSenderConfig) -> Result<Self> {
        let transport = BuiltInConnectionString::connect(&cfg.rpc_url)
            .await
            .with_context(|| format!("connect rpc: {}", cfg.rpc_url))?;
        let client = RpcClient::builder().transport(transport, false);
        let provider = ProviderBuilder::default().connect_client(client);
        let provider = DynProvider::new(provider);

        let chain_id = match cfg.chain_id {
            Some(id) => id,
            None => provider.get_chain_id().await.context("eth_chainId")?,
        };

        let owner_key =
            SigningKey::from_slice(&cfg.owner_private_key).context("invalid owner private key")?;

        let safe = match cfg.safe {
            Some(addr) if addr != Address::ZERO => addr,
            _ => {
                let deploy = cfg
                    .safe_deployment
                    .clone()
                    .context("HUB_SAFE_ADDRESS is not set; HUB_SAFE_PROXY_FACTORY_ADDRESS/HUB_SAFE_SINGLETON_ADDRESS/HUB_SAFE_MODULE_SETUP_ADDRESS must be set")?;
                let safe_4337 = Safe4337Config {
                    entrypoint: cfg.entrypoint,
                    safe_4337_module: cfg.safe_4337_module,
                };
                ensure_safe_deployed(
                    &cfg.rpc_url,
                    chain_id,
                    cfg.owner_private_key,
                    &safe_4337,
                    &deploy,
                )
                .await
                .context("ensure safe deployed")?
            }
        };

        let mut bundlers = BundlerPool::new(cfg.bundler_urls.clone()).await?;
        if cfg.options.check_bundler_entrypoints {
            match bundlers.supported_entry_points().await {
                Ok(eps) => {
                    if !eps.contains(&cfg.entrypoint) {
                        tracing::warn!(
                            entrypoint = %cfg.entrypoint,
                            supported = ?eps,
                            "bundler does not advertise configured entrypoint"
                        );
                    }
                }
                Err(err) => {
                    tracing::warn!(err = %err, "failed to query eth_supportedEntryPoints");
                }
            }
        }

        let paymasters = if cfg.paymasters.is_empty() {
            None
        } else {
            Some(PaymasterPool::new(cfg.paymasters.clone())?)
        };

        let cfg = Safe4337UserOpSenderConfig {
            safe: Some(safe),
            ..cfg
        };

        Ok(Self {
            cfg,
            provider,
            chain_id,
            owner_key,
            safe,
            bundlers,
            paymasters,
            cached_nonce: None,
        })
    }

    pub async fn current_nonce(&self) -> Result<U256> {
        let chain_nonce = self
            .entrypoint()
            .getNonce(self.safe, alloy::primitives::Uint::<192, 3>::ZERO)
            .call()
            .await
            .context("EntryPoint.getNonce")?;
        match self.cached_nonce {
            Some(cached) if cached > chain_nonce => Ok(cached),
            _ => Ok(chain_nonce),
        }
    }

    pub async fn chain_nonce(&self) -> Result<U256> {
        self.entrypoint()
            .getNonce(self.safe, alloy::primitives::Uint::<192, 3>::ZERO)
            .call()
            .await
            .context("EntryPoint.getNonce")
    }

    /// Sets a local nonce floor, used to avoid AA25 when the bundler has accepted previous ops
    /// that are not yet reflected in `EntryPoint.getNonce` (e.g. after a solver restart).
    pub fn set_nonce_floor(&mut self, floor: U256) {
        self.cached_nonce = Some(match self.cached_nonce {
            Some(existing) => existing.max(floor),
            None => floor,
        });
    }

    pub fn safe_address(&self) -> Address {
        self.safe
    }

    pub async fn build_call_userop(
        &mut self,
        to: Address,
        data: Vec<u8>,
    ) -> Result<PackedUserOperation> {
        let base_userop = self
            .build_call_userop_unestimated(to, data)
            .await
            .context("build_call_userop_unestimated")?;
        self.prepare_self_paid(base_userop).await
    }

    /// Build a self-paid Safe4337 PackedUserOperation for `to(data)` without calling the bundler.
    ///
    /// This is primarily useful for debugging (e.g. reproducing simulation failures) where we want
    /// the exact signed payload before gas estimation.
    pub async fn build_call_userop_unestimated(
        &mut self,
        to: Address,
        data: Vec<u8>,
    ) -> Result<PackedUserOperation> {
        let nonce = self.current_nonce().await?;

        // Prefer standard EIP-1559 fee estimation (eth_feeHistory). This avoids bundler-specific gas APIs.
        let (max_fee_per_gas, max_priority_fee_per_gas) =
            match self.provider.estimate_eip1559_fees().await {
                Ok(est) => {
                    let mut max_fee = est.max_fee_per_gas;
                    let max_priority = est.max_priority_fee_per_gas.max(MIN_PRIORITY_FEE_WEI);
                    if max_fee < max_priority {
                        max_fee = max_priority;
                    }
                    (U256::from(max_fee), U256::from(max_priority))
                }
                Err(err) => {
                    // Fallback for non-EIP-1559 chains / RPCs: eth_gasPrice with a 2x buffer.
                    tracing::warn!(
                        err = %err,
                        "estimate_eip1559_fees failed; falling back to eth_gasPrice"
                    );
                    let gas_price: u128 = self
                        .provider
                        .get_gas_price()
                        .await
                        .context("eth_gasPrice")?;
                    let max_fee = gas_price.saturating_mul(2);
                    (
                        U256::from(max_fee),
                        U256::from(MIN_PRIORITY_FEE_WEI.min(max_fee)),
                    )
                }
            };

        // Prefer `executeUserOpWithErrorString` so bundler simulations return a useful revert reason
        // (instead of the generic `ExecutionFailed()`).
        let call_data = Safe4337Module::executeUserOpWithErrorStringCall {
            to,
            value: U256::ZERO,
            data: data.into(),
            operation: 0u8,
        }
        .abi_encode();

        let mut userop = PackedUserOperation {
            sender: self.safe,
            nonce,
            factory: None,
            factory_data: None,
            call_data: call_data.into(),
            call_gas_limit: U256::from(5_000_000u64),
            // Alto's simulation may fail with AA23 if the initial verification gas is too low to
            // run Safe signature checks. Use a generous placeholder; the bundler will return the
            // tight values via eth_estimateUserOperationGas.
            verification_gas_limit: U256::from(6_000_000u64),
            pre_verification_gas: U256::from(200_000u64),
            max_fee_per_gas,
            max_priority_fee_per_gas,
            paymaster: None,
            paymaster_verification_gas_limit: None,
            paymaster_post_op_gas_limit: None,
            paymaster_data: None,
            signature: Bytes::new(),
        };

        if self.paymasters.is_some() {
            tracing::warn!(
                safe = %self.safe,
                "paymasters configured but currently ignored (self-paid userops only)"
            );
        }

        if self.paymasters.is_none() {
            self.preflight_self_paid().await?;
        }

        userop.signature = self.sign_userop(&userop)?.into();
        Ok(userop)
    }

    pub async fn send_userop(
        &mut self,
        userop: &PackedUserOperation,
    ) -> Result<Safe4337UserOpSubmission> {
        let resp = match self
            .bundlers
            .send_user_operation(userop, self.cfg.entrypoint)
            .await
        {
            Ok(v) => v,
            Err(err) => {
                let msg = format!("{err:#}");
                if msg.contains("AA25 invalid account nonce") {
                    let chain_nonce = self
                        .entrypoint()
                        .getNonce(self.safe, alloy::primitives::Uint::<192, 3>::ZERO)
                        .call()
                        .await
                        .unwrap_or_default();

                    let before = self.cached_nonce;
                    // If we're behind: follow chain_nonce.
                    // If we're exactly on chain_nonce but bundler rejects: assume it already has a
                    // pending op with this nonce and bump by 1.
                    // If we're ahead of chain_nonce: we likely created a nonce gap (or the bundler
                    // doesn't accept future nonces). Reset down to chain_nonce so we can retry once
                    // earlier ops are included.
                    let next = if userop.nonce < chain_nonce {
                        chain_nonce
                    } else if userop.nonce == chain_nonce {
                        chain_nonce.saturating_add(U256::from(1u64))
                    } else {
                        chain_nonce
                    };
                    self.cached_nonce = Some(next);

                    tracing::warn!(
                        safe = %self.safe,
                        userop_nonce = %userop.nonce,
                        chain_nonce = %chain_nonce,
                        cached_nonce_before = ?before,
                        cached_nonce_after = ?self.cached_nonce,
                        "bundler rejected userop with AA25; adjusted local nonce floor"
                    );
                }
                return Err(err).context("bundler send userop");
            }
        };

        // Bundler accepted the op; advance our local nonce floor to allow queueing further ops
        // before the chain nonce advances.
        self.cached_nonce = Some(userop.nonce.saturating_add(U256::from(1u64)));

        Ok(Safe4337UserOpSubmission {
            userop_hash: hex_bytes0x(&resp.user_op_hash),
            nonce: userop.nonce,
        })
    }

    pub async fn send_call(
        &mut self,
        to: Address,
        data: Vec<u8>,
    ) -> Result<Safe4337UserOpSubmission> {
        let userop = self.build_call_userop(to, data).await?;
        self.send_userop(&userop).await
    }

    fn entrypoint(&self) -> IEntryPointNonces::IEntryPointNoncesInstance<&DynProvider> {
        IEntryPointNonces::new(self.cfg.entrypoint, &self.provider)
    }

    fn entrypoint_deposits(
        &self,
    ) -> IEntryPointDeposits::IEntryPointDepositsInstance<&DynProvider> {
        IEntryPointDeposits::new(self.cfg.entrypoint, &self.provider)
    }

    async fn preflight_self_paid(&self) -> Result<()> {
        let deposit = self
            .entrypoint_deposits()
            .balanceOf(self.safe)
            .call()
            .await
            .context("EntryPoint.balanceOf")?;
        let eth_balance = self
            .provider
            .get_balance(self.safe)
            .await
            .context("eth_getBalance(safe)")?;

        if deposit.is_zero() && eth_balance.is_zero() {
            anyhow::bail!(
                "safe has no EntryPoint deposit and no ETH balance (cannot self-pay userops): safe={:#x} entrypoint={:#x}; configure a paymaster (HUB_PAYMASTERS_JSON) or fund+deposit for self-paid userops",
                self.safe,
                self.cfg.entrypoint
            );
        }

        if deposit.is_zero() {
            tracing::warn!(
                safe = %self.safe,
                entrypoint = %self.cfg.entrypoint,
                eth_balance = %eth_balance,
                "safe has zero EntryPoint deposit; self-paid userops may fail unless deposit is funded"
            );
        }

        Ok(())
    }

    async fn prepare_self_paid(
        &mut self,
        mut userop: PackedUserOperation,
    ) -> Result<PackedUserOperation> {
        // If we're here, either no paymasters are configured or they all failed. Surface a clearer
        // error than "eth_estimateUserOperationGas" if the Safe is unfunded.
        if self.paymasters.is_none() {
            self.preflight_self_paid().await?;
        }

        userop.signature = self.sign_userop(&userop)?.into();

        match self
            .bundlers
            .estimate_user_operation_gas(&userop, self.cfg.entrypoint)
            .await
        {
            Ok(estimate) => {
                userop.call_gas_limit = add_gas_buffer(estimate.call_gas_limit, GAS_BUFFER_PCT)?;
                userop.verification_gas_limit =
                    add_gas_buffer(estimate.verification_gas_limit, GAS_BUFFER_PCT)?;
                userop.pre_verification_gas =
                    add_gas_buffer(estimate.pre_verification_gas, GAS_BUFFER_PCT)?;

                userop.signature = self.sign_userop(&userop)?.into();
                Ok(userop)
            }
            Err(err) => {
                // In practice, some bundlers/RPCs fail `eth_estimateUserOperationGas` even when
                // `eth_sendUserOperation` succeeds (or can succeed with conservative gas values).
                // Prefer being resilient here; callers can still choose to pre-estimate separately.
                tracing::warn!(
                    safe = %self.safe,
                    entrypoint = %self.cfg.entrypoint,
                    err = %format!("{err:#}"),
                    "bundler gas estimation failed; using conservative unestimated gas limits"
                );
                Ok(userop)
            }
        }
    }

    fn sign_userop(&self, userop: &PackedUserOperation) -> Result<Vec<u8>> {
        sign_userop_with_key(
            &self.owner_key,
            self.chain_id,
            self.cfg.safe_4337_module,
            self.cfg.entrypoint,
            userop,
        )
    }
}
