use serde::Serialize;
use thiserror::Error;

use crate::{
    enchant_conflict::FishingRodEnchant,
    fishing_capability::NORMAL_ROD_DURABILITY_PER_COMPLETED_CAST_ATTEMPT,
    fishing_rod_level_x::{
        FishingRodLevelXEffect, FishingRodLevelXPolicyError, fishing_rod_level_x_policy,
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FishingRodDurabilityResolution {
    CompletedCastAttempt {
        ordinary_event_prevented_by_unbreaking: bool,
    },
    LineBreak,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FishingRodDurabilityConsequence {
    OrdinaryWearApplied,
    OrdinaryWearPreventedByUnbreaking,
    LineBreakDestroyedRod,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FishingRodDurabilityPreview {
    pub current_durability: u32,
    pub max_durability: u32,
    pub resulting_durability: u32,
    pub consequence: FishingRodDurabilityConsequence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FishingUnbreakingLevelXPolicy {
    pub prevents_line_break: bool,
    ordinary_event_prevention_probability_numerator: u16,
    ordinary_event_prevention_probability_denominator: u16,
}

impl FishingUnbreakingLevelXPolicy {
    #[must_use]
    pub const fn ordinary_event_prevention_probability_numerator(self) -> u16 {
        self.ordinary_event_prevention_probability_numerator
    }

    #[must_use]
    pub const fn ordinary_event_prevention_probability_denominator(self) -> u16 {
        self.ordinary_event_prevention_probability_denominator
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum FishingRodDurabilityPolicyError {
    #[error("Fishing Rod definition is not an ordinary Rod")]
    NotOrdinaryFishingRod,
    #[error("maximum Fishing Rod durability must be positive")]
    InvalidMaxDurability,
    #[error("current Fishing Rod durability must be between 1 and maximum durability")]
    InvalidCurrentDurability,
    #[error(transparent)]
    LevelXPolicy(#[from] FishingRodLevelXPolicyError),
    #[error("Unbreaking Level X scalar policy returned an unexpected non-Unbreaking effect")]
    UnexpectedUnbreakingLevelXPolicy,
}

/// Resolves the frozen Unbreaking Level X chance for an ordinary Rod durability event.
///
/// The scalar `20%` remains owned by [`fishing_rod_level_x_policy`]. This policy reads that value and
/// exposes it as the exact reduced probability `1/5` for the one ordinary durability event attached
/// to a completed cast attempt. It also freezes the existing durability rule that Unbreaking never
/// prevents a line-break destruction event.
///
/// This policy performs no RNG draw and does not mutate durability. A future Fishing lifecycle must
/// draw from its authoritative RNG domain and pass the resulting boolean into
/// [`FishingRodDurabilityResolution::CompletedCastAttempt`]. Line breaks bypass that draw entirely.
pub fn fishing_unbreaking_level_x_policy()
-> Result<FishingUnbreakingLevelXPolicy, FishingRodDurabilityPolicyError> {
    let policy = fishing_rod_level_x_policy(FishingRodEnchant::Unbreaking)?;
    let FishingRodLevelXEffect::Unbreaking {
        ignore_normal_rod_durability_event_chance_percent,
    } = policy.effect
    else {
        return Err(FishingRodDurabilityPolicyError::UnexpectedUnbreakingLevelXPolicy);
    };

    let (
        ordinary_event_prevention_probability_numerator,
        ordinary_event_prevention_probability_denominator,
    ) = reduced_percent(ignore_normal_rod_durability_event_chance_percent);

    Ok(FishingUnbreakingLevelXPolicy {
        prevents_line_break: false,
        ordinary_event_prevention_probability_numerator,
        ordinary_event_prevention_probability_denominator,
    })
}

/// Previews the frozen durability consequence for one completed cast attempt using an ordinary Rod.
///
/// `is_ordinary_rod` must be derived from authoritative versioned ItemDefinition/ItemInstance state.
/// The separate Starter Basic Rod is system-bound and unbreakable, so it must not enter this policy
/// merely because its current metadata is Wood-like.
///
/// For a normal completed cast attempt, exactly one ordinary Rod durability event exists regardless
/// of Multicatch or School Bait quantity. `ordinary_event_prevented_by_unbreaking` is an already-
/// authoritative RNG result supplied by the future owning lifecycle; this pure policy does not draw
/// RNG or infer an Unbreaking chance/level curve. When prevention did not occur, exactly one
/// durability is consumed and durability may reach zero/Broken.
///
/// A line break is a distinct terminal override: resulting durability is always zero and neither
/// Unbreaking nor Mending may prevent that destruction. The [`FishingRodDurabilityResolution`] enum
/// intentionally makes it impossible for a caller to combine `LineBreak` with an Unbreaking-
/// prevention flag.
///
/// This preview does not mutate ItemInstance state, consume bait, grant AEXP, or apply Mending after
/// ordinary wear. Those actions remain the responsibility of the future atomic Fishing settlement.
pub const fn preview_fishing_rod_durability(
    current_durability: u32,
    max_durability: u32,
    is_ordinary_rod: bool,
    resolution: FishingRodDurabilityResolution,
) -> Result<FishingRodDurabilityPreview, FishingRodDurabilityPolicyError> {
    if !is_ordinary_rod {
        return Err(FishingRodDurabilityPolicyError::NotOrdinaryFishingRod);
    }
    if max_durability == 0 {
        return Err(FishingRodDurabilityPolicyError::InvalidMaxDurability);
    }
    if current_durability == 0 || current_durability > max_durability {
        return Err(FishingRodDurabilityPolicyError::InvalidCurrentDurability);
    }

    let (resulting_durability, consequence) = match resolution {
        FishingRodDurabilityResolution::CompletedCastAttempt {
            ordinary_event_prevented_by_unbreaking: true,
        } => (
            current_durability,
            FishingRodDurabilityConsequence::OrdinaryWearPreventedByUnbreaking,
        ),
        FishingRodDurabilityResolution::CompletedCastAttempt {
            ordinary_event_prevented_by_unbreaking: false,
        } => (
            current_durability - NORMAL_ROD_DURABILITY_PER_COMPLETED_CAST_ATTEMPT as u32,
            FishingRodDurabilityConsequence::OrdinaryWearApplied,
        ),
        FishingRodDurabilityResolution::LineBreak => {
            (0, FishingRodDurabilityConsequence::LineBreakDestroyedRod)
        }
    };

    Ok(FishingRodDurabilityPreview {
        current_durability,
        max_durability,
        resulting_durability,
        consequence,
    })
}

const fn reduced_percent(percent: u8) -> (u16, u16) {
    let numerator = percent as u16;
    let denominator = 100_u16;
    let divisor = gcd(numerator, denominator);
    (numerator / divisor, denominator / divisor)
}

const fn gcd(mut left: u16, mut right: u16) -> u16 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unbreaking_level_x_is_exactly_one_fifth_of_ordinary_events_only() {
        let policy = fishing_unbreaking_level_x_policy().unwrap();
        assert_eq!(
            (
                policy.ordinary_event_prevention_probability_numerator(),
                policy.ordinary_event_prevention_probability_denominator(),
            ),
            (1, 5)
        );
        assert!(!policy.prevents_line_break);
    }

    #[test]
    fn ordinary_completed_cast_consumes_exactly_one_durability() {
        assert_eq!(
            preview_fishing_rod_durability(
                17,
                100,
                true,
                FishingRodDurabilityResolution::CompletedCastAttempt {
                    ordinary_event_prevented_by_unbreaking: false,
                },
            ),
            Ok(FishingRodDurabilityPreview {
                current_durability: 17,
                max_durability: 100,
                resulting_durability: 16,
                consequence: FishingRodDurabilityConsequence::OrdinaryWearApplied,
            })
        );
    }

    #[test]
    fn ordinary_wear_can_break_a_one_durability_rod() {
        assert_eq!(
            preview_fishing_rod_durability(
                1,
                100,
                true,
                FishingRodDurabilityResolution::CompletedCastAttempt {
                    ordinary_event_prevented_by_unbreaking: false,
                },
            )
            .unwrap()
            .resulting_durability,
            0
        );
    }

    #[test]
    fn authoritative_unbreaking_prevention_preserves_ordinary_durability() {
        let preview = preview_fishing_rod_durability(
            17,
            100,
            true,
            FishingRodDurabilityResolution::CompletedCastAttempt {
                ordinary_event_prevented_by_unbreaking: true,
            },
        )
        .unwrap();

        assert_eq!(preview.resulting_durability, 17);
        assert_eq!(
            preview.consequence,
            FishingRodDurabilityConsequence::OrdinaryWearPreventedByUnbreaking
        );
    }

    #[test]
    fn line_break_forces_zero_durability_from_any_usable_state() {
        for current_durability in [1, 2, 550, 11_000] {
            let preview = preview_fishing_rod_durability(
                current_durability,
                11_000,
                true,
                FishingRodDurabilityResolution::LineBreak,
            )
            .unwrap();
            assert_eq!(preview.resulting_durability, 0);
            assert_eq!(
                preview.consequence,
                FishingRodDurabilityConsequence::LineBreakDestroyedRod
            );
        }
    }

    #[test]
    fn starter_or_special_nonordinary_rods_fail_closed() {
        assert_eq!(
            preview_fishing_rod_durability(
                100,
                100,
                false,
                FishingRodDurabilityResolution::CompletedCastAttempt {
                    ordinary_event_prevented_by_unbreaking: false,
                },
            ),
            Err(FishingRodDurabilityPolicyError::NotOrdinaryFishingRod)
        );
    }

    #[test]
    fn malformed_or_already_broken_durability_fails_closed() {
        for (current_durability, max_durability, expected) in [
            (1, 0, FishingRodDurabilityPolicyError::InvalidMaxDurability),
            (
                0,
                100,
                FishingRodDurabilityPolicyError::InvalidCurrentDurability,
            ),
            (
                101,
                100,
                FishingRodDurabilityPolicyError::InvalidCurrentDurability,
            ),
        ] {
            assert_eq!(
                preview_fishing_rod_durability(
                    current_durability,
                    max_durability,
                    true,
                    FishingRodDurabilityResolution::LineBreak,
                ),
                Err(expected)
            );
        }
    }
}
