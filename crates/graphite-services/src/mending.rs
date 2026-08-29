use crate::equipment_policy::EquipmentSlot;
use serde::Serialize;
use thiserror::Error;

pub const MENDING_MANUAL_AEXP_PER_DURABILITY: i64 = 5;
pub const MENDING_AUTOMATION_AEXP_PER_DURABILITY: i64 = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MendingContext {
    Manual,
    Automation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct MendingPreview {
    pub slot: EquipmentSlot,
    pub context: MendingContext,
    pub durability_to_restore: i64,
    pub activity_xp_per_durability: i64,
    pub activity_xp_cost: i64,
    pub resolves_before_machine_experience_pool: bool,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum MendingPolicyError {
    #[error("Mending durability to restore cannot be negative")]
    NegativeDurabilityToRestore,
    #[error("automation Mending is defined only for Pickaxe and Fishing Rod; got {0:?}")]
    AutomationUnsupportedForSlot(EquipmentSlot),
    #[error(
        "NUKE_BURNOUT blocks Mending restoration for the Pickaxe until the expedition is terminal"
    )]
    NukeBurnoutBlocksRestoration,
    #[error("Mending Activity EXP arithmetic exceeded supported integer bounds")]
    ArithmeticOverflow,
}

/// Previews the frozen Mending I Activity EXP cost for one item.
///
/// Manual Mending costs 5 Activity EXP per restored durability for Pickaxe, Fishing Rod, Sword,
/// and each Armor piece. Automation is defined only for Pickaxe and Fishing Rod and costs 8
/// Activity EXP per restored durability. Automation Mending resolves before AEXP enters the machine
/// Experience Pool; this preview records that ordering without deciding which future transaction
/// owns the earned/spendable AEXP flow.
///
/// `pickaxe_nuke_burnout_active` is authoritative only for a Pickaxe. When true on a Pickaxe,
/// Mending is rejected until the owning expedition reaches `SETTLED`, `ESCAPED`, or `TRUE_DEATH`.
/// The caller must also prove from authoritative ItemInstance/definition state that Mending I is
/// actually present and applicable, the item is not an unbreakable Starter item, and there is real
/// missing durability to restore. This pure function does not load or mutate ItemInstance durability,
/// enchant state, AEXP, expedition state, or machine Experience Pool state.
pub fn preview_mending(
    slot: EquipmentSlot,
    context: MendingContext,
    durability_to_restore: i64,
    pickaxe_nuke_burnout_active: bool,
) -> Result<MendingPreview, MendingPolicyError> {
    if durability_to_restore < 0 {
        return Err(MendingPolicyError::NegativeDurabilityToRestore);
    }
    if slot == EquipmentSlot::Pickaxe && pickaxe_nuke_burnout_active {
        return Err(MendingPolicyError::NukeBurnoutBlocksRestoration);
    }

    let activity_xp_per_durability = match context {
        MendingContext::Manual => MENDING_MANUAL_AEXP_PER_DURABILITY,
        MendingContext::Automation => match slot {
            EquipmentSlot::Pickaxe | EquipmentSlot::FishingRod => {
                MENDING_AUTOMATION_AEXP_PER_DURABILITY
            }
            _ => return Err(MendingPolicyError::AutomationUnsupportedForSlot(slot)),
        },
    };

    let activity_xp_cost = durability_to_restore
        .checked_mul(activity_xp_per_durability)
        .ok_or(MendingPolicyError::ArithmeticOverflow)?;

    Ok(MendingPreview {
        slot,
        context,
        durability_to_restore,
        activity_xp_per_durability,
        activity_xp_cost,
        resolves_before_machine_experience_pool: context == MendingContext::Automation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_mending_costs_five_aexp_per_durability_for_every_supported_slot() {
        for slot in [
            EquipmentSlot::Pickaxe,
            EquipmentSlot::FishingRod,
            EquipmentSlot::Sword,
            EquipmentSlot::Helmet,
            EquipmentSlot::Chestplate,
            EquipmentSlot::Leggings,
            EquipmentSlot::Boots,
        ] {
            let preview = preview_mending(slot, MendingContext::Manual, 17, false).unwrap();
            assert_eq!(preview.activity_xp_per_durability, 5);
            assert_eq!(preview.activity_xp_cost, 85);
            assert!(!preview.resolves_before_machine_experience_pool);
        }
    }

    #[test]
    fn automation_mending_costs_eight_only_for_pickaxe_and_rod() {
        for slot in [EquipmentSlot::Pickaxe, EquipmentSlot::FishingRod] {
            let preview = preview_mending(slot, MendingContext::Automation, 17, false).unwrap();
            assert_eq!(preview.activity_xp_per_durability, 8);
            assert_eq!(preview.activity_xp_cost, 136);
            assert!(preview.resolves_before_machine_experience_pool);
        }

        for slot in [
            EquipmentSlot::Sword,
            EquipmentSlot::Helmet,
            EquipmentSlot::Chestplate,
            EquipmentSlot::Leggings,
            EquipmentSlot::Boots,
        ] {
            assert_eq!(
                preview_mending(slot, MendingContext::Automation, 1, false),
                Err(MendingPolicyError::AutomationUnsupportedForSlot(slot))
            );
        }
    }

    #[test]
    fn pickaxe_nuke_burnout_blocks_manual_and_automation_mending() {
        for context in [MendingContext::Manual, MendingContext::Automation] {
            assert_eq!(
                preview_mending(EquipmentSlot::Pickaxe, context, 1, true),
                Err(MendingPolicyError::NukeBurnoutBlocksRestoration)
            );
        }
    }

    #[test]
    fn zero_cost_is_a_pure_projection_but_negative_and_overflow_are_rejected() {
        assert_eq!(
            preview_mending(EquipmentSlot::Sword, MendingContext::Manual, 0, false)
                .unwrap()
                .activity_xp_cost,
            0
        );
        assert_eq!(
            preview_mending(EquipmentSlot::Sword, MendingContext::Manual, -1, false),
            Err(MendingPolicyError::NegativeDurabilityToRestore)
        );
        assert_eq!(
            preview_mending(
                EquipmentSlot::FishingRod,
                MendingContext::Automation,
                i64::MAX,
                false,
            ),
            Err(MendingPolicyError::ArithmeticOverflow)
        );
    }
}
