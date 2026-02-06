use super::{HubTxMode, PaymasterServiceConfig, TronMode};
use alloy::primitives::Address;
use anyhow::{Context, Result};
use tron::JsonApiRentalProviderConfig;

pub(super) fn parse_address(label: &str, s: &str) -> Result<Address> {
    s.parse::<Address>()
        .with_context(|| format!("invalid {label}: {s}"))
}

pub(super) fn parse_optional_address(label: &str, s: &str) -> Result<Option<Address>> {
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

pub(super) fn parse_hex_32(label: &str, s: &str) -> Result<[u8; 32]> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    let bytes = hex::decode(s).with_context(|| format!("invalid hex for {label}"))?;
    if bytes.len() != 32 {
        anyhow::bail!("{label} must be 32 bytes (got {})", bytes.len());
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

pub(super) fn parse_csv(label: &str, s: &str) -> Result<Vec<String>> {
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

pub(super) fn parse_addresses_csv(label: &str, s: &str) -> Result<Vec<Address>> {
    let mut out = Vec::new();
    for raw in s.split(',') {
        let v = raw.trim();
        if v.is_empty() {
            continue;
        }
        out.push(parse_address(label, v)?);
    }
    Ok(out)
}

pub(super) fn parse_selectors_csv(label: &str, s: &str) -> Result<Vec<[u8; 4]>> {
    let mut out = Vec::new();
    for raw in s.split(',') {
        let v = raw.trim();
        if v.is_empty() {
            continue;
        }
        let v = v.strip_prefix("0x").unwrap_or(v);
        let bytes =
            hex::decode(v).with_context(|| format!("invalid selector hex in {label}: {v}"))?;
        if bytes.len() != 4 {
            anyhow::bail!("{label} entries must be 4 bytes (got {})", bytes.len());
        }
        let mut sel = [0u8; 4];
        sel.copy_from_slice(&bytes);
        out.push(sel);
    }
    Ok(out)
}

pub(super) fn opt_u64(v: u64) -> Option<u64> {
    if v == 0 { None } else { Some(v) }
}

pub(super) fn parse_paymasters_json(s: &str) -> Result<Vec<PaymasterServiceConfig>> {
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

pub(super) fn parse_tron_energy_rental_apis_json(
    s: &str,
) -> Result<Vec<JsonApiRentalProviderConfig>> {
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

pub(super) fn parse_hub_tx_mode(s: &str) -> Result<HubTxMode> {
    match s.trim().to_ascii_lowercase().as_str() {
        "" | "eoa" => Ok(HubTxMode::Eoa),
        "safe4337" | "safe_4337" | "aa" => Ok(HubTxMode::Safe4337),
        other => anyhow::bail!("unsupported HUB_TX_MODE: {other} (expected: eoa|safe4337)"),
    }
}

pub(super) fn parse_tron_mode(s: &str) -> Result<TronMode> {
    match s.trim().to_ascii_lowercase().as_str() {
        "" | "grpc" => Ok(TronMode::Grpc),
        "mock" => Ok(TronMode::Mock),
        other => anyhow::bail!("unsupported TRON_MODE: {other} (expected: grpc|mock)"),
    }
}

pub(super) fn parse_intent_types(s: &str) -> Result<Vec<crate::types::IntentType>> {
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

pub(super) fn parse_hex_32_csv(label: &str, s: &str) -> Result<Vec<[u8; 32]>> {
    let items = parse_csv(label, s)?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(parse_hex_32(label, &item)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::IntentType;

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
