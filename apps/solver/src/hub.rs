use crate::metrics::SolverTelemetry;
use aa::{Safe4337UserOpSender, Safe4337UserOpSenderConfig, Safe4337UserOpSenderOptions};
use alloy::primitives::{Address, B256, U256};
use alloy::providers::{DynProvider, Provider, ProviderBuilder};
use alloy::rpc::types::TransactionReceipt;
use alloy::signers::Signer;
use alloy::signers::local::PrivateKeySigner;
use alloy::sol_types::SolCall;
use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use std::time::Instant;
use url::Url;

alloy::sol! {
    #[sol(rpc)]
    interface IUntronIntents {
        function USDT() external view returns (address);
        function claimIntent(bytes32 id) external;
        function proveIntentFill(bytes32 id, bytes[20] calldata blocks, bytes calldata encodedTx, bytes32[] calldata proof, uint256 index) external;
    }

    #[sol(rpc)]
    interface IERC20 {
        function allowance(address owner, address spender) external view returns (uint256);
        function approve(address spender, uint256 amount) external returns (bool);
    }

    struct TransferContract {
        bytes32 txId;
        uint256 tronBlockNumber;
        uint32 tronBlockTimestamp;
        bytes21 senderTron;
        bytes21 toTron;
        uint256 amountSun;
    }

    struct DelegateResourceContract {
        bytes32 txId;
        uint256 tronBlockNumber;
        uint256 balanceSun;
        uint256 lockPeriod;
        bytes21 ownerTron;
        bytes21 receiverTron;
        uint32 tronBlockTimestamp;
        uint8 resource;
        bool lock;
    }

    #[sol(rpc)]
    interface IMockTronTxReader {
        function setTransferTx(TransferContract calldata tx_) external;
        function setDelegateResourceTx(DelegateResourceContract calldata tx_) external;
    }
}

pub struct HubClient {
    inner: HubClientInner,
}

enum HubClientInner {
    Eoa(HubEoaClient),
    Safe4337(HubSafe4337Client),
}

struct HubEoaClient {
    pool: Address,
    provider: DynProvider,
    eoa: Address,
    telemetry: SolverTelemetry,
}

struct HubSafe4337Client {
    pool: Address,
    provider: DynProvider,
    solver: Address,
    bundler_urls: Vec<String>,
    sender: tokio::sync::Mutex<Safe4337UserOpSender>,
    http: Client,
    telemetry: SolverTelemetry,
}

#[derive(Debug, Clone)]
pub struct TronProof {
    pub blocks: [Vec<u8>; 20],
    pub encoded_tx: Vec<u8>,
    pub proof: Vec<B256>,
    pub index: U256,
}

