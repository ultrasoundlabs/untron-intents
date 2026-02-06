use crate::http::{http_get_json, wait_for_http_ok};
use anyhow::{Context, Result};
use prost::Message;
use sha2::{Digest, Sha256};
use std::time::Duration;

pub fn collect_hex_32_strings(v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::String(s) => {
            let t = s.trim_start_matches("0x");
            if t.len() == 64 && t.chars().all(|c| c.is_ascii_hexdigit()) {
                out.push(format!("0x{}", t));
            }
        }
        serde_json::Value::Array(a) => {
            for x in a {
                collect_hex_32_strings(x, out);
            }
        }
        serde_json::Value::Object(m) => {
            for (_, x) in m {
                collect_hex_32_strings(x, out);
            }
        }
        _ => {}
    }
}

pub async fn wait_for_tronbox_accounts(
    tron_http_base: &str,
    timeout: Duration,
) -> Result<Vec<String>> {
    let start = std::time::Instant::now();
    loop {
        let v = http_get_json(&format!("{tron_http_base}/admin/accounts-json"))
            .await
            .context("GET /admin/accounts-json")?;

        let mut out = Vec::new();
        collect_hex_32_strings(&v, &mut out);
        out.sort();
        out.dedup();

        if out.len() >= 2 {
            return Ok(out);
        }

        if start.elapsed() > timeout {
            anyhow::bail!(
                "timed out waiting for tronbox accounts; got {} keys",
                out.len()
            );
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

pub fn decode_hex32(s: &str) -> Result<[u8; 32]> {
    let s = s.trim();
    let s = s.strip_prefix("0x").unwrap_or(s);
    let bytes = hex::decode(s).context("decode hex")?;
    if bytes.len() != 32 {
        anyhow::bail!("expected 32-byte hex, got {}", bytes.len());
    }
    Ok(bytes.try_into().unwrap())
}

pub async fn fetch_tron_tx_by_id_from_block(
    grpc: &mut tron::TronGrpc,
    txid: [u8; 32],
    block_number: i64,
) -> Result<tron::protocol::Transaction> {
    let (_bext, raw_txs) = grpc
        .get_block_by_num2_raw_txs(block_number)
        .await
        .context("get_block_by_num2_raw_txs")?;

    for raw in raw_txs {
        let tx =
            tron::protocol::Transaction::decode(raw.as_slice()).context("decode Transaction")?;
        let raw_data = tx
            .raw_data
            .as_ref()
            .context("Transaction missing raw_data")?;
        let raw_bytes = raw_data.encode_to_vec();
        let got = Sha256::digest(&raw_bytes);
        let got: [u8; 32] = got.into();
        if got == txid {
            return Ok(tx);
        }
    }

    anyhow::bail!("txid not found in block {block_number}");
}

pub async fn wait_for_tronbox_admin(tron_http_base: &str, timeout: Duration) -> Result<()> {
    wait_for_http_ok(&format!("{tron_http_base}/admin/accounts"), timeout).await
}
