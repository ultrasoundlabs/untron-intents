use crate::metrics::SolverTelemetry;
use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use std::time::Instant;

fn de_string_or_number<'de, D>(d: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(d)?;
    match v {
        serde_json::Value::String(s) => Ok(s),
        serde_json::Value::Number(n) => Ok(n.to_string()),
        other => Err(serde::de::Error::custom(format!(
            "expected string/number, got {other:?}"
        ))),
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PoolOpenIntentRow {
    pub id: String,
    pub intent_type: i16,
    pub intent_specs: String,
    pub escrow_token: String,
    #[serde(deserialize_with = "de_string_or_number")]
    pub escrow_amount: String,
    #[serde(default)]
    pub solver: Option<String>,
    pub deadline: i64,
    pub solved: bool,
    pub funded: bool,
    #[serde(default)]
    pub settled: bool,
    pub closed: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct EventBlockRow {
    pub block_number: u64,
}

#[derive(Clone)]
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

    pub async fn fetch_intent(&self, id: &str) -> Result<Option<PoolOpenIntentRow>> {
        let url = format!("{}/pool_intents?id=eq.{}&limit=1", self.base_url, id);
        let started = Instant::now();
        let resp = self.http.get(&url).send().await;
        let ok = resp
            .as_ref()
            .map(|r| r.status().is_success())
            .unwrap_or(false);
        self.telemetry
            .indexer_http_ms("pool_intents_by_id", ok, started.elapsed().as_millis() as u64);
        let resp = resp.context("GET /pool_intents (by id)")?;
        if !resp.status().is_success() {
            anyhow::bail!("indexer /pool_intents failed: {}", resp.status());
        }
        let rows: Vec<PoolOpenIntentRow> = resp.json().await.context("decode pool_intents")?;
        Ok(rows.into_iter().next())
    }

    pub async fn latest_indexed_pool_block_number(&self) -> Result<Option<u64>> {
        // Derive lag from the highest indexed pool event block. This is robust across schema changes
        // because it's sourced from the canonical `api.event_appended` view.
        let url = format!(
            "{}/event_appended?stream=eq.pool&order=block_number.desc&limit=1&select=block_number",
            self.base_url
        );
        let started = Instant::now();
        let resp = self.http.get(&url).send().await;
        let ok = resp
            .as_ref()
            .map(|r| r.status().is_success())
            .unwrap_or(false);
        self.telemetry.indexer_http_ms(
            "event_appended_latest_pool_block",
            ok,
            started.elapsed().as_millis() as u64,
        );
        let resp = resp.context("GET /event_appended (latest pool block)")?;
        if !resp.status().is_success() {
            anyhow::bail!("indexer /event_appended failed: {}", resp.status());
        }
        let rows: Vec<EventBlockRow> = resp.json().await.context("decode event_appended")?;
        Ok(rows.first().map(|r| r.block_number))
    }
}
