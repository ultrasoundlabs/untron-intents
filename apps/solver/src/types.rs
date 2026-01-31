use alloy::primitives::{Address, B256, U256};
use anyhow::{Context, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentType {
    TriggerSmartContract = 0,
    UsdtTransfer = 1,
    TrxTransfer = 2,
    DelegateResource = 3,
}

impl IntentType {
    pub fn from_i16(v: i16) -> Result<Self> {
        match v {
            0 => Ok(Self::TriggerSmartContract),
            1 => Ok(Self::UsdtTransfer),
            2 => Ok(Self::TrxTransfer),
            3 => Ok(Self::DelegateResource),
            other => anyhow::bail!("unknown intent_type={other}"),
        }
    }
}

pub fn parse_hex_bytes(s: &str) -> Result<Vec<u8>> {
    let trimmed = s.trim();
    if trimmed == "0x" || trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let s = trimmed.strip_prefix("0x").unwrap_or(trimmed);
    let bytes = hex::decode(s).context("decode hex bytes")?;
    Ok(bytes)
}

pub fn parse_b256(s: &str) -> Result<B256> {
    let trimmed = s.trim();
    let s = trimmed.strip_prefix("0x").unwrap_or(trimmed);
    let bytes = hex::decode(s).context("decode hex b256")?;
    if bytes.len() != 32 {
        anyhow::bail!("expected 32-byte hex, got {}", bytes.len());
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(B256::from(out))
}

pub fn parse_address(s: &str) -> Result<Address> {
    s.trim().parse().context("parse address")
}

pub fn parse_u256_dec(s: &str) -> Result<U256> {
    let s = s.trim();
    U256::from_str_radix(s, 10).context("parse u256 decimal")
}
