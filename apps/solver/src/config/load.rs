use super::env::Env;
use super::parse::{
    opt_u64, parse_address, parse_addresses_csv, parse_csv, parse_hex_32, parse_hex_32_csv,
    parse_hub_tx_mode, parse_intent_types, parse_optional_address, parse_paymasters_json,
    parse_selectors_csv, parse_tron_energy_rental_apis_json, parse_tron_mode,
};
use super::{
    AppConfig, HubConfig, HubTxMode, IndexerConfig, JobConfig, PolicyConfig, TronConfig, TronMode,
};
use aa::SafeDeterministicDeploymentConfig;
use anyhow::{Context, Result};
use std::time::Duration;

pub(super) fn load_config() -> Result<AppConfig> {
    let env: Env = envy::from_env().context("load solver env config")?;

    if env.solver_db_url.trim().is_empty() {
        anyhow::bail!("SOLVER_DB_URL must be set");
    }
    if env.indexer_api_base_url.trim().is_empty() {
        anyhow::bail!("INDEXER_API_BASE_URL must be set");
    }
    if env.hub_rpc_url.trim().is_empty() {
        anyhow::bail!("HUB_RPC_URL must be set");
    }
    if env.hub_pool_address.trim().is_empty() && env.hub_untron_v3_address.trim().is_empty() {
        anyhow::bail!("HUB_POOL_ADDRESS must be set");
    }

    let hub_tx_mode = parse_hub_tx_mode(&env.hub_tx_mode)?;

    let hub_pool = if !env.hub_pool_address.trim().is_empty() {
        parse_address("HUB_POOL_ADDRESS", &env.hub_pool_address)?
    } else {
        tracing::warn!("HUB_POOL_ADDRESS is empty; falling back to HUB_UNTRON_V3_ADDRESS");
        parse_address("HUB_UNTRON_V3_ADDRESS", &env.hub_untron_v3_address)?
    };

    let enabled_intent_types = parse_intent_types(&env.solver_enabled_intent_types)?;

    let trigger_contract_allowlist = parse_addresses_csv(
        "SOLVER_TRIGGER_CONTRACT_ALLOWLIST_CSV",
        &env.solver_trigger_contract_allowlist_csv,
    )?;
    if enabled_intent_types.contains(&crate::types::IntentType::TriggerSmartContract)
        && trigger_contract_allowlist.is_empty()
    {
        anyhow::bail!(
            "TRIGGER_SMART_CONTRACT enabled but SOLVER_TRIGGER_CONTRACT_ALLOWLIST_CSV is empty"
        );
    }

    if env.hub_signer_private_key_hex.trim().is_empty() {
        anyhow::bail!("HUB_SIGNER_PRIVATE_KEY_HEX must be set");
    }
    let hub_signer_private_key = parse_hex_32(
        "HUB_SIGNER_PRIVATE_KEY_HEX",
        &env.hub_signer_private_key_hex,
    )?;

    let (hub_entrypoint, hub_safe, hub_module, hub_safe_deployment, bundlers, paymasters) =
        if hub_tx_mode == HubTxMode::Safe4337 {
            if env.hub_entrypoint_address.trim().is_empty() {
                anyhow::bail!("HUB_ENTRYPOINT_ADDRESS must be set in HUB_TX_MODE=safe4337");
            }
            if env.hub_safe_4337_module_address.trim().is_empty() {
                anyhow::bail!("HUB_SAFE_4337_MODULE_ADDRESS must be set in HUB_TX_MODE=safe4337");
            }
            if env.hub_bundler_urls.trim().is_empty() {
                anyhow::bail!("HUB_BUNDLER_URLS must be set in HUB_TX_MODE=safe4337");
            }

            let entrypoint = Some(parse_address(
                "HUB_ENTRYPOINT_ADDRESS",
                &env.hub_entrypoint_address,
            )?);
            let safe = parse_optional_address("HUB_SAFE_ADDRESS", &env.hub_safe_address)?;
            let module = Some(parse_address(
                "HUB_SAFE_4337_MODULE_ADDRESS",
                &env.hub_safe_4337_module_address,
            )?);
            let safe_deployment = if safe.is_some() {
                None
            } else {
                Some(SafeDeterministicDeploymentConfig {
                    proxy_factory: parse_address(
                        "HUB_SAFE_PROXY_FACTORY_ADDRESS",
                        &env.hub_safe_proxy_factory_address,
                    )?,
                    singleton: parse_address(
                        "HUB_SAFE_SINGLETON_ADDRESS",
                        &env.hub_safe_singleton_address,
                    )?,
                    module_setup: parse_address(
                        "HUB_SAFE_MODULE_SETUP_ADDRESS",
                        &env.hub_safe_module_setup_address,
                    )?,
                    salt_nonce: alloy::primitives::U256::ZERO,
                })
            };
            let bundlers = parse_csv("HUB_BUNDLER_URLS", &env.hub_bundler_urls)?;
            let paymasters = parse_paymasters_json(&env.hub_paymasters_json)?;
            (
                entrypoint,
                safe,
                module,
                safe_deployment,
                bundlers,
                paymasters,
            )
        } else {
            (None, None, None, None, Vec::new(), Vec::new())
        };

    let tron_mode = parse_tron_mode(&env.tron_mode)?;
    if tron_mode == TronMode::Grpc {
        if env.tron_grpc_url.trim().is_empty() {
            anyhow::bail!("TRON_GRPC_URL must be set in TRON_MODE=grpc");
        }
        if env.tron_private_key_hex.trim().is_empty()
            && env.tron_private_keys_hex_csv.trim().is_empty()
        {
            anyhow::bail!(
                "TRON_PRIVATE_KEY_HEX or TRON_PRIVATE_KEYS_HEX_CSV must be set in TRON_MODE=grpc"
            );
        }
        if env.tron_controller_address.trim().is_empty() {
            anyhow::bail!("TRON_CONTROLLER_ADDRESS must be set in TRON_MODE=grpc");
        }
    } else if env.tron_mock_reader_address.trim().is_empty() {
        anyhow::bail!("TRON_MOCK_READER_ADDRESS must be set in TRON_MODE=mock");
    }

    let tron_private_keys = if tron_mode == TronMode::Grpc {
        let mut keys: Vec<[u8; 32]> = Vec::new();
        if !env.tron_private_key_hex.trim().is_empty() {
            keys.push(parse_hex_32(
                "TRON_PRIVATE_KEY_HEX",
                &env.tron_private_key_hex,
            )?);
        }
        if !env.tron_private_keys_hex_csv.trim().is_empty() {
            keys.extend(parse_hex_32_csv(
                "TRON_PRIVATE_KEYS_HEX_CSV",
                &env.tron_private_keys_hex_csv,
            )?);
        }
        // Dedup preserving order.
        let mut out: Vec<[u8; 32]> = Vec::new();
        for k in keys {
            if !out.contains(&k) {
                out.push(k);
            }
        }
        if out.is_empty() {
            anyhow::bail!("no Tron keys configured");
        }
        out
    } else {
        Vec::new()
    };

    Ok(AppConfig {
        indexer: IndexerConfig {
            base_url: env.indexer_api_base_url,
            timeout: Duration::from_secs(env.indexer_timeout_secs.max(1)),
            max_head_lag_blocks: env.indexer_max_head_lag_blocks.max(1),
        },
        hub: HubConfig {
            tx_mode: hub_tx_mode,
            rpc_url: env.hub_rpc_url,
            chain_id: env.hub_chain_id,
            pool: hub_pool,

            entrypoint: hub_entrypoint,
            safe: hub_safe,
            safe_4337_module: hub_module,
            safe_deployment: hub_safe_deployment,
            bundler_urls: bundlers,
            signer_private_key: hub_signer_private_key,
            paymasters,
        },
        tron: TronConfig {
            mode: tron_mode,
            grpc_url: env.tron_grpc_url,
            api_key: env.tron_api_key.filter(|s| !s.trim().is_empty()),
            private_key: if tron_mode == TronMode::Grpc {
                tron_private_keys
                    .first()
                    .copied()
                    .context("missing Tron private key")?
            } else {
                [0u8; 32]
            },
            private_keys: tron_private_keys,
            controller_address: env.tron_controller_address,
            mock_reader_address: parse_optional_address(
                "TRON_MOCK_READER_ADDRESS",
                &env.tron_mock_reader_address,
            )?,
            block_lag: env.tron_block_lag,
            fee_limit_cap_sun: env.tron_fee_limit_cap_sun.max(1_000_000),
            fee_limit_headroom_ppm: env.tron_fee_limit_headroom_ppm.min(1_000_000),
            stake_totals_cache_ttl_secs: env.tron_stake_totals_cache_ttl_secs.max(1),
            energy_rental_providers: parse_tron_energy_rental_apis_json(
                &env.tron_energy_rental_apis_json,
            )?,
            delegate_resource_resell_enabled: env.tron_delegate_resource_resell_enabled
                && !env.tron_energy_rental_apis_json.trim().is_empty(),
            rental_provider_fail_threshold: env.tron_rental_provider_fail_threshold.max(1),
            rental_provider_fail_window_secs: env.tron_rental_provider_fail_window_secs.max(1),
            rental_provider_freeze_secs: env.tron_rental_provider_freeze_secs.max(0),
            resell_energy_headroom_ppm: env.tron_resell_energy_headroom_ppm.min(1_000_000),
            emulation_enabled: env.solver_tron_emulation_enabled,
        },
        jobs: JobConfig {
            tick_interval: Duration::from_secs(env.solver_tick_interval_secs.max(1)),
            tron_finality_blocks: env.tron_finality_blocks,
            tip_proof_resend_blocks: env.tron_tip_proof_resend_blocks.max(1),
            process_controller_max_events: env.process_controller_max_events,
            fill_max_claims: env.fill_max_claims,
            max_in_flight_jobs: env
                .solver_max_in_flight_jobs
                .max(1)
                .min(env.fill_max_claims.max(1)),
            concurrency_trx_transfer: env.solver_concurrency_trx_transfer.max(1),
            concurrency_usdt_transfer: env.solver_concurrency_usdt_transfer.max(1),
            concurrency_delegate_resource: env.solver_concurrency_delegate_resource.max(1),
            concurrency_trigger_smart_contract: env
                .solver_concurrency_trigger_smart_contract
                .max(1),
            concurrency_tron_broadcast: env.solver_concurrency_tron_broadcast.max(1),
            consolidation_enabled: env.solver_consolidation_enabled,
            consolidation_max_pre_txs: env.solver_consolidation_max_pre_txs,
            consolidation_max_total_trx_pull_sun: env.solver_consolidation_max_total_trx_pull_sun,
            consolidation_max_per_tx_trx_pull_sun: env.solver_consolidation_max_per_tx_trx_pull_sun,
            consolidation_max_total_usdt_pull_amount: env
                .solver_consolidation_max_total_usdt_pull_amount,
            consolidation_max_per_tx_usdt_pull_amount: env
                .solver_consolidation_max_per_tx_usdt_pull_amount,
            rate_limit_claims_per_minute_global: env.solver_rate_limit_claims_per_minute_global,
            rate_limit_claims_per_minute_trx_transfer: env
                .solver_rate_limit_claims_per_minute_trx_transfer,
            rate_limit_claims_per_minute_usdt_transfer: env
                .solver_rate_limit_claims_per_minute_usdt_transfer,
            rate_limit_claims_per_minute_delegate_resource: env
                .solver_rate_limit_claims_per_minute_delegate_resource,
            rate_limit_claims_per_minute_trigger_smart_contract: env
                .solver_rate_limit_claims_per_minute_trigger_smart_contract,
            global_pause_fatal_threshold: env.solver_global_pause_fatal_threshold,
            global_pause_window_secs: env.solver_global_pause_window_secs.max(1),
            global_pause_duration_secs: env.solver_global_pause_duration_secs.max(1),
            breaker_mismatch_penalty: env.solver_breaker_mismatch_penalty.clamp(1, 100),
            delegate_reservation_ttl_secs: env.solver_delegate_reservation_ttl_secs.max(30),
            controller_rebalance_threshold_usdt: env.controller_rebalance_threshold_usdt,
            controller_rebalance_keep_usdt: env.controller_rebalance_keep_usdt,
            pull_liquidity_ppm: env.pull_liquidity_ppm.min(1_000_000),
        },
        policy: PolicyConfig {
            enabled_intent_types,
            min_deadline_slack_secs: env.solver_min_deadline_slack_secs,
            min_profit_usd: env.solver_min_profit_usd,
            hub_cost_usd: env.solver_hub_cost_usd,
            hub_cost_history_lookback: env.solver_hub_cost_history_lookback.max(1),
            hub_cost_headroom_ppm: env.solver_hub_cost_headroom_ppm.min(1_000_000),
            tron_fee_usd: env.solver_tron_fee_usd,
            tron_fee_history_lookback: env.solver_tron_fee_history_lookback.max(1),
            tron_fee_headroom_ppm: env.solver_tron_fee_headroom_ppm.min(1_000_000),
            capital_lock_ppm_per_day: env.solver_capital_lock_ppm_per_day.min(1_000_000),
            require_priced_escrow: env.solver_require_priced_escrow,
            allowed_escrow_tokens: parse_addresses_csv(
                "SOLVER_ALLOWED_ESCROW_TOKENS_CSV",
                &env.solver_allowed_escrow_tokens_csv,
            )?,

            trigger_contract_allowlist,
            trigger_contract_denylist: parse_addresses_csv(
                "SOLVER_TRIGGER_CONTRACT_DENYLIST_CSV",
                &env.solver_trigger_contract_denylist_csv,
            )?,
            trigger_selector_denylist: parse_selectors_csv(
                "SOLVER_TRIGGER_SELECTOR_DENYLIST_CSV",
                &env.solver_trigger_selector_denylist_csv,
            )?,
            trigger_allow_fallback_calls: env.solver_trigger_allow_fallback_calls,

            max_trx_transfer_sun: opt_u64(env.solver_max_trx_transfer_sun),
            max_usdt_transfer_amount: opt_u64(env.solver_max_usdt_transfer_amount),
            max_delegate_balance_sun: opt_u64(env.solver_max_delegate_balance_sun),
            max_delegate_lock_period_secs: opt_u64(env.solver_max_delegate_lock_period_secs),
            max_trigger_call_value_sun: opt_u64(env.solver_max_trigger_call_value_sun),
            max_trigger_calldata_len: opt_u64(env.solver_max_trigger_calldata_len),
        },
        pricing: crate::pricing::PricingConfig {
            trx_usd_override: env.solver_trx_usd_override,
            trx_usd_ttl: Duration::from_secs(env.solver_trx_usd_ttl_secs.max(1)),
            trx_usd_url: env.solver_trx_usd_url,
            eth_usd_override: env.solver_eth_usd_override,
            eth_usd_ttl: Duration::from_secs(env.solver_eth_usd_ttl_secs.max(1)),
            eth_usd_url: env.solver_eth_usd_url,
        },
        db_url: env.solver_db_url,
        instance_id: if env.solver_instance_id.trim().is_empty() {
            format!("solver:{}", std::process::id())
        } else {
            env.solver_instance_id
        },
    })
}
