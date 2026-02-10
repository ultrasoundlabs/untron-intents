use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(default)]
pub(super) struct Env {
    pub solver_db_url: String,

    pub indexer_api_base_url: String,

    pub indexer_timeout_secs: u64,

    pub indexer_max_head_lag_blocks: u64,

    pub hub_rpc_url: String,

    pub hub_chain_id: Option<u64>,

    /// Pool contract address (UntronIntents).
    pub hub_pool_address: String,
    /// Back-compat: older config used HUB_UNTRON_V3_ADDRESS for the pool.
    #[serde(default)]
    pub hub_untron_v3_address: String,

    #[serde(default)]
    pub hub_tx_mode: String,

    #[serde(default)]
    pub hub_entrypoint_address: String,

    #[serde(default)]
    pub hub_safe_address: String,

    #[serde(default)]
    pub hub_safe_4337_module_address: String,

    #[serde(default)]
    pub hub_safe_proxy_factory_address: String,

    #[serde(default)]
    pub hub_safe_singleton_address: String,

    #[serde(default)]
    pub hub_safe_module_setup_address: String,

    pub hub_signer_private_key_hex: String,

    #[serde(default)]
    pub hub_bundler_urls: String,

    #[serde(default)]
    pub hub_paymasters_json: String,

    #[serde(default)]
    pub tron_mode: String,

    pub tron_grpc_url: String,

    pub tron_api_key: Option<String>,

    pub tron_private_key_hex: String,

    #[serde(default)]
    pub tron_private_keys_hex_csv: String,

    pub tron_controller_address: String,

    #[serde(default)]
    pub tron_mock_reader_address: String,

    pub tron_block_lag: u64,

    #[serde(default)]
    pub tron_fee_limit_cap_sun: u64,

    #[serde(default)]
    pub tron_fee_limit_headroom_ppm: u64,

    #[serde(default)]
    pub tron_stake_totals_cache_ttl_secs: u64,

    #[serde(default)]
    pub tron_energy_rental_apis_json: String,

    #[serde(default)]
    pub tron_delegate_resource_resell_enabled: bool,

    #[serde(default)]
    pub tron_rental_provider_fail_threshold: i32,
    #[serde(default)]
    pub tron_rental_provider_fail_window_secs: i64,
    #[serde(default)]
    pub tron_rental_provider_freeze_secs: i64,

    #[serde(default)]
    pub tron_resell_energy_headroom_ppm: u64,

    #[serde(default)]
    pub solver_tron_emulation_enabled: bool,

    pub solver_tick_interval_secs: u64,

    pub tron_finality_blocks: u64,

    pub tron_tip_proof_resend_blocks: u64,

    pub process_controller_max_events: u64,

    pub fill_max_claims: u64,

    #[serde(default)]
    pub solver_max_in_flight_jobs: u64,
    #[serde(default)]
    pub solver_safe4337_max_claimed_unproved_jobs: u64,

    #[serde(default)]
    pub solver_concurrency_trx_transfer: u64,
    #[serde(default)]
    pub solver_concurrency_usdt_transfer: u64,
    #[serde(default)]
    pub solver_concurrency_delegate_resource: u64,
    #[serde(default)]
    pub solver_concurrency_trigger_smart_contract: u64,
    #[serde(default)]
    pub solver_concurrency_tron_broadcast: u64,

    #[serde(default)]
    pub solver_consolidation_enabled: bool,
    #[serde(default)]
    pub solver_consolidation_max_pre_txs: u64,
    #[serde(default)]
    pub solver_consolidation_max_total_trx_pull_sun: u64,
    #[serde(default)]
    pub solver_consolidation_max_per_tx_trx_pull_sun: u64,
    #[serde(default)]
    pub solver_consolidation_max_total_usdt_pull_amount: u64,
    #[serde(default)]
    pub solver_consolidation_max_per_tx_usdt_pull_amount: u64,

    #[serde(default)]
    pub solver_rate_limit_claims_per_minute_global: u64,
    #[serde(default)]
    pub solver_rate_limit_claims_per_minute_trx_transfer: u64,
    #[serde(default)]
    pub solver_rate_limit_claims_per_minute_usdt_transfer: u64,
    #[serde(default)]
    pub solver_rate_limit_claims_per_minute_delegate_resource: u64,
    #[serde(default)]
    pub solver_rate_limit_claims_per_minute_trigger_smart_contract: u64,

