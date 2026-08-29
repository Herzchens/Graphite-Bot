use crate::equipment_policy::{EquipmentMaterial, EquipmentSlot, EquipmentTier};
use serde::Serialize;
use thiserror::Error;

const MONEY_ROUNDING_UNIT: i128 = 100;
const MONEY_MINIMUM: i128 = 100;
const PERCENT_DENOMINATOR: i128 = 100;
const GOLD_AEXP_PER_BASE_UNIT: i128 = 250;
const CANCEL_REFUND_NUMERATOR: i128 = 4;
const CANCEL_REFUND_DENOMINATOR: i128 = 5;

pub type RepairTier = EquipmentTier;
pub type RepairSlot = EquipmentSlot;
pub type RepairMaterial = EquipmentMaterial;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct RepairPreview {
    pub tier: RepairTier,
    pub slot: RepairSlot,
    pub current_durability: i64,
    pub max_durability: i64,
    pub missing_durability: i64,
    pub recraft_appraisal: i64,
    pub money_cost: i64,
    pub material: RepairMaterial,
    pub material_units: i64,
    pub activity_xp_cost: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct RepairCancelRefund {
    pub material_units: i64,
    pub money: i64,
    pub activity_xp: i64,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum RepairMathError {
    #[error("maximum durability must be positive")]
    InvalidMaxDurability,
    #[error("current durability must be between zero and maximum durability")]
    InvalidCurrentDurability,
    #[error("repair requires positive structural recraft appraisal")]
    InvalidRecraftAppraisal,
    #[error("equipment has no missing durability")]
    NoMissingDurability,
    #[error("the canonical repair Money ratio is not defined for tier {0:?}")]
    UndefinedTierRepairRatio(RepairTier),
    #[error("eligible repair material units cannot be negative")]
    NegativeEligibleMaterialUnits,
    #[error("repair arithmetic exceeded supported integer bounds")]
    ArithmeticOverflow,
}

/// Computes the frozen full-repair economic recipe for one already-resolved ItemInstance.
///
/// `recraft_appraisal` must be the canonical structural appraisal resolved by the owning item/
/// appraisal layer. This kernel intentionally does not derive creation-roll or +N appraisal state,
/// mutate durability, reserve assets, create a service job, apply Grinding/Mosaic, or invent the
/// still-unspecified Repair time formula.
///
/// Starter Leather Armor is represented because its primary material is frozen, but the active
/// specification does not define a `TierRepairRatio` for Leather. It therefore returns
/// [`RepairMathError::UndefinedTierRepairRatio`] instead of borrowing another tier's Money ratio.
pub fn preview_full_repair(
    tier: RepairTier,
    slot: RepairSlot,
    current_durability: i64,
    max_durability: i64,
    recraft_appraisal: i64,
) -> Result<RepairPreview, RepairMathError> {
    if max_durability <= 0 {
        return Err(RepairMathError::InvalidMaxDurability);
    }
    if current_durability < 0 || current_durability > max_durability {
        return Err(RepairMathError::InvalidCurrentDurability);
    }
    if recraft_appraisal <= 0 {
        return Err(RepairMathError::InvalidRecraftAppraisal);
    }

    let missing_durability = max_durability
        .checked_sub(current_durability)
        .ok_or(RepairMathError::ArithmeticOverflow)?;
    if missing_durability == 0 {
        return Err(RepairMathError::NoMissingDurability);
    }

    let ratio_percent = tier
        .repair_ratio_percent()
        .ok_or(RepairMathError::UndefinedTierRepairRatio(tier))?;
    let base_material_units = i128::from(slot.base_material_units());
    let missing = i128::from(missing_durability);
    let maximum = i128::from(max_durability);
    let appraisal = i128::from(recraft_appraisal);

    let money_numerator = checked_mul(checked_mul(appraisal, ratio_percent)?, missing)?;
    let money_denominator = checked_mul(PERCENT_DENOMINATOR, maximum)?;
    let money_cost = round100_with_minimum(money_numerator, money_denominator)?;

    let material_units = ceil_ratio(checked_mul(base_material_units, missing)?, maximum)?;
    let activity_xp_cost = if tier == RepairTier::Gold {
        ceil_ratio(
            checked_mul(
                checked_mul(GOLD_AEXP_PER_BASE_UNIT, base_material_units)?,
                missing,
            )?,
            maximum,
        )?
    } else {
        0
    };

    Ok(RepairPreview {
        tier,
        slot,
        current_durability,
        max_durability,
        missing_durability,
        recraft_appraisal,
        money_cost: to_i64(money_cost)?,
        material: tier.material(),
        material_units: to_i64(material_units)?,
        activity_xp_cost: to_i64(activity_xp_cost)?,
    })
}

/// Applies the frozen Repair-cancel refund rule to already-eligible material units.
///
/// Callers must pass the material quantity after any modifier that changes the amount actually
/// eligible for refund. Cancellation returns `floor(80%)` of those units and never refunds Money or
/// Activity EXP. Returning the original ItemInstance unchanged remains the owning service's job.
pub fn repair_cancel_refund(
    eligible_material_units: i64,
) -> Result<RepairCancelRefund, RepairMathError> {
    if eligible_material_units < 0 {
        return Err(RepairMathError::NegativeEligibleMaterialUnits);
    }
    let material_units = checked_mul(i128::from(eligible_material_units), CANCEL_REFUND_NUMERATOR)?
        / CANCEL_REFUND_DENOMINATOR;
    Ok(RepairCancelRefund {
        material_units: to_i64(material_units)?,
        money: 0,
        activity_xp: 0,
    })
}

fn round100_with_minimum(numerator: i128, denominator: i128) -> Result<i128, RepairMathError> {
    debug_assert!(numerator >= 0 && denominator > 0);
    if numerator < checked_mul(MONEY_MINIMUM, denominator)? {
        return Ok(MONEY_MINIMUM);
    }

    let rounding_offset = checked_mul(MONEY_ROUNDING_UNIT / 2, denominator)?;
    let rounded_units = numerator
        .checked_add(rounding_offset)
        .ok_or(RepairMathError::ArithmeticOverflow)?
        / checked_mul(MONEY_ROUNDING_UNIT, denominator)?;
    checked_mul(rounded_units, MONEY_ROUNDING_UNIT)
}

fn ceil_ratio(numerator: i128, denominator: i128) -> Result<i128, RepairMathError> {
    debug_assert!(numerator > 0 && denominator > 0);
    let quotient = numerator / denominator;
    quotient
        .checked_add(i128::from(numerator % denominator != 0))
        .ok_or(RepairMathError::ArithmeticOverflow)
}

fn checked_mul(left: i128, right: i128) -> Result<i128, RepairMathError> {
    left.checked_mul(right)
        .ok_or(RepairMathError::ArithmeticOverflow)
}

fn to_i64(value: i128) -> Result<i64, RepairMathError> {
    i64::try_from(value).map_err(|_| RepairMathError::ArithmeticOverflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_broken_repairs_match_every_frozen_tier_ratio() {
        for (tier, ratio, material) in [
            (RepairTier::Wood, 10, RepairMaterial::WoodLog),
            (RepairTier::Stone, 11, RepairMaterial::Stone),
            (RepairTier::Copper, 13, RepairMaterial::CopperIngot),
            (RepairTier::Gold, 25, RepairMaterial::GoldIngot),
            (RepairTier::Iron, 14, RepairMaterial::IronIngot),
            (RepairTier::Diamond, 16, RepairMaterial::Diamond),
            (RepairTier::Obsidian, 18, RepairMaterial::Obsidian),
            (RepairTier::Netherite, 20, RepairMaterial::NetheriteScrap),
            (RepairTier::Graphite, 23, RepairMaterial::GraphiteLayer),
        ] {
            let preview =
                preview_full_repair(tier, RepairSlot::Pickaxe, 0, 1_000, 100_000).unwrap();
            assert_eq!(preview.money_cost, 1_000 * ratio);
            assert_eq!(preview.material, material);
            assert_eq!(preview.material_units, 2);
            assert_eq!(
                preview.activity_xp_cost,
                if tier == RepairTier::Gold { 500 } else { 0 }
            );
        }
    }

    #[test]
    fn slot_material_units_and_gold_aexp_follow_exact_ceil_rules() {
        for (slot, full_units) in [
            (RepairSlot::Pickaxe, 2),
            (RepairSlot::Sword, 2),
            (RepairSlot::FishingRod, 2),
            (RepairSlot::Helmet, 2),
            (RepairSlot::Chestplate, 4),
            (RepairSlot::Leggings, 3),
            (RepairSlot::Boots, 2),
        ] {
            let full = preview_full_repair(RepairTier::Gold, slot, 0, 100, 100_000).unwrap();
            assert_eq!(full.material_units, full_units);
            assert_eq!(full.activity_xp_cost, 250 * full_units);

            let one_percent =
                preview_full_repair(RepairTier::Gold, slot, 99, 100, 100_000).unwrap();
            assert_eq!(one_percent.material_units, 1);
            assert_eq!(one_percent.activity_xp_cost, (250 * full_units + 99) / 100);
        }
    }

    #[test]
    fn money_uses_minimum_and_exact_round100_half_up() {
        let minimum =
            preview_full_repair(RepairTier::Wood, RepairSlot::Sword, 99, 100, 1_000).unwrap();
        assert_eq!(minimum.money_cost, 100);

        let below_half =
            preview_full_repair(RepairTier::Wood, RepairSlot::Sword, 1, 2, 2_999).unwrap();
        assert_eq!(below_half.money_cost, 100);

        let half = preview_full_repair(RepairTier::Wood, RepairSlot::Sword, 1, 2, 3_000).unwrap();
        assert_eq!(half.money_cost, 200);

        let above_half =
            preview_full_repair(RepairTier::Wood, RepairSlot::Sword, 1, 2, 3_001).unwrap();
        assert_eq!(above_half.money_cost, 200);
    }

    #[test]
    fn starter_leather_ratio_gap_is_explicit_instead_of_guessed() {
        assert_eq!(
            RepairTier::StarterLeather.material(),
            RepairMaterial::Leather
        );
        assert_eq!(
            preview_full_repair(
                RepairTier::StarterLeather,
                RepairSlot::Helmet,
                0,
                100,
                10_000,
            ),
            Err(RepairMathError::UndefinedTierRepairRatio(
                RepairTier::StarterLeather
            ))
        );
    }

    #[test]
    fn cancel_refund_is_floor_eighty_percent_and_never_refunds_money_or_aexp() {
        for (reserved, returned) in [
            (0, 0),
            (1, 0),
            (2, 1),
            (3, 2),
            (4, 3),
            (5, 4),
            (9, 7),
            (10, 8),
        ] {
            assert_eq!(
                repair_cancel_refund(reserved).unwrap(),
                RepairCancelRefund {
                    material_units: returned,
                    money: 0,
                    activity_xp: 0,
                }
            );
        }
    }

    #[test]
    fn cost_invariants_hold_across_small_durability_domains() {
        for tier in [
            RepairTier::Wood,
            RepairTier::Stone,
            RepairTier::Copper,
            RepairTier::Gold,
            RepairTier::Iron,
            RepairTier::Diamond,
            RepairTier::Obsidian,
            RepairTier::Netherite,
            RepairTier::Graphite,
        ] {
            for slot in [
                RepairSlot::Pickaxe,
                RepairSlot::Sword,
                RepairSlot::FishingRod,
                RepairSlot::Helmet,
                RepairSlot::Chestplate,
                RepairSlot::Leggings,
                RepairSlot::Boots,
            ] {
                for maximum in 1..=64 {
                    let mut previous_money = 0;
                    let mut previous_material = 0;
                    let mut previous_aexp = 0;
                    for missing in 1..=maximum {
                        let current = maximum - missing;
                        let preview =
                            preview_full_repair(tier, slot, current, maximum, 123_456).unwrap();
                        assert!(preview.money_cost >= 100 && preview.money_cost % 100 == 0);
                        assert!((1..=slot.base_material_units()).contains(&preview.material_units));
                        assert!(preview.money_cost >= previous_money);
                        assert!(preview.material_units >= previous_material);
                        assert!(preview.activity_xp_cost >= previous_aexp);
                        if tier != RepairTier::Gold {
                            assert_eq!(preview.activity_xp_cost, 0);
                        }
                        previous_money = preview.money_cost;
                        previous_material = preview.material_units;
                        previous_aexp = preview.activity_xp_cost;
                    }
                }
            }
        }
    }

    #[test]
    fn invalid_boundaries_and_intermediate_overflow_are_rejected() {
        assert_eq!(
            preview_full_repair(RepairTier::Wood, RepairSlot::Pickaxe, 0, 0, 1_000),
            Err(RepairMathError::InvalidMaxDurability)
        );
        assert_eq!(
            preview_full_repair(RepairTier::Wood, RepairSlot::Pickaxe, -1, 10, 1_000),
            Err(RepairMathError::InvalidCurrentDurability)
        );
        assert_eq!(
            preview_full_repair(RepairTier::Wood, RepairSlot::Pickaxe, 11, 10, 1_000),
            Err(RepairMathError::InvalidCurrentDurability)
        );
        assert_eq!(
            preview_full_repair(RepairTier::Wood, RepairSlot::Pickaxe, 0, 10, 0),
            Err(RepairMathError::InvalidRecraftAppraisal)
        );
        assert_eq!(
            preview_full_repair(RepairTier::Wood, RepairSlot::Pickaxe, 10, 10, 1_000),
            Err(RepairMathError::NoMissingDurability)
        );
        assert_eq!(
            repair_cancel_refund(-1),
            Err(RepairMathError::NegativeEligibleMaterialUnits)
        );

        let largest =
            preview_full_repair(RepairTier::Graphite, RepairSlot::Chestplate, 0, 1, i64::MAX)
                .unwrap();
        assert_eq!(largest.money_cost, 2_121_375_568_476_598_400);

        assert_eq!(
            preview_full_repair(
                RepairTier::Graphite,
                RepairSlot::Chestplate,
                0,
                i64::MAX,
                i64::MAX,
            ),
            Err(RepairMathError::ArithmeticOverflow)
        );
    }
}
