use alloy::primitives::U256;

pub fn estimate_hub_cost_usd_from_userops(
    eth_usd: f64,
    claim_actual_gas_cost_wei: U256,
    prove_actual_gas_cost_wei: U256,
    headroom_ppm: u64,
) -> Option<f64> {
    if !(eth_usd.is_finite()) || eth_usd <= 0.0 {
        return None;
    }
    let wei = claim_actual_gas_cost_wei.saturating_add(prove_actual_gas_cost_wei);
    let eth = wei_to_eth_f64(wei)?;
    let mut usd = eth * eth_usd;
    usd *= 1.0 + (headroom_ppm.min(1_000_000) as f64 / 1e6);
    Some(usd)
}

fn wei_to_eth_f64(wei: U256) -> Option<f64> {
    // Typical mainnet costs fit in u128; keep it simple and deterministic.
    let w: u128 = wei.try_into().ok()?;
    Some((w as f64) / 1e18)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_hub_cost_usd_applies_headroom() {
        // 0.001 ETH @ $2,000 => $2.0
        let eth_usd = 2_000.0;
        let claim = U256::from(500_000_000_000_000u64); // 0.0005
        let prove = U256::from(500_000_000_000_000u64); // 0.0005
        let usd = estimate_hub_cost_usd_from_userops(eth_usd, claim, prove, 100_000).unwrap();
        assert!((usd - 2.2).abs() < 1e-9);
    }
}
