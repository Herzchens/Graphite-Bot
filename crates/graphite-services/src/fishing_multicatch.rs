use serde::Serialize;

pub const MULTICATCH_PROBABILITY_BASIS_POINTS: u16 = 10_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MulticatchLevelXCount {
    Single,
    Double,
    Triple,
    Quadruple,
    Quintuple,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct MulticatchLevelXCountPolicy {
    pub outcome: MulticatchLevelXCount,
    pub fish_count: u8,
    pub probability_basis_points: u16,
}

/// Resolves the exact fish-count distribution frozen for Multicatch at Level X.
///
/// The authoritative Level X rows are 10% double, 3% triple, 1% quadruple, and 0.25% quintuple.
/// The complementary single-fish probability is therefore exactly 85.75%. Integer basis points
/// represent the complete distribution as `8_575 / 1_000 / 300 / 100 / 25`, whose weighted expected
/// count is exactly 1.20 fish after an initial Fish result.
///
/// This function deliberately does not accept an enchant level. The current specification freezes
/// no table or interpolation rule for Levels I-IX, so callers must not reuse the Level X distribution
/// at lower levels or infer a linear progression.
///
/// This count policy does not select species, weight, variant, or any other FishInstance identity for
/// additional fish. School Bait is an independent quantity branch and both effects remain subject to
/// the shared [`crate::MAX_FISH_PER_CAST`] cap. Multi Treasure is a separate Treasure-only branch and
/// never multiplies Multicatch.
#[must_use]
pub const fn multicatch_level_x_count_policy(
    outcome: MulticatchLevelXCount,
) -> MulticatchLevelXCountPolicy {
    let (fish_count, probability_basis_points) = match outcome {
        MulticatchLevelXCount::Single => (1, 8_575),
        MulticatchLevelXCount::Double => (2, 1_000),
        MulticatchLevelXCount::Triple => (3, 300),
        MulticatchLevelXCount::Quadruple => (4, 100),
        MulticatchLevelXCount::Quintuple => (5, 25),
    };

    MulticatchLevelXCountPolicy {
        outcome,
        fish_count,
        probability_basis_points,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTCOMES: [MulticatchLevelXCount; 5] = [
        MulticatchLevelXCount::Single,
        MulticatchLevelXCount::Double,
        MulticatchLevelXCount::Triple,
        MulticatchLevelXCount::Quadruple,
        MulticatchLevelXCount::Quintuple,
    ];

    #[test]
    fn level_x_distribution_matches_latest_master_exactly() {
        let policies = OUTCOMES.map(multicatch_level_x_count_policy);
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
    }

    #[test]
    fn level_x_expected_count_is_exactly_one_point_two() {
        let weighted_count_basis_points: u32 = OUTCOMES
            .map(multicatch_level_x_count_policy)
            .iter()
            .map(|policy| u32::from(policy.fish_count) * u32::from(policy.probability_basis_points))
            .sum();

        assert_eq!(weighted_count_basis_points, 12_000);
        assert_eq!(
            weighted_count_basis_points,
            120 * u32::from(MULTICATCH_PROBABILITY_BASIS_POINTS) / 100
        );
    }

    #[test]
    fn quintuple_matches_the_shared_global_fish_cap() {
        let quintuple = multicatch_level_x_count_policy(MulticatchLevelXCount::Quintuple);
        assert_eq!(quintuple.fish_count, crate::MAX_FISH_PER_CAST);
    }
}
