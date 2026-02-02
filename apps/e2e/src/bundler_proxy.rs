use anyhow::{Context, Result};
use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use serde_json::{Value, json};
use std::{net::SocketAddr, sync::Arc};
use tokio::task::JoinHandle;

#[derive(Clone)]
struct ProxyState {
    upstream_url: String,
    client: reqwest::Client,
    /// If true, `eth_getUserOperationReceipt` always returns `result=null` to force the solver's
    /// EntryPoint log fallback.
    drop_receipts: bool,
}

pub struct BundlerProxy {
    pub base_url: String,
    handle: JoinHandle<()>,
}

impl BundlerProxy {
    pub async fn start(upstream_url: String, drop_receipts: bool) -> Result<Self> {
        let state = ProxyState {
            upstream_url,
            client: reqwest::Client::new(),
            drop_receipts,
        };
        let state = Arc::new(state);

        let app = Router::new().route("/", post(handle)).with_state(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .context("bind bundler proxy")?;
        let addr: SocketAddr = listener.local_addr().context("bundler proxy local_addr")?;

        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });

        Ok(Self {
            base_url: format!("http://{addr}"),
            handle,
        })
    }
}

impl Drop for BundlerProxy {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn handle(
    State(state): State<Arc<ProxyState>>,
    Json(mut req): Json<Value>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");
    if state.drop_receipts && method == "eth_getUserOperationReceipt" {
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        let resp = json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": null
        });
        return Ok(Json(resp));
    }

    // Forward everything else to the real bundler.
    // Preserve `id` to keep clients happy.
    if req.get("jsonrpc").is_none() {
        req["jsonrpc"] = Value::String("2.0".to_string());
    }

    let resp = state
        .client
        .post(&state.upstream_url)
        .json(&req)
        .send()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("upstream request failed: {e:#}"),
            )
        })?;

    let status = resp.status();
    let val: Value = resp.json().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            format!("decode upstream json failed: {e:#}"),
        )
    })?;

    if !status.is_success() {
        return Err((
            StatusCode::BAD_GATEWAY,
            format!("upstream non-200: {status} body={val}"),
        ));
    }
    Ok(Json(val))
}
