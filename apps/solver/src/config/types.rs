use aa::SafeDeterministicDeploymentConfig;
use alloy::primitives::Address;
use serde::Deserialize;
use std::time::Duration;
use tron::JsonApiRentalProviderConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubTxMode {
    Eoa,
    Safe4337,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TronMode {
    Grpc,
    Mock,
}

#[derive(Debug, Clone)]
pub struct PolicyConfig {
    pub enabled_intent_types: Vec<crate::types::IntentType>,
    pub min_deadline_slack_secs: u64,
    pub min_profit_usd: f64,
    /// Fallback hub tx cost used when we don't have enough historical receipt data.
    pub hub_cost_usd: f64,
    /// Number of recent included userops per kind used to estimate hub cost.
    pub hub_cost_history_lookback: u64,
    /// Extra headroom applied to the hub cost estimate (ppm, i.e. 100_000 = +10%).
    pub hub_cost_headroom_ppm: u64,
    pub tron_fee_usd: f64,
    /// Number of recent confirmed Tron txs per intent type used to estimate Tron fees.
    pub tron_fee_history_lookback: u64,
    /// Extra headroom applied to the Tron fee estimate (ppm, i.e. 100_000 = +10%).
    pub tron_fee_headroom_ppm: u64,
    pub capital_lock_ppm_per_day: u64,
    pub require_priced_escrow: bool,
    pub allowed_escrow_tokens: Vec<Address>,

    pub trigger_contract_allowlist: Vec<Address>,
    pub trigger_contract_denylist: Vec<Address>,
    pub trigger_selector_denylist: Vec<[u8; 4]>,
    pub trigger_allow_fallback_calls: bool,

    pub max_trx_transfer_sun: Option<u64>,
    pub max_usdt_transfer_amount: Option<u64>,
    pub max_delegate_balance_sun: Option<u64>,
    pub max_delegate_lock_period_secs: Option<u64>,
    pub max_trigger_call_value_sun: Option<u64>,
    pub max_trigger_calldata_len: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PaymasterServiceConfig {
    pub url: String,
    #[serde(default)]
    pub context: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub indexer: IndexerConfig,
    pub hub: HubConfig,
    pub tron: TronConfig,
    pub jobs: JobConfig,
    pub policy: PolicyConfig,
    pub pricing: crate::pricing::PricingConfig,
    pub db_url: String,
    pub instance_id: String,
}

#[derive(Debug, Clone)]
pub struct IndexerConfig {
    pub base_url: String,
    pub timeout: Duration,
    pub max_head_lag_blocks: u64,
}

#[derive(Debug, Clone)]
pub struct HubConfig {
    pub tx_mode: HubTxMode,
    pub rpc_url: String,
    pub chain_id: Option<u64>,
    pub pool: Address,

    // AA/Safe4337 options (only used when tx_mode == Safe4337).
    pub entrypoint: Option<Address>,
    pub safe: Option<Address>,
    pub safe_4337_module: Option<Address>,
    pub safe_deployment: Option<SafeDeterministicDeploymentConfig>,
    pub bundler_urls: Vec<String>,
    pub paymasters: Vec<PaymasterServiceConfig>,

    /// Private key used to sign hub chain transactions.
    /// - In EOA mode: the EOA's private key.
    /// - In Safe4337 mode: the Safe owner key.
    pub signer_private_key: [u8; 32],
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TronConfig {
    pub mode: TronMode,
    pub grpc_url: String,
    pub api_key: Option<String>,
    /// Default Tron key (back-compat; also used when only one key is configured).
    pub private_key: [u8; 32],
    /// All configured Tron keys (one or more) for inventory selection and consolidation.
    pub private_keys: Vec<[u8; 32]>,
    pub controller_address: String,
    pub mock_reader_address: Option<Address>,

    pub block_lag: u64,
    pub fee_limit_cap_sun: u64,
    /// Extra headroom on computed fee_limit (ppm, i.e. 100_000 = +10%).
    pub fee_limit_headroom_ppm: u64,
    /// Cache TTL (seconds) for global stake totals (TotalEnergyLimit/Weight, TotalNetLimit/Weight).
    pub stake_totals_cache_ttl_secs: u64,
    /// Optional list of external energy rental providers.
    pub energy_rental_providers: Vec<JsonApiRentalProviderConfig>,
    /// If true, fill `DELEGATE_RESOURCE` intents by requesting resource rentals from configured
    /// providers instead of delegating from the solver's own staked accounts.
    pub delegate_resource_resell_enabled: bool,
    /// Rental provider freeze: number of failures within a window before freezing.
    pub rental_provider_fail_threshold: i32,
    /// Rental provider freeze window (seconds) for counting failures.
    pub rental_provider_fail_window_secs: i64,
    /// Rental provider freeze duration in seconds.
    pub rental_provider_freeze_secs: i64,
    /// When converting `balanceSun` -> energy units for rental APIs, add headroom (ppm).
    pub resell_energy_headroom_ppm: u64,

    /// If true (and TRON_MODE=grpc), run pre-claim emulation checks for contract-call intents.
    pub emulation_enabled: bool,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct JobConfig {
    pub tick_interval: Duration,
    pub tron_finality_blocks: u64,
    pub tip_proof_resend_blocks: u64,

    pub process_controller_max_events: u64,
    pub fill_max_claims: u64,
    pub max_in_flight_jobs: u64,

    /// Max concurrent jobs per intent type (best-effort backpressure).
    pub concurrency_trx_transfer: u64,
    pub concurrency_usdt_transfer: u64,
    pub concurrency_delegate_resource: u64,
    pub concurrency_trigger_smart_contract: u64,
    /// Max concurrent Tron broadcasts (avoid ref-block collisions / node overload).
    pub concurrency_tron_broadcast: u64,

    /// Enable consolidation pre-transactions for TRX/USDT intents (moves funds into executor key).
    pub consolidation_enabled: bool,
    /// Maximum number of pre-transactions per job.
    pub consolidation_max_pre_txs: u64,
    /// Maximum total TRX pulled into executor across all pre-transactions (SUN). 0 = unlimited.
    pub consolidation_max_total_trx_pull_sun: u64,
    /// Maximum TRX pulled in a single pre-transaction (SUN). 0 = unlimited.
    pub consolidation_max_per_tx_trx_pull_sun: u64,
    /// Maximum total USDT pulled into executor across all pre-transactions (token base units). 0 = unlimited.
    pub consolidation_max_total_usdt_pull_amount: u64,
    /// Maximum USDT pulled in a single pre-transaction (token base units). 0 = unlimited.
    pub consolidation_max_per_tx_usdt_pull_amount: u64,

    /// Rate limit: max claim submissions per minute (global). 0 = unlimited.
    pub rate_limit_claims_per_minute_global: u64,
    /// Rate limit: max claim submissions per minute per intent type. 0 = unlimited.
    pub rate_limit_claims_per_minute_trx_transfer: u64,
    pub rate_limit_claims_per_minute_usdt_transfer: u64,
    pub rate_limit_claims_per_minute_delegate_resource: u64,
    pub rate_limit_claims_per_minute_trigger_smart_contract: u64,

    /// Optional auto-pause when fatal errors spike. 0 disables auto-pause.
    pub global_pause_fatal_threshold: u64,
    pub global_pause_window_secs: u64,
    pub global_pause_duration_secs: u64,

    /// If Tron emulation says "ok" but tx fails onchain, apply this multiplier to breaker fail_count.
    pub breaker_mismatch_penalty: u64,

    /// Capacity reservation TTL for delegate jobs (seconds).
    pub delegate_reservation_ttl_secs: u64,

    pub controller_rebalance_threshold_usdt: String,
    pub controller_rebalance_keep_usdt: String,

    pub pull_liquidity_ppm: u64,
}
