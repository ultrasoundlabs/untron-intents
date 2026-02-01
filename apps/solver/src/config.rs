use aa::SafeDeterministicDeploymentConfig;
use alloy::primitives::Address;
use anyhow::{Context, Result};
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
pub struct TronConfig {
    pub mode: TronMode,
    pub grpc_url: String,
    pub api_key: Option<String>,
    pub private_key: [u8; 32],
    pub controller_address: String,
    pub mock_reader_address: Option<Address>,

    pub block_lag: u64,
    pub fee_limit_cap_sun: u64,
    /// Extra headroom on computed fee_limit (ppm, i.e. 100_000 = +10%).
    pub fee_limit_headroom_ppm: u64,
    /// Optional list of external energy rental providers.
    pub energy_rental_providers: Vec<JsonApiRentalProviderConfig>,
}

#[derive(Debug, Clone)]
pub struct JobConfig {
    pub tick_interval: Duration,
    pub tron_finality_blocks: u64,
    pub tip_proof_resend_blocks: u64,

    pub process_controller_max_events: u64,
    pub fill_max_claims: u64,

    pub controller_rebalance_threshold_usdt: String,
    pub controller_rebalance_keep_usdt: String,

    pub pull_liquidity_ppm: u64,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct Env {
    solver_db_url: String,

    indexer_api_base_url: String,

    indexer_timeout_secs: u64,

    indexer_max_head_lag_blocks: u64,

    hub_rpc_url: String,

    hub_chain_id: Option<u64>,

    /// Pool contract address (UntronIntents).
    hub_pool_address: String,
    /// Back-compat: older config used HUB_UNTRON_V3_ADDRESS for the pool.
    #[serde(default)]
    hub_untron_v3_address: String,

    #[serde(default)]
    hub_tx_mode: String,

    #[serde(default)]
    hub_entrypoint_address: String,

    #[serde(default)]
    hub_safe_address: String,

    #[serde(default)]
    hub_safe_4337_module_address: String,

    #[serde(default)]
    hub_safe_proxy_factory_address: String,

    #[serde(default)]
    hub_safe_singleton_address: String,

    #[serde(default)]
    hub_safe_module_setup_address: String,

    hub_signer_private_key_hex: String,

    #[serde(default)]
    hub_bundler_urls: String,

    #[serde(default)]
    hub_paymasters_json: String,

    #[serde(default)]
    tron_mode: String,

    tron_grpc_url: String,

    tron_api_key: Option<String>,

    tron_private_key_hex: String,

    tron_controller_address: String,

    #[serde(default)]
    tron_mock_reader_address: String,

    tron_block_lag: u64,

    #[serde(default)]
    tron_fee_limit_cap_sun: u64,

    #[serde(default)]
    tron_fee_limit_headroom_ppm: u64,

    #[serde(default)]
    tron_energy_rental_apis_json: String,

    solver_tick_interval_secs: u64,

    tron_finality_blocks: u64,

    tron_tip_proof_resend_blocks: u64,

    process_controller_max_events: u64,

    fill_max_claims: u64,

    controller_rebalance_threshold_usdt: String,

    controller_rebalance_keep_usdt: String,

    pull_liquidity_ppm: u64,

    #[serde(default)]
    solver_enabled_intent_types: String,

    #[serde(default)]
    solver_min_deadline_slack_secs: u64,

    #[serde(default)]
    solver_instance_id: String,
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
            tron_controller_address: String::new(),
            tron_mock_reader_address: String::new(),
            tron_block_lag: 0,
            tron_fee_limit_cap_sun: 200_000_000,
            tron_fee_limit_headroom_ppm: 100_000,
            tron_energy_rental_apis_json: String::new(),
            solver_tick_interval_secs: 5,
            tron_finality_blocks: 19,
            tron_tip_proof_resend_blocks: 20,
            process_controller_max_events: 100,
            fill_max_claims: 50,
            controller_rebalance_threshold_usdt: "0".to_string(),
            controller_rebalance_keep_usdt: "1".to_string(),
            pull_liquidity_ppm: 500_000,

            solver_enabled_intent_types: "trx_transfer,delegate_resource".to_string(),
            solver_min_deadline_slack_secs: 30,
            solver_instance_id: String::new(),
        }
    }
}

fn parse_address(label: &str, s: &str) -> Result<Address> {
    s.parse::<Address>()
        .with_context(|| format!("invalid {label}: {s}"))
}

fn parse_optional_address(label: &str, s: &str) -> Result<Option<Address>> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let addr = parse_address(label, trimmed)?;
    if addr == Address::ZERO {
        Ok(None)
    } else {
        Ok(Some(addr))
    }
}

