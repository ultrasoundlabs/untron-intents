use alloy::primitives::{Address, U256, keccak256};

pub fn selector(sig: &str) -> [u8; 4] {
    let hash = keccak256(sig.as_bytes());
    [hash[0], hash[1], hash[2], hash[3]]
}

pub fn encode_address(addr: Address) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[12..].copy_from_slice(addr.as_slice());
    out
}

pub fn encode_u256(v: U256) -> [u8; 32] {
    v.to_be_bytes()
}

pub fn encode_trc20_transfer(to: Address, amount: U256) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 32 + 32);
    out.extend_from_slice(&selector("transfer(address,uint256)"));
    out.extend_from_slice(&encode_address(to));
    out.extend_from_slice(&encode_u256(amount));
    out
}

pub fn encode_trc20_balance_of(owner: Address) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 32);
    out.extend_from_slice(&selector("balanceOf(address)"));
    out.extend_from_slice(&encode_address(owner));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_trc20_transfer_layout() {
        let to: Address = "0x00000000000000000000000000000000000000aa"
            .parse()
            .unwrap();
        let amount = U256::from(1234u64);
        let data = encode_trc20_transfer(to, amount);

        assert_eq!(data.len(), 4 + 32 + 32);
        assert_eq!(&data[..4], &selector("transfer(address,uint256)"));

        // The address is right-aligned in the first arg word.
        assert_eq!(&data[4..4 + 12], &[0u8; 12]);
        assert_eq!(&data[4 + 12..4 + 32], to.as_slice());

        // U256 is big-endian.
        assert_eq!(&data[4 + 32..4 + 32 + 30], &[0u8; 30]);
        assert_eq!(data[4 + 32 + 30], 0x04);
        assert_eq!(data[4 + 32 + 31], 0xD2);
    }

    #[test]
    fn encode_trc20_balance_of_layout() {
        let owner: Address = "0x00000000000000000000000000000000000000bb"
            .parse()
            .unwrap();
        let data = encode_trc20_balance_of(owner);
        assert_eq!(data.len(), 4 + 32);
        assert_eq!(&data[..4], &selector("balanceOf(address)"));
        assert_eq!(&data[4..4 + 12], &[0u8; 12]);
        assert_eq!(&data[4 + 12..4 + 32], owner.as_slice());
    }
}
