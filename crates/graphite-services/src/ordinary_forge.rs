use crate::equipment_policy::{
    BaseEquipmentAppraisal, EquipmentAppraisalError, EquipmentMaterial, EquipmentSlot,
    EquipmentTier, base_equipment_appraisal,
};
use crate::forge::ForgePostConfirmCancellation;
use serde::Serialize;
use thiserror::Error;

const SECONDS_PER_MINUTE: i64 = 60;
const FORGE_FEE_RATE_NUMERATOR: i128 = 8;
const FORGE_FEE_RATE_DENOMINATOR: i128 = 100;
const FORGE_MONEY_ROUNDING_UNIT: i128 = 1_000;
const MINIMUM_FORGE_MONEY_COST: i128 = 1_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FreshForgeOutcomePolicy {
    Guaranteed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FreshForgeOutputLocation {
    ToolLocker,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FreshOrdinaryForgePreview {
    pub tier: EquipmentTier,
    pub slot: EquipmentSlot,
    pub base_appraisal: BaseEquipmentAppraisal,
    pub primary_material: EquipmentMaterial,
    pub primary_material_units: i64,
    pub auxiliary_wood_logs: i64,
    pub money_cost: i64,
    pub activity_xp_cost: i64,
    pub duration_seconds: i64,
    pub outcome: FreshForgeOutcomePolicy,
    pub cancellation_after_confirm: ForgePostConfirmCancellation,
    pub output_location: FreshForgeOutputLocation,
    pub output_upgrade_level: u64,
    pub requires_new_positive_creation_roll: bool,
    pub npc_resale_path: bool,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum FreshOrdinaryForgePolicyError {
    #[error("tier {0:?} is not a fresh ordinary Forge tier")]
    UnsupportedFreshForgeTier(EquipmentTier),
    #[error("Gold fresh Forge does not support equipment slot {0:?}")]
    UnsupportedGoldSlot(EquipmentSlot),
    #[error("base equipment appraisal failed: {0}")]
    EquipmentAppraisal(EquipmentAppraisalError),
    #[error("ordinary Forge policy arithmetic exceeded supported integer bounds")]
    ArithmeticOverflow,
}

impl From<EquipmentAppraisalError> for FreshOrdinaryForgePolicyError {
    fn from(value: EquipmentAppraisalError) -> Self {
        Self::EquipmentAppraisal(value)
    }
}

/// Previews one fresh ordinary-equipment Forge recipe from Wood through Obsidian.
///
/// The preview deliberately resolves the standard ordinary base-appraisal table itself rather than
/// accepting an arbitrary caller-provided appraisal. This prevents a special-definition override
/// from being accidentally treated as an ordinary fresh Forge recipe. Netherite and Graphite are
/// promotions and therefore reject this path; Starter Leather is not a Forge input/output tier.
///
/// Fresh Forge is guaranteed, creates a new ItemInstance at +0, and requires a new **positive**
/// creation roll. The active specification does not freeze that roll's RNG distribution or storage
/// precision, so this pure policy records the requirement without generating a roll. It also does
/// not reserve assets, create a service job, choose a recipe-specific cancellation rule, or mutate
/// persistent state.
pub fn preview_fresh_ordinary_forge(
    tier: EquipmentTier,
    slot: EquipmentSlot,
) -> Result<FreshOrdinaryForgePreview, FreshOrdinaryForgePolicyError> {
    validate_fresh_forge_target(tier, slot)?;

    let base_appraisal = base_equipment_appraisal(tier, slot, None)?;
    let money_cost = forge_money_cost(base_appraisal.value)?;
    let (activity_xp_cost, duration_seconds) = tier_aexp_and_duration(tier)?;

    Ok(FreshOrdinaryForgePreview {
        tier,
        slot,
        base_appraisal,
        primary_material: tier.material(),
        primary_material_units: primary_material_units(slot),
        auxiliary_wood_logs: auxiliary_wood_logs(slot),
        money_cost,
        activity_xp_cost,
        duration_seconds,
        outcome: FreshForgeOutcomePolicy::Guaranteed,
        cancellation_after_confirm: ForgePostConfirmCancellation::Unspecified,
        output_location: FreshForgeOutputLocation::ToolLocker,
        output_upgrade_level: 0,
        requires_new_positive_creation_roll: true,
        npc_resale_path: false,
    })
}

fn validate_fresh_forge_target(
    tier: EquipmentTier,
    slot: EquipmentSlot,
) -> Result<(), FreshOrdinaryForgePolicyError> {
    match tier {
        EquipmentTier::StarterLeather | EquipmentTier::Netherite | EquipmentTier::Graphite => {
            return Err(FreshOrdinaryForgePolicyError::UnsupportedFreshForgeTier(
                tier,
            ));
        }
        EquipmentTier::Gold
            if matches!(
                slot,
                EquipmentSlot::Helmet
                    | EquipmentSlot::Chestplate
                    | EquipmentSlot::Leggings
                    | EquipmentSlot::Boots
            ) =>
        {
            return Err(FreshOrdinaryForgePolicyError::UnsupportedGoldSlot(slot));
        }
        EquipmentTier::Wood
        | EquipmentTier::Stone
        | EquipmentTier::Copper
        | EquipmentTier::Gold
        | EquipmentTier::Iron
        | EquipmentTier::Diamond
        | EquipmentTier::Obsidian => {}
    }
    Ok(())
}

const fn primary_material_units(slot: EquipmentSlot) -> i64 {
    match slot {
        EquipmentSlot::Pickaxe => 3,
        EquipmentSlot::Sword | EquipmentSlot::FishingRod => 2,
        EquipmentSlot::Helmet => 5,
        EquipmentSlot::Chestplate => 8,
        EquipmentSlot::Leggings => 7,
        EquipmentSlot::Boots => 4,
    }
}

const fn auxiliary_wood_logs(slot: EquipmentSlot) -> i64 {
    match slot {
        EquipmentSlot::Pickaxe | EquipmentSlot::Sword | EquipmentSlot::FishingRod => 1,
        EquipmentSlot::Helmet
        | EquipmentSlot::Chestplate
        | EquipmentSlot::Leggings
        | EquipmentSlot::Boots => 0,
    }
}

fn forge_money_cost(base_appraisal: i64) -> Result<i64, FreshOrdinaryForgePolicyError> {
    if base_appraisal < 0 {
        return Err(FreshOrdinaryForgePolicyError::ArithmeticOverflow);
    }

    let numerator = i128::from(base_appraisal)
        .checked_mul(FORGE_FEE_RATE_NUMERATOR)
        .ok_or(FreshOrdinaryForgePolicyError::ArithmeticOverflow)?;
    let rounding_denominator = FORGE_FEE_RATE_DENOMINATOR
        .checked_mul(FORGE_MONEY_ROUNDING_UNIT)
        .ok_or(FreshOrdinaryForgePolicyError::ArithmeticOverflow)?;
    let rounded_units = numerator
        .checked_add(rounding_denominator / 2)
        .ok_or(FreshOrdinaryForgePolicyError::ArithmeticOverflow)?
        / rounding_denominator;
    let rounded = rounded_units
        .checked_mul(FORGE_MONEY_ROUNDING_UNIT)
        .ok_or(FreshOrdinaryForgePolicyError::ArithmeticOverflow)?;
    let cost = rounded.max(MINIMUM_FORGE_MONEY_COST);

    i64::try_from(cost).map_err(|_| FreshOrdinaryForgePolicyError::ArithmeticOverflow)
}

fn tier_aexp_and_duration(
    tier: EquipmentTier,
) -> Result<(i64, i64), FreshOrdinaryForgePolicyError> {
    let policy: (i64, i64) = match tier {
        EquipmentTier::Wood => (0, 2),
        EquipmentTier::Stone => (50, 3),
        EquipmentTier::Copper => (100, 5),
        EquipmentTier::Gold => (300, 8),
        EquipmentTier::Iron => (250, 8),
        EquipmentTier::Diamond => (700, 15),
        EquipmentTier::Obsidian => (1_800, 30),
        EquipmentTier::StarterLeather | EquipmentTier::Netherite | EquipmentTier::Graphite => {
            return Err(FreshOrdinaryForgePolicyError::UnsupportedFreshForgeTier(
                tier,
            ));
        }
    };

    let duration_seconds = policy
        .1
        .checked_mul(SECONDS_PER_MINUTE)
        .ok_or(FreshOrdinaryForgePolicyError::ArithmeticOverflow)?;
    Ok((policy.0, duration_seconds))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::equipment_policy::BaseEquipmentAppraisalSource;

    const ALL_SLOTS: [EquipmentSlot; 7] = [
        EquipmentSlot::Pickaxe,
        EquipmentSlot::Sword,
        EquipmentSlot::FishingRod,
        EquipmentSlot::Helmet,
        EquipmentSlot::Chestplate,
        EquipmentSlot::Leggings,
        EquipmentSlot::Boots,
    ];

    #[test]
    fn primary_and_auxiliary_recipe_units_match_frozen_slot_table() {
        let expected = [
            (EquipmentSlot::Pickaxe, 3, 1),
            (EquipmentSlot::Sword, 2, 1),
            (EquipmentSlot::FishingRod, 2, 1),
            (EquipmentSlot::Helmet, 5, 0),
            (EquipmentSlot::Chestplate, 8, 0),
            (EquipmentSlot::Leggings, 7, 0),
            (EquipmentSlot::Boots, 4, 0),
        ];

        for (slot, primary, wood) in expected {
            assert_eq!(primary_material_units(slot), primary);
            assert_eq!(auxiliary_wood_logs(slot), wood);
        }
    }

    #[test]
    fn tier_cost_time_and_material_policy_match_frozen_values() {
        let tiers = [
            (EquipmentTier::Wood, EquipmentMaterial::WoodLog, 0, 2),
            (EquipmentTier::Stone, EquipmentMaterial::Stone, 50, 3),
            (
                EquipmentTier::Copper,
                EquipmentMaterial::CopperIngot,
                100,
                5,
            ),
            (EquipmentTier::Gold, EquipmentMaterial::GoldIngot, 300, 8),
            (EquipmentTier::Iron, EquipmentMaterial::IronIngot, 250, 8),
            (EquipmentTier::Diamond, EquipmentMaterial::Diamond, 700, 15),
            (
                EquipmentTier::Obsidian,
                EquipmentMaterial::Obsidian,
                1_800,
                30,
            ),
        ];

        for (tier, material, aexp, minutes) in tiers {
            let slots: &[EquipmentSlot] = if tier == EquipmentTier::Gold {
                &ALL_SLOTS[..3]
            } else {
                &ALL_SLOTS
            };
            for &slot in slots {
                let preview = preview_fresh_ordinary_forge(tier, slot).unwrap();
                assert_eq!(preview.primary_material, material, "{tier:?} {slot:?}");
                assert_eq!(preview.activity_xp_cost, aexp, "{tier:?} {slot:?}");
                assert_eq!(
                    preview.duration_seconds,
                    minutes * SECONDS_PER_MINUTE,
                    "{tier:?} {slot:?}"
                );
                assert_eq!(
                    preview.base_appraisal.source,
                    BaseEquipmentAppraisalSource::StandardTable
                );
                assert_eq!(preview.outcome, FreshForgeOutcomePolicy::Guaranteed);
                assert_eq!(
                    preview.cancellation_after_confirm,
                    ForgePostConfirmCancellation::Unspecified
                );
                assert_eq!(
                    preview.output_location,
                    FreshForgeOutputLocation::ToolLocker
                );
                assert_eq!(preview.output_upgrade_level, 0);
                assert!(preview.requires_new_positive_creation_roll);
                assert!(!preview.npc_resale_path);
            }
        }
    }

    #[test]
    fn money_cost_matches_frozen_standard_appraisals_across_all_valid_targets() {
        let expected = [
            (
                EquipmentTier::Wood,
                [1_000, 1_000, 1_000, 1_000, 1_000, 1_000, 1_000],
            ),
            (
                EquipmentTier::Stone,
                [1_000, 1_000, 1_000, 1_000, 1_000, 1_000, 1_000],
            ),
            (
                EquipmentTier::Copper,
                [2_000, 2_000, 2_000, 2_000, 3_000, 2_000, 2_000],
            ),
            (EquipmentTier::Gold, [9_000, 9_000, 9_000, 0, 0, 0, 0]),
            (
                EquipmentTier::Iron,
                [6_000, 6_000, 6_000, 5_000, 8_000, 7_000, 4_000],
            ),
            (
                EquipmentTier::Diamond,
                [22_000, 22_000, 22_000, 17_000, 29_000, 25_000, 15_000],
            ),
            (
                EquipmentTier::Obsidian,
                [70_000, 70_000, 70_000, 56_000, 95_000, 81_000, 49_000],
            ),
        ];

        for (tier, costs) in expected {
            for (slot, expected_cost) in ALL_SLOTS.into_iter().zip(costs) {
                if tier == EquipmentTier::Gold
                    && matches!(
                        slot,
                        EquipmentSlot::Helmet
                            | EquipmentSlot::Chestplate
                            | EquipmentSlot::Leggings
                            | EquipmentSlot::Boots
                    )
                {
                    continue;
                }
                let preview = preview_fresh_ordinary_forge(tier, slot).unwrap();
                assert_eq!(preview.money_cost, expected_cost, "{tier:?} {slot:?}");
            }
        }
    }

    #[test]
    fn round1000_is_half_up_before_minimum_fee_is_applied() {
        assert_eq!(forge_money_cost(31_249).unwrap(), 2_000);
        assert_eq!(forge_money_cost(31_250).unwrap(), 3_000);
        assert_eq!(forge_money_cost(0).unwrap(), 1_000);
    }

    #[test]
    fn invalid_fresh_targets_fail_closed() {
        for tier in [
            EquipmentTier::StarterLeather,
            EquipmentTier::Netherite,
            EquipmentTier::Graphite,
        ] {
            assert_eq!(
                preview_fresh_ordinary_forge(tier, EquipmentSlot::Pickaxe),
                Err(FreshOrdinaryForgePolicyError::UnsupportedFreshForgeTier(
                    tier
                ))
            );
        }

        for slot in [
            EquipmentSlot::Helmet,
            EquipmentSlot::Chestplate,
            EquipmentSlot::Leggings,
            EquipmentSlot::Boots,
        ] {
            assert_eq!(
                preview_fresh_ordinary_forge(EquipmentTier::Gold, slot),
                Err(FreshOrdinaryForgePolicyError::UnsupportedGoldSlot(slot))
            );
        }
    }

    #[test]
    fn money_helper_rejects_negative_input() {
        assert_eq!(
            forge_money_cost(-1),
            Err(FreshOrdinaryForgePolicyError::ArithmeticOverflow)
        );
    }
}
