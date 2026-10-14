use serde::Serialize;

use crate::{FishingCatchBranch, fishing_base_catch_branch_policy};

/// Common denominator for the two independently frozen 23/20 Treasure boosts.
///
/// The result is deliberately *pre-cap*: callers MUST NOT use it for a cast RNG draw
/// until the shared Fishing caps/normalization boundary has been resolved.
pub const FISHING_BRANCH_PRE_CAP_WEIGHT_DENOMINATOR_V1: u64 = 400;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FishingCatchBranchPreCapWeightsV1 {
    pub fish: u64,
    pub junk: u64,
    pub treasure: u64,
    pub common_denominator: u64,
    pub treasure_bait_active: bool,
    pub ordinary_gold_rod_active: bool,
}

/// Composes only the frozen Treasure Bait and ordinary Gold Rod branch modifiers.
///
/// Treasure Bait: Fish x1, Junk x9/10, Treasure x23/20.
/// Ordinary Gold Rod: Treasure x23/20. These are *relative weights*,
/// not percentage-point adjustments to final probabilities. A caller must derive
/// both flags from authoritative locked cast state, not from Discord input.
///
/// This function intentionally does not claim to implement shared Fishing caps,
/// any other branch modifier, or final RNG selection.
#[must_use]
pub fn fishing_catch_branch_pre_cap_weights_v1(
    treasure_bait_active: bool,
    ordinary_gold_rod_active: bool,
) -> FishingCatchBranchPreCapWeightsV1 {
    let scale = FISHING_BRANCH_PRE_CAP_WEIGHT_DENOMINATOR_V1;
    let fish =
        u64::from(fishing_base_catch_branch_policy(FishingCatchBranch::Fish).relative_weight)
            * scale;
    let mut junk =
        u64::from(fishing_base_catch_branch_policy(FishingCatchBranch::Junk).relative_weight)
            * scale;
    let mut treasure =
        u64::from(fishing_base_catch_branch_policy(FishingCatchBranch::Treasure).relative_weight)
            * scale;

    if treasure_bait_active {
        junk = junk * 9 / 10;
        treasure = treasure * 23 / 20;
    }
    if ordinary_gold_rod_active {
        treasure = treasure * 23 / 20;
    }
    FishingCatchBranchPreCapWeightsV1 {
        fish,
        junk,
        treasure,
        common_denominator: scale,
        treasure_bait_active,
        ordinary_gold_rod_active,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_baseline_and_each_modifier_combination() {
        let cases = [
            (false, false, [70_400, 6_800, 2_800]),
            (true, false, [70_400, 6_120, 3_220]),
            (false, true, [70_400, 6_800, 3_220]),
            (true, true, [70_400, 6_120, 3_703]),
        ];
        for (bait, gold, expected) in cases {
            let weights = fishing_catch_branch_pre_cap_weights_v1(bait, gold);
            assert_eq!([weights.fish, weights.junk, weights.treasure], expected);
            assert_eq!(weights.common_denominator, 400);
            assert_eq!(weights.treasure_bait_active, bait);
            assert_eq!(weights.ordinary_gold_rod_active, gold);
        }
    }

    #[test]
    fn relative_weights_are_not_treated_as_final_probabilities() {
        let base = fishing_catch_branch_pre_cap_weights_v1(false, false);
        let modified = fishing_catch_branch_pre_cap_weights_v1(true, true);
        assert_eq!(modified.fish, base.fish);
        assert!(modified.junk < base.junk);
        assert!(modified.treasure > base.treasure);
        assert_ne!(
            base.fish + base.junk + base.treasure,
            modified.fish + modified.junk + modified.treasure
        );
    }
}
