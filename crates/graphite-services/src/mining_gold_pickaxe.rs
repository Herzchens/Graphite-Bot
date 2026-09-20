use serde::Serialize;
use thiserror::Error;

use crate::{
    EquipmentTier, MiningCapabilityClass, MiningCapabilityError, mining_pickaxe_max_capability,
};

pub const GOLD_PICKAXE_ACTION_SPEED_RATING_PERCENT: u8 = 10;
pub const GOLD_PICKAXE_ELIGIBLE_TREASURE_RARE_RELATIVE_WEIGHT_INCREASE_PERCENT: u8 = 20;
pub const GOLD_PICKAXE_ELIGIBLE_TREASURE_RARE_RELATIVE_WEIGHT_NUMERATOR: u8 = 6;
pub const GOLD_PICKAXE_ELIGIBLE_TREASURE_RARE_RELATIVE_WEIGHT_DENOMINATOR: u8 = 5;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GoldMiningPickaxeModifierStage {
    SourceLocalBeforeSharedMiningCaps,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct GoldMiningPickaxeSideGradePolicy {
    pub max_capability: MiningCapabilityClass,
    pub action_speed_rating_percent: u8,
    pub eligible_treasure_rare_relative_weight_numerator: u8,
    pub eligible_treasure_rare_relative_weight_denominator: u8,
    pub modifier_stage: GoldMiningPickaxeModifierStage,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum GoldMiningPickaxePolicyError {
    #[error("Pickaxe definition is not an ordinary Pickaxe")]
    NotOrdinaryPickaxe,
    #[error(transparent)]
    InvalidPickaxeTier(#[from] MiningCapabilityError),
    #[error("ordinary Pickaxe is not the Gold side-grade")]
    NotGoldPickaxe,
}

/// Returns the frozen source-local modifiers of the ordinary Gold Pickaxe side-grade.
///
/// The caller must already have resolved authoritative ordinary-Pickaxe classification from the
/// pinned ItemDefinition version. Gold-like request metadata or a non-ordinary/special definition
/// must not receive these bonuses.
///
/// The +10% value is a Mining action-speed **rating input**, not a direct cooldown multiplier. The
/// +20% value is an exact relative eligible treasure/rare-find weight multiplier of 6/5, not
/// +20 percentage points of final probability. This function deliberately does not decide which
/// Mining rows belong to that eligible pool because the current repository has no canonical
/// treasure/rare-find classifier for the future roll owner.
///
/// The returned capability is re-derived through the canonical Mining capability owner, so Gold
/// remains capped at C3 instead of acquiring a second capability table here. Final modifier
/// composition, shared caps, depletion/density effects, normalization and RNG remain outside this
/// source-local policy.
pub fn gold_mining_pickaxe_side_grade_policy(
    tier: EquipmentTier,
    is_ordinary_pickaxe: bool,
) -> Result<GoldMiningPickaxeSideGradePolicy, GoldMiningPickaxePolicyError> {
    if !is_ordinary_pickaxe {
        return Err(GoldMiningPickaxePolicyError::NotOrdinaryPickaxe);
    }

    let max_capability = mining_pickaxe_max_capability(tier)?;
    if tier != EquipmentTier::Gold {
        return Err(GoldMiningPickaxePolicyError::NotGoldPickaxe);
    }

    Ok(GoldMiningPickaxeSideGradePolicy {
        max_capability,
        action_speed_rating_percent: GOLD_PICKAXE_ACTION_SPEED_RATING_PERCENT,
        eligible_treasure_rare_relative_weight_numerator:
            GOLD_PICKAXE_ELIGIBLE_TREASURE_RARE_RELATIVE_WEIGHT_NUMERATOR,
        eligible_treasure_rare_relative_weight_denominator:
            GOLD_PICKAXE_ELIGIBLE_TREASURE_RARE_RELATIVE_WEIGHT_DENOMINATOR,
        modifier_stage: GoldMiningPickaxeModifierStage::SourceLocalBeforeSharedMiningCaps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gold_side_grade_matches_every_frozen_source_local_value() {
        let policy = gold_mining_pickaxe_side_grade_policy(EquipmentTier::Gold, true).unwrap();

        assert_eq!(policy.max_capability, MiningCapabilityClass::C3);
        assert_eq!(policy.action_speed_rating_percent, 10);
        assert_eq!(
            (
                policy.eligible_treasure_rare_relative_weight_numerator,
                policy.eligible_treasure_rare_relative_weight_denominator,
            ),
            (6, 5)
        );
        assert_eq!(
            policy.modifier_stage,
            GoldMiningPickaxeModifierStage::SourceLocalBeforeSharedMiningCaps
        );
    }

    #[test]
    fn non_gold_ordinary_pickaxes_cannot_borrow_side_grade_modifiers() {
        for tier in [
            EquipmentTier::Wood,
            EquipmentTier::Stone,
            EquipmentTier::Copper,
            EquipmentTier::Iron,
            EquipmentTier::Diamond,
            EquipmentTier::Obsidian,
            EquipmentTier::Netherite,
            EquipmentTier::Graphite,
        ] {
            assert_eq!(
                gold_mining_pickaxe_side_grade_policy(tier, true),
                Err(GoldMiningPickaxePolicyError::NotGoldPickaxe)
            );
        }
    }

    #[test]
    fn non_ordinary_gold_like_definition_fails_closed() {
        assert_eq!(
            gold_mining_pickaxe_side_grade_policy(EquipmentTier::Gold, false),
            Err(GoldMiningPickaxePolicyError::NotOrdinaryPickaxe)
        );
    }

    #[test]
    fn non_pickaxe_tier_still_uses_the_canonical_capability_guard() {
        assert_eq!(
            gold_mining_pickaxe_side_grade_policy(EquipmentTier::StarterLeather, true),
            Err(GoldMiningPickaxePolicyError::InvalidPickaxeTier(
                MiningCapabilityError::UnsupportedPickaxeTier(EquipmentTier::StarterLeather)
            ))
        );
    }
}