    #[serde(default)]
    pub solver_global_pause_fatal_threshold: u64,
    #[serde(default)]
    pub solver_global_pause_window_secs: u64,
    #[serde(default)]
    pub solver_global_pause_duration_secs: u64,

    #[serde(default)]
    pub solver_breaker_mismatch_penalty: u64,

    #[serde(default)]
    pub solver_delegate_reservation_ttl_secs: u64,

    pub controller_rebalance_threshold_usdt: String,

    pub controller_rebalance_keep_usdt: String,

    pub pull_liquidity_ppm: u64,

    #[serde(default)]
    pub solver_enabled_intent_types: String,

    #[serde(default)]
    pub solver_min_deadline_slack_secs: u64,

    #[serde(default)]
    pub solver_min_profit_usd: f64,

    #[serde(default)]
    pub solver_hub_cost_usd: f64,

    #[serde(default)]
    pub solver_hub_cost_history_lookback: u64,

    #[serde(default)]
    pub solver_hub_cost_headroom_ppm: u64,

    #[serde(default)]
    pub solver_tron_fee_usd: f64,

    #[serde(default)]
    pub solver_tron_fee_history_lookback: u64,

    #[serde(default)]
    pub solver_tron_fee_headroom_ppm: u64,

    #[serde(default)]
    pub solver_capital_lock_ppm_per_day: u64,

    #[serde(default)]
    pub solver_require_priced_escrow: bool,

    #[serde(default)]
    pub solver_allowed_escrow_tokens_csv: String,

    #[serde(default)]
    pub solver_trigger_contract_allowlist_csv: String,

    #[serde(default)]
    pub solver_trigger_contract_denylist_csv: String,

    #[serde(default)]
    pub solver_trigger_selector_denylist_csv: String,

    #[serde(default)]
    pub solver_trigger_allow_fallback_calls: bool,

    #[serde(default)]
    pub solver_max_trx_transfer_sun: u64,

    #[serde(default)]
    pub solver_max_usdt_transfer_amount: u64,

    #[serde(default)]
    pub solver_max_delegate_balance_sun: u64,

    #[serde(default)]
    pub solver_max_delegate_lock_period_secs: u64,

    #[serde(default)]
    pub solver_max_trigger_call_value_sun: u64,

    #[serde(default)]
    pub solver_max_trigger_calldata_len: u64,

    #[serde(default)]
    pub solver_trx_usd_override: Option<f64>,

    #[serde(default)]
    pub solver_trx_usd_ttl_secs: u64,

    #[serde(default)]
    pub solver_trx_usd_url: String,

    #[serde(default)]
    pub solver_eth_usd_override: Option<f64>,

    #[serde(default)]
    pub solver_eth_usd_ttl_secs: u64,

    #[serde(default)]
    pub solver_eth_usd_url: String,

    #[serde(default)]
    pub solver_instance_id: String,
}

