use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MULTICATCH_PROBABILITY_BASIS_POINTS: u16 = 10_000;
pub const MULTICATCH_PROBABILITY_SCALE_PPM: u32 = 1_000_000;
pub const MULTICATCH_MAX_LEVEL: u8 = 10;

const MULTICATCH_DOUBLE_PPM_PER_LEVEL: u32 = 10_000;
const MULTICATCH_TRIPLE_PPM_PER_LEVEL: u32 = 3_000;
const MULTICATCH_QUADRUPLE_PPM_PER_LEVEL: u32 = 1_000;
const MULTICATCH_QUINTUPLE_PPM_PER_LEVEL: u32 = 250;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MulticatchLevelXCount {
    Single,
    Double,
    Triple,
    Quadruple,
    Quintuple,
}

pub type MulticatchCount = MulticatchLevelXCount;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct MulticatchLevelXCountPolicy {
    pub outcome: MulticatchLevelXCount,
    pub fish_count: u8,
    pub probability_basis_points: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct MulticatchCountPolicy {
    pub level: u8,
    pub outcome: MulticatchCount,
    pub fish_count: u8,
    pub probability_ppm: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AdditionalFishInstanceIdentityPolicy {
    IndependentFullRoll,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FishingQuantityComposition {
    pub multicatch_fish_count: u8,
    pub school_bait_proc_triggered: bool,
    pub school_bait_extra_applied: bool,
    pub final_fish_count: u8,
    pub additional_fish_identity: AdditionalFishInstanceIdentityPolicy,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum MulticatchPolicyError {
    #[error("Multicatch level must be within 1..={MULTICATCH_MAX_LEVEL}; got {0}")]
    InvalidLevel(u8),
    #[error("resolved Multicatch fish count is outside the canonical 1..=5 domain: {0}")]
    FishCountOutOfRange(u8),
}

/// Resolves the owner-approved linear Multicatch curve for Levels I-X.
///
/// Each level contributes exactly 1.0% double, 0.30% triple, 0.10% quadruple and 0.025%
/// quintuple probability. The remainder is single-fish probability. Parts-per-million preserve the
/// 0.025% increment exactly without floating-point probability authority.
pub fn multicatch_count_policy(
    level: u8,
    outcome: MulticatchCount,
) -> Result<MulticatchCountPolicy, MulticatchPolicyError> {
    if !(1..=MULTICATCH_MAX_LEVEL).contains(&level) {
        return Err(MulticatchPolicyError::InvalidLevel(level));
    }

    let level_scale = u32::from(level);
    let double = level_scale * MULTICATCH_DOUBLE_PPM_PER_LEVEL;
    let triple = level_scale * MULTICATCH_TRIPLE_PPM_PER_LEVEL;
    let quadruple = level_scale * MULTICATCH_QUADRUPLE_PPM_PER_LEVEL;
    let quintuple = level_scale * MULTICATCH_QUINTUPLE_PPM_PER_LEVEL;
    let non_single = double + triple + quadruple + quintuple;

    let (fish_count, probability_ppm) = match outcome {
        MulticatchCount::Single => (1, MULTICATCH_PROBABILITY_SCALE_PPM - non_single),
        MulticatchCount::Double => (2, double),
        MulticatchCount::Triple => (3, triple),
        MulticatchCount::Quadruple => (4, quadruple),
        MulticatchCount::Quintuple => (5, quintuple),
    };

    Ok(MulticatchCountPolicy {
        level,
        outcome,
        fish_count,
        probability_ppm,
    })
}

/// Composes Multicatch and School Bait without an order-dependent cap race.
///
/// Both quantity effects resolve independently. School Bait may add exactly one fish after a Fish
/// result, but the global five-fish cap applies to the combined count. Every fish beyond the base
/// fish is a new FishInstance that independently rolls species, weight, variant and biological
/// noise in the same area; no additional fish clones the base result.
pub fn compose_multicatch_and_school_bait(
    multicatch_fish_count: u8,
    school_bait_proc_triggered: bool,
) -> Result<FishingQuantityComposition, MulticatchPolicyError> {
    if !(1..=crate::MAX_FISH_PER_CAST).contains(&multicatch_fish_count) {
        return Err(MulticatchPolicyError::FishCountOutOfRange(
            multicatch_fish_count,
        ));
    }

    let school_bait_extra_applied =
        school_bait_proc_triggered && multicatch_fish_count < crate::MAX_FISH_PER_CAST;
    let final_fish_count = if school_bait_extra_applied {
        multicatch_fish_count + 1
    } else {
        multicatch_fish_count
    };

    Ok(FishingQuantityComposition {
        multicatch_fish_count,
        school_bait_proc_triggered,
        school_bait_extra_applied,
        final_fish_count,
        additional_fish_identity: AdditionalFishInstanceIdentityPolicy::IndependentFullRoll,
    })
}

/// Preserves the historical exact Level-X basis-point API.
///
/// Level X is 85.75% single, 10% double, 3% triple, 1% quadruple and 0.25% quintuple.
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

    const OUTCOMES: [MulticatchCount; 5] = [
        MulticatchCount::Single,
        MulticatchCount::Double,
        MulticatchCount::Triple,
        MulticatchCount::Quadruple,
        MulticatchCount::Quintuple,
    ];

    #[test]
    fn every_level_is_complete_and_level_x_matches_the_frozen_anchor() {
        for level in 1..=MULTICATCH_MAX_LEVEL {
            let policies = OUTCOMES.map(|outcome| multicatch_count_policy(level, outcome).unwrap());
            assert_eq!(
                policies
                    .iter()
                    .map(|policy| policy.probability_ppm)
                    .sum::<u32>(),
                MULTICATCH_PROBABILITY_SCALE_PPM
            );
        }

        let level_x = OUTCOMES.map(|outcome| multicatch_count_policy(10, outcome).unwrap());
        assert_eq!(
            level_x.map(|policy| policy.probability_ppm),
            [857_500, 100_000, 30_000, 10_000, 2_500]
        );
    }

    #[test]
    fn quantity_composition_applies_one_shared_cap_and_independent_identity() {
        let composed = compose_multicatch_and_school_bait(4, true).unwrap();
        assert_eq!(composed.final_fish_count, 5);
        assert!(composed.school_bait_extra_applied);
        assert_eq!(
            composed.additional_fish_identity,
            AdditionalFishInstanceIdentityPolicy::IndependentFullRoll
        );

        let capped = compose_multicatch_and_school_bait(5, true).unwrap();
        assert_eq!(capped.final_fish_count, 5);
        assert!(!capped.school_bait_extra_applied);
    }

    #[test]
    fn invalid_levels_and_counts_fail_closed() {
        assert_eq!(
            multicatch_count_policy(0, MulticatchCount::Single),
            Err(MulticatchPolicyError::InvalidLevel(0))
        );
        assert_eq!(
            multicatch_count_policy(11, MulticatchCount::Single),
            Err(MulticatchPolicyError::InvalidLevel(11))
        );
        assert_eq!(
            compose_multicatch_and_school_bait(0, false),
            Err(MulticatchPolicyError::FishCountOutOfRange(0))
        );
        assert_eq!(
            compose_multicatch_and_school_bait(crate::MAX_FISH_PER_CAST + 1, false),
            Err(MulticatchPolicyError::FishCountOutOfRange(
                crate::MAX_FISH_PER_CAST + 1
            ))
        );
    }
}
