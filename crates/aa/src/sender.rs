use crate::bundler_pool::BundlerPool;
use crate::contracts::{IEntryPointDeposits, IEntryPointNonces, Safe4337Module};
use crate::packing::{add_gas_buffer, hex_bytes0x, redact_url};
use crate::paymaster::{PaymasterPool, PaymasterService, PaymasterUserOp};
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
const PAYMASTER_POST_OP_GAS_BUFFER_PCT: u64 = 10;

// ERC-4337 bundler sanity checks expect paymasterVerificationGasLimit < MAX_VERIFICATION_GAS (500_000).
const MAX_PAYMASTER_VERIFICATION_GAS_LIMIT_EXCLUSIVE: u64 = 500_000;
const DEFAULT_PAYMASTER_VERIFICATION_GAS_LIMIT: u64 = 450_000;
const MIN_PRIORITY_FEE_WEI: u128 = 200_000;

fn cap_paymaster_verification_gas_limit(v: U256) -> (U256, bool) {
    let max = U256::from(MAX_PAYMASTER_VERIFICATION_GAS_LIMIT_EXCLUSIVE - 1);
    if v > max { (max, true) } else { (v, false) }
}

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
        })
    }

    pub async fn current_nonce(&self) -> Result<U256> {
        self.entrypoint()
            .getNonce(self.safe, alloy::primitives::Uint::<192, 3>::ZERO)
            .call()
            .await
            .context("EntryPoint.getNonce")
    }

    pub fn safe_address(&self) -> Address {
        self.safe
    }

    pub async fn build_call_userop(
        &mut self,
        to: Address,
        data: Vec<u8>,
    ) -> Result<PackedUserOperation> {
        let nonce = self.current_nonce().await?;

        // Prefer standard EIP-1559 fee estimation (eth_feeHistory). This avoids bundler-specific gas APIs.
        let (max_fee_per_gas, max_priority_fee_per_gas) = match self
            .provider
            .estimate_eip1559_fees()
            .await
        {
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

        let call_data = Safe4337Module::executeUserOpCall {
            to,
            value: U256::ZERO,
            data: data.into(),
            operation: 0u8,
        }
        .abi_encode();

        let base_userop = PackedUserOperation {
            sender: self.safe,
            nonce,
            factory: None,
            factory_data: None,
            call_data: call_data.into(),
            call_gas_limit: U256::from(5_000_000u64),
            verification_gas_limit: U256::from(1_500_000u64),
            pre_verification_gas: U256::from(150_000u64),
            max_fee_per_gas,
            max_priority_fee_per_gas,
            paymaster: None,
            paymaster_verification_gas_limit: None,
            paymaster_post_op_gas_limit: None,
            paymaster_data: None,
            signature: Bytes::new(),
        };

        if let Some(mut pool) = self.paymasters.take() {
            let attempts = pool
                .order()
                .filter_map(|idx| pool.service(idx).cloned().map(|svc| (idx, svc)))
                .collect::<Vec<_>>();

            for (idx, svc) in attempts {
                match self
                    .prepare_with_paymaster(&mut pool, idx, &svc, base_userop.clone())
                    .await
                {
                    Ok(userop) => {
                        pool.mark_success(idx);
                        self.paymasters = Some(pool);
                        return Ok(userop);
                    }
                    Err(err) => {
                        tracing::warn!(
                            paymaster = %redact_url(&svc.url),
                            err = %err,
                            "paymaster attempt failed; trying next"
                        );
                    }
                }
            }

            tracing::warn!("all configured paymasters failed; falling back to self-paid userop");
            self.paymasters = Some(pool);
        }

        if self.cfg.paymasters.is_empty() {
            tracing::warn!(
                safe = %self.safe,
                entrypoint = %self.cfg.entrypoint,
                "no paymasters configured; attempting self-paid userop"
            );
        }

        self.prepare_self_paid(base_userop).await
    }

    pub async fn send_userop(&mut self, userop: &PackedUserOperation) -> Result<Safe4337UserOpSubmission> {
        let resp = self
            .bundlers
            .send_user_operation(userop, self.cfg.entrypoint)
            .await
            .context("bundler send userop")?;

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

        let estimate = self
            .bundlers
            .estimate_user_operation_gas(&userop, self.cfg.entrypoint)
            .await
            .context("bundler estimate userop gas")?;

        userop.call_gas_limit = add_gas_buffer(estimate.call_gas_limit, GAS_BUFFER_PCT)?;
        userop.verification_gas_limit =
            add_gas_buffer(estimate.verification_gas_limit, GAS_BUFFER_PCT)?;
        userop.pre_verification_gas =
            add_gas_buffer(estimate.pre_verification_gas, GAS_BUFFER_PCT)?;

        userop.signature = self.sign_userop(&userop)?.into();

        Ok(userop)
    }

    async fn prepare_with_paymaster(
        &mut self,
        pool: &mut PaymasterPool,
        idx: usize,
        svc: &PaymasterService,
        mut userop: PackedUserOperation,
    ) -> Result<PackedUserOperation> {
        let pm_userop = to_paymaster_userop(&userop, self.cfg.options.paymaster_finalization)?;
        let stub = pool
            .get_stub_data(idx, &pm_userop, self.cfg.entrypoint, self.chain_id)
            .await?;

        if self.cfg.options.paymaster_finalization == PaymasterFinalizationMode::SkipIfStubFinal
            && let Some(s) = &stub.sponsor
        {
            tracing::info!(
                paymaster = %redact_url(&svc.url),
                sponsor = %s.name,
                icon = ?s.icon,
                "paymaster stub sponsor"
            );
        }

        let paymaster = stub
            .paymaster
            .context("pm_getPaymasterStubData missing paymaster")?;
        let paymaster_data = stub
            .paymaster_data
            .context("pm_getPaymasterStubData missing paymasterData")?;
        let stub_len = paymaster_data.len();

        userop.paymaster = Some(paymaster);
        userop.paymaster_data = Some(paymaster_data);
        let stub_pm_ver = stub.paymaster_verification_gas_limit;
        userop.paymaster_verification_gas_limit =
            Some(stub_pm_ver.unwrap_or(U256::from(DEFAULT_PAYMASTER_VERIFICATION_GAS_LIMIT)));

        let stub_pm_post = stub.paymaster_post_op_gas_limit;
        let pm_post = stub_pm_post.context(
            "pm_getPaymasterStubData missing paymasterPostOpGasLimit (required for v0.7)",
        )?;
        userop.paymaster_post_op_gas_limit =
            Some(add_gas_buffer(pm_post, PAYMASTER_POST_OP_GAS_BUFFER_PCT)?);

        userop.signature = self.sign_userop(&userop)?.into();
        let estimate = self
            .bundlers
            .estimate_user_operation_gas(&userop, self.cfg.entrypoint)
            .await
            .context("bundler estimate userop gas")?;

        userop.call_gas_limit = add_gas_buffer(estimate.call_gas_limit, GAS_BUFFER_PCT)?;
        userop.verification_gas_limit =
            add_gas_buffer(estimate.verification_gas_limit, GAS_BUFFER_PCT)?;
        userop.pre_verification_gas =
            add_gas_buffer(estimate.pre_verification_gas, GAS_BUFFER_PCT)?;

        let pm_ver_base = match userop.paymaster_verification_gas_limit {
            Some(v) if v > estimate.paymaster_verification_gas_limit => v,
            _ => estimate.paymaster_verification_gas_limit,
        };
        let (pm_ver, capped) =
            cap_paymaster_verification_gas_limit(add_gas_buffer(pm_ver_base, GAS_BUFFER_PCT)?);
        if capped {
            tracing::warn!(
                paymaster = %redact_url(&svc.url),
                pm_ver = %pm_ver,
                "capped paymasterVerificationGasLimit to bundler MAX_VERIFICATION_GAS-1"
            );
        }
        userop.paymaster_verification_gas_limit = Some(pm_ver);

        match self.cfg.options.paymaster_finalization {
            PaymasterFinalizationMode::SkipIfStubFinal => {
                if stub.is_final != Some(true) {
                    let pm_userop_final =
                        to_paymaster_userop(&userop, self.cfg.options.paymaster_finalization)?;
                    let final_data = pool
                        .get_data(idx, &pm_userop_final, self.cfg.entrypoint, self.chain_id)
                        .await?;

                    if let Some(p) = final_data.paymaster
                        && p != paymaster
                    {
                        anyhow::bail!("pm_getPaymasterData returned unexpected paymaster address");
                    }
                    let paymaster_data = final_data
                        .paymaster_data
                        .context("pm_getPaymasterData missing paymasterData")?;
                    if paymaster_data.len() != stub_len {
                        tracing::warn!(
                            paymaster = %redact_url(&svc.url),
                            stub_len,
                            final_len = paymaster_data.len(),
                            "paymasterData length differs between stub and final; bundler preVerificationGas may be off"
                        );
                    }
                    userop.paymaster_data = Some(paymaster_data);

                    userop.signature = self.sign_userop(&userop)?.into();
                    let estimate = self
                        .bundlers
                        .estimate_user_operation_gas(&userop, self.cfg.entrypoint)
                        .await
                        .context("bundler re-estimate userop gas after paymaster finalization")?;

                    userop.call_gas_limit =
                        add_gas_buffer(estimate.call_gas_limit, GAS_BUFFER_PCT)?;
                    userop.verification_gas_limit =
                        add_gas_buffer(estimate.verification_gas_limit, GAS_BUFFER_PCT)?;
                    userop.pre_verification_gas =
                        add_gas_buffer(estimate.pre_verification_gas, GAS_BUFFER_PCT)?;

                    let pm_ver_base = match userop.paymaster_verification_gas_limit {
                        Some(v) if v > estimate.paymaster_verification_gas_limit => v,
                        _ => estimate.paymaster_verification_gas_limit,
                    };
                    let (pm_ver, capped) = cap_paymaster_verification_gas_limit(add_gas_buffer(
                        pm_ver_base,
                        GAS_BUFFER_PCT,
                    )?);
                    if capped {
                        tracing::warn!(
                            paymaster = %redact_url(&svc.url),
                            pm_ver = %pm_ver,
                            "capped paymasterVerificationGasLimit to bundler MAX_VERIFICATION_GAS-1"
                        );
                    }
                    userop.paymaster_verification_gas_limit = Some(pm_ver);
                }
            }
            PaymasterFinalizationMode::AlwaysFetchFinal => {
                let pm_userop =
                    to_paymaster_userop(&userop, self.cfg.options.paymaster_finalization)?;
                let final_data = pool
                    .get_data(idx, &pm_userop, self.cfg.entrypoint, self.chain_id)
                    .await?;

                if let Some(p) = final_data.paymaster
                    && p != paymaster
                {
                    anyhow::bail!("pm_getPaymasterData returned unexpected paymaster address");
                }
                let paymaster_data = final_data
                    .paymaster_data
                    .context("pm_getPaymasterData missing paymasterData")?;
                let final_len = paymaster_data.len();
                if final_len != stub_len {
                    tracing::warn!(
                        paymaster = %redact_url(&svc.url),
                        stub_len,
                        final_len,
                        "paymasterData length differs between stub and final; bundler preVerificationGas may be off"
                    );
                }
                userop.paymaster = Some(paymaster);
                userop.paymaster_data = Some(paymaster_data);

                userop.signature = self.sign_userop(&userop)?.into();
                let estimate = self
                    .bundlers
                    .estimate_user_operation_gas(&userop, self.cfg.entrypoint)
                    .await
                    .context("bundler re-estimate userop gas after paymaster finalization")?;

                userop.call_gas_limit = add_gas_buffer(estimate.call_gas_limit, GAS_BUFFER_PCT)?;
                userop.verification_gas_limit =
                    add_gas_buffer(estimate.verification_gas_limit, GAS_BUFFER_PCT)?;
                userop.pre_verification_gas =
                    add_gas_buffer(estimate.pre_verification_gas, GAS_BUFFER_PCT)?;

                let pm_ver_base = match userop.paymaster_verification_gas_limit {
                    Some(v) if v > estimate.paymaster_verification_gas_limit => v,
                    _ => estimate.paymaster_verification_gas_limit,
                };
                let (pm_ver, capped) = cap_paymaster_verification_gas_limit(add_gas_buffer(
                    pm_ver_base,
                    GAS_BUFFER_PCT,
                )?);
                if capped {
                    tracing::warn!(
                        paymaster = %redact_url(&svc.url),
                        pm_ver = %pm_ver,
                        "capped paymasterVerificationGasLimit to bundler MAX_VERIFICATION_GAS-1"
                    );
                }
                userop.paymaster_verification_gas_limit = Some(pm_ver);
            }
        }

        userop.signature = self.sign_userop(&userop)?.into();

        if self.cfg.options.paymaster_finalization == PaymasterFinalizationMode::AlwaysFetchFinal {
            tracing::info!(paymaster = %redact_url(&svc.url), "userop prepared (sponsored)");
        }

        Ok(userop)
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

fn to_paymaster_userop(
    op: &PackedUserOperation,
    mode: PaymasterFinalizationMode,
) -> Result<PaymasterUserOp> {
    Ok(PaymasterUserOp {
        sender: op.sender,
        nonce: op.nonce,
        call_data: op.call_data.clone(),
        call_gas_limit: op.call_gas_limit,
        verification_gas_limit: op.verification_gas_limit,
        pre_verification_gas: op.pre_verification_gas,
        max_fee_per_gas: op.max_fee_per_gas,
        max_priority_fee_per_gas: op.max_priority_fee_per_gas,
        factory: op.factory,
        factory_data: op.factory_data.clone(),
        paymaster: op.paymaster,
        paymaster_verification_gas_limit: op.paymaster_verification_gas_limit,
        paymaster_post_op_gas_limit: op.paymaster_post_op_gas_limit,
        paymaster_data: match mode {
            PaymasterFinalizationMode::SkipIfStubFinal => None,
            PaymasterFinalizationMode::AlwaysFetchFinal => op.paymaster_data.clone(),
        },
    })
}
