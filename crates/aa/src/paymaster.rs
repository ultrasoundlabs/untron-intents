use alloy::primitives::{Address, Bytes, U256};
use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct PaymasterService {
    pub url: String,
    pub context: Value,
}

/// ERC-7677 paymaster service `userOp` param shape for EntryPoint v0.7.
///
/// This matches the bundler JSON shape for `PackedUserOperation` (minus signature).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymasterUserOp {
    pub sender: Address,
    pub nonce: U256,
    pub call_data: Bytes,
    pub call_gas_limit: U256,
    pub verification_gas_limit: U256,
    pub pre_verification_gas: U256,
    pub max_fee_per_gas: U256,
    pub max_priority_fee_per_gas: U256,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub factory: Option<Address>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub factory_data: Option<Bytes>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paymaster: Option<Address>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paymaster_verification_gas_limit: Option<U256>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paymaster_post_op_gas_limit: Option<U256>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paymaster_data: Option<Bytes>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SponsorInfo {
    pub name: String,
    #[serde(default)]
    pub icon: Option<String>,
}

fn deserialize_sponsor<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<SponsorInfo>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v: Option<Value> = Option::deserialize(deserializer)?;
    let Some(v) = v else {
        return Ok(None);
    };

    match v {
        Value::String(name) => Ok(Some(SponsorInfo { name, icon: None })),
        Value::Object(_) => {
            let info: SponsorInfo = serde_json::from_value(v).map_err(serde::de::Error::custom)?;
            Ok(Some(info))
        }
        other => Err(serde::de::Error::custom(format!(
            "invalid sponsor type: {other}"
        ))),
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymasterStubDataResult {
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_sponsor")]
    pub sponsor: Option<SponsorInfo>,

    #[serde(default)]
    pub paymaster: Option<Address>,
    #[serde(default)]
    pub paymaster_data: Option<Bytes>,

    #[serde(default)]
    pub paymaster_verification_gas_limit: Option<U256>,
    #[serde(default)]
    pub paymaster_post_op_gas_limit: Option<U256>,

    #[serde(default)]
    pub is_final: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymasterDataResult {
    #[serde(default)]
    pub paymaster: Option<Address>,
    #[serde(default)]
    pub paymaster_data: Option<Bytes>,
}

#[derive(Debug, Clone)]
pub struct PaymasterPool {
    http: Client,
    selector: PaymasterSelector,
    next_id: u64,
}

#[derive(Debug, Clone)]
struct PaymasterSelector {
    services: Vec<PaymasterService>,
    next_idx: usize,
}

impl PaymasterSelector {
    fn new(services: Vec<PaymasterService>) -> Result<Self> {
        if services.is_empty() {
            anyhow::bail!("paymaster pool must be non-empty");
        }
        Ok(Self {
            services,
            next_idx: 0,
        })
    }

    fn order(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.services.len()).map(move |o| (self.next_idx + o) % self.services.len())
    }

    fn service(&self, idx: usize) -> Option<&PaymasterService> {
        self.services.get(idx)
    }

    fn mark_success(&mut self, idx: usize) {
        if !self.services.is_empty() {
            self.next_idx = (idx + 1) % self.services.len();
        }
    }
}

impl PaymasterPool {
    pub fn new(services: Vec<PaymasterService>) -> Result<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .context("build paymaster http client")?;
        Ok(Self {
            http,
            selector: PaymasterSelector::new(services)?,
            next_id: 1,
        })
    }

    pub fn order(&self) -> impl Iterator<Item = usize> + '_ {
        self.selector.order()
    }

    pub fn service(&self, idx: usize) -> Option<&PaymasterService> {
        self.selector.service(idx)
    }

    pub fn mark_success(&mut self, idx: usize) {
        self.selector.mark_success(idx);
    }

    pub async fn get_stub_data(
        &mut self,
        idx: usize,
        user_op: &PaymasterUserOp,
        entry_point: Address,
        chain_id: u64,
    ) -> Result<PaymasterStubDataResult> {
        let Some(svc) = self.selector.services.get(idx) else {
            anyhow::bail!("paymaster idx out of range");
        };
        let url = svc.url.clone();
        let context = svc.context.clone();
        self.jsonrpc(
            &url,
            "pm_getPaymasterStubData",
            serde_json::json!([user_op, entry_point, hex_chain_id(chain_id), context]),
        )
        .await
        .context("pm_getPaymasterStubData")
    }

    pub async fn get_data(
        &mut self,
        idx: usize,
        user_op: &PaymasterUserOp,
        entry_point: Address,
        chain_id: u64,
    ) -> Result<PaymasterDataResult> {
        let Some(svc) = self.selector.services.get(idx) else {
            anyhow::bail!("paymaster idx out of range");
        };
        let url = svc.url.clone();
        let context = svc.context.clone();
        self.jsonrpc(
            &url,
            "pm_getPaymasterData",
            serde_json::json!([user_op, entry_point, hex_chain_id(chain_id), context]),
        )
        .await
        .context("pm_getPaymasterData")
    }

    async fn jsonrpc<T: for<'de> Deserialize<'de>>(
        &mut self,
        url: &str,
        method: &str,
        params: serde_json::Value,
    ) -> Result<T> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);

        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        let resp = self
            .http
            .post(url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;

        let status = resp.status();
        let text = resp.text().await.context("read response body")?;
        if !status.is_success() {
            anyhow::bail!("http {status}: {text}");
        }

        #[derive(Deserialize)]
        struct JsonRpcEnvelope<T> {
            #[serde(default)]
            jsonrpc: Option<String>,
            #[serde(default)]
            id: Option<Value>,
            result: Option<T>,
            error: Option<JsonRpcError>,
        }
        #[derive(Deserialize)]
        struct JsonRpcError {
            code: i64,
            #[serde(default)]
            message: String,
            #[serde(default)]
            data: Option<serde_json::Value>,
        }

        let env: JsonRpcEnvelope<T> = serde_json::from_str(&text).context("decode jsonrpc")?;
        if let Some(v) = env.jsonrpc.as_deref()
            && v != "2.0"
        {
            anyhow::bail!("unexpected jsonrpc version: {v}");
        }
        if let Some(v) = &env.id
            && v != &Value::from(id)
        {
            anyhow::bail!("jsonrpc id mismatch (expected {id}, got {v})");
        }
        if let Some(err) = env.error {
            anyhow::bail!(
                "jsonrpc error {}: {} ({:?})",
                err.code,
                err.message,
                err.data
            );
        }
        env.result.context("missing jsonrpc result")
    }
}

fn hex_chain_id(chain_id: u64) -> String {
    format!("0x{chain_id:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paymaster_pool_order_rotates_on_success() {
        let services = vec![
            PaymasterService {
                url: "http://a".to_string(),
                context: serde_json::json!({}),
            },
            PaymasterService {
                url: "http://b".to_string(),
                context: serde_json::json!({}),
            },
            PaymasterService {
                url: "http://c".to_string(),
                context: serde_json::json!({}),
            },
        ];
        let mut sel = PaymasterSelector::new(services).unwrap();

        let order = sel.order().collect::<Vec<_>>();
        assert_eq!(order, vec![0, 1, 2]);

        sel.mark_success(1);
        let order = sel.order().collect::<Vec<_>>();
        assert_eq!(order, vec![2, 0, 1]);
    }
}
