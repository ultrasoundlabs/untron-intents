use super::*;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize)]
struct JsonRpcResponse<T> {
    pub result: Option<T>,
    pub error: Option<serde_json::Value>,
}

impl HubSafe4337Client {
    pub(super) async fn build_call_userop(
        &self,
        to: Address,
        data: Vec<u8>,
    ) -> Result<PackedUserOperation> {
        let mut sender = self.sender.lock().await;
        sender.build_call_userop(to, data).await
    }

    pub(super) async fn send_userop(
        &self,
        userop: PackedUserOperation,
    ) -> Result<aa::Safe4337UserOpSubmission> {
        let mut sender = self.sender.lock().await;
        sender.send_userop(&userop).await
    }

    pub(super) async fn get_userop_receipt(
        &self,
        userop_hash: &str,
    ) -> Result<Option<HubUserOpReceipt>> {
        let mut i = 0usize;
        // Try all bundlers once each; treat this as a "poll" (no internal waiting).
        for _ in 0..self.bundler_urls.len().max(1) {
            let url = self
                .bundler_urls
                .get(i % self.bundler_urls.len())
                .context("no HUB_BUNDLER_URLS configured")?
                .to_string();
            i = i.wrapping_add(1);

            if let Some(raw) = self.query_userop_receipt_raw(&url, userop_hash).await? {
                let (tx_hash, block_number, success, actual_gas_cost_wei, actual_gas_used, reason) =
                    extract_userop_receipt_fields(&raw)?;
                let (actual_gas_cost_wei, actual_gas_used, raw) =
                    if actual_gas_cost_wei.is_none() || actual_gas_used.is_none() {
                        if let Some(fallback) = self
                            .query_userop_receipt_from_entrypoint_log(userop_hash)
                            .await?
                        {
                            (
                                actual_gas_cost_wei.or(fallback.actual_gas_cost_wei),
                                actual_gas_used.or(fallback.actual_gas_used),
                                merge_userop_cost_fields(raw, &fallback.raw),
                            )
                        } else {
                            (actual_gas_cost_wei, actual_gas_used, raw)
                        }
                    } else {
                        (actual_gas_cost_wei, actual_gas_used, raw)
                    };
                return Ok(Some(HubUserOpReceipt {
                    tx_hash,
                    block_number,
                    success,
                    actual_gas_cost_wei,
                    actual_gas_used,
                    reason,
                    raw,
                }));
            }
        }
        // Bundlers may not retain receipts forever and some implementations can be flaky under
        // load. As a deterministic fallback, query the canonical chain via EntryPoint's
        // `UserOperationEvent` (indexed by userOpHash). This also lets the solver become
        // "eventually consistent" after temporary bundler outages.
        self.query_userop_receipt_from_entrypoint_log(userop_hash)
            .await
    }

    async fn query_userop_receipt_from_entrypoint_log(
        &self,
        userop_hash: &str,
    ) -> Result<Option<HubUserOpReceipt>> {
        let userop_hash: B256 = userop_hash.parse().context("parse userop_hash")?;

        // EntryPoint v0.7:
        // UserOperationEvent(bytes32 indexed userOpHash,address indexed sender,address indexed paymaster,uint256 nonce,bool success,uint256 actualGasCost,uint256 actualGasUsed)
        let topic0 = alloy::primitives::keccak256(
            "UserOperationEvent(bytes32,address,address,uint256,bool,uint256,uint256)".as_bytes(),
        );

        let head = self
            .provider
            .get_block_number()
            .await
            .context("eth_blockNumber")?;
        let head_u64: u64 = head;
        let from = head_u64.saturating_sub(10_000);

        let filter = Filter::new()
            .address(self.entrypoint)
            .event_signature(topic0)
            .topic1(userop_hash)
            .from_block(BlockNumberOrTag::Number(from))
            .to_block(BlockNumberOrTag::Number(head_u64));

        let logs = self
            .provider
            .get_logs(&filter)
            .await
            .context("eth_getLogs")?;
        let Some(log) = logs.into_iter().next() else {
            return Ok(None);
        };

        let tx_hash = log
            .transaction_hash
            .context("UserOperationEvent log missing transaction_hash")?;
        let block_number = log.block_number;

        // EntryPoint v0.7 ABI-encoded data: (nonce, success, actualGasCost, actualGasUsed).
        let data = log.data().data.as_ref();
        let nonce = abi_word_u256(data, 0);
        let success = abi_word_bool(data, 1);
        let actual_gas_cost_wei = abi_word_u256(data, 2);
        let actual_gas_used = abi_word_u256(data, 3);

        let raw = json!({
            "source": "entrypoint_log",
            "userOpHash": format!("{userop_hash:#x}"),
            "transactionHash": format!("{tx_hash:#x}"),
            "blockNumber": block_number.map(|n| format!("0x{:x}", n)),
            "nonce": nonce.map(|v| format!("{v:#x}")),
            "success": success,
            "actualGasCost": actual_gas_cost_wei.map(|v| format!("{v:#x}")),
            "actualGasUsed": actual_gas_used.map(|v| format!("{v:#x}")),
        });

        Ok(Some(HubUserOpReceipt {
            tx_hash: Some(tx_hash),
            block_number,
            success,
            actual_gas_cost_wei,
            actual_gas_used,
            reason: None,
            raw,
        }))
    }

