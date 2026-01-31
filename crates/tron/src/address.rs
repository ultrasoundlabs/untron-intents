use alloy::primitives::Address;
use std::fmt;
use std::str::FromStr;

/// Tron mainnet base58check address (0x41 || addr20).
///
/// - In-memory representation is always the 20-byte EVM-form `Address`.
/// - String representation is always canonical Tron base58check (T...).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TronAddress(Address);

impl TronAddress {
    pub const MAINNET_PREFIX: u8 = 0x41;

    pub fn from_evm(addr: Address) -> Self {
        Self(addr)
    }

    pub fn evm(self) -> Address {
        self.0
    }

    pub fn from_base58check(value: &str) -> anyhow::Result<Self> {
        let payload = bs58::decode(value)
            .with_check(None)
            .into_vec()
            .map_err(|e| anyhow::Error::new(e).context("base58check decode"))?;

        if payload.len() != 21 {
            anyhow::bail!(
                "invalid Tron address length: expected 21 bytes, got {}",
                payload.len()
            );
        }
        if payload[0] != Self::MAINNET_PREFIX {
            anyhow::bail!(
                "unexpected Tron address prefix: expected 0x{:02x}, got 0x{:02x}",
                Self::MAINNET_PREFIX,
                payload[0],
            );
        }

        Ok(Self(Address::from_slice(&payload[1..])))
    }

    /// Parses either Tron base58check (T...) or EVM hex (0x...).
    pub fn parse_text(value: &str) -> anyhow::Result<Self> {
        let trimmed = value.trim();
        if trimmed.starts_with('T') {
            return Self::from_base58check(trimmed);
        }
        if let Ok(evm) = trimmed.parse::<Address>() {
            return Ok(Self::from_evm(evm));
        }
        anyhow::bail!("invalid Tron address text: {value}");
    }

    pub fn to_base58check(self) -> String {
        let mut payload = [0u8; 21];
        payload[0] = Self::MAINNET_PREFIX;
        payload[1..].copy_from_slice(self.0.as_slice());
        bs58::encode(payload).with_check().into_string()
    }

    /// 0x41 || addr20 (21 bytes), for Tron gRPC `owner_address`/`contract_address`.
    pub fn prefixed_bytes(self) -> [u8; 21] {
        let mut out = [0u8; 21];
        out[0] = Self::MAINNET_PREFIX;
        out[1..].copy_from_slice(self.0.as_slice());
        out
    }
}

impl From<Address> for TronAddress {
    fn from(value: Address) -> Self {
        Self::from_evm(value)
    }
}

impl From<TronAddress> for Address {
    fn from(value: TronAddress) -> Self {
        value.0
    }
}

impl fmt::Display for TronAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_base58check())
    }
}

impl FromStr for TronAddress {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_text(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::Address;

    #[test]
    fn base58check_roundtrip() {
        let addr20 = Address::from_slice(&[0x11u8; 20]);
        let tron = TronAddress::from_evm(addr20);

        let s = tron.to_base58check();
        let parsed = TronAddress::from_base58check(&s).unwrap();
        assert_eq!(parsed, tron);
        assert_eq!(parsed.to_string(), s);
    }

    #[test]
    fn parse_text_accepts_hex_or_tron() {
        let addr: Address = "0x0000000000000000000000000000000000000001"
            .parse()
            .unwrap();
        let tron = TronAddress::parse_text("0x0000000000000000000000000000000000000001").unwrap();
        assert_eq!(tron.evm(), addr);

        let tron2 = TronAddress::parse_text(&tron.to_base58check()).unwrap();
        assert_eq!(tron2, tron);
    }

    #[test]
    fn from_base58check_rejects_wrong_prefix() {
        let addr20 = Address::from_slice(&[0x22u8; 20]);
        let mut payload = [0u8; 21];
        payload[0] = 0x42;
        payload[1..].copy_from_slice(addr20.as_slice());

        let s = bs58::encode(payload).with_check().into_string();
        let err = TronAddress::from_base58check(&s).unwrap_err().to_string();
        assert!(err.contains("unexpected Tron address prefix"));
    }

    #[test]
    fn prefixed_bytes_has_mainnet_prefix() {
        let addr20 = Address::from_slice(&[0x33u8; 20]);
        let tron = TronAddress::from_evm(addr20);
        let b = tron.prefixed_bytes();
        assert_eq!(b[0], TronAddress::MAINNET_PREFIX);
        assert_eq!(&b[1..], addr20.as_slice());
    }
}
