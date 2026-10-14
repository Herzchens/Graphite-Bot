use serde::Serialize;
use thiserror::Error;

pub const MULTI_TREASURE_MAX_ITEMS: u8 = 3;
pub const MULTI_TREASURE_PROBABILITY_BASIS_POINTS: u16 = 10_000;
pub const MULTI_TREASURE_PROBABILITY_SCALE_PPM: u32 = 1_000_000;
pub const MULTI_TREASURE_MAX_LEVEL: u8 = 10;

const MULTI_TREASURE_DOUBLE_PPM_PER_LEVEL: u32 = 6_000;
const MULTI_TREASURE_TRIPLE_PPM_PER_LEVEL: u32 = 1_500;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MultiTreasureLevelXCount {
    Single,
    Double,
    Triple,
}

pub type MultiTreasureCount = MultiTreasureLevelXCount;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct MultiTreasureLevelXCountPolicy {
    pub outcome: MultiTreasureLevelXCount,
    pub treasure_count: u8,
    pub probability_basis_points: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct MultiTreasureCountPolicy {
    pub level: u8,
    pub outcome: MultiTreasureCount,
    pub treasure_count: u8,
    pub probability_ppm: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RepeatedTreasurePayloadResolutionPolicy {
    IndependentRerollWithinSelectedResult,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum MultiTreasurePolicyError {
    #[error("Multi Treasure level must be within 1..={MULTI_TREASURE_MAX_LEVEL}; got {0}")]
    InvalidLevel(u8),
}

/// Resolves the owner-approved linear Multi Treasure curve for Levels I-X.
///
/// Each level contributes exactly 0.60% double-Treasure and 0.15% triple-Treasure probability.
/// The remainder is single Treasure. Parts-per-million preserve the curve exactly.
pub fn multi_treasure_count_policy(
    level: u8,
    outcome: MultiTreasureCount,
) -> Result<MultiTreasureCountPolicy, MultiTreasurePolicyError> {
    if !(1..=MULTI_TREASURE_MAX_LEVEL).contains(&level) {
        return Err(MultiTreasurePolicyError::InvalidLevel(level));
    }

    let level_scale = u32::from(level);
    let double = level_scale * MULTI_TREASURE_DOUBLE_PPM_PER_LEVEL;
    let triple = level_scale * MULTI_TREASURE_TRIPLE_PPM_PER_LEVEL;
    let non_single = double + triple;

    let (treasure_count, probability_ppm) = match outcome {
        MultiTreasureCount::Single => (1, MULTI_TREASURE_PROBABILITY_SCALE_PPM - non_single),
        MultiTreasureCount::Double => (2, double),
        MultiTreasureCount::Triple => (3, triple),
    };

    Ok(MultiTreasureCountPolicy {
        level,
        outcome,
        treasure_count,
        probability_ppm,
    })
}

/// Repeated Treasure payloads reroll independently within the already-selected Treasure result.
///
/// Multi Treasure does not reroll the outer Treasure-result category. If that selected category is
/// Enchant Book, each repeated book independently rerolls its pool, member and level. The owning
/// Fishing lifecycle must use distinct deterministic RNG domains/indices for those payload draws;
/// cloning one already-selected book payload is not canonical.
#[must_use]
pub const fn repeated_treasure_payload_resolution_policy() -> RepeatedTreasurePayloadResolutionPolicy
{
    RepeatedTreasurePayloadResolutionPolicy::IndependentRerollWithinSelectedResult
}

/// Preserves the historical exact Level-X basis-point API.
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

    const OUTCOMES: [MultiTreasureCount; 3] = [
        MultiTreasureCount::Single,
        MultiTreasureCount::Double,
        MultiTreasureCount::Triple,
    ];

    #[test]
    fn every_level_is_complete_and_level_x_matches_the_frozen_anchor() {
        for level in 1..=MULTI_TREASURE_MAX_LEVEL {
            let policies =
                OUTCOMES.map(|outcome| multi_treasure_count_policy(level, outcome).unwrap());
            assert_eq!(
                policies
                    .iter()
                    .map(|policy| policy.probability_ppm)
                    .sum::<u32>(),
                MULTI_TREASURE_PROBABILITY_SCALE_PPM
            );
        }

        let level_x = OUTCOMES.map(|outcome| multi_treasure_count_policy(10, outcome).unwrap());
        assert_eq!(
            level_x.map(|policy| policy.probability_ppm),
            [925_000, 60_000, 15_000]
        );
    }

    #[test]
    fn repeated_results_are_independent_rerolls() {
        assert_eq!(
            repeated_treasure_payload_resolution_policy(),
            RepeatedTreasurePayloadResolutionPolicy::IndependentRerollWithinSelectedResult
        );
    }

    #[test]
    fn invalid_levels_fail_closed() {
        assert_eq!(
            multi_treasure_count_policy(0, MultiTreasureCount::Single),
            Err(MultiTreasurePolicyError::InvalidLevel(0))
        );
        assert_eq!(
            multi_treasure_count_policy(11, MultiTreasureCount::Single),
            Err(MultiTreasurePolicyError::InvalidLevel(11))
        );
    }
}
