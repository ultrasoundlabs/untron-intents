use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RentalResourceKind {
    Energy,
    Bandwidth,
}

#[derive(Debug, Clone)]
pub struct RentalContext {
    pub resource: RentalResourceKind,
    pub amount: u64,

    /// Tron address in base58check (T...).
    pub address_base58check: String,
    /// Tron address bytes in hex ("41" + 20 bytes), 0x-prefixed.
    pub address_hex41: String,
    /// EVM address (20 bytes), 0x-prefixed.
    pub address_evm_hex: String,

    /// Optional txid for correlation (0x-prefixed 32-byte hex).
    pub txid: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JsonApiRentalProviderConfig {
    pub name: String,
    pub url: String,
    #[serde(default = "default_method")]
    pub method: String, // "POST" or "GET"
    #[serde(default)]
    pub headers: BTreeMap<String, String>,

    /// JSON body template. Any string leaf may contain `{{placeholders}}`.
    pub body: Value,

    pub response: JsonApiResponseMapping,
}

fn default_method() -> String {
    "POST".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct JsonApiResponseMapping {
    /// JSON pointer to a truthy success flag (bool/number/string).
    pub success_pointer: String,
    /// Optional exact-match requirement for `success_pointer`.
    /// If present, success is determined by equality with this value (with light normalization).
    /// Otherwise, the value at `success_pointer` is interpreted as truthy.
    #[serde(default)]
    pub success_equals: Option<Value>,
    /// Optional JSON pointer to an order id / request id.
    #[serde(default)]
    pub order_id_pointer: Option<String>,
    /// Optional JSON pointer to an error message.
    #[serde(default)]
    pub error_pointer: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RentalAttempt {
    pub provider: String,
    pub ok: bool,
    pub order_id: Option<String>,
    pub response_json: Option<Value>,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct JsonApiRentalProvider {
    cfg: JsonApiRentalProviderConfig,
    client: reqwest::Client,
}

impl JsonApiRentalProvider {
    pub fn new(cfg: JsonApiRentalProviderConfig) -> Self {
        Self {
            cfg,
            client: reqwest::Client::new(),
        }
    }

    pub fn name(&self) -> &str {
        &self.cfg.name
    }

    pub async fn rent(&self, ctx: &RentalContext) -> Result<RentalAttempt> {
        let mut body = self.cfg.body.clone();
        render_in_place(&mut body, ctx);

        let url = render_str(&self.cfg.url, ctx);
        let mut req = match self.cfg.method.to_uppercase().as_str() {
            "POST" => self.client.post(url),
            "GET" => self.client.get(url),
            other => anyhow::bail!("unsupported rental provider method: {other}"),
        };

        for (k, v) in &self.cfg.headers {
            req = req.header(k, render_str(v, ctx));
        }

        // Keep it simple: JSON body for POST. GET bodies are ignored.
        if self.cfg.method.to_uppercase() == "POST" {
            req = req.json(&body);
        }

        let resp = req.send().await.context("rental provider http")?;
        let status = resp.status();
        let text = resp.text().await.context("read rental response body")?;
        Ok(interpret_json_response(&self.cfg, status.as_u16(), &text))
    }
}

fn interpret_json_response(
    cfg: &JsonApiRentalProviderConfig,
    status_code: u16,
    body: &str,
) -> RentalAttempt {
    let parsed: Option<Value> = serde_json::from_str(body).ok();

    if !(200..=299).contains(&status_code) {
        return RentalAttempt {
            provider: cfg.name.clone(),
            ok: false,
            order_id: None,
            response_json: parsed,
            error: Some(format!("http status {status_code}")),
        };
    }

    let Some(json) = parsed.clone() else {
        return RentalAttempt {
            provider: cfg.name.clone(),
            ok: false,
            order_id: None,
            response_json: None,
            error: Some("response was not valid JSON".to_string()),
        };
    };

    let ok_val = json
        .pointer(&cfg.response.success_pointer)
        .cloned()
        .unwrap_or(Value::Null);
    let ok = if let Some(expected) = &cfg.response.success_equals {
        is_equalish(&ok_val, expected)
    } else {
        is_truthy(&ok_val)
    };

    let order_id = cfg
        .response
        .order_id_pointer
        .as_ref()
        .and_then(|p| json.pointer(p))
        .and_then(value_to_string);

    let error = if ok {
        None
    } else {
        cfg.response
            .error_pointer
            .as_ref()
            .and_then(|p| json.pointer(p))
            .and_then(value_to_string)
    };

    RentalAttempt {
        provider: cfg.name.clone(),
        ok,
        order_id,
        response_json: Some(json),
        error,
    }
}

fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_i64().unwrap_or(0) != 0,
        Value::String(s) => {
            let t = s.trim().to_ascii_lowercase();
            matches!(t.as_str(), "true" | "1" | "ok" | "success" | "yes")
        }
        _ => false,
    }
}

fn is_equalish(actual: &Value, expected: &Value) -> bool {
    if actual == expected {
        return true;
    }

    match (actual, expected) {
        (Value::Number(a), Value::String(e)) => e.trim() == a.to_string(),
        (Value::String(a), Value::Number(e)) => a.trim() == e.to_string(),
        (Value::Bool(a), Value::String(e)) => e.trim().eq_ignore_ascii_case(&a.to_string()),
        (Value::String(a), Value::Bool(e)) => a.trim().eq_ignore_ascii_case(&e.to_string()),
        _ => false,
    }
}

fn value_to_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn render_in_place(v: &mut Value, ctx: &RentalContext) {
    match v {
        Value::String(s) => {
            *s = render_str(s, ctx);
        }
        Value::Array(a) => {
            for x in a {
                render_in_place(x, ctx);
            }
        }
        Value::Object(m) => {
            for (_, x) in m.iter_mut() {
                render_in_place(x, ctx);
            }
        }
        _ => {}
    }
}

fn render_str(s: &str, ctx: &RentalContext) -> String {
    let mut out = s.to_string();
    out = out.replace(
        "{{resource_kind}}",
        match ctx.resource {
            RentalResourceKind::Energy => "energy",
            RentalResourceKind::Bandwidth => "bandwidth",
        },
    );
    out = out.replace("{{amount}}", &ctx.amount.to_string());
    out = out.replace("{{address_base58check}}", &ctx.address_base58check);
    out = out.replace("{{address_hex41}}", &ctx.address_hex41);
    out = out.replace("{{address_evm_hex}}", &ctx.address_evm_hex);
    out = out.replace("{{txid}}", ctx.txid.as_deref().unwrap_or(""));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_substitution_works_recursively() {
        let ctx = RentalContext {
            resource: RentalResourceKind::Energy,
            amount: 123,
            address_base58check: "T...".to_string(),
            address_hex41: "0x41abcd".to_string(),
            address_evm_hex: "0xabcd".to_string(),
            txid: Some("0x11".to_string()),
        };

        let mut v = serde_json::json!({
            "kind": "{{resource_kind}}",
            "amount": "{{amount}}",
            "nested": ["{{address_base58check}}", {"tx":"{{txid}}"}]
        });

        render_in_place(&mut v, &ctx);
        assert_eq!(v["kind"], "energy");
        assert_eq!(v["amount"], "123");
        assert_eq!(v["nested"][0], "T...");
        assert_eq!(v["nested"][1]["tx"], "0x11");
    }

    #[test]
    fn interpret_json_response_success_pointer_controls_ok() {
        let cfg = JsonApiRentalProviderConfig {
            name: "p1".to_string(),
            url: "http://example".to_string(),
            method: "POST".to_string(),
            headers: BTreeMap::new(),
            body: serde_json::json!({}),
            response: JsonApiResponseMapping {
                success_pointer: "/success".to_string(),
                success_equals: None,
                order_id_pointer: Some("/data/orderId".to_string()),
                error_pointer: Some("/error".to_string()),
            },
        };

        let res =
            interpret_json_response(&cfg, 200, r#"{"success":true,"data":{"orderId":"abc"}}"#);
        assert!(res.ok);
        assert_eq!(res.order_id.as_deref(), Some("abc"));
    }

    #[test]
    fn interpret_json_response_success_equals_controls_ok() {
        let cfg = JsonApiRentalProviderConfig {
            name: "p1".to_string(),
            url: "http://example".to_string(),
            method: "POST".to_string(),
            headers: BTreeMap::new(),
            body: serde_json::json!({}),
            response: JsonApiResponseMapping {
                success_pointer: "/code".to_string(),
                success_equals: Some(serde_json::json!(200)),
                order_id_pointer: None,
                error_pointer: Some("/message".to_string()),
            },
        };

        let res = interpret_json_response(&cfg, 200, r#"{"code":200,"message":"ok"}"#);
        assert!(res.ok);

        let res = interpret_json_response(&cfg, 200, r#"{"code":500,"message":"nope"}"#);
        assert!(!res.ok);
        assert_eq!(res.error.as_deref(), Some("nope"));
    }

    #[test]
    fn interpret_json_response_false_success_extracts_error() {
        let cfg = JsonApiRentalProviderConfig {
            name: "p1".to_string(),
            url: "http://example".to_string(),
            method: "POST".to_string(),
            headers: BTreeMap::new(),
            body: serde_json::json!({}),
            response: JsonApiResponseMapping {
                success_pointer: "/ok".to_string(),
                success_equals: None,
                order_id_pointer: None,
                error_pointer: Some("/error/message".to_string()),
            },
        };

        let res =
            interpret_json_response(&cfg, 200, r#"{"ok":0,"error":{"message":"no liquidity"}}"#);
        assert!(!res.ok);
        assert_eq!(res.error.as_deref(), Some("no liquidity"));
    }

    #[test]
    fn interpret_json_response_non_json_is_failure() {
        let cfg = JsonApiRentalProviderConfig {
            name: "p1".to_string(),
            url: "http://example".to_string(),
            method: "POST".to_string(),
            headers: BTreeMap::new(),
            body: serde_json::json!({}),
            response: JsonApiResponseMapping {
                success_pointer: "/success".to_string(),
                success_equals: None,
                order_id_pointer: None,
                error_pointer: None,
            },
        };

        let res = interpret_json_response(&cfg, 200, "not json");
        assert!(!res.ok);
        assert_eq!(res.error.as_deref(), Some("response was not valid JSON"));
    }

    #[test]
    fn interpret_json_response_non_2xx_is_failure() {
        let cfg = JsonApiRentalProviderConfig {
            name: "p1".to_string(),
            url: "http://example".to_string(),
            method: "POST".to_string(),
            headers: BTreeMap::new(),
            body: serde_json::json!({}),
            response: JsonApiResponseMapping {
                success_pointer: "/success".to_string(),
                success_equals: None,
                order_id_pointer: None,
                error_pointer: None,
            },
        };

        let res = interpret_json_response(&cfg, 503, r#"{"success":true}"#);
        assert!(!res.ok);
        assert_eq!(res.error.as_deref(), Some("http status 503"));
        assert!(res.response_json.is_some());
    }
}
