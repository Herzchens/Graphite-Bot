use graphite_services::{
    MULTI_TREASURE_MAX_ITEMS, MULTI_TREASURE_PROBABILITY_BASIS_POINTS, MultiTreasureLevelXCount,
    multi_treasure_level_x_count_policy,
};

#[test]
fn public_api_matches_the_frozen_level_x_distribution() {
    let outcomes = [
        MultiTreasureLevelXCount::Single,
        MultiTreasureLevelXCount::Double,
        MultiTreasureLevelXCount::Triple,
    ];
    let policies = outcomes.map(multi_treasure_level_x_count_policy);

    assert_eq!(
        policies.map(|policy| policy.probability_basis_points),
        [9_250, 600, 150]
    );
    assert_eq!(policies.map(|policy| policy.treasure_count), [1, 2, 3]);
    assert_eq!(MULTI_TREASURE_MAX_ITEMS, 3);
    assert_eq!(
        policies
            .iter()
            .map(|policy| policy.probability_basis_points)
            .sum::<u16>(),
        MULTI_TREASURE_PROBABILITY_BASIS_POINTS
    );

    for (policy, outcome) in policies.into_iter().zip(outcomes) {
        assert_eq!(policy.outcome, outcome);
    }
}

#[test]
fn public_api_preserves_the_exact_level_x_expected_count() {
    let weighted_count_basis_points: u32 = [
        MultiTreasureLevelXCount::Single,
        MultiTreasureLevelXCount::Double,
        MultiTreasureLevelXCount::Triple,
    ]
    .map(multi_treasure_level_x_count_policy)
    .iter()
    .map(|policy| u32::from(policy.treasure_count) * u32::from(policy.probability_basis_points))
    .sum();

    assert_eq!(weighted_count_basis_points, 10_900);
    assert_eq!(
        weighted_count_basis_points,
        109 * u32::from(MULTI_TREASURE_PROBABILITY_BASIS_POINTS) / 100
    );
}
