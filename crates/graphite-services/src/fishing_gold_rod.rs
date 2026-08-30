use serde::Serialize;
use thiserror::Error;

use crate::{
    equipment_policy::EquipmentTier,
    fishing_capability::{FishingCapabilityError, ordinary_fishing_rod_base_stats},
};

pub const GOLD_ROD_ACTION_SPEED_RATING_PERCENT: u8 = 10;
pub const GOLD_ROD_RARE_OR_BETTER_RELATIVE_WEIGHT_PERCENT: u8 = 15;
pub const GOLD_ROD_TREASURE_RELATIVE_WEIGHT_PERCENT: u8 = 15;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FishingRelativeWeightMultiplier {
    numerator: u16,
    denominator: u16,
}

impl FishingRelativeWeightMultiplier {
    #[must_use]
    pub const fn numerator(self) -> u16 {
        self.numerator
    }

    #[must_use]
    pub const fn denominator(self) -> u16 {
        self.denominator
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GoldFishingRodModifierStage {
    BeforeSharedFishingCaps,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct GoldFishingRodSideGradePolicy {
    pub action_speed_rating_percent: u8,
    pub rare_or_better_species_relative_weight_multiplier: FishingRelativeWeightMultiplier,
    pub treasure_branch_relative_weight_multiplier: FishingRelativeWeightMultiplier,
    pub modifier_stage: GoldFishingRodModifierStage,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum GoldFishingRodPolicyError {
    #[error(transparent)]
    InvalidOrdinaryRod(#[from] FishingCapabilityError),
    #[error("ordinary Fishing Rod is not the Gold side-grade")]
    NotGoldFishingRod,
}

/// Resolves the fixed pre-cap modifiers of the ordinary Gold Fishing Rod side-grade.
///
/// The caller must supply authoritative ordinary-Rod classification. This reuses the ordinary Rod
/// capability owner so a system-bound or special definition cannot obtain Gold side-grade bonuses
/// merely by carrying Gold-like tier metadata.
///
/// The +10% value is an action-speed **rating**, not a direct duration multiplier. The two +15%
/// values are relative selection-weight multipliers (23/20), not +15 percentage points of final
/// probability. All three modifiers feed their respective shared Fishing modifier buckets before
/// shared caps. This policy intentionally does not normalize weights, apply shared caps, or convert
/// speed rating into a cooldown/duration because those composition rules are not owned here.
pub fn gold_fishing_rod_side_grade_policy(
    tier: EquipmentTier,
    is_ordinary_rod: bool,
) -> Result<GoldFishingRodSideGradePolicy, GoldFishingRodPolicyError> {
    let base_stats = ordinary_fishing_rod_base_stats(tier, is_ordinary_rod)?;
    if !base_stats.gold_side_grade {
        return Err(GoldFishingRodPolicyError::NotGoldFishingRod);
    }

    let relative_weight_multiplier =
        relative_weight_multiplier(GOLD_ROD_RARE_OR_BETTER_RELATIVE_WEIGHT_PERCENT);
    let treasure_relative_weight_multiplier =
        relative_weight_multiplier(GOLD_ROD_TREASURE_RELATIVE_WEIGHT_PERCENT);

    Ok(GoldFishingRodSideGradePolicy {
        action_speed_rating_percent: GOLD_ROD_ACTION_SPEED_RATING_PERCENT,
        rare_or_better_species_relative_weight_multiplier: relative_weight_multiplier,
        treasure_branch_relative_weight_multiplier: treasure_relative_weight_multiplier,
        modifier_stage: GoldFishingRodModifierStage::BeforeSharedFishingCaps,
    })
}

const fn relative_weight_multiplier(
    relative_increase_percent: u8,
) -> FishingRelativeWeightMultiplier {
    let numerator = 100_u16 + relative_increase_percent as u16;
    let denominator = 100_u16;
    let divisor = gcd(numerator, denominator);

    FishingRelativeWeightMultiplier {
        numerator: numerator / divisor,
        denominator: denominator / divisor,
    }
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
    fn gold_rod_policy_matches_all_frozen_side_grade_inputs() {
        let policy = gold_fishing_rod_side_grade_policy(EquipmentTier::Gold, true).unwrap();

        assert_eq!(policy.action_speed_rating_percent, 10);
        assert_eq!(
            (
                policy
                    .rare_or_better_species_relative_weight_multiplier
                    .numerator(),
                policy
                    .rare_or_better_species_relative_weight_multiplier
                    .denominator(),
            ),
            (23, 20)
        );
        assert_eq!(
            (
                policy
                    .treasure_branch_relative_weight_multiplier
                    .numerator(),
                policy
                    .treasure_branch_relative_weight_multiplier
                    .denominator(),
            ),
            (23, 20)
        );
        assert_eq!(
            policy.modifier_stage,
            GoldFishingRodModifierStage::BeforeSharedFishingCaps
        );
    }

    #[test]
    fn non_gold_ordinary_rod_cannot_borrow_side_grade_modifiers() {
        assert_eq!(
            gold_fishing_rod_side_grade_policy(EquipmentTier::Diamond, true),
            Err(GoldFishingRodPolicyError::NotGoldFishingRod)
        );
    }

    #[test]
    fn non_ordinary_gold_like_definition_fails_closed() {
        assert_eq!(
            gold_fishing_rod_side_grade_policy(EquipmentTier::Gold, false),
            Err(GoldFishingRodPolicyError::InvalidOrdinaryRod(
                FishingCapabilityError::NotOrdinaryFishingRod
            ))
        );
    }
}
