use crate::equipment_policy::EquipmentTier;
use crate::percentage_fee::checked_ceil_percentage;
use serde::Serialize;
use thiserror::Error;

const PROTECTION_PERCENT: u8 = 60;
const UNBIND_PERCENT: u8 = 20;
const SECONDS_PER_DAY: i64 = 24 * 60 * 60;

pub const SOULBIND_MIN_REBIRTH: u64 = 1;
pub const SOULBIND_REBIND_COOLDOWN_SECONDS: i64 = 7 * SECONDS_PER_DAY;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SoulBindTierComponent {
    NetheriteBillet,
    GraphiteLayer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct SoulBindBindingPackage {
    pub tier: EquipmentTier,
    pub soulbind_rune_quantity: u32,
    pub onyx_quantity: u32,
    pub platinum_ingot_quantity: u32,
    pub tier_component: SoulBindTierComponent,
    pub tier_component_quantity: u32,
    pub fixed_money_cost: i64,
    pub activity_xp_cost: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct SoulBindBindingPreview {
    pub package: SoulBindBindingPackage,
    pub rebirth_count: u64,
    pub current_enhanced_appraisal: i64,
    pub initial_protection_charge: i64,
    pub total_money_cost: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct SoulBindTopUpPreview {
    pub previous_enhanced_appraisal: i64,
    pub new_enhanced_appraisal: i64,
    pub positive_appraisal_delta: i64,
    pub money_charge: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct SoulBindUnbindPreview {
    pub current_enhanced_appraisal: i64,
    pub money_fee: i64,
    pub rebind_cooldown_seconds: i64,
    pub refunds_binding_resources: bool,
    pub requires_unprotected: bool,
    pub requires_unfavorited: bool,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum SoulBindPolicyError {
    #[error("SoulBind is only eligible for Netherite or Graphite equipment")]
    IneligibleTier,
    #[error("SoulBind is only eligible for ordinary equipment")]
    NonOrdinaryEquipment,
    #[error("SoulBind requires account Rebirth {required} or higher; current Rebirth is {current}")]
    RebirthRequired { required: u64, current: u64 },
    #[error("enhanced canonical appraisal cannot be negative")]
    NegativeEnhancedAppraisal,
    #[error("SoulBind Money arithmetic exceeded supported integer bounds")]
    ArithmeticOverflow,
}

/// Returns the frozen tier-specific SoulBind package row.
///
/// This function does not by itself prove full binding eligibility. The owning definition resolver
/// must still establish that the target is ordinary equipment, and the binding preview additionally
/// enforces Rebirth and appraisal requirements.
pub const fn soulbind_binding_package(
    tier: EquipmentTier,
) -> Result<SoulBindBindingPackage, SoulBindPolicyError> {
    match tier {
        EquipmentTier::Netherite => Ok(SoulBindBindingPackage {
            tier,
            soulbind_rune_quantity: 1,
            onyx_quantity: 20,
            platinum_ingot_quantity: 8,
            tier_component: SoulBindTierComponent::NetheriteBillet,
            tier_component_quantity: 2,
            fixed_money_cost: 250_000,
            activity_xp_cost: 25_000,
        }),
        EquipmentTier::Graphite => Ok(SoulBindBindingPackage {
            tier,
            soulbind_rune_quantity: 1,
            onyx_quantity: 32,
            platinum_ingot_quantity: 12,
            tier_component: SoulBindTierComponent::GraphiteLayer,
            tier_component_quantity: 2,
            fixed_money_cost: 500_000,
            activity_xp_cost: 50_000,
        }),
        _ => Err(SoulBindPolicyError::IneligibleTier),
    }
}

/// Previews the frozen initial SoulBind package and Money charge.
///
/// The caller must supply an already-resolved `is_ordinary_equipment` classification and current
/// `EnhancedCanonicalAppraisal`. The future owning stateful service must derive/revalidate those
/// values from the authoritative versioned ItemDefinition/ItemInstance instead of trusting Discord
/// input or cached state. This pure kernel does not invent a definition-classification schema, prove
/// Rune/material ownership, reserve assets, or mutate binding state.
pub fn preview_soulbind_binding(
    tier: EquipmentTier,
    is_ordinary_equipment: bool,
    rebirth_count: u64,
    current_enhanced_appraisal: i64,
) -> Result<SoulBindBindingPreview, SoulBindPolicyError> {
    if !is_ordinary_equipment {
        return Err(SoulBindPolicyError::NonOrdinaryEquipment);
    }
    let package = soulbind_binding_package(tier)?;
    if rebirth_count < SOULBIND_MIN_REBIRTH {
        return Err(SoulBindPolicyError::RebirthRequired {
            required: SOULBIND_MIN_REBIRTH,
            current: rebirth_count,
        });
    }
    if current_enhanced_appraisal < 0 {
        return Err(SoulBindPolicyError::NegativeEnhancedAppraisal);
    }

    let initial_protection_charge =
        checked_ceil_percentage(current_enhanced_appraisal, PROTECTION_PERCENT)
            .ok_or(SoulBindPolicyError::ArithmeticOverflow)?;
    let total_money_cost = package
        .fixed_money_cost
        .checked_add(initial_protection_charge)
        .ok_or(SoulBindPolicyError::ArithmeticOverflow)?;

    Ok(SoulBindBindingPreview {
        package,
        rebirth_count,
        current_enhanced_appraisal,
        initial_protection_charge,
        total_money_cost,
    })
}

/// Previews the Money top-up for one appraisal mutation on an already SoulBound item.
///
/// The specification requires a 60% charge for positive appraisal mutations, player-paid percentage
/// fees to use integer ceiling, and `bind-early + all top-ups == bind-late` for appraisal charge.
/// Independently ceiling each raw delta violates the last invariant at rounding boundaries. For an
/// increase, this kernel therefore charges the difference between the two cumulative integer
/// liabilities: `ceil(60% × new) - ceil(60% × previous)`. Across any monotonic increase path, those
/// differences telescope exactly to the same charge as binding at the final appraisal.
///
/// Equal or decreasing appraisal has no positive delta and therefore produces no top-up or refund.
/// This pure policy intentionally does not invent historical-high-watermark state or define a
/// non-monotonic lifecycle beyond the current mutation; the future owning service must atomically
/// revalidate the actual previous/new authoritative appraisals before settling a positive mutation.
pub fn preview_soulbind_top_up(
    previous_enhanced_appraisal: i64,
    new_enhanced_appraisal: i64,
) -> Result<SoulBindTopUpPreview, SoulBindPolicyError> {
    if previous_enhanced_appraisal < 0 || new_enhanced_appraisal < 0 {
        return Err(SoulBindPolicyError::NegativeEnhancedAppraisal);
    }

    if new_enhanced_appraisal <= previous_enhanced_appraisal {
        return Ok(SoulBindTopUpPreview {
            previous_enhanced_appraisal,
            new_enhanced_appraisal,
            positive_appraisal_delta: 0,
            money_charge: 0,
        });
    }

    let previous_required =
        checked_ceil_percentage(previous_enhanced_appraisal, PROTECTION_PERCENT)
            .ok_or(SoulBindPolicyError::ArithmeticOverflow)?;
    let new_required = checked_ceil_percentage(new_enhanced_appraisal, PROTECTION_PERCENT)
        .ok_or(SoulBindPolicyError::ArithmeticOverflow)?;
    let positive_appraisal_delta = new_enhanced_appraisal
        .checked_sub(previous_enhanced_appraisal)
        .ok_or(SoulBindPolicyError::ArithmeticOverflow)?;
    let money_charge = new_required
        .checked_sub(previous_required)
        .ok_or(SoulBindPolicyError::ArithmeticOverflow)?;

    Ok(SoulBindTopUpPreview {
        previous_enhanced_appraisal,
        new_enhanced_appraisal,
        positive_appraisal_delta,
        money_charge,
    })
}

/// Previews the frozen SoulBind removal fee and cooldown policy.
///
/// The owning stateful service must separately verify the item is SoulBound and that Protected and
/// Favorite are cleared before charging or mutating anything. No binding resource is refunded.
pub fn preview_soulbind_unbind(
    current_enhanced_appraisal: i64,
) -> Result<SoulBindUnbindPreview, SoulBindPolicyError> {
    if current_enhanced_appraisal < 0 {
        return Err(SoulBindPolicyError::NegativeEnhancedAppraisal);
    }

    Ok(SoulBindUnbindPreview {
        current_enhanced_appraisal,
        money_fee: checked_ceil_percentage(current_enhanced_appraisal, UNBIND_PERCENT)
            .ok_or(SoulBindPolicyError::ArithmeticOverflow)?,
        rebind_cooldown_seconds: SOULBIND_REBIND_COOLDOWN_SECONDS,
        refunds_binding_resources: false,
        requires_unprotected: true,
        requires_unfavorited: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_packages_match_frozen_tier_costs() {
        let netherite = soulbind_binding_package(EquipmentTier::Netherite).unwrap();
        assert_eq!(netherite.soulbind_rune_quantity, 1);
        assert_eq!(netherite.onyx_quantity, 20);
        assert_eq!(netherite.platinum_ingot_quantity, 8);
        assert_eq!(
            netherite.tier_component,
            SoulBindTierComponent::NetheriteBillet
        );
        assert_eq!(netherite.tier_component_quantity, 2);
        assert_eq!(netherite.fixed_money_cost, 250_000);
        assert_eq!(netherite.activity_xp_cost, 25_000);

        let graphite = soulbind_binding_package(EquipmentTier::Graphite).unwrap();
        assert_eq!(graphite.soulbind_rune_quantity, 1);
        assert_eq!(graphite.onyx_quantity, 32);
        assert_eq!(graphite.platinum_ingot_quantity, 12);
        assert_eq!(graphite.tier_component, SoulBindTierComponent::GraphiteLayer);
        assert_eq!(graphite.tier_component_quantity, 2);
        assert_eq!(graphite.fixed_money_cost, 500_000);
        assert_eq!(graphite.activity_xp_cost, 50_000);
    }

    #[test]
    fn non_endgame_tiers_are_rejected() {
        for tier in [
            EquipmentTier::StarterLeather,
            EquipmentTier::Wood,
            EquipmentTier::Stone,
            EquipmentTier::Copper,
            EquipmentTier::Gold,
            EquipmentTier::Iron,
            EquipmentTier::Diamond,
            EquipmentTier::Obsidian,
        ] {
            assert_eq!(
                soulbind_binding_package(tier),
                Err(SoulBindPolicyError::IneligibleTier)
            );
        }
    }

    #[test]
    fn non_ordinary_endgame_equipment_is_rejected_explicitly() {
        for tier in [EquipmentTier::Netherite, EquipmentTier::Graphite] {
            assert_eq!(
                preview_soulbind_binding(tier, false, 1, 1_000),
                Err(SoulBindPolicyError::NonOrdinaryEquipment)
            );
        }
    }

    #[test]
    fn rebirth_gate_is_enforced_before_binding_charge() {
        assert_eq!(
            preview_soulbind_binding(EquipmentTier::Netherite, true, 0, 1_000),
            Err(SoulBindPolicyError::RebirthRequired {
                required: 1,
                current: 0,
            })
        );
        assert!(preview_soulbind_binding(EquipmentTier::Netherite, true, 1, 1_000).is_ok());
        assert!(preview_soulbind_binding(EquipmentTier::Graphite, true, 100, 1_000).is_ok());
    }

    #[test]
    fn initial_protection_charge_uses_integer_ceiling() {
        let cases = [(0, 0), (1, 1), (5, 3), (6, 4), (10, 6), (101, 61)];
        for (appraisal, expected_charge) in cases {
            let preview =
                preview_soulbind_binding(EquipmentTier::Netherite, true, 1, appraisal).unwrap();
            assert_eq!(preview.initial_protection_charge, expected_charge);
            assert_eq!(preview.total_money_cost, 250_000 + expected_charge);
        }
    }

    #[test]
    fn top_up_is_cumulative_and_path_independent_across_rounding_boundaries() {
        let initial = preview_soulbind_binding(EquipmentTier::Netherite, true, 1, 2).unwrap();
        assert_eq!(initial.initial_protection_charge, 2);

        let to_three = preview_soulbind_top_up(2, 3).unwrap();
        assert_eq!(to_three.positive_appraisal_delta, 1);
        assert_eq!(to_three.money_charge, 0);

        let to_four = preview_soulbind_top_up(3, 4).unwrap();
        assert_eq!(to_four.positive_appraisal_delta, 1);
        assert_eq!(to_four.money_charge, 1);

        let bind_late =
            preview_soulbind_binding(EquipmentTier::Netherite, true, 1, 4).unwrap();
        assert_eq!(
            initial.initial_protection_charge + to_three.money_charge + to_four.money_charge,
            bind_late.initial_protection_charge
        );

        let direct = preview_soulbind_top_up(2, 4).unwrap();
        assert_eq!(direct.positive_appraisal_delta, 2);
        assert_eq!(
            direct.money_charge, 1,
            "independently ceiling 60% of the raw +2 delta would overcharge and violate bind-late equality"
        );
    }

    #[test]
    fn non_positive_appraisal_mutation_has_no_top_up_or_refund() {
        let equal = preview_soulbind_top_up(100, 100).unwrap();
        assert_eq!(equal.positive_appraisal_delta, 0);
        assert_eq!(equal.money_charge, 0);

        let down = preview_soulbind_top_up(100, 80).unwrap();
        assert_eq!(down.positive_appraisal_delta, 0);
        assert_eq!(down.money_charge, 0);
    }

    #[test]
    fn unbind_fee_uses_ceiling_and_freezes_no_refund_cooldown_preconditions() {
        let zero = preview_soulbind_unbind(0).unwrap();
        assert_eq!(zero.money_fee, 0);
        assert_eq!(zero.rebind_cooldown_seconds, 7 * 24 * 60 * 60);
        assert!(!zero.refunds_binding_resources);
        assert!(zero.requires_unprotected);
        assert!(zero.requires_unfavorited);

        assert_eq!(preview_soulbind_unbind(1).unwrap().money_fee, 1);
        assert_eq!(preview_soulbind_unbind(5).unwrap().money_fee, 1);
        assert_eq!(preview_soulbind_unbind(6).unwrap().money_fee, 2);
        assert_eq!(preview_soulbind_unbind(101).unwrap().money_fee, 21);
    }

    #[test]
    fn largest_supported_appraisal_remains_representable() {
        let binding =
            preview_soulbind_binding(EquipmentTier::Graphite, true, 1, i64::MAX).unwrap();
        assert_eq!(binding.initial_protection_charge, 5_534_023_222_112_865_485);
        assert_eq!(binding.total_money_cost, 5_534_023_222_113_365_485);

        let unbind = preview_soulbind_unbind(i64::MAX).unwrap();
        assert_eq!(unbind.money_fee, 1_844_674_407_370_955_162);
    }

    #[test]
    fn negative_appraisal_inputs_fail_closed() {
        assert_eq!(
            preview_soulbind_binding(EquipmentTier::Netherite, true, 1, -1),
            Err(SoulBindPolicyError::NegativeEnhancedAppraisal)
        );
        assert_eq!(
            preview_soulbind_top_up(-1, 10),
            Err(SoulBindPolicyError::NegativeEnhancedAppraisal)
        );
        assert_eq!(
            preview_soulbind_top_up(10, -1),
            Err(SoulBindPolicyError::NegativeEnhancedAppraisal)
        );
        assert_eq!(
            preview_soulbind_unbind(-1),
            Err(SoulBindPolicyError::NegativeEnhancedAppraisal)
        );
    }
}