    pub(super) async fn send_call_and_wait(
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
        loop {
            if start.elapsed() > std::time::Duration::from_secs(120) {
                anyhow::bail!("timeout waiting for userop receipt: {userop_hash}");
            }

            if let Some(r) = self.get_userop_receipt(userop_hash).await?
                && let Some(txh) = r.tx_hash
            {
                return Ok(txh);
            }

            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
    }

    async fn query_userop_receipt_raw(
        &self,
        bundler_url: &str,
        userop_hash: &str,
    ) -> Result<Option<serde_json::Value>> {
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

        let val: JsonRpcResponse<serde_json::Value> =
            resp.json().await.context("decode jsonrpc")?;
        if let Some(err) = val.error {
            tracing::warn!(bundler_url, err = %err, "bundler error");
            return Ok(None);
        }
        Ok(val.result)
    }
}

type UserOpReceiptFields = (
    Option<B256>,
    Option<u64>,
    Option<bool>,
    Option<U256>,
    Option<U256>,
    Option<serde_json::Value>,
);

fn extract_userop_receipt_fields(raw: &serde_json::Value) -> Result<UserOpReceiptFields> {
    // We keep this intentionally defensive: bundlers differ slightly in where fields appear.
    let tx_hash_str = raw
        .get("transactionHash")
        .and_then(|v| v.as_str())
        .or_else(|| {
            raw.get("receipt")
                .and_then(|r| r.get("transactionHash"))
                .and_then(|v| v.as_str())
        });
    let tx_hash = match tx_hash_str {
        Some(s) => Some(s.parse().context("parse transactionHash")?),
        None => None,
    };

    let block_number = raw
        .get("receipt")
        .and_then(|r| r.get("blockNumber"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.strip_prefix("0x"))
        .and_then(|s| u64::from_str_radix(s, 16).ok())
        .or_else(|| {
            raw.get("receipt")
                .and_then(|r| r.get("blockNumber"))
                .and_then(|v| v.as_u64())
        });

    let success = raw.get("success").and_then(|v| v.as_bool());
    let actual_gas_cost_wei = extract_u256_field(raw, &["actualGasCost", "actual_gas_cost"]);
    let actual_gas_used = extract_u256_field(raw, &["actualGasUsed", "actual_gas_used"]);
    let reason = raw.get("reason").cloned();

    Ok((
        tx_hash,
        block_number,
        success,
        actual_gas_cost_wei,
        actual_gas_used,
        reason,
    ))
}

fn extract_u256_field(raw: &serde_json::Value, keys: &[&str]) -> Option<U256> {
    for k in keys {
        if let Some(v) = raw.get(*k)
            && let Some(out) = parse_u256_json(v)
        {
            return Some(out);
        }
    }
    if let Some(r) = raw.get("receipt") {
        for k in keys {
            if let Some(v) = r.get(*k)
                && let Some(out) = parse_u256_json(v)
            {
                return Some(out);
            }
        }
    }
    None
}

fn parse_u256_json(v: &serde_json::Value) -> Option<U256> {
    if let Some(s) = v.as_str() {
        let s = s.trim();
        if let Some(hex) = s.strip_prefix("0x") {
            return U256::from_str_radix(hex, 16).ok();
        }
        return s.parse::<U256>().ok();
    }
    v.as_u64().map(U256::from)
}

fn abi_word_u256(data: &[u8], word_index: usize) -> Option<U256> {
    let start = word_index.checked_mul(32)?;
    let end = start.checked_add(32)?;
    if data.len() < end {
        return None;
    }
    Some(U256::from_be_slice(&data[start..end]))
}

fn abi_word_bool(data: &[u8], word_index: usize) -> Option<bool> {
    let start = word_index.checked_mul(32)?;
    let end = start.checked_add(32)?;
    if data.len() < end {
        return None;
    }
    Some(data[end - 1] == 1)
}

fn merge_userop_cost_fields(
    mut bundler_raw: serde_json::Value,
    entrypoint_raw: &serde_json::Value,
) -> serde_json::Value {
    // Best-effort: keep bundler-specific fields but prefer canonical cost numbers from EntryPoint.
    let Some(b) = bundler_raw.as_object_mut() else {
        return bundler_raw;
    };
    for k in ["actualGasCost", "actualGasUsed"] {
        if let Some(v) = entrypoint_raw.get(k) {
            b.insert(k.to_string(), v.clone());
        }
    }
    b.insert(
        "costSource".to_string(),
        serde_json::Value::String("entrypoint_log".to_string()),
    );
    bundler_raw
}
