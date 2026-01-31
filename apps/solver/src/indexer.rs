use crate::metrics::SolverTelemetry;
use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use std::time::Instant;

#[derive(Debug, Clone, Deserialize)]
pub struct PoolOpenIntentRow {
    pub id: String,
    pub intent_type: i16,
    pub intent_specs: String,
    #[serde(default)]
    pub solver: Option<String>,
    pub deadline: i64,
    pub solved: bool,
    pub funded: bool,
    pub closed: bool,
}

pub struct IndexerClient {
    base_url: String,
    http: Client,
    telemetry: SolverTelemetry,
}

impl IndexerClient {
    pub fn new(base_url: String, timeout: std::time::Duration, telemetry: SolverTelemetry) -> Self {
        let http = Client::builder().timeout(timeout).build().expect("reqwest");
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http,
            telemetry,
        }
    }

    pub async fn health(&self) -> Result<()> {
        let url = format!("{}/health", self.base_url);
        let started = Instant::now();
        let resp = self.http.get(&url).send().await;
        let ok = resp
            .as_ref()
            .map(|r| r.status().is_success())
            .unwrap_or(false);
        self.telemetry
            .indexer_http_ms("health", ok, started.elapsed().as_millis() as u64);
        let resp = resp.context("GET /health")?;
        if !resp.status().is_success() {
            anyhow::bail!("indexer /health failed: {}", resp.status());
        }
        Ok(())
    }

    pub async fn fetch_open_intents(&self, limit: u64) -> Result<Vec<PoolOpenIntentRow>> {
        let url = format!(
            "{}/pool_open_intents?order=valid_from_seq.asc&limit={}",
            self.base_url, limit
        );
        let started = Instant::now();
        let resp = self.http.get(&url).send().await;
        let ok = resp
            .as_ref()
            .map(|r| r.status().is_success())
            .unwrap_or(false);
        self.telemetry.indexer_http_ms(
            "pool_open_intents",
            ok,
            started.elapsed().as_millis() as u64,
        );
        let resp = resp.context("GET /pool_open_intents")?;
        if !resp.status().is_success() {
            anyhow::bail!("indexer /pool_open_intents failed: {}", resp.status());
        }
        let rows: Vec<PoolOpenIntentRow> = resp.json().await.context("decode open intents")?;
        Ok(rows)
    }
}
