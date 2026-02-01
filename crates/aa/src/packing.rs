use alloy::primitives::{Address, Bytes, U256};
use anyhow::{Context, Result};

pub(crate) fn add_gas_buffer(v: U256, pct: u64) -> Result<U256> {
    if pct == 0 {
        return Ok(v);
    }
    let mul = U256::from(100u64 + pct);
    v.checked_mul(mul)
        .and_then(|x| x.checked_div(U256::from(100u64)))
        .context("overflow adding gas buffer")
}

pub(crate) fn pack_init_code(
    factory: Option<Address>,
    factory_data: Option<&Bytes>,
) -> Result<Vec<u8>> {
    match factory {
        None => Ok(Vec::new()),
        Some(f) => {
            let data = factory_data.context("factory_data must be set when factory is set")?;
            let mut out = Vec::with_capacity(20 + data.len());
            out.extend_from_slice(f.as_slice());
            out.extend_from_slice(data.as_ref());
            Ok(out)
        }
    }
}

pub(crate) fn pack_paymaster_and_data(
    paymaster: Option<Address>,
    paymaster_verification_gas_limit: Option<U256>,
    paymaster_post_op_gas_limit: Option<U256>,
    paymaster_data: Option<&Bytes>,
) -> Result<Vec<u8>> {
    match paymaster {
        None => Ok(Vec::new()),
        Some(p) => {
            let ver = paymaster_verification_gas_limit
                .context("paymaster_verification_gas_limit must be set when paymaster is set")?;
            let post = paymaster_post_op_gas_limit
                .context("paymaster_post_op_gas_limit must be set when paymaster is set")?;
            let data =
                paymaster_data.context("paymaster_data must be set when paymaster is set")?;

            let ver_u128 = u128::try_from(ver)
                .context("paymaster_verification_gas_limit overflows uint128")?;
            let post_u128 =
                u128::try_from(post).context("paymaster_post_op_gas_limit overflows uint128")?;

            let mut out = Vec::with_capacity(20 + 16 + 16 + data.len());
            out.extend_from_slice(p.as_slice());
            out.extend_from_slice(&ver_u128.to_be_bytes());
            out.extend_from_slice(&post_u128.to_be_bytes());
            out.extend_from_slice(data.as_ref());
            Ok(out)
        }
    }
}

pub(crate) fn ensure_u48(v: u64, label: &'static str) -> Result<()> {
    if v > 0xFFFF_FFFF_FFFF {
        anyhow::bail!("{label} must fit in uint48");
    }
    Ok(())
}

pub(crate) fn u48_be_bytes(v: u64) -> [u8; 6] {
    let b = v.to_be_bytes();
    [b[2], b[3], b[4], b[5], b[6], b[7]]
}

pub(crate) fn hex_bytes0x(bytes: &Bytes) -> String {
    if bytes.is_empty() {
        return "0x".to_string();
    }
    format!("0x{}", hex::encode(bytes.as_ref()))
}

pub(crate) fn redact_url(url: &str) -> String {
    url.split('?').next().unwrap_or(url).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u48_helpers() {
        ensure_u48(0, "x").unwrap();
        ensure_u48(0xFFFF_FFFF_FFFF, "x").unwrap();
        assert!(ensure_u48(0x1_0000_0000_0000, "x").is_err());

        assert_eq!(u48_be_bytes(0), [0, 0, 0, 0, 0, 0]);
        assert_eq!(u48_be_bytes(1), [0, 0, 0, 0, 0, 1]);
    }

    #[test]
    fn pack_init_code_layout() {
        let factory = Address::repeat_byte(0x11);
        let data = Bytes::from(vec![0xAA, 0xBB]);
        let out = pack_init_code(Some(factory), Some(&data)).unwrap();
        assert_eq!(out.len(), 20 + 2);
        assert_eq!(&out[..20], factory.as_slice());
        assert_eq!(&out[20..], data.as_ref());
    }

    #[test]
    fn pack_paymaster_and_data_layout() {
        let paymaster = Address::repeat_byte(0x22);
        let ver = U256::from(5u64);
        let post = U256::from(7u64);
        let data = Bytes::from(vec![1, 2, 3]);
        let out =
            pack_paymaster_and_data(Some(paymaster), Some(ver), Some(post), Some(&data)).unwrap();

        assert_eq!(&out[..20], paymaster.as_slice());
        assert_eq!(&out[20 + 16 + 16..], data.as_ref());
        assert_eq!(out.len(), 20 + 16 + 16 + 3);
    }

    #[test]
    fn redact_url_strips_query() {
        assert_eq!(redact_url("http://x?a=b"), "http://x");
        assert_eq!(redact_url("http://x"), "http://x");
    }

    #[test]
    fn hex_bytes0x_formats() {
        assert_eq!(hex_bytes0x(&Bytes::new()), "0x");
        assert_eq!(hex_bytes0x(&Bytes::from(vec![0x12, 0x34])), "0x1234");
    }
}
