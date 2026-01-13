use anyhow::{Context, Result};

#[derive(Clone)]
pub struct ValidatedLog {
    pub log: alloy::rpc::types::Log,
    pub block_number: u64,
    pub block_hash: alloy::primitives::B256,
    pub tx_hash: alloy::primitives::B256,
    pub log_index: u32,
}

pub fn validate_and_sort_logs(logs: Vec<alloy::rpc::types::Log>) -> Result<Vec<ValidatedLog>> {
    let mut out = logs
        .into_iter()
        .map(|l| {
            let block_number = l
                .block_number
                .with_context(|| format!("log missing block_number: {l:?}"))?;
            let block_hash = l
                .block_hash
                .with_context(|| format!("log missing block_hash: {l:?}"))?;
            let tx_hash = l
                .transaction_hash
                .with_context(|| format!("log missing transaction_hash: {l:?}"))?;
            let log_index = l
                .log_index
                .with_context(|| format!("log missing log_index: {l:?}"))?;
            let log_index = u32::try_from(log_index).context("log_index out of range")?;
            Ok(ValidatedLog {
                log: l,
                block_number,
                block_hash,
                tx_hash,
                log_index,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    out.sort_by_key(|l| (l.block_number, l.log_index));
    Ok(out)
}
