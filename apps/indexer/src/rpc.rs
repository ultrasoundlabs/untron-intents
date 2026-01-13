use anyhow::{Context, Result};
use reqwest::StatusCode;
use serde_json::Value;
use std::sync::{
    Arc,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};
use std::time::Duration;

#[derive(Clone)]
pub struct RpcClient {
    urls: Arc<Vec<String>>,
    http: reqwest::Client,
    next_id: Arc<AtomicU64>,
    preferred_url: Arc<AtomicUsize>,
}

impl RpcClient {
    pub fn new(urls: Vec<String>) -> Result<Self> {
        if urls.is_empty() {
            anyhow::bail!("rpc urls must not be empty");
        }
        let http = reqwest::ClientBuilder::new()
            .timeout(Duration::from_secs(30))
            .build()
            .context("build reqwest client")?;

        Ok(Self {
            urls: Arc::new(urls),
            http,
            next_id: Arc::new(AtomicU64::new(1)),
            preferred_url: Arc::new(AtomicUsize::new(0)),
        })
    }

    pub async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        // Stick to a single "preferred" RPC endpoint for consistency (avoids mixing
        // slightly-different views across endpoints), but still fall back to others.
        let start = self
            .preferred_url
            .load(Ordering::Relaxed)
            .wrapping_rem(self.urls.len());

        let mut last_err: Option<anyhow::Error> = None;
        for offset in 0..self.urls.len() {
            let idx = (start + offset) % self.urls.len();
            let url = &self.urls[idx];
            match self
                .http
                .post(url)
                .json(&body)
                .send()
                .await
                .with_context(|| format!("{method} POST {url}"))
            {
                Ok(resp) => {
                    let status = resp.status();
                    let text = resp
                        .text()
                        .await
                        .with_context(|| format!("{method} read body {url}"))?;
                    if status != StatusCode::OK {
                        last_err = Some(anyhow::anyhow!(
                            "{method} http status={} url={} body={}",
                            status.as_u16(),
                            url,
                            text
                        ));
                        continue;
                    }
                    let v: Value = serde_json::from_str(&text)
                        .with_context(|| format!("{method} parse json"))?;
                    if let Some(err) = v.get("error") {
                        last_err = Some(anyhow::anyhow!("{method} rpc error: {err}"));
                        continue;
                    }
                    let Some(result) = v.get("result") else {
                        last_err = Some(anyhow::anyhow!("{method} missing result field"));
                        continue;
                    };
                    self.preferred_url.store(idx, Ordering::Relaxed);
                    return Ok(result.clone());
                }
                Err(e) => {
                    last_err = Some(e);
                    continue;
                }
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("{method} failed")))
    }

    pub async fn block_number(&self) -> Result<u64> {
        let v = self
            .request("eth_blockNumber", serde_json::json!([]))
            .await?;
        parse_quantity_u64(v).context("parse eth_blockNumber")
    }

    pub async fn get_logs(&self, filter: Value) -> Result<Vec<alloy::rpc::types::Log>> {
        let v = self
            .request("eth_getLogs", serde_json::json!([filter]))
            .await?;
        serde_json::from_value(v).context("parse eth_getLogs result as logs")
    }

    pub async fn get_block_by_number(&self, block_number: u64) -> Result<Option<Value>> {
        let v = self
            .request(
                "eth_getBlockByNumber",
                serde_json::json!([format_quantity(block_number), false]),
            )
            .await?;
        if v.is_null() {
            return Ok(None);
        }
        Ok(Some(v))
    }
}

pub fn format_quantity(value: u64) -> String {
    format!("0x{value:x}")
}

pub fn parse_quantity_u64(v: Value) -> Result<u64> {
    match v {
        Value::String(s) => parse_quantity_u64_str(&s),
        Value::Number(n) => n
            .as_u64()
            .context("quantity number not representable as u64"),
        other => anyhow::bail!("unexpected quantity json type: {other}"),
    }
}

fn parse_quantity_u64_str(s: &str) -> Result<u64> {
    let trimmed = s.trim();
    let Some(hex) = trimmed.strip_prefix("0x") else {
        return trimmed
            .parse::<u64>()
            .with_context(|| format!("invalid decimal u64: {trimmed}"));
    };
    if hex.is_empty() {
        anyhow::bail!("invalid hex quantity: {trimmed}");
    }
    u64::from_str_radix(hex, 16).with_context(|| format!("invalid hex quantity: {trimmed}"))
}

pub fn looks_like_range_too_large(err: &anyhow::Error) -> bool {
    let msg = err.to_string().to_lowercase();
    msg.contains("range too large")
        || msg.contains("block range")
        || msg.contains("too many results")
        || msg.contains("response size exceeded")
        || msg.contains("payload too large")
}

pub fn looks_like_transient(err: &anyhow::Error) -> bool {
    let msg = err.to_string().to_lowercase();
    msg.contains("timeout")
        || msg.contains("timed out")
        || msg.contains("deadline")
        || msg.contains("too many requests")
        || msg.contains("rate limit")
        || msg.contains("429")
        || msg.contains("bad gateway")
        || msg.contains("gateway")
        || msg.contains("service unavailable")
        || msg.contains("503")
        || msg.contains("502")
        || msg.contains("504")
        || msg.contains("connection reset")
        || msg.contains("connection closed")
        || msg.contains("connection refused")
        || msg.contains("broken pipe")
        || msg.contains("temporarily unavailable")
}
