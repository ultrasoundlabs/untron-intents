use super::{HubClient, planner};
use alloy::primitives::{FixedBytes, U256};
use anyhow::Result;

pub(super) fn empty_proof() -> crate::hub::TronProof {
    crate::hub::TronProof {
        blocks: std::array::from_fn(|_| Vec::new()),
        encoded_tx: Vec::new(),
        proof: Vec::new(),
        index: U256::ZERO,
    }
}

pub(super) fn evm_to_tron_raw21(a: alloy::primitives::Address) -> FixedBytes<21> {
    let mut out = [0u8; 21];
    out[0] = 0x41;
    out[1..].copy_from_slice(a.as_slice());
    FixedBytes::from(out)
}

pub(super) fn tron_sender_from_privkey_or_fallback(
    tron_pk: [u8; 32],
    hub: &HubClient,
) -> FixedBytes<21> {
    if tron_pk != [0u8; 32]
        && let Ok(w) = tron::TronWallet::new(tron_pk)
    {
        let b = w.address().prefixed_bytes();
        return FixedBytes::from_slice(&b);
    }
    evm_to_tron_raw21(hub.solver_address())
}

pub(super) fn validate_trx_consolidation_caps(
    plan: &planner::TrxConsolidationPlan,
    max_total_pull_sun: u64,
    max_per_tx_pull_sun: u64,
) -> Result<()> {
    let total: i64 = plan.transfers.iter().map(|(_, a)| *a).sum();
    if max_total_pull_sun > 0 && total > i64::try_from(max_total_pull_sun).unwrap_or(i64::MAX) {
        anyhow::bail!("consolidation max_total_trx_pull_sun exceeded");
    }
    if max_per_tx_pull_sun > 0 {
        let cap = i64::try_from(max_per_tx_pull_sun).unwrap_or(i64::MAX);
        if plan.transfers.iter().any(|(_, a)| *a > cap) {
            anyhow::bail!("consolidation max_per_tx_trx_pull_sun exceeded");
        }
    }
    Ok(())
}

pub(super) fn validate_trc20_consolidation_caps(
    plan: &planner::Trc20ConsolidationPlan,
    max_total_pull_amount: u64,
    max_per_tx_pull_amount: u64,
) -> Result<()> {
    let total: u64 = plan.transfers.iter().map(|(_, a)| *a).sum();
    if max_total_pull_amount > 0 && total > max_total_pull_amount {
        anyhow::bail!("consolidation max_total_usdt_pull_amount exceeded");
    }
    if max_per_tx_pull_amount > 0
        && plan
            .transfers
            .iter()
            .any(|(_, a)| *a > max_per_tx_pull_amount)
    {
        anyhow::bail!("consolidation max_per_tx_usdt_pull_amount exceeded");
    }
    Ok(())
}

pub fn select_delegate_executor_index(
    available_sun: &[i64],
    reserved_sun: &[i64],
    needed_sun: i64,
) -> Option<usize> {
    if available_sun.len() != reserved_sun.len() {
        return None;
    }
    let mut best: Option<(usize, i64)> = None;
    for (i, (&a, &r)) in available_sun.iter().zip(reserved_sun).enumerate() {
        let effective = a.saturating_sub(r).max(0);
        if effective < needed_sun {
            continue;
        }
        match best {
            None => best = Some((i, effective)),
            Some((_, cur)) if effective > cur => best = Some((i, effective)),
            _ => {}
        }
    }
    best.map(|(i, _)| i)
}

#[cfg(test)]
mod delegate_selection_tests {
    use super::select_delegate_executor_index;

    #[test]
    fn select_delegate_executor_picks_best_effective_capacity() {
        let available = [100, 200, 150];
        let reserved = [0, 50, 0];
        assert_eq!(
            select_delegate_executor_index(&available, &reserved, 120),
            Some(1)
        );
        assert_eq!(
            select_delegate_executor_index(&available, &reserved, 180),
            None
        );
    }

    #[test]
    fn select_delegate_executor_handles_reservations() {
        let available = [500, 500];
        let reserved = [450, 0];
        assert_eq!(
            select_delegate_executor_index(&available, &reserved, 100),
            Some(1)
        );
    }
}
