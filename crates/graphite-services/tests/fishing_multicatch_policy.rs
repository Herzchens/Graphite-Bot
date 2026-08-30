use graphite_services::{
    MAX_FISH_PER_CAST, MULTICATCH_PROBABILITY_BASIS_POINTS, MulticatchLevelXCount,
    multicatch_level_x_count_policy,
};

#[test]
fn public_api_matches_the_frozen_level_x_distribution() {
    let outcomes = [
        MulticatchLevelXCount::Single,
        MulticatchLevelXCount::Double,
        MulticatchLevelXCount::Triple,
        MulticatchLevelXCount::Quadruple,
        MulticatchLevelXCount::Quintuple,
    ];
    let policies = outcomes.map(multicatch_level_x_count_policy);

    assert_eq!(
        policies.map(|policy| policy.probability_basis_points),
        [8_575, 1_000, 300, 100, 25]
    );
    assert_eq!(policies.map(|policy| policy.fish_count), [1, 2, 3, 4, 5]);
    assert_eq!(
        policies
            .iter()
            .map(|policy| policy.probability_basis_points)
            .sum::<u16>(),
        MULTICATCH_PROBABILITY_BASIS_POINTS
    );

    for (policy, outcome) in policies.into_iter().zip(outcomes) {
        assert_eq!(policy.outcome, outcome);
    }
}

#[test]
fn public_api_preserves_exact_expected_count_and_global_cap() {
    let weighted_count_basis_points: u32 = [
        MulticatchLevelXCount::Single,
        MulticatchLevelXCount::Double,
        MulticatchLevelXCount::Triple,
        MulticatchLevelXCount::Quadruple,
        MulticatchLevelXCount::Quintuple,
    ]
    .map(multicatch_level_x_count_policy)
    .iter()
    .map(|policy| u32::from(policy.fish_count) * u32::from(policy.probability_basis_points))
    .sum();

    assert_eq!(weighted_count_basis_points, 12_000);
    assert_eq!(
        weighted_count_basis_points,
        120 * u32::from(MULTICATCH_PROBABILITY_BASIS_POINTS) / 100
    );
    assert_eq!(MAX_FISH_PER_CAST, 5);
    assert_eq!(
        multicatch_level_x_count_policy(MulticatchLevelXCount::Quintuple).fish_count,
        MAX_FISH_PER_CAST
    );
}
