use anyhow::{Context, Result};
use futures::{StreamExt, stream};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use tokio_util::sync::CancellationToken;

use crate::rpc::parse_quantity_u64;

#[derive(Clone)]
pub struct TimestampCache {
    map: HashMap<u64, u64>,
    capacity: usize,
}

impl TimestampCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            map: HashMap::new(),
            capacity: capacity.max(1),
        }
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }

    pub fn get(&self, block_number: u64) -> Option<u64> {
        self.map.get(&block_number).copied()
    }

    pub fn insert(&mut self, block_number: u64, ts: u64) {
        if self.map.len() >= self.capacity {
            // Simple cap: keep it cheap. A full LRU isn't necessary here.
            self.map.clear();
        }
        self.map.insert(block_number, ts);
    }
}

pub fn normalize_timestamp_seconds(timestamp: u64) -> u64 {
    if timestamp >= 20_000_000_000 {
        timestamp / 1000
    } else {
        timestamp
    }
}

pub fn parse_block_timestamp(block: &Value) -> Result<u64> {
    let ts = block.get("timestamp").context("missing block.timestamp")?;
    Ok(normalize_timestamp_seconds(
        parse_quantity_u64(ts.clone()).context("timestamp is not a valid quantity")?,
    ))
}

pub fn parse_block_hash(block: &Value) -> Result<String> {
    let h = block
        .get("hash")
        .and_then(|v| v.as_str())
        .context("missing block.hash")?;
    Ok(h.to_lowercase())
}

pub async fn populate_timestamps(
    shutdown: &CancellationToken,
    rpc: &crate::rpc::RpcClient,
    cache: &mut TimestampCache,
    block_numbers: &[u64],
    concurrency: usize,
) -> Result<()> {
    let mut unique = HashSet::new();
    for b in block_numbers {
        if cache.get(*b).is_none() {
            unique.insert(*b);
        }
    }
    if unique.is_empty() {
        return Ok(());
    }

    let rpc = rpc.clone();
    let shutdown_token = shutdown.clone();
    let concurrency = concurrency.max(1);
    let mut tasks = stream::iter(unique.into_iter())
        .map(move |block_number| {
            let rpc = rpc.clone();
            let shutdown = shutdown_token.clone();
            async move {
                tokio::select! {
                    _ = shutdown.cancelled() => Ok::<Option<(u64, u64)>, anyhow::Error>(None),
                    res = rpc.get_block_by_number(block_number) => {
                        let Some(block) = res? else { return Ok(None); };
                        let ts = parse_block_timestamp(&block)
                            .with_context(|| format!("parse block.timestamp for {block_number}"))?;
                        Ok(Some((block_number, ts)))
                    }
                }
            }
        })
        .buffer_unordered(concurrency);

    while let Some(res) = tasks.next().await {
        if shutdown.is_cancelled() {
            return Ok(());
        }
        if let Some((b, ts)) = res? {
            cache.insert(b, ts);
        }
    }

    Ok(())
}