impl HubClient {
    pub async fn new_eoa(
        rpc_url: &str,
        chain_id: Option<u64>,
        pool: Address,
        signer_private_key: [u8; 32],
        telemetry: SolverTelemetry,
    ) -> Result<Self> {
        let url: Url = rpc_url.parse().context("parse HUB_RPC_URL")?;
        let base_provider = ProviderBuilder::new().connect_http(url.clone());
        let base_provider = DynProvider::new(base_provider);

        // Discover chain id (required for EIP-155 signatures) and optionally validate it.
        let started = Instant::now();
        let discovered = base_provider.get_chain_id().await.context("eth_chainId")?;
        telemetry.hub_rpc_ms("eth_chainId", true, started.elapsed().as_millis() as u64);
        let chain_id = match chain_id {
            Some(expected) => {
                if discovered != expected {
                    anyhow::bail!("HUB_CHAIN_ID mismatch: configured={expected} rpc={discovered}");
                }
                expected
            }
            None => discovered,
        };

        let signer = PrivateKeySigner::from_slice(&signer_private_key)
            .context("invalid HUB_SIGNER_PRIVATE_KEY_HEX")?
            .with_chain_id(Some(chain_id));
        let eoa = signer.address();
        let wallet = alloy::network::EthereumWallet::from(signer);

        let provider = ProviderBuilder::new().wallet(wallet).connect_http(url);
        let provider = DynProvider::new(provider);

        Ok(Self {
            inner: HubClientInner::Eoa(HubEoaClient {
                pool,
                provider,
                eoa,
                telemetry,
            }),
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn new_safe4337(
        rpc_url: &str,
        chain_id: Option<u64>,
        pool: Address,
        entrypoint: Address,
        safe: Option<Address>,
        safe_4337_module: Address,
        safe_deployment: Option<aa::SafeDeterministicDeploymentConfig>,
        bundler_urls: Vec<String>,
        paymasters: Vec<aa::paymaster::PaymasterService>,
        signer_private_key: [u8; 32],
        telemetry: SolverTelemetry,
    ) -> Result<Self> {
        let url: Url = rpc_url.parse().context("parse HUB_RPC_URL")?;
        let provider = ProviderBuilder::new().connect_http(url.clone());
        let provider = DynProvider::new(provider);

        let started = Instant::now();
        let discovered = provider.get_chain_id().await.context("eth_chainId")?;
        telemetry.hub_rpc_ms("eth_chainId", true, started.elapsed().as_millis() as u64);
        let chain_id = match chain_id {
            Some(expected) => {
                if discovered != expected {
                    anyhow::bail!("HUB_CHAIN_ID mismatch: configured={expected} rpc={discovered}");
                }
                Some(expected)
            }
            None => Some(discovered),
        };

        let sender = Safe4337UserOpSender::new(Safe4337UserOpSenderConfig {
            rpc_url: rpc_url.to_string(),
            chain_id,
            entrypoint,
            safe,
            safe_4337_module,
            safe_deployment,
            bundler_urls: bundler_urls.clone(),
            owner_private_key: signer_private_key,
            paymasters,
            options: Safe4337UserOpSenderOptions::default(),
        })
        .await
        .context("init Safe4337UserOpSender")?;

        Ok(Self {
            inner: HubClientInner::Safe4337(HubSafe4337Client {
                pool,
                provider,
                solver: sender.safe_address(),
                bundler_urls,
                sender: tokio::sync::Mutex::new(sender),
                http: Client::new(),
                telemetry,
            }),
        })
    }

    pub fn pool_address(&self) -> Address {
        match &self.inner {
            HubClientInner::Eoa(c) => c.pool,
            HubClientInner::Safe4337(c) => c.pool,
        }
    }

    pub fn solver_address(&self) -> Address {
        match &self.inner {
            HubClientInner::Eoa(c) => c.eoa,
            HubClientInner::Safe4337(c) => c.solver,
        }
    }

    pub async fn pool_usdt(&self) -> Result<Address> {
        let (pool_addr, provider, telemetry) = match &self.inner {
            HubClientInner::Eoa(c) => (c.pool, c.provider.clone(), c.telemetry.clone()),
            HubClientInner::Safe4337(c) => (c.pool, c.provider.clone(), c.telemetry.clone()),
        };
        let pool = IUntronIntents::new(pool_addr, provider);
        let started = Instant::now();
        let res = pool.USDT().call().await;
        let ok = res.is_ok();
        telemetry.hub_rpc_ms("pool_usdt", ok, started.elapsed().as_millis() as u64);
        Ok(res.context("UntronIntents.USDT")?)
    }

    pub async fn ensure_erc20_allowance(
        &self,
        token: Address,
        spender: Address,
        min_allowance: U256,
    ) -> Result<()> {
        let (owner, provider, telemetry) = match &self.inner {
            HubClientInner::Eoa(c) => (c.eoa, c.provider.clone(), c.telemetry.clone()),
            HubClientInner::Safe4337(c) => (c.solver, c.provider.clone(), c.telemetry.clone()),
        };
        let erc20 = IERC20::new(token, provider);
        let started = Instant::now();
        let allowance = erc20
            .allowance(owner, spender)
            .call()
            .await
            .context("ERC20.allowance")?;
        telemetry.hub_rpc_ms(
            "erc20_allowance",
            true,
            started.elapsed().as_millis() as u64,
        );
        if allowance >= min_allowance {
            return Ok(());
        }

        match &self.inner {
            HubClientInner::Eoa(c) => {
                let started = Instant::now();
                let pending = erc20.approve(spender, U256::MAX).send().await;
                let ok = pending.is_ok();
                c.telemetry
                    .hub_rpc_ms("erc20_approve", ok, started.elapsed().as_millis() as u64);
                let receipt = pending.context("ERC20.approve send")?.get_receipt().await?;
                tracing::info!(tx = %receipt.transaction_hash, "approved erc20 allowance");
                Ok(())
            }
            HubClientInner::Safe4337(c) => {
                let call = IERC20::approveCall {
                    spender,
                    amount: U256::MAX,
                };
                c.send_call_and_wait(token, call.abi_encode(), "erc20_approve")
                    .await?;
                Ok(())
            }
        }
    }

    pub async fn claim_intent(&self, id: B256) -> Result<TransactionReceipt> {
        match &self.inner {
            HubClientInner::Eoa(c) => {
                let pool = IUntronIntents::new(c.pool, c.provider.clone());
                let started = Instant::now();
                let pending = pool.claimIntent(id).send().await;
                let ok = pending.is_ok();
                c.telemetry
                    .hub_rpc_ms("claim_intent", ok, started.elapsed().as_millis() as u64);
                Ok(pending?.get_receipt().await?)
            }
            HubClientInner::Safe4337(c) => {
                let call = IUntronIntents::claimIntentCall { id };
                c.send_call_and_wait(c.pool, call.abi_encode(), "claim_intent")
                    .await
            }
        }
    }

    pub async fn prove_intent_fill(&self, id: B256, tron: TronProof) -> Result<TransactionReceipt> {
        let blocks: [alloy::primitives::Bytes; 20] =
            tron.blocks.map(alloy::primitives::Bytes::from);
        let encoded_tx = alloy::primitives::Bytes::from(tron.encoded_tx);
        let proof: Vec<B256> = tron.proof;

        match &self.inner {
            HubClientInner::Eoa(c) => {
                let pool = IUntronIntents::new(c.pool, c.provider.clone());
                let started = Instant::now();
                let pending = pool
                    .proveIntentFill(id, blocks, encoded_tx, proof, tron.index)
                    .send()
                    .await;
                let ok = pending.is_ok();
                c.telemetry.hub_rpc_ms(
                    "prove_intent_fill",
                    ok,
                    started.elapsed().as_millis() as u64,
                );
                Ok(pending?.get_receipt().await?)
            }
            HubClientInner::Safe4337(c) => {
                let call = IUntronIntents::proveIntentFillCall {
                    id,
                    blocks,
                    encodedTx: encoded_tx,
                    proof,
                    index: tron.index,
                };
                c.send_call_and_wait(c.pool, call.abi_encode(), "prove_intent_fill")
                    .await
            }
        }
    }

    pub async fn mock_set_transfer_tx(
        &self,
        reader: Address,
        tx: TransferContract,
    ) -> Result<TransactionReceipt> {
        match &self.inner {
            HubClientInner::Eoa(c) => {
                let r = IMockTronTxReader::new(reader, c.provider.clone());
                let started = Instant::now();
                let pending = r.setTransferTx(tx).send().await;
                let ok = pending.is_ok();
                c.telemetry.hub_rpc_ms(
                    "mock_set_transfer_tx",
                    ok,
                    started.elapsed().as_millis() as u64,
                );
                Ok(pending?.get_receipt().await?)
            }
            HubClientInner::Safe4337(c) => {
                let call = IMockTronTxReader::setTransferTxCall { tx_: tx };
                c.send_call_and_wait(reader, call.abi_encode(), "mock_set_transfer_tx")
                    .await
            }
        }
    }

    pub async fn mock_set_delegate_resource_tx(
        &self,
        reader: Address,
        tx: DelegateResourceContract,
    ) -> Result<TransactionReceipt> {
        match &self.inner {
            HubClientInner::Eoa(c) => {
                let r = IMockTronTxReader::new(reader, c.provider.clone());
                let started = Instant::now();
                let pending = r.setDelegateResourceTx(tx).send().await;
                let ok = pending.is_ok();
                c.telemetry.hub_rpc_ms(
                    "mock_set_delegate_resource_tx",
                    ok,
                    started.elapsed().as_millis() as u64,
                );
                Ok(pending?.get_receipt().await?)
            }
            HubClientInner::Safe4337(c) => {
                let call = IMockTronTxReader::setDelegateResourceTxCall { tx_: tx };
                c.send_call_and_wait(reader, call.abi_encode(), "mock_set_delegate_resource_tx")
                    .await
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse<T> {
    pub result: Option<T>,
    pub error: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct UserOpReceipt {
    #[serde(rename = "transactionHash")]
    pub transaction_hash: Option<String>,
}

impl HubSafe4337Client {
    async fn send_call_and_wait(
        &self,
        to: Address,
        data: Vec<u8>,
        op: &'static str,
    ) -> Result<TransactionReceipt> {
        let started = Instant::now();
        let submission = {
            let mut sender = self.sender.lock().await;
            sender.send_call(to, data).await?
        };
        self.telemetry
            .hub_rpc_ms(op, true, started.elapsed().as_millis() as u64);

        let tx_hash = self.wait_userop_tx_hash(&submission.userop_hash).await?;

        let start = Instant::now();
        loop {
            if start.elapsed() > std::time::Duration::from_secs(120) {
                anyhow::bail!("timeout waiting for tx receipt: {tx_hash:#x}");
            }
            match self
                .provider
                .get_transaction_receipt(tx_hash)
                .await
                .context("eth_getTransactionReceipt")?
            {
                Some(r) => return Ok(r),
                None => tokio::time::sleep(std::time::Duration::from_millis(200)).await,
            }
        }
    }

    async fn wait_userop_tx_hash(&self, userop_hash: &str) -> Result<B256> {
        let start = Instant::now();
        let mut i = 0usize;
        loop {
            if start.elapsed() > std::time::Duration::from_secs(120) {
                anyhow::bail!("timeout waiting for userop receipt: {userop_hash}");
            }
            let url = self
                .bundler_urls
                .get(i % self.bundler_urls.len())
                .context("no HUB_BUNDLER_URLS configured")?
                .to_string();
            i = i.wrapping_add(1);

            if let Some(txh) = self.query_userop_receipt(&url, userop_hash).await? {
                return Ok(txh);
            }

            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
    }

    async fn query_userop_receipt(
        &self,
        bundler_url: &str,
        userop_hash: &str,
    ) -> Result<Option<B256>> {
        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_getUserOperationReceipt",
            "params": [userop_hash],
        });
        let resp = self
            .http
            .post(bundler_url)
            .json(&payload)
            .send()
            .await
            .context("post bundler jsonrpc")?;

        let val: JsonRpcResponse<UserOpReceipt> = resp.json().await.context("decode jsonrpc")?;
        if let Some(err) = val.error {
            tracing::warn!(bundler_url, err = %err, "bundler error");
            return Ok(None);
        }
        let Some(result) = val.result else {
            return Ok(None);
        };
        let Some(txh) = result.transaction_hash else {
            return Ok(None);
        };
        Ok(Some(txh.parse().context("parse transactionHash")?))
    }
}
