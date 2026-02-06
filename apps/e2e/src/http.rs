use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::Value;
use std::time::{Duration, Instant};

pub async fn http_get_json(url: &str) -> Result<Value> {
    let c = Client::new();
    let resp = c.get(url).send().await.context("http get")?;
    let status = resp.status();
    let body = resp.text().await.context("read body")?;
    if !status.is_success() {
        anyhow::bail!("non-2xx response: {status} body={body}");
    }
    serde_json::from_str(&body).context("parse json")
}

pub async fn wait_for_http_ok(url: &str, timeout: Duration) -> Result<()> {
    let start = Instant::now();
    loop {
        match Client::new().get(url).send().await {
            Ok(resp) if resp.status().is_success() => return Ok(()),
            _ => {
                if start.elapsed() > timeout {
                    anyhow::bail!("timed out waiting for http ok: {url}");
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }
}