fn parse_hex_32(label: &str, s: &str) -> Result<[u8; 32]> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    let bytes = hex::decode(s).with_context(|| format!("invalid hex for {label}"))?;
    if bytes.len() != 32 {
        anyhow::bail!("{label} must be 32 bytes (got {})", bytes.len());
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn parse_csv(label: &str, s: &str) -> Result<Vec<String>> {
    let urls = s
        .split(',')
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if urls.is_empty() {
        anyhow::bail!("{label} must be non-empty");
    }
    Ok(urls)
}

fn parse_paymasters_json(s: &str) -> Result<Vec<PaymasterServiceConfig>> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let mut v: Vec<PaymasterServiceConfig> =
        serde_json::from_str(trimmed).context("parse HUB_PAYMASTERS_JSON")?;
    for pm in &mut v {
        pm.url = pm.url.trim().to_string();
        if pm.url.is_empty() {
            anyhow::bail!("HUB_PAYMASTERS_JSON contains an empty url");
        }
        if !pm.context.is_object() {
            anyhow::bail!("HUB_PAYMASTERS_JSON paymaster.context must be a JSON object");
        }
    }
    Ok(v)
}

fn parse_tron_energy_rental_apis_json(s: &str) -> Result<Vec<JsonApiRentalProviderConfig>> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let mut v: Vec<JsonApiRentalProviderConfig> =
        serde_json::from_str(trimmed).context("parse TRON_ENERGY_RENTAL_APIS_JSON")?;
    for p in &mut v {
        p.name = p.name.trim().to_string();
        p.url = p.url.trim().to_string();
        if p.name.is_empty() {
            anyhow::bail!("TRON_ENERGY_RENTAL_APIS_JSON contains an empty provider name");
        }
        if p.url.is_empty() {
            anyhow::bail!("TRON_ENERGY_RENTAL_APIS_JSON contains an empty provider url");
        }
        if p.method.trim().is_empty() {
            p.method = "POST".to_string();
        }
    }
    Ok(v)
}

fn parse_hub_tx_mode(s: &str) -> Result<HubTxMode> {
    match s.trim().to_ascii_lowercase().as_str() {
        "" | "eoa" => Ok(HubTxMode::Eoa),
        "safe4337" | "safe_4337" | "aa" => Ok(HubTxMode::Safe4337),
        other => anyhow::bail!("unsupported HUB_TX_MODE: {other} (expected: eoa|safe4337)"),
    }
}

fn parse_tron_mode(s: &str) -> Result<TronMode> {
    match s.trim().to_ascii_lowercase().as_str() {
        "" | "grpc" => Ok(TronMode::Grpc),
        "mock" => Ok(TronMode::Mock),
        other => anyhow::bail!("unsupported TRON_MODE: {other} (expected: grpc|mock)"),
    }
}

fn parse_intent_types(s: &str) -> Result<Vec<crate::types::IntentType>> {
    if s.trim().is_empty() {
        return Ok(vec![
            crate::types::IntentType::TrxTransfer,
            crate::types::IntentType::DelegateResource,
        ]);
    }
    let mut out = Vec::new();
    for raw in s.split(',') {
        let v = raw.trim();
        if v.is_empty() {
            continue;
        }
        let ty = match v {
            "trigger_smart_contract" => crate::types::IntentType::TriggerSmartContract,
            "usdt_transfer" => crate::types::IntentType::UsdtTransfer,
            "trx_transfer" => crate::types::IntentType::TrxTransfer,
            "delegate_resource" => crate::types::IntentType::DelegateResource,
            other => anyhow::bail!("unknown intent type: {other}"),
        };
        if !out.contains(&ty) {
            out.push(ty);
        }
    }
    if out.is_empty() {
        anyhow::bail!("SOLVER_ENABLED_INTENT_TYPES must not be empty");
    }
    Ok(out)
}

