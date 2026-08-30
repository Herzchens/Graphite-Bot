use graphite_services::{
    FishingCatchBranch, FishingTreasureResult, fishing_base_catch_branch_policy,
    fishing_base_treasure_result_policy,
};

const CATCH_BRANCHES: [FishingCatchBranch; 3] = [
    FishingCatchBranch::Fish,
    FishingCatchBranch::Junk,
    FishingCatchBranch::Treasure,
];
const TREASURE_RESULTS: [FishingTreasureResult; 6] = [
    FishingTreasureResult::MaterialBundle,
    FishingTreasureResult::CrateOrChest,
    FishingTreasureResult::EnchantBook,
    FishingTreasureResult::OrbOrCatalystFragmentOrItem,
    FishingTreasureResult::RareBaitOrUtilityItem,
    FishingTreasureResult::RelicOrCollectible,
];

#[test]
fn public_api_reproduces_exact_zero_modifier_catch_branch_chances() {
    let policies = CATCH_BRANCHES.map(fishing_base_catch_branch_policy);
    assert_eq!(
        policies.map(|policy| policy.relative_weight),
        [176, 17, 7]
    );
    for (policy, branch) in policies.into_iter().zip(CATCH_BRANCHES) {
        assert_eq!(policy.branch, branch);
    }

    let weights = policies.map(|policy| policy.relative_weight);
    let total: u16 = weights.iter().sum();
    assert_eq!(total, 200);
    assert_eq!(
        weights.map(|weight| u32::from(weight) * 10_000 / u32::from(total)),
        [8_800, 850, 350]
    );
}

#[test]
fn public_api_reproduces_exact_within_treasure_chances() {
    let policies = TREASURE_RESULTS.map(fishing_base_treasure_result_policy);
    assert_eq!(
        policies.map(|policy| policy.relative_weight),
        [19, 13, 5, 4, 5, 4]
    );
    for (policy, result) in policies.into_iter().zip(TREASURE_RESULTS) {
        assert_eq!(policy.result, result);
    }

    let weights = policies.map(|policy| policy.relative_weight);
    let total: u16 = weights.iter().sum();
    assert_eq!(total, 50);
    assert_eq!(
        weights.map(|weight| u32::from(weight) * 10_000 / u32::from(total)),
        [3_800, 2_600, 1_000, 800, 1_000, 800]
    );
}

#[test]
fn public_api_preserves_exact_nested_treasure_base_chances() {
    let branch_weights = CATCH_BRANCHES
        .map(fishing_base_catch_branch_policy)
        .map(|policy| policy.relative_weight);
    let treasure_weights = TREASURE_RESULTS
        .map(fishing_base_treasure_result_policy)
        .map(|policy| policy.relative_weight);
    let branch_total: u32 = branch_weights.iter().map(|weight| u32::from(*weight)).sum();
    let treasure_total: u32 = treasure_weights.iter().map(|weight| u32::from(*weight)).sum();
    let treasure_branch_weight =
        fishing_base_catch_branch_policy(FishingCatchBranch::Treasure).relative_weight;

    let overall_basis_points = treasure_weights.map(|result_weight| {
        u32::from(treasure_branch_weight) * u32::from(result_weight) * 10_000
            / (branch_total * treasure_total)
    });
    assert_eq!(overall_basis_points, [133, 91, 35, 28, 35, 28]);
}
