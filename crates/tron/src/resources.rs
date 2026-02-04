use super::protocol::{AccountResourceMessage, ChainParameters};
use anyhow::{Context, Result};

/// Key names returned by `GetChainParameters`.
///
/// Tron nodes expose a list of (key,value) params. We only consume the fee-related ones.
pub const CHAIN_PARAM_ENERGY_FEE: &str = "getEnergyFee";
pub const CHAIN_PARAM_TX_FEE_PER_BYTE: &str = "getTransactionFee";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainFees {
    /// Sun per energy unit.
    pub energy_fee_sun_per_energy: u64,
    /// Sun per bandwidth byte.
    pub tx_fee_sun_per_byte: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccountResources {
    pub energy_used: u64,
    pub energy_limit: u64,
    pub net_used: u64,
    pub net_limit: u64,
    pub free_net_used: u64,
    pub free_net_limit: u64,
}

impl AccountResources {
    pub fn energy_available(self) -> u64 {
        self.energy_limit.saturating_sub(self.energy_used)
    }

    pub fn net_available(self) -> u64 {
        self.net_limit.saturating_sub(self.net_used)
    }

    pub fn free_net_available(self) -> u64 {
        self.free_net_limit.saturating_sub(self.free_net_used)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TxCostQuote {
    pub energy_required: u64,
    pub tx_size_bytes: u64,
    pub fees: ChainFees,
    /// Worst-case fee limit (sun) based on chain parameters:
    /// `energy_required * energy_fee + tx_size_bytes * tx_fee_per_byte`.
    pub fee_limit_sun: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceStakeTotals {
    /// Total resource capacity on the network (energy units or bandwidth units).
    pub total_limit: u64,
    /// Total stake weight backing this resource (in sun).
    pub total_weight: u64,
}

fn ceil_div_u128(n: u128, d: u128) -> u128 {
    if d == 0 {
        return u128::MAX;
    }
    (n + d - 1) / d
}

/// Converts a desired delegated TRX amount (sun) into the minimum energy units to order so that a
/// provider who computes TRX from energy using the same totals will end up delegating at least the
/// requested sun.
pub fn resource_units_for_min_trx_sun(
    min_balance_sun: u64,
    totals: ResourceStakeTotals,
    headroom_ppm: u64,
) -> u64 {
    let l = u128::from(totals.total_limit.max(1));
    let w = u128::from(totals.total_weight.max(1));
    let sun = u128::from(min_balance_sun);
    let mut energy = ceil_div_u128(sun.saturating_mul(l), w);
    energy = ceil_div_u128(
        energy.saturating_mul(u128::from(1_000_000 + headroom_ppm)),
        1_000_000,
    );
    u64::try_from(energy).unwrap_or(u64::MAX)
}

/// Converts ordered energy units into the TRX delegation amount (sun) implied by the current
/// network totals.
pub fn trx_sun_for_resource_units(units: u64, totals: ResourceStakeTotals) -> u64 {
    let l = u128::from(totals.total_limit.max(1));
    let w = u128::from(totals.total_weight.max(1));
    let e = u128::from(units);
    let sun = ceil_div_u128(e.saturating_mul(w), l);
    u64::try_from(sun).unwrap_or(u64::MAX)
}

pub fn energy_units_for_min_trx_sun(
    min_balance_sun: u64,
    totals: ResourceStakeTotals,
    headroom_ppm: u64,
) -> u64 {
    resource_units_for_min_trx_sun(min_balance_sun, totals, headroom_ppm)
}

pub fn trx_sun_for_energy_units(energy_units: u64, totals: ResourceStakeTotals) -> u64 {
    trx_sun_for_resource_units(energy_units, totals)
}

pub fn parse_chain_fees(params: &ChainParameters) -> Result<ChainFees> {
    let mut energy_fee: Option<u64> = None;
    let mut tx_fee: Option<u64> = None;

    for p in &params.chain_parameter {
        match p.key.as_str() {
            CHAIN_PARAM_ENERGY_FEE => {
                energy_fee = Some(u64::try_from(p.value).context("energy_fee out of range")?);
            }
            CHAIN_PARAM_TX_FEE_PER_BYTE => {
                tx_fee = Some(u64::try_from(p.value).context("tx_fee_per_byte out of range")?);
            }
            _ => {}
        }
    }

    Ok(ChainFees {
        energy_fee_sun_per_energy: energy_fee.context("missing chain parameter getEnergyFee")?,
        tx_fee_sun_per_byte: tx_fee.context("missing chain parameter getTransactionFee")?,
    })
}

pub fn parse_account_resources(msg: &AccountResourceMessage) -> Result<AccountResources> {
    Ok(AccountResources {
        energy_used: u64::try_from(msg.energy_used).context("EnergyUsed out of range")?,
        energy_limit: u64::try_from(msg.energy_limit).context("EnergyLimit out of range")?,
        net_used: u64::try_from(msg.net_used).context("NetUsed out of range")?,
        net_limit: u64::try_from(msg.net_limit).context("NetLimit out of range")?,
        free_net_used: u64::try_from(msg.free_net_used).context("freeNetUsed out of range")?,
        free_net_limit: u64::try_from(msg.free_net_limit).context("freeNetLimit out of range")?,
    })
}

pub fn parse_energy_stake_totals(msg: &AccountResourceMessage) -> Result<ResourceStakeTotals> {
    Ok(ResourceStakeTotals {
        total_limit: u64::try_from(msg.total_energy_limit)
            .context("TotalEnergyLimit out of range")?,
        total_weight: u64::try_from(msg.total_energy_weight)
            .context("TotalEnergyWeight out of range")?,
    })
}

pub fn parse_net_stake_totals(msg: &AccountResourceMessage) -> Result<ResourceStakeTotals> {
    Ok(ResourceStakeTotals {
        total_limit: u64::try_from(msg.total_net_limit).context("TotalNetLimit out of range")?,
        total_weight: u64::try_from(msg.total_net_weight).context("TotalNetWeight out of range")?,
    })
}

pub fn quote_fee_limit_sun(energy_required: u64, tx_size_bytes: u64, fees: ChainFees) -> u64 {
    energy_required
        .saturating_mul(fees.energy_fee_sun_per_energy)
        .saturating_add(tx_size_bytes.saturating_mul(fees.tx_fee_sun_per_byte))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ChainParameters as ProtoChainParameters;
    use crate::protocol::chain_parameters::ChainParameter;

    #[test]
    fn parse_chain_fees_extracts_expected_keys() {
        let params = ProtoChainParameters {
            chain_parameter: vec![
                ChainParameter {
                    key: "getEnergyFee".to_string(),
                    value: 100,
                },
                ChainParameter {
                    key: "getTransactionFee".to_string(),
                    value: 1000,
                },
            ],
        };

        let fees = parse_chain_fees(&params).unwrap();
        assert_eq!(
            fees,
            ChainFees {
                energy_fee_sun_per_energy: 100,
                tx_fee_sun_per_byte: 1000
            }
        );
    }

    #[test]
    fn parse_chain_fees_errors_if_missing() {
        let params = ProtoChainParameters {
            chain_parameter: vec![ChainParameter {
                key: "getEnergyFee".to_string(),
                value: 1,
            }],
        };
        assert!(parse_chain_fees(&params).is_err());
    }

    #[test]
    fn quote_fee_limit_sun_matches_formula() {
        let fees = ChainFees {
            energy_fee_sun_per_energy: 100,
            tx_fee_sun_per_byte: 1000,
        };
        assert_eq!(quote_fee_limit_sun(3, 10, fees), 3 * 100 + 10 * 1000);
    }

    #[test]
    fn energy_units_for_min_trx_sun_roundtrips_to_at_least_original() {
        let totals = ResourceStakeTotals {
            total_limit: 1000,
            total_weight: 10_000_000, // 10 TRX in sun
        };
        let min_sun = 2_000_000; // 2 TRX
        let e = resource_units_for_min_trx_sun(min_sun, totals, 0);
        let sun2 = trx_sun_for_resource_units(e, totals);
        assert!(sun2 >= min_sun, "sun2={sun2} min_sun={min_sun} e={e}");
    }
}