pub fn load_config() -> Result<AppConfig> {
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
        if env.tron_private_key_hex.trim().is_empty() {
            anyhow::bail!("TRON_PRIVATE_KEY_HEX must be set in TRON_MODE=grpc");
        }
        if env.tron_controller_address.trim().is_empty() {
            anyhow::bail!("TRON_CONTROLLER_ADDRESS must be set in TRON_MODE=grpc");
        }
    } else if env.tron_mock_reader_address.trim().is_empty() {
        anyhow::bail!("TRON_MOCK_READER_ADDRESS must be set in TRON_MODE=mock");
    }

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
                parse_hex_32("TRON_PRIVATE_KEY_HEX", &env.tron_private_key_hex)?
            } else {
                [0u8; 32]
            },
            controller_address: env.tron_controller_address,
            mock_reader_address: parse_optional_address(
                "TRON_MOCK_READER_ADDRESS",
                &env.tron_mock_reader_address,
            )?,
            block_lag: env.tron_block_lag,
            fee_limit_cap_sun: env.tron_fee_limit_cap_sun.max(1_000_000),
            fee_limit_headroom_ppm: env.tron_fee_limit_headroom_ppm.min(1_000_000),
            energy_rental_providers: parse_tron_energy_rental_apis_json(
                &env.tron_energy_rental_apis_json,
            )?,
        },
        jobs: JobConfig {
            tick_interval: Duration::from_secs(env.solver_tick_interval_secs.max(1)),
            tron_finality_blocks: env.tron_finality_blocks,
            tip_proof_resend_blocks: env.tron_tip_proof_resend_blocks.max(1),
            process_controller_max_events: env.process_controller_max_events,
            fill_max_claims: env.fill_max_claims,
            controller_rebalance_threshold_usdt: env.controller_rebalance_threshold_usdt,
            controller_rebalance_keep_usdt: env.controller_rebalance_keep_usdt,
            pull_liquidity_ppm: env.pull_liquidity_ppm.min(1_000_000),
        },
        policy: PolicyConfig {
            enabled_intent_types,
            min_deadline_slack_secs: env.solver_min_deadline_slack_secs,
        },
        db_url: env.solver_db_url,
        instance_id: if env.solver_instance_id.trim().is_empty() {
            format!("solver:{}", std::process::id())
        } else {
            env.solver_instance_id
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::IntentType;
    use alloy::primitives::Address;

    #[test]
    fn parse_hex_32_accepts_0x_and_rejects_wrong_len() {
        let ok = format!("0x{}", "11".repeat(32));
        let out = parse_hex_32("K", &ok).unwrap();
        assert_eq!(out, [0x11u8; 32]);

        let err = parse_hex_32("K", "0x11").unwrap_err().to_string();
        assert!(err.contains("must be 32 bytes"));
    }

    #[test]
    fn parse_csv_trims_and_requires_non_empty() {
        let urls = parse_csv("U", " a, ,b ,, c ").unwrap();
        assert_eq!(
            urls,
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );

        let err = parse_csv("U", " , , ").unwrap_err().to_string();
        assert!(err.contains("must be non-empty"));
    }

    #[test]
    fn parse_paymasters_json_empty_ok() {
        assert!(parse_paymasters_json("   ").unwrap().is_empty());
    }

    #[test]
    fn parse_paymasters_json_validates_url_and_context_object() {
        let ok = r#"[{"url":" https://pm.example ","context":{}}]"#;
        let v = parse_paymasters_json(ok).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].url, "https://pm.example");
        assert!(v[0].context.is_object());

        let err = parse_paymasters_json(r#"[{"url":" ","context":{}}]"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("empty url"));

        let err = parse_paymasters_json(r#"[{"url":"x","context":123}]"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("must be a JSON object"));
    }

    #[test]
    fn parse_tron_energy_rental_apis_json_empty_ok() {
        assert!(
            parse_tron_energy_rental_apis_json("   ")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn parse_tron_energy_rental_apis_json_validates_name_and_url() {
        let ok = r#"[{"name":" p1 ","url":" https://r.example ","method":"POST","headers":{},"body":{},"response":{"success_pointer":"/ok"}}]"#;
        let v = parse_tron_energy_rental_apis_json(ok).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].name, "p1");
        assert_eq!(v[0].url, "https://r.example");

        let err = parse_tron_energy_rental_apis_json(
            r#"[{"name":" ","url":"x","method":"POST","headers":{},"body":{},"response":{"success_pointer":"/ok"}}]"#,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("empty provider name"));

        let err = parse_tron_energy_rental_apis_json(
            r#"[{"name":"x","url":" ","method":"POST","headers":{},"body":{},"response":{"success_pointer":"/ok"}}]"#,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("empty provider url"));
    }

    #[test]
    fn parse_address_accepts_valid_and_rejects_invalid() {
        let a = parse_address("A", "0x0000000000000000000000000000000000000001").unwrap();
        let expected: Address = "0x0000000000000000000000000000000000000001"
            .parse()
            .unwrap();
        assert_eq!(a, expected);

        assert!(parse_address("A", "not an address").is_err());
    }

    #[test]
    fn parse_intent_types_dedups_and_preserves_order() {
        let got = parse_intent_types("trx_transfer,delegate_resource,trx_transfer").unwrap();
        assert_eq!(
            got,
            vec![IntentType::TrxTransfer, IntentType::DelegateResource]
        );
    }

    #[test]
    fn parse_intent_types_rejects_unknown() {
        let err = parse_intent_types("nope").unwrap_err().to_string();
        assert!(err.contains("unknown intent type"));
    }
}
