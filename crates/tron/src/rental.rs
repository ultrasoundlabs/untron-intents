use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RentalResourceKind {
    Energy,
    Bandwidth,
    TronPower,
}

#[derive(Debug, Clone)]
pub struct RentalContext {
    pub resource: RentalResourceKind,
    /// Resource quantity in provider units (e.g. energy units / bandwidth units).
    pub amount: u64,
    /// Optional lock period (Tron blocks) for DelegateResource-like rentals.
    pub lock_period: Option<u64>,
    /// Optional rental duration (hours) for APIs that take durations in human time units.
    pub duration_hours: Option<u64>,
    /// Optional delegated TRX amount in sun (protocol-level `DelegateResourceContract.balance`).
    pub balance_sun: Option<u64>,

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

    /// Optional quote endpoint for profitability gating / provider selection.
    #[serde(default)]
    pub quote: Option<JsonApiQuoteConfig>,
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
    /// Optional JSON pointer to a Tron transaction id / hash (0x-prefixed 32-byte hex).
    #[serde(default)]
    pub txid_pointer: Option<String>,
    /// Optional JSON pointer to an error message.
    #[serde(default)]
    pub error_pointer: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JsonApiQuoteConfig {
    pub url: String,
    #[serde(default = "default_method")]
    pub method: String, // "POST" or "GET"
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// JSON body template. Any string leaf may contain `{{placeholders}}`.
    #[serde(default)]
    pub body: Value,
    pub response: JsonApiQuoteResponseMapping,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JsonApiQuoteResponseMapping {
    /// JSON pointer to a truthy success flag (bool/number/string).
    pub success_pointer: String,
    /// Optional exact-match requirement for `success_pointer`.
    #[serde(default)]
    pub success_equals: Option<Value>,
    /// JSON pointer to total cost (TRX or SUN) for this quote.
    ///
    /// This pointer may contain `{{placeholders}}` (e.g. `/data/{{duration_hours}}`).
    #[serde(default)]
    pub cost_pointer: Option<String>,
    /// Cost unit: "trx" or "sun".
    #[serde(default = "default_cost_unit")]
    pub cost_unit: String,
    /// Optional JSON pointer to an error message.
    #[serde(default)]
    pub error_pointer: Option<String>,
    /// Optional bucket selection for providers that return multiple pricing tiers in one response.
    #[serde(default)]
    pub buckets: Option<JsonApiQuoteBuckets>,
}

fn default_cost_unit() -> String {
    "trx".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct JsonApiQuoteBuckets {
    /// JSON pointer to an array of periods.
    pub periods_pointer: String,
    /// JSON pointer (within each period object) to the active flag.
    pub period_active_pointer: String,
    /// JSON pointer (within the selected period object) to the prices object.
    pub period_prices_pointer: String,

    pub lt_threshold: u64,
    pub lt_pointer: String,
    pub eq_value: u64,
    pub eq_pointer: String,
    pub gt_pointer: String,
}

#[derive(Debug, Clone)]
pub struct RentalAttempt {
    pub provider: String,
    pub ok: bool,
    pub order_id: Option<String>,
    pub txid: Option<String>,
    pub response_json: Option<Value>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct QuoteAttempt {
    pub provider: String,
    pub ok: bool,
    pub cost_trx: Option<f64>,
    pub response_json: Option<Value>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RenderedJsonApiRequest {
    pub url: String,
    pub method: String,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
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
        let (_req, attempt) = self.rent_with_rendered_request(ctx).await?;
        Ok(attempt)
    }

    pub async fn rent_with_rendered_request(
        &self,
        ctx: &RentalContext,
    ) -> Result<(RenderedJsonApiRequest, RentalAttempt)> {
        let rendered = self.render_request(ctx);

        let mut req = match rendered.method.to_uppercase().as_str() {
            "POST" => self.client.post(rendered.url.clone()),
            "GET" => self.client.get(rendered.url.clone()),
            other => anyhow::bail!("unsupported rental provider method: {other}"),
        };

        for (k, v) in &rendered.headers {
            req = req.header(k, v);
        }

        // Keep it simple: JSON body for POST. GET bodies are ignored.
        if rendered.method.to_uppercase() == "POST" {
            req = req.json(&rendered.body);
        }

        let resp = req.send().await.context("rental provider http")?;
        let status = resp.status();
        let text = resp.text().await.context("read rental response body")?;
        let attempt = interpret_json_response(&self.cfg, status.as_u16(), &text);
        Ok((rendered, attempt))
    }

    pub async fn quote_with_rendered_request(
        &self,
        ctx: &RentalContext,
    ) -> Result<(RenderedJsonApiRequest, QuoteAttempt)> {
        let Some(cfg) = &self.cfg.quote else {
            anyhow::bail!("rental provider has no quote config");
        };
        let rendered = render_quote_request(cfg, ctx);

        let mut req = match rendered.method.to_uppercase().as_str() {
            "POST" => self.client.post(rendered.url.clone()),
            "GET" => self.client.get(rendered.url.clone()),
            other => anyhow::bail!("unsupported rental provider method: {other}"),
        };

        for (k, v) in &rendered.headers {
            req = req.header(k, v);
        }

        if rendered.method.to_uppercase() == "POST" {
            req = req.json(&rendered.body);
        }

        let resp = req.send().await.context("rental provider quote http")?;
        let status = resp.status();
        let text = resp.text().await.context("read quote response body")?;
        let attempt = interpret_quote_response(&self.cfg.name, cfg, ctx, status.as_u16(), &text);
        Ok((rendered, attempt))
    }

    fn render_request(&self, ctx: &RentalContext) -> RenderedJsonApiRequest {
        let mut body = self.cfg.body.clone();
        render_in_place(&mut body, ctx);

        let url = render_str(&self.cfg.url, ctx);
        let mut headers = BTreeMap::new();
        for (k, v) in &self.cfg.headers {
            headers.insert(k.clone(), render_str(v, ctx));
        }

        RenderedJsonApiRequest {
            url,
            method: self.cfg.method.clone(),
            headers,
            body,
        }
    }
}

fn render_quote_request(cfg: &JsonApiQuoteConfig, ctx: &RentalContext) -> RenderedJsonApiRequest {
    let mut body = cfg.body.clone();
    render_in_place(&mut body, ctx);

    let url = render_str(&cfg.url, ctx);
    let mut headers = BTreeMap::new();
    for (k, v) in &cfg.headers {
        headers.insert(k.clone(), render_str(v, ctx));
    }

    RenderedJsonApiRequest {
        url,
        method: cfg.method.clone(),
        headers,
        body,
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
            txid: None,
            response_json: parsed,
            error: Some(format!("http status {status_code}")),
        };
    }

    let Some(json) = parsed.clone() else {
        return RentalAttempt {
            provider: cfg.name.clone(),
            ok: false,
            order_id: None,
            txid: None,
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

    let txid = cfg
        .response
        .txid_pointer
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
        txid,
        response_json: Some(json),
        error,
    }
}

fn interpret_quote_response(
    provider_name: &str,
    cfg: &JsonApiQuoteConfig,
    ctx: &RentalContext,
    status_code: u16,
    body: &str,
) -> QuoteAttempt {
    let parsed: Option<Value> = serde_json::from_str(body).ok();

    if !(200..=299).contains(&status_code) {
        return QuoteAttempt {
            provider: provider_name.to_string(),
            ok: false,
            cost_trx: None,
            response_json: parsed,
            error: Some(format!("http status {status_code}")),
        };
    }

    let Some(json) = parsed.clone() else {
        return QuoteAttempt {
            provider: provider_name.to_string(),
            ok: false,
            cost_trx: None,
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

    let error = if ok {
        None
    } else {
        cfg.response
            .error_pointer
            .as_ref()
            .and_then(|p| json.pointer(p))
            .and_then(value_to_string)
    };

    let cost_val = if let Some(b) = &cfg.response.buckets {
        extract_bucketed_cost_value(&json, ctx, b)
    } else {
        cfg.response
            .cost_pointer
            .as_ref()
            .map(|p| render_str(p, ctx))
            .and_then(|p| json.pointer(&p).cloned())
    };

    let cost_trx = if ok {
        cost_val
            .as_ref()
            .and_then(value_to_string)
            .and_then(|s| s.parse::<f64>().ok())
            .map(
                |v| match cfg.response.cost_unit.trim().to_ascii_lowercase().as_str() {
                    "sun" => v / 1e6,
                    _ => v,
                },
            )
    } else {
        None
    };

    QuoteAttempt {
        provider: provider_name.to_string(),
        ok,
        cost_trx,
        response_json: Some(json),
        error,
    }
}

fn extract_bucketed_cost_value(
    root: &Value,
    ctx: &RentalContext,
    b: &JsonApiQuoteBuckets,
) -> Option<Value> {
    let periods = root.pointer(&b.periods_pointer)?;
    let arr = periods.as_array()?;
    let period = arr.iter().find(|p| {
        p.pointer(&b.period_active_pointer)
            .map(is_truthy)
            .unwrap_or(false)
    })?;
    let prices = period.pointer(&b.period_prices_pointer)?;

    let ptr = if ctx.amount == b.eq_value {
        &b.eq_pointer
    } else if ctx.amount < b.lt_threshold {
        &b.lt_pointer
    } else {
        &b.gt_pointer
    };
    prices.pointer(ptr).cloned()
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
            RentalResourceKind::TronPower => "tron_power",
        },
    );
    out = out.replace("{{amount}}", &ctx.amount.to_string());
    out = out.replace(
        "{{balance_sun}}",
        &ctx.balance_sun.map(|v| v.to_string()).unwrap_or_default(),
    );
    out = out.replace(
        "{{lock_period}}",
        &ctx.lock_period.map(|v| v.to_string()).unwrap_or_default(),
    );
    out = out.replace(
        "{{duration_hours}}",
        &ctx.duration_hours
            .map(|v| v.to_string())
            .unwrap_or_default(),
    );
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
            lock_period: Some(10),
            duration_hours: Some(1),
            balance_sun: Some(456),
            address_base58check: "T...".to_string(),
            address_hex41: "0x41abcd".to_string(),
            address_evm_hex: "0xabcd".to_string(),
            txid: Some("0x11".to_string()),
        };

        let mut v = serde_json::json!({
            "kind": "{{resource_kind}}",
            "amount": "{{amount}}",
            "balance": "{{balance_sun}}",
            "lock": "{{lock_period}}",
            "hours": "{{duration_hours}}",
            "nested": ["{{address_base58check}}", {"tx":"{{txid}}"}]
        });

        render_in_place(&mut v, &ctx);
        assert_eq!(v["kind"], "energy");
        assert_eq!(v["amount"], "123");
        assert_eq!(v["balance"], "456");
        assert_eq!(v["lock"], "10");
        assert_eq!(v["hours"], "1");
        assert_eq!(v["nested"][0], "T...");
        assert_eq!(v["nested"][1]["tx"], "0x11");
    }

    #[test]
    fn interpret_quote_response_pointer_cost_trx() {
        let cfg = JsonApiQuoteConfig {
            url: "http://example".to_string(),
            method: "GET".to_string(),
            headers: BTreeMap::new(),
            body: serde_json::json!({}),
            response: JsonApiQuoteResponseMapping {
                success_pointer: "/ok".to_string(),
                success_equals: None,
                cost_pointer: Some("/data/{{duration_hours}}".to_string()),
                cost_unit: "trx".to_string(),
                error_pointer: Some("/error".to_string()),
                buckets: None,
            },
        };
        let ctx = RentalContext {
            resource: RentalResourceKind::Energy,
            amount: 131000,
            lock_period: Some(10),
            duration_hours: Some(1),
            balance_sun: Some(1_000_000),
            address_base58check: "T...".to_string(),
            address_hex41: "0x41abcd".to_string(),
            address_evm_hex: "0xabcd".to_string(),
            txid: None,
        };

        let attempt = interpret_quote_response(
            "apitrx",
            &cfg,
            &ctx,
            200,
            r#"{"ok":true,"data":{"1":2.25}}"#,
        );
        assert!(attempt.ok);
        assert_eq!(attempt.cost_trx, Some(2.25));
    }

    #[test]
    fn interpret_quote_response_bucketed_cost_sun() {
        let cfg = JsonApiQuoteConfig {
            url: "http://example".to_string(),
            method: "GET".to_string(),
            headers: BTreeMap::new(),
            body: serde_json::json!({}),
            response: JsonApiQuoteResponseMapping {
                success_pointer: "/status".to_string(),
                success_equals: Some(serde_json::Value::String("OK".to_string())),
                cost_pointer: None,
                cost_unit: "sun".to_string(),
                error_pointer: Some("/message".to_string()),
                buckets: Some(JsonApiQuoteBuckets {
                    periods_pointer: "/periods".to_string(),
                    period_active_pointer: "/is_active".to_string(),
                    period_prices_pointer: "/prices".to_string(),
                    lt_threshold: 200000,
                    lt_pointer: "/less_than_200k/price_sun".to_string(),
                    eq_value: 131000,
                    eq_pointer: "/equal_131k/price_sun".to_string(),
                    gt_pointer: "/more_than_200k/price_sun".to_string(),
                }),
            },
        };
        let ctx = RentalContext {
            resource: RentalResourceKind::Energy,
            amount: 131000,
            lock_period: Some(10),
            duration_hours: Some(1),
            balance_sun: Some(1_000_000),
            address_base58check: "T...".to_string(),
            address_hex41: "0x41abcd".to_string(),
            address_evm_hex: "0xabcd".to_string(),
            txid: None,
        };

        let body = r#"{
          "status": "OK",
          "periods": [
            {
              "label": "off",
              "is_active": false,
              "prices": { "equal_131k": { "price_sun": 1000000 } }
            },
            {
              "label": "on",
              "is_active": true,
              "prices": {
                "less_than_200k": { "price_sun": 2500000 },
                "equal_131k": { "price_sun": 2250000 },
                "more_than_200k": { "price_sun": 4000000 }
              }
            }
          ]
        }"#;

        let attempt = interpret_quote_response("netts", &cfg, &ctx, 200, body);
        assert!(attempt.ok);
        // 2.25 TRX in sun.
        assert!((attempt.cost_trx.unwrap_or(0.0) - 2.25).abs() < 1e-9);
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
                txid_pointer: Some("/data/txid".to_string()),
                error_pointer: Some("/error".to_string()),
            },
            quote: None,
        };

        let res = interpret_json_response(
            &cfg,
            200,
            r#"{"success":true,"data":{"orderId":"abc","txid":"0x11"}}"#,
        );
        assert!(res.ok);
        assert_eq!(res.order_id.as_deref(), Some("abc"));
        assert_eq!(res.txid.as_deref(), Some("0x11"));
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
                txid_pointer: None,
                error_pointer: Some("/message".to_string()),
            },
            quote: None,
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
                txid_pointer: None,
                error_pointer: Some("/error/message".to_string()),
            },
            quote: None,
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
                txid_pointer: None,
                error_pointer: None,
            },
            quote: None,
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
                txid_pointer: None,
                error_pointer: None,
            },
            quote: None,
        };

        let res = interpret_json_response(&cfg, 503, r#"{"success":true}"#);
        assert!(!res.ok);
        assert_eq!(res.error.as_deref(), Some("http status 503"));
        assert!(res.response_json.is_some());
    }
}
