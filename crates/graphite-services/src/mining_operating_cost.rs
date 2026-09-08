use graphite_core::Money;

use crate::{MINING_DEPTH_COUNT, MiningDepth};

const MONEY_PER_EXPEDITION_BY_DEPTH: [i64; MINING_DEPTH_COUNT] = [
    0, 5, 8, 10, 15, 20, 30, 40, 55, 70, 100, 125, 150, 200, 275, 400,
];

/// Canonical non-mutating operating-cost contract for one manual Mining expedition.
///
/// The amount is reserved at `/mine` Confirm and becomes a committed Money sink when the
/// expedition commits. A pre-validation failure costs zero. Once the expedition has committed,
/// later death or escape does not refund the operating cost. Money Reward buffs, Shop discounts,
/// and Bank perks do not reduce the amount.
///
/// This policy does not reserve or spend Money and deliberately does not decide whether `/mine`
/// authorizes automatic Bank pull when Wallet is insufficient. That funding decision belongs to
/// the future stateful `/mine` owner because the global Wallet policy permits automatic pull only
/// for actions explicitly configured to allow it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MiningOperatingCostPolicy {
    pub depth: MiningDepth,
    pub money_per_expedition: Money,
    pub pre_validation_failure_cost: Money,
    pub reserve_at_confirm: bool,
    pub charge_when_expedition_commits: bool,
    pub refund_committed_cost_after_death_or_escape: bool,
    pub money_reward_buffs_apply: bool,
    pub shop_discounts_apply: bool,
    pub bank_perks_apply: bool,
}

impl MiningOperatingCostPolicy {
    /// Surface has a canonical zero operating cost and therefore needs no positive Money
    /// reservation. Other depths have positive fixed costs.
    #[must_use]
    pub fn requires_positive_money_reservation(self) -> bool {
        self.money_per_expedition != Money::ZERO
    }
}

/// Returns the exact current-v1 manual Mining operating-cost row for `depth`.
///
/// This function is pure policy only: it performs no validation, reservation, ledger mutation,
/// operation finalization, Bank withdrawal, cooldown, RNG, or `/mine` command work.
#[must_use]
pub fn mining_operating_cost_policy(depth: MiningDepth) -> MiningOperatingCostPolicy {
    let money_per_expedition =
        Money::new(MONEY_PER_EXPEDITION_BY_DEPTH[usize::from(depth.index())])
            .expect("canonical Mining operating costs are non-negative and fit Money");

    MiningOperatingCostPolicy {
        depth,
        money_per_expedition,
        pre_validation_failure_cost: Money::ZERO,
        reserve_at_confirm: true,
        charge_when_expedition_commits: true,
        refund_committed_cost_after_death_or_escape: false,
        money_reward_buffs_apply: false,
        shop_discounts_apply: false,
        bank_perks_apply: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ALL_MINING_DEPTHS;

    #[test]
    fn every_depth_matches_the_frozen_money_table() {
        let actual = ALL_MINING_DEPTHS.map(|depth| {
            mining_operating_cost_policy(depth)
                .money_per_expedition
                .get()
        });
        assert_eq!(actual, MONEY_PER_EXPEDITION_BY_DEPTH);
    }

    #[test]
    fn lifecycle_and_modifier_contract_is_fixed_for_every_depth() {
        for depth in ALL_MINING_DEPTHS {
            let policy = mining_operating_cost_policy(depth);
            assert_eq!(policy.depth, depth);
            assert_eq!(policy.pre_validation_failure_cost, Money::ZERO);
            assert!(policy.reserve_at_confirm);
            assert!(policy.charge_when_expedition_commits);
            assert!(!policy.refund_committed_cost_after_death_or_escape);
            assert!(!policy.money_reward_buffs_apply);
            assert!(!policy.shop_discounts_apply);
            assert!(!policy.bank_perks_apply);
        }
    }

    #[test]
    fn only_surface_skips_a_positive_money_reservation() {
        assert!(
            !mining_operating_cost_policy(MiningDepth::Surface)
                .requires_positive_money_reservation()
        );
        for depth in ALL_MINING_DEPTHS.into_iter().skip(1) {
            assert!(mining_operating_cost_policy(depth).requires_positive_money_reservation());
        }
    }
}
