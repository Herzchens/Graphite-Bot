use serde::Serialize;

pub const MULTI_TREASURE_MAX_ITEMS: u8 = 3;
pub const MULTI_TREASURE_PROBABILITY_BASIS_POINTS: u16 = 10_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MultiTreasureLevelXCount {
    Single,
    Double,
    Triple,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct MultiTreasureLevelXCountPolicy {
    pub outcome: MultiTreasureLevelXCount,
    pub treasure_count: u8,
    pub probability_basis_points: u16,
}

/// Resolves the exact count distribution frozen for Multi Treasure at Level X.
///
/// The authoritative Level X distribution is 92.5% single, 6% double, and 1.5% triple Treasure.
/// Probabilities use integer basis points so the policy is exact and replay-friendly without
/// floating-point arithmetic. The weighted expected count is exactly 1.09 Treasure items after a
/// Treasure proc.
///
/// This function deliberately does not accept an enchant level. The current specification freezes
/// only the Level X distribution and gives no interpolation/table for Levels I-IX. A future owner
/// must not infer linear scaling or reuse these Level X probabilities at lower levels.
///
/// Multi Treasure resolves only after the Treasure branch/result has been selected and remains
/// independent from Multi Catch. This pure policy performs no RNG and does not decide whether
/// repeated stateful results such as Enchant Books are rerolled or cloned.
#[must_use]
pub const fn multi_treasure_level_x_count_policy(
    outcome: MultiTreasureLevelXCount,
) -> MultiTreasureLevelXCountPolicy {
    let (treasure_count, probability_basis_points) = match outcome {
        MultiTreasureLevelXCount::Single => (1, 9_250),
        MultiTreasureLevelXCount::Double => (2, 600),
        MultiTreasureLevelXCount::Triple => (3, 150),
    };

    MultiTreasureLevelXCountPolicy {
        outcome,
        treasure_count,
        probability_basis_points,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTCOMES: [MultiTreasureLevelXCount; 3] = [
        MultiTreasureLevelXCount::Single,
        MultiTreasureLevelXCount::Double,
        MultiTreasureLevelXCount::Triple,
    ];

    #[test]
    fn level_x_distribution_matches_latest_master_exactly() {
        let policies = OUTCOMES.map(multi_treasure_level_x_count_policy);
        assert_eq!(
            policies.map(|policy| policy.probability_basis_points),
            [9_250, 600, 150]
        );
        assert_eq!(policies.map(|policy| policy.treasure_count), [1, 2, 3]);
        assert_eq!(
            policies
                .iter()
                .map(|policy| policy.probability_basis_points)
                .sum::<u16>(),
            MULTI_TREASURE_PROBABILITY_BASIS_POINTS
        );
    }

    #[test]
    fn level_x_expected_count_is_exactly_one_point_zero_nine() {
        let weighted_count_basis_points: u32 = OUTCOMES
            .map(multi_treasure_level_x_count_policy)
            .iter()
            .map(|policy| {
                u32::from(policy.treasure_count) * u32::from(policy.probability_basis_points)
            })
            .sum();

        assert_eq!(weighted_count_basis_points, 10_900);
        assert_eq!(
            weighted_count_basis_points,
            109 * u32::from(MULTI_TREASURE_PROBABILITY_BASIS_POINTS) / 100
        );
    }

    #[test]
    fn triple_is_the_canonical_max_count() {
        let triple = multi_treasure_level_x_count_policy(MultiTreasureLevelXCount::Triple);
        assert_eq!(triple.treasure_count, MULTI_TREASURE_MAX_ITEMS);
    }
}
