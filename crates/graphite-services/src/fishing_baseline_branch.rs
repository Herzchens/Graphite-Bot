use crate::{
    FishingCatchBranch, ManualFishingRngContext, ManualFishingRngDrawError,
    ManualFishingWeightedDrawV1, fishing_base_catch_branch_policy,
};

/// Canonical zero-modifier branch order in the versioned manual Fishing RNG.
const BASE_BRANCHES: [FishingCatchBranch; 3] = [
    FishingCatchBranch::Fish,
    FishingCatchBranch::Junk,
    FishingCatchBranch::Treasure,
];

/// Resolves the baseline 88% Fish, 8.5% Junk and 3.5% Treasure outcome from
/// the operation-owned CatchBranch RNG domain.
///
/// This selector is ONLY valid after the cast owner has proven there are no
/// bait, Rod side-grade or enchant effects. It must not substitute for shared
/// modifier caps and normalization in a modified cast.
pub fn resolve_manual_fishing_unmodified_base_branch_v1(
    rng: ManualFishingRngContext,
) -> Result<FishingCatchBranch, ManualFishingRngDrawError> {
    let weights = BASE_BRANCHES
        .map(|branch| u64::from(fishing_base_catch_branch_policy(branch).relative_weight));
    let selected = rng.sample_weighted_v1(ManualFishingWeightedDrawV1::CatchBranch, &weights)?;
    Ok(BASE_BRANCHES[selected])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_base_weights_and_selection_order_sum_to_200() {
        assert_eq!(
            BASE_BRANCHES.map(|branch| fishing_base_catch_branch_policy(branch).relative_weight),
            [176, 17, 7]
        );
        let total: u64 = BASE_BRANCHES
            .map(|branch| u64::from(fishing_base_catch_branch_policy(branch).relative_weight))
            .into_iter()
            .sum();
        assert_eq!(total, 200);
    }
}
