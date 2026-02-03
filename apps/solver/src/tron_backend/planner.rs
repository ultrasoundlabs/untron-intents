use anyhow::Result;
use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrxConsolidationPlan {
    pub executor_index: usize,
    /// Transfers (from_index, amount_sun) into the executor account.
    pub transfers: Vec<(usize, i64)>,
}

/// Best-effort consolidation plan for native TRX (SUN).
///
/// - Picks an executor (the account with the highest current balance).
/// - Pulls from other accounts until the executor can cover `required_sun`.
/// - Respects `max_pre_txs`.
pub fn plan_trx_consolidation(
    balances_sun: &[i64],
    required_sun: i64,
    max_pre_txs: usize,
) -> Result<Option<TrxConsolidationPlan>> {
    if required_sun <= 0 {
        return Ok(Some(TrxConsolidationPlan {
            executor_index: 0,
            transfers: Vec::new(),
        }));
    }
    if balances_sun.is_empty() {
        return Ok(None);
    }

    let (executor_index, &executor_balance) = balances_sun
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.cmp(b))
        .unwrap();

    if executor_balance >= required_sun {
        return Ok(Some(TrxConsolidationPlan {
            executor_index,
            transfers: Vec::new(),
        }));
    }

    if max_pre_txs == 0 {
        return Ok(None);
    }

    let total: i64 = balances_sun.iter().copied().sum();
    if total < required_sun {
        return Ok(None);
    }

    let mut deficit = required_sun.saturating_sub(executor_balance).max(0);

    let mut donors: Vec<(usize, i64)> = balances_sun
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != executor_index)
        .map(|(i, &b)| (i, b))
        .collect();
    donors.sort_by(|a, b| b.1.cmp(&a.1));

    let mut transfers: Vec<(usize, i64)> = Vec::new();
    for (idx, bal) in donors {
        if deficit <= 0 || transfers.len() >= max_pre_txs {
            break;
        }
        if bal <= 0 {
            continue;
        }
        let amt = bal.min(deficit);
        if amt <= 0 {
            continue;
        }
        transfers.push((idx, amt));
        deficit = deficit.saturating_sub(amt);
    }

    if deficit > 0 {
        return Ok(None);
    }

    Ok(Some(TrxConsolidationPlan {
        executor_index,
        transfers,
    }))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trc20ConsolidationPlan {
    pub executor_index: usize,
    /// Transfers (from_index, amount) into the executor account.
    pub transfers: Vec<(usize, u64)>,
}

pub fn plan_trc20_consolidation(
    balances: &[u64],
    required: u64,
    max_pre_txs: usize,
) -> Result<Option<Trc20ConsolidationPlan>> {
    if required == 0 {
        return Ok(Some(Trc20ConsolidationPlan {
            executor_index: 0,
            transfers: Vec::new(),
        }));
    }
    if balances.is_empty() {
        return Ok(None);
    }

    let (executor_index, &executor_balance) = balances
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.cmp(b))
        .unwrap();

    if executor_balance >= required {
        return Ok(Some(Trc20ConsolidationPlan {
            executor_index,
            transfers: Vec::new(),
        }));
    }

    if max_pre_txs == 0 {
        return Ok(None);
    }

    let total: u64 = balances.iter().copied().sum();
    if total < required {
        return Ok(None);
    }

    let mut deficit = required.saturating_sub(executor_balance);

    let mut donors: Vec<(usize, u64)> = balances
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != executor_index)
        .map(|(i, &b)| (i, b))
        .collect();
    donors.sort_by(|a, b| match b.1.cmp(&a.1) {
        Ordering::Equal => a.0.cmp(&b.0),
        other => other,
    });

    let mut transfers: Vec<(usize, u64)> = Vec::new();
    for (idx, bal) in donors {
        if deficit == 0 || transfers.len() >= max_pre_txs {
            break;
        }
        if bal == 0 {
            continue;
        }
        let amt = bal.min(deficit);
        if amt == 0 {
            continue;
        }
        transfers.push((idx, amt));
        deficit = deficit.saturating_sub(amt);
    }

    if deficit != 0 {
        return Ok(None);
    }

    Ok(Some(Trc20ConsolidationPlan {
        executor_index,
        transfers,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trx_consolidation_picks_best_executor_and_plans_min_transfers() {
        let balances = vec![10, 5, 100];
        let plan = plan_trx_consolidation(&balances, 115, 2).unwrap().unwrap();
        assert_eq!(plan.executor_index, 2);
        assert_eq!(plan.transfers, vec![(0, 10), (1, 5)]);
    }

    #[test]
    fn trx_consolidation_respects_max_pre_txs() {
        let balances = vec![10, 5, 100];
        let plan = plan_trx_consolidation(&balances, 115, 1).unwrap();
        assert!(plan.is_none());
    }

    #[test]
    fn trc20_consolidation_works_like_trx() {
        let balances = vec![10u64, 5, 100];
        let plan = plan_trc20_consolidation(&balances, 115, 2)
            .unwrap()
            .unwrap();
        assert_eq!(plan.executor_index, 2);
        assert_eq!(plan.transfers, vec![(0, 10), (1, 5)]);
    }
}