impl Default for Env {
    fn default() -> Self {
        Self {
            solver_db_url: String::new(),
            indexer_api_base_url: String::new(),
            indexer_timeout_secs: 10,
            indexer_max_head_lag_blocks: 50,
            hub_rpc_url: String::new(),
            hub_chain_id: None,
            hub_pool_address: String::new(),
            hub_untron_v3_address: String::new(),
            hub_tx_mode: "eoa".to_string(),
            hub_entrypoint_address: String::new(),
            hub_safe_address: String::new(),
            hub_safe_4337_module_address: String::new(),
            hub_safe_proxy_factory_address: String::new(),
            hub_safe_singleton_address: String::new(),
            hub_safe_module_setup_address: String::new(),
            hub_signer_private_key_hex: String::new(),
            hub_bundler_urls: String::new(),
            hub_paymasters_json: String::new(),
            tron_mode: "grpc".to_string(),
            tron_grpc_url: String::new(),
            tron_api_key: None,
            tron_private_key_hex: String::new(),
            tron_private_keys_hex_csv: String::new(),
            tron_controller_address: String::new(),
            tron_mock_reader_address: String::new(),
            tron_block_lag: 0,
            tron_fee_limit_cap_sun: 200_000_000,
            tron_fee_limit_headroom_ppm: 100_000,
            tron_stake_totals_cache_ttl_secs: 10,
            tron_energy_rental_apis_json: String::new(),
            tron_delegate_resource_resell_enabled: false,
            tron_rental_provider_fail_threshold: 3,
            tron_rental_provider_fail_window_secs: 60,
            tron_rental_provider_freeze_secs: 300,
            tron_resell_energy_headroom_ppm: 50_000,
            solver_tron_emulation_enabled: true,
            solver_tick_interval_secs: 5,
            tron_finality_blocks: 19,
            tron_tip_proof_resend_blocks: 20,
            process_controller_max_events: 100,
            fill_max_claims: 50,
            solver_max_in_flight_jobs: 50,
            solver_safe4337_max_claimed_unproved_jobs: 1,
            solver_concurrency_trx_transfer: 4,
            solver_concurrency_usdt_transfer: 2,
            solver_concurrency_delegate_resource: 1,
            solver_concurrency_trigger_smart_contract: 1,
            solver_concurrency_tron_broadcast: 1,
            solver_consolidation_enabled: false,
            solver_consolidation_max_pre_txs: 0,
            solver_consolidation_max_total_trx_pull_sun: 0,
            solver_consolidation_max_per_tx_trx_pull_sun: 0,
            solver_consolidation_max_total_usdt_pull_amount: 0,
            solver_consolidation_max_per_tx_usdt_pull_amount: 0,
            solver_rate_limit_claims_per_minute_global: 0,
            solver_rate_limit_claims_per_minute_trx_transfer: 0,
            solver_rate_limit_claims_per_minute_usdt_transfer: 0,
            solver_rate_limit_claims_per_minute_delegate_resource: 0,
            solver_rate_limit_claims_per_minute_trigger_smart_contract: 0,
            solver_global_pause_fatal_threshold: 0,
            solver_global_pause_window_secs: 300,
            solver_global_pause_duration_secs: 300,
            solver_breaker_mismatch_penalty: 2,
            solver_delegate_reservation_ttl_secs: 600,
            controller_rebalance_threshold_usdt: "0".to_string(),
            controller_rebalance_keep_usdt: "1".to_string(),
            pull_liquidity_ppm: 500_000,

            solver_enabled_intent_types: "trx_transfer,delegate_resource".to_string(),
            solver_min_deadline_slack_secs: 30,
            solver_instance_id: String::new(),
            solver_min_profit_usd: 0.0,
            solver_hub_cost_usd: 0.0,
            solver_hub_cost_history_lookback: 50,
            solver_hub_cost_headroom_ppm: 200_000,
            solver_tron_fee_usd: 0.0,
            solver_tron_fee_history_lookback: 50,
            solver_tron_fee_headroom_ppm: 200_000,
            solver_capital_lock_ppm_per_day: 0,
            solver_require_priced_escrow: false,
            solver_allowed_escrow_tokens_csv: String::new(),
            solver_trigger_contract_allowlist_csv: String::new(),
            solver_trigger_contract_denylist_csv: String::new(),
            solver_trigger_selector_denylist_csv: "0x095ea7b3,0x39509351".to_string(),
            solver_trigger_allow_fallback_calls: false,
            solver_max_trx_transfer_sun: 0,
            solver_max_usdt_transfer_amount: 0,
            solver_max_delegate_balance_sun: 0,
            solver_max_delegate_lock_period_secs: 0,
            solver_max_trigger_call_value_sun: 0,
            solver_max_trigger_calldata_len: 0,
            solver_trx_usd_override: None,
            solver_trx_usd_ttl_secs: 60,
            solver_trx_usd_url:
                "https://api.coingecko.com/api/v3/simple/price?ids=tron&vs_currencies=usd"
                    .to_string(),
            solver_eth_usd_override: None,
            solver_eth_usd_ttl_secs: 60,
            solver_eth_usd_url:
                "https://api.coingecko.com/api/v3/simple/price?ids=ethereum&vs_currencies=usd"
                    .to_string(),
        }
    }
}
