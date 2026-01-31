use anyhow::{Context, Result};
use prost::Message;
use tron::TronGrpc;

fn decode_hex32(s: &str) -> Result<[u8; 32]> {
    let s = s.trim();
    let s = s.strip_prefix("0x").unwrap_or(s);
    let bytes = hex::decode(s).context("invalid hex")?;
    if bytes.len() != 32 {
        anyhow::bail!("expected 32-byte hex, got {}", bytes.len());
    }
    Ok(bytes.try_into().unwrap())
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let grpc_url = args
        .next()
        .or_else(|| std::env::var("TRON_GRPC_URL").ok())
        .context("missing TRON_GRPC_URL (arg1 or env)")?;
    let block_number: i64 = args
        .next()
        .context("missing block_number (arg2)")?
        .parse()
        .context("block_number must be an integer")?;
    let txid = args.next().context("missing txid hex (arg3)")?.to_string();
    let out_path = args.next();

    let api_key = std::env::var("TRON_API_KEY").ok();

    let rt = tokio::runtime::Runtime::new().context("create tokio runtime")?;
    let json = rt.block_on(async move {
        let mut grpc = TronGrpc::connect(&grpc_url, api_key.as_deref()).await?;
        let raw = grpc.get_block_by_num2_raw_bytes(block_number).await?;
        let txid_bytes = decode_hex32(&txid)?;

        let block =
            tron::protocol::BlockExtention::decode(raw.clone()).context("decode BlockExtention")?;
        let expected_tx_index = block
            .transactions
            .iter()
            .position(|txe| txe.txid.as_slice() == txid_bytes.as_slice());

        Ok::<_, anyhow::Error>(
            serde_json::json!({
                "grpcUrl": grpc_url,
                "blockNumber": block_number.to_string(),
                "txId": format!("0x{}", hex::encode(txid_bytes)),
                "blockExtention": format!("0x{}", hex::encode(raw)),
                "expectedTxIndex": expected_tx_index,
            })
            .to_string(),
        )
    })?;

    if let Some(path) = out_path {
        std::fs::write(&path, json.as_bytes()).with_context(|| format!("write {path}"))?;
    } else {
        println!("{json}");
    }

    Ok(())
}
