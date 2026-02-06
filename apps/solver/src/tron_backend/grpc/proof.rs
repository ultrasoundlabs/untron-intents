use super::connect_grpc;
use crate::{
    config::{JobConfig, TronConfig},
    hub::TronProof,
    metrics::SolverTelemetry,
};
use alloy::primitives::B256;
use anyhow::{Context, Result};
use prost::Message;
use tron::{TronGrpc, TronTxProofBuilder};

pub(crate) async fn build_proof(
    cfg: &TronConfig,
    jobs: &JobConfig,
    txid: [u8; 32],
) -> Result<TronProof> {
    let mut grpc = connect_grpc(cfg).await?;
    build_proof_with(&mut grpc, jobs, txid).await
}

pub(crate) async fn tx_is_known(
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    txid: [u8; 32],
) -> bool {
    let mut grpc = match connect_grpc(cfg).await {
        Ok(v) => v,
        Err(_) => return false,
    };
    let started = std::time::Instant::now();
    let res = grpc.get_transaction_info_by_id(txid).await;
    let ok = match res {
        Ok(info) => {
            // Some nodes return an "empty" TransactionInfo for unknown txids. Treat a tx as
            // known only if we see either:
            // - the id field populated and equal to the requested txid, or
            // - a non-zero block_number (confirmed).
            let id_matches = info.id.len() == 32 && info.id.as_slice() == txid;
            let confirmed = info.block_number > 0;
            id_matches || confirmed
        }
        Err(_) => false,
    };
    telemetry.tron_grpc_ms(
        "get_transaction_info_by_id",
        ok,
        started.elapsed().as_millis() as u64,
    );
    ok
}

pub(crate) async fn broadcast_signed_tx(
    cfg: &TronConfig,
    telemetry: &SolverTelemetry,
    tx_bytes: &[u8],
) -> Result<()> {
    let tx = tron::protocol::Transaction::decode(tx_bytes).context("decode signed tx bytes")?;
    let mut grpc = connect_grpc(cfg).await?;
    let started = std::time::Instant::now();
    let ret = grpc
        .broadcast_transaction(tx)
        .await
        .context("broadcast_transaction")?;
    telemetry.tron_grpc_ms(
        "broadcast_transaction",
        ret.result,
        started.elapsed().as_millis() as u64,
    );
    if !ret.result {
        // Re-broadcasts are expected after restarts (or if state was updated after a broadcast
        // but before persisting). Treat "duplicate" responses as success.
        let msg_utf8 = String::from_utf8_lossy(&ret.message).to_string();
        let msg_upper = msg_utf8.to_ascii_uppercase();
        if msg_upper.contains("DUP") || msg_upper.contains("EXISTS") {
            return Ok(());
        }
        anyhow::bail!(
            "broadcast failed: msg_hex=0x{}, msg_utf8={}",
            hex::encode(&ret.message),
            msg_utf8
        );
    }
    Ok(())
}

async fn build_proof_with(
    grpc: &mut TronGrpc,
    jobs: &JobConfig,
    txid: [u8; 32],
) -> Result<TronProof> {
    let builder = TronTxProofBuilder::new(jobs.tron_finality_blocks);

    // `build` already checks finality. We retry here to avoid making callers implement
    // their own polling loops.
    let start = std::time::Instant::now();
    loop {
        match builder.build(grpc, txid).await {
            Ok(bundle) => {
                let info = grpc
                    .get_transaction_info_by_id(txid)
                    .await
                    .context("get_transaction_info_by_id (post-finality)")?;
                let failed = info.block_number > 0
                    && info.result == tron::protocol::transaction_info::Code::Failed as i32;
                if failed {
                    let msg = String::from_utf8_lossy(&info.res_message).into_owned();
                    anyhow::bail!("tron_tx_failed: {msg}");
                }

                let proof = bundle
                    .proof
                    .into_iter()
                    .map(|p| B256::from_slice(p.as_slice()))
                    .collect::<Vec<_>>();

                return Ok(TronProof {
                    blocks: bundle.blocks,
                    encoded_tx: bundle.encoded_tx,
                    proof,
                    index: bundle.index,
                });
            }
            Err(err) => {
                if start.elapsed() > std::time::Duration::from_secs(180) {
                    return Err(err).context("build tron proof (timeout)");
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
    }
}
