use anyhow::{Context, Result};
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Pool,
    Forwarder,
}

impl Stream {
    pub const fn as_str(self) -> &'static str {
        match self {
            Stream::Pool => "pool",
            Stream::Forwarder => "forwarder",
        }
    }

    /// Onchain index contract name used for `EventChainGenesis` derivation.
    pub const fn index_name(self) -> &'static str {
        match self {
            Stream::Pool => "UntronIntentsIndex",
            Stream::Forwarder => "IntentsForwarderIndex",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RpcConfig {
    pub urls: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct InstanceConfig {
    pub stream: Stream,
    pub chain_id: u64,
    pub rpc: RpcConfig,
    /// EVM 0x-address (for both pool + forwarder).
    pub contract_address: String,
    pub deployment_block: u64,

    pub confirmations: u64,
    pub poll_interval: Duration,
    pub chunk_blocks: u64,
    pub reorg_scan_depth: u64,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub database_url: String,
    pub db_max_connections: u32,

    pub block_header_concurrency: usize,
    pub block_timestamp_cache_size: usize,

    pub progress_interval: Duration,
    pub progress_tail_lag_blocks: u64,

    pub pool: InstanceConfig,
    pub forwarders: Vec<InstanceConfig>,

    /// Optional: only run a subset of streams ("pool" | "forwarder" | "all").
    pub only_stream: Option<StreamSelection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamSelection {
    Pool,
    Forwarder,
    All,
}

impl StreamSelection {
    fn parse(value: &str) -> Result<Self> {
        match value.trim().to_lowercase().as_str() {
            "pool" => Ok(Self::Pool),
            "forwarder" => Ok(Self::Forwarder),
            "all" => Ok(Self::All),
            other => {
                anyhow::bail!("invalid INDEXER_STREAM value: {other} (expected pool|forwarder|all)")
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct BaseEnv {
    database_url: String,

    db_max_connections: u32,

    block_header_concurrency: usize,
    block_timestamp_cache_size: usize,

    #[serde(rename = "indexer_progress_interval_secs")]
    progress_interval_secs: u64,

    #[serde(rename = "indexer_progress_tail_lag_blocks")]
    progress_tail_lag_blocks: u64,

    #[serde(rename = "indexer_stream")]
    stream: Option<String>,
}

impl Default for BaseEnv {
    fn default() -> Self {
        Self {
            database_url: String::new(),
            db_max_connections: DEFAULT_DB_MAX_CONNECTIONS,
            block_header_concurrency: DEFAULT_BLOCK_HEADER_CONCURRENCY,
            block_timestamp_cache_size: DEFAULT_BLOCK_TIMESTAMP_CACHE_SIZE,
            progress_interval_secs: DEFAULT_PROGRESS_INTERVAL_SECS,
            progress_tail_lag_blocks: DEFAULT_PROGRESS_TAIL_LAG_BLOCKS,
            stream: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct PoolEnv {
    #[serde(rename = "rpc_urls")]
    rpc_urls_raw: String,
    chain_id: u64,
    contract_address: String,
    deployment_block: u64,

    confirmations: Option<u64>,
    poll_interval_secs: Option<u64>,
    chunk_blocks: Option<u64>,
    reorg_scan_depth: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct ForwardersEnv {
    /// Default forwarder contract address for all chains (may be overridden per entry).
    #[serde(rename = "forwarder_contract_address")]
    forwarder_contract_address: String,

    /// JSON array of forwarder chain entries.
    #[serde(rename = "forwarders_chains")]
    forwarders_chains: String,

    // Optional defaults (can be overridden per chain entry).
    forwarder_confirmations: Option<u64>,
    forwarder_poll_interval_secs: Option<u64>,
    forwarder_chunk_blocks: Option<u64>,
    forwarder_reorg_scan_depth: Option<u64>,
}

impl Default for ForwardersEnv {
    fn default() -> Self {
        Self {
            forwarder_contract_address: String::new(),
            forwarders_chains: "[]".to_string(),
            forwarder_confirmations: None,
            forwarder_poll_interval_secs: None,
            forwarder_chunk_blocks: None,
            forwarder_reorg_scan_depth: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ForwarderChainEntry {
    #[serde(rename = "chainId")]
    chain_id: u64,

    #[serde(rename = "rpcs")]
    rpc_urls: Vec<String>,

    #[serde(rename = "forwarderDeploymentBlock")]
    forwarder_deployment_block: u64,

    /// Optional override per chain. If missing, uses `FORWARDER_CONTRACT_ADDRESS`.
    #[serde(rename = "forwarderContractAddress")]
    forwarder_contract_address: Option<String>,

    confirmations: Option<u64>,
    poll_interval_secs: Option<u64>,
    chunk_blocks: Option<u64>,
    reorg_scan_depth: Option<u64>,
}

pub fn load_config() -> Result<AppConfig> {
    let base: BaseEnv = envy::from_env().context("load base env config")?;
    if base.database_url.trim().is_empty() {
        anyhow::bail!("DATABASE_URL must be set");
    }

    let pool_env: PoolEnv = envy::prefixed("POOL_")
        .from_env()
        .context("load POOL_* env config")?;

    let forwarders_env: ForwardersEnv = envy::from_env().context("load forwarders env config")?;

    let pool_rpc_urls = parse_list(&pool_env.rpc_urls_raw);
    if pool_rpc_urls.is_empty() {
        anyhow::bail!("POOL_RPC_URLS must not be empty");
    }

    let pool = InstanceConfig {
        stream: Stream::Pool,
        chain_id: pool_env.chain_id,
        rpc: RpcConfig {
            urls: pool_rpc_urls,
        },
        contract_address: pool_env.contract_address,
        deployment_block: pool_env.deployment_block,
        confirmations: pool_env.confirmations.unwrap_or(DEFAULT_POOL_CONFIRMATIONS),
        poll_interval: Duration::from_secs(
            pool_env
                .poll_interval_secs
                .unwrap_or(DEFAULT_POOL_POLL_INTERVAL_SECS)
                .max(1),
        ),
        chunk_blocks: pool_env
            .chunk_blocks
            .unwrap_or(DEFAULT_POOL_CHUNK_BLOCKS)
            .max(1),
        reorg_scan_depth: pool_env
            .reorg_scan_depth
            .unwrap_or(DEFAULT_POOL_REORG_SCAN_DEPTH)
            .max(1),
    };

    let forwarder_chains: Vec<ForwarderChainEntry> =
        serde_json::from_str(forwarders_env.forwarders_chains.trim().if_empty("[]"))
            .with_context(|| "parse FORWARDERS_CHAINS as JSON array")?;

    let mut forwarders = Vec::with_capacity(forwarder_chains.len());
    for entry in forwarder_chains {
        let contract_address = entry
            .forwarder_contract_address
            .or_else(|| {
                if forwarders_env.forwarder_contract_address.trim().is_empty() {
                    None
                } else {
                    Some(forwarders_env.forwarder_contract_address.clone())
                }
            })
            .with_context(|| {
                format!(
                    "missing forwarder contract address for chain_id {} (set FORWARDER_CONTRACT_ADDRESS or forwarderContractAddress in FORWARDERS_CHAINS entry)",
                    entry.chain_id
                )
            })?;

        if entry.rpc_urls.is_empty() {
            anyhow::bail!(
                "FORWARDERS_CHAINS entry for chain_id {} has empty rpcs",
                entry.chain_id
            );
        }

        forwarders.push(InstanceConfig {
            stream: Stream::Forwarder,
            chain_id: entry.chain_id,
            rpc: RpcConfig {
                urls: entry.rpc_urls,
            },
            contract_address,
            deployment_block: entry.forwarder_deployment_block,
            confirmations: entry
                .confirmations
                .or(forwarders_env.forwarder_confirmations)
                .unwrap_or(DEFAULT_FORWARDER_CONFIRMATIONS),
            poll_interval: Duration::from_secs(
                entry
                    .poll_interval_secs
                    .or(forwarders_env.forwarder_poll_interval_secs)
                    .unwrap_or(DEFAULT_FORWARDER_POLL_INTERVAL_SECS)
                    .max(1),
            ),
            chunk_blocks: entry
                .chunk_blocks
                .or(forwarders_env.forwarder_chunk_blocks)
                .unwrap_or(DEFAULT_FORWARDER_CHUNK_BLOCKS)
                .max(1),
            reorg_scan_depth: entry
                .reorg_scan_depth
                .or(forwarders_env.forwarder_reorg_scan_depth)
                .unwrap_or(DEFAULT_FORWARDER_REORG_SCAN_DEPTH)
                .max(1),
        });
    }

    let only_stream = match base.stream.as_deref() {
        None => None,
        Some(s) => Some(StreamSelection::parse(s).context("INDEXER_STREAM")?),
    };

    Ok(AppConfig {
        database_url: base.database_url,
        db_max_connections: base.db_max_connections,
        block_header_concurrency: base.block_header_concurrency.max(1),
        block_timestamp_cache_size: base.block_timestamp_cache_size.max(1),
        progress_interval: Duration::from_secs(base.progress_interval_secs.max(1)),
        progress_tail_lag_blocks: base.progress_tail_lag_blocks,
        pool,
        forwarders,
        only_stream,
    })
}

fn parse_list(raw: &str) -> Vec<String> {
    raw.split(|c: char| c == ',' || c.is_whitespace())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

trait IfEmpty {
    fn if_empty<'a>(&'a self, fallback: &'a str) -> &'a str;
}

impl IfEmpty for str {
    fn if_empty<'a>(&'a self, fallback: &'a str) -> &'a str {
        if self.trim().is_empty() {
            fallback
        } else {
            self
        }
    }
}

const DEFAULT_DB_MAX_CONNECTIONS: u32 = 5;
const DEFAULT_BLOCK_HEADER_CONCURRENCY: usize = 16;
const DEFAULT_BLOCK_TIMESTAMP_CACHE_SIZE: usize = 2048;
const DEFAULT_PROGRESS_INTERVAL_SECS: u64 = 5;
const DEFAULT_PROGRESS_TAIL_LAG_BLOCKS: u64 = 0;

// Pool stream defaults (tuned for typical EVM RPCs).
const DEFAULT_POOL_CONFIRMATIONS: u64 = 0;
const DEFAULT_POOL_POLL_INTERVAL_SECS: u64 = 1;
const DEFAULT_POOL_CHUNK_BLOCKS: u64 = 2_000;
const DEFAULT_POOL_REORG_SCAN_DEPTH: u64 = 256;

// Forwarder stream defaults.
const DEFAULT_FORWARDER_CONFIRMATIONS: u64 = 0;
const DEFAULT_FORWARDER_POLL_INTERVAL_SECS: u64 = 1;
const DEFAULT_FORWARDER_CHUNK_BLOCKS: u64 = 2_000;
const DEFAULT_FORWARDER_REORG_SCAN_DEPTH: u64 = 256;
