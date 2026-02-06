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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Ready,
    Claimed,
    TronPrepared,
    TronSent,
    ProofBuilt,
    Proved,
    ProvedWaitingFunding,
    ProvedWaitingSettlement,
    Done,
    FailedFatal,
}

impl JobState {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Claimed => "claimed",
            Self::TronPrepared => "tron_prepared",
            Self::TronSent => "tron_sent",
            Self::ProofBuilt => "proof_built",
            Self::Proved => "proved",
            Self::ProvedWaitingFunding => "proved_waiting_funding",
            Self::ProvedWaitingSettlement => "proved_waiting_settlement",
            Self::Done => "done",
            Self::FailedFatal => "failed_fatal",
        }
    }

    pub fn parse(v: &str) -> Result<Self> {
        match v {
            "ready" => Ok(Self::Ready),
            "claimed" => Ok(Self::Claimed),
            "tron_prepared" => Ok(Self::TronPrepared),
            "tron_sent" => Ok(Self::TronSent),
            "proof_built" => Ok(Self::ProofBuilt),
            "proved" => Ok(Self::Proved),
            "proved_waiting_funding" => Ok(Self::ProvedWaitingFunding),
            "proved_waiting_settlement" => Ok(Self::ProvedWaitingSettlement),
            "done" => Ok(Self::Done),
            "failed_fatal" => Ok(Self::FailedFatal),
            other => anyhow::bail!("unknown job state: {other}"),
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

#[allow(dead_code)]
pub fn parse_address(s: &str) -> Result<Address> {
    s.trim().parse().context("parse address")
}

pub fn parse_u256_dec(s: &str) -> Result<U256> {
    let s = s.trim();
    U256::from_str_radix(s, 10).context("parse u256 decimal")
}

#[cfg(test)]
mod tests {
    use super::JobState;

    #[test]
    fn job_state_roundtrip_db_strings() {
        let states = [
            JobState::Ready,
            JobState::Claimed,
            JobState::TronPrepared,
            JobState::TronSent,
            JobState::ProofBuilt,
            JobState::Proved,
            JobState::ProvedWaitingFunding,
            JobState::ProvedWaitingSettlement,
            JobState::Done,
            JobState::FailedFatal,
        ];

        for state in states {
            let db = state.as_db_str();
            let parsed = JobState::parse(db).expect("parse known state");
            assert_eq!(parsed, state, "roundtrip mismatch for state={db}");
        }
    }

    #[test]
    fn job_state_parse_rejects_unknown() {
        assert!(JobState::parse("not_a_real_state").is_err());
    }
}
