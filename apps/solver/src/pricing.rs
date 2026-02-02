use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct PricingConfig {
    pub trx_usd_override: Option<f64>,
    pub trx_usd_ttl: Duration,
    pub trx_usd_url: String,
    pub eth_usd_override: Option<f64>,
    pub eth_usd_ttl: Duration,
    pub eth_usd_url: String,
}

#[derive(Debug, Clone)]
pub struct Pricing {
    http: Client,
    cfg: PricingConfig,
    cached_trx: Option<(f64, Instant)>,
    cached_eth: Option<(f64, Instant)>,
}

#[derive(Debug, Deserialize)]
struct CoingeckoSimplePrice {
    tron: CoingeckoTron,
    ethereum: CoingeckoEthereum,
}

#[derive(Debug, Deserialize)]
struct CoingeckoTron {
    usd: f64,
}

#[derive(Debug, Deserialize)]
struct CoingeckoEthereum {
    usd: f64,
}

impl Pricing {
    pub fn new(cfg: PricingConfig) -> Self {
        Self {
            http: Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .expect("reqwest"),
            cfg,
            cached_trx: None,
            cached_eth: None,
        }
    }

    pub async fn trx_usd(&mut self) -> Result<f64> {
        if let Some(v) = self.cfg.trx_usd_override {
            return Ok(v);
        }

        if let Some((price, at)) = self.cached_trx {
            if at.elapsed() <= self.cfg.trx_usd_ttl {
                return Ok(price);
            }
        }

        let resp = self
            .http
            .get(&self.cfg.trx_usd_url)
            .send()
            .await
            .context("GET trx_usd_url")?;
        if !resp.status().is_success() {
            anyhow::bail!("trx_usd_url returned {}", resp.status());
        }

        // Default URL is Coingecko's simple price endpoint:
        //   https://api.coingecko.com/api/v3/simple/price?ids=tron&vs_currencies=usd
        let body: CoingeckoSimplePrice = resp.json().await.context("decode trx_usd json")?;
        let price = body.tron.usd;
        if !(price.is_finite()) || price <= 0.0 {
            anyhow::bail!("invalid trx usd price: {price}");
        }
        self.cached_trx = Some((price, Instant::now()));
        Ok(price)
    }

    pub async fn eth_usd(&mut self) -> Result<f64> {
        if let Some(v) = self.cfg.eth_usd_override {
            return Ok(v);
        }

        if let Some((price, at)) = self.cached_eth {
            if at.elapsed() <= self.cfg.eth_usd_ttl {
                return Ok(price);
            }
        }

        let resp = self
            .http
            .get(&self.cfg.eth_usd_url)
            .send()
            .await
            .context("GET eth_usd_url")?;
        if !resp.status().is_success() {
            anyhow::bail!("eth_usd_url returned {}", resp.status());
        }

        // Default URL is Coingecko's simple price endpoint:
        //   https://api.coingecko.com/api/v3/simple/price?ids=ethereum&vs_currencies=usd
        let body: CoingeckoSimplePrice = resp.json().await.context("decode eth_usd json")?;
        let price = body.ethereum.usd;
        if !(price.is_finite()) || price <= 0.0 {
            anyhow::bail!("invalid eth usd price: {price}");
        }
        self.cached_eth = Some((price, Instant::now()));
        Ok(price)
    }
}
