use sqlx::{Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    OrdinaryEquipmentEnhancedResolverError, SlotOrbAttemptPreview, SlotOrbFamily,
    SlotOrbPolicyError, SlotOrbUnlock, lock_owned_ordinary_equipment_enhanced_appraisal,
    preview_slot_orb_attempt, slot_orb_policy,
};

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum SlotOrbCapacityStateError {
    #[error(
        "Slot Orb {family:?} family requires {required_unlocked_slots} predecessor slots unlocked; current capacity is {current_unlocked_slots}"
    )]
    PredecessorSlotsLocked {
        family: SlotOrbFamily,
        required_unlocked_slots: u8,
        current_unlocked_slots: u8,
    },
    #[error(
        "Slot Orb {family:?} target slot #{target_slot_number} is already unlocked; current capacity is {current_unlocked_slots}"
    )]
    TargetSlotAlreadyUnlocked {
        family: SlotOrbFamily,
        target_slot_number: u8,
        current_unlocked_slots: u8,
    },
}

#[derive(Debug, Error)]
pub enum OrdinarySlotOrbPreflightResolverError {
    #[error(transparent)]
    Enhanced(#[from] OrdinaryEquipmentEnhancedResolverError),
    #[error("starter equipment cannot receive Slot Orb unlocks")]
    StarterEquipment,
    #[error("the ItemInstance is not enchantable and cannot receive Slot Orb unlocks")]
    ItemNotEnchantable,
    #[error(transparent)]
    Capacity(#[from] SlotOrbCapacityStateError),
    #[error(transparent)]
    Policy(#[from] SlotOrbPolicyError),
}

/// Locks authoritative ordinary equipment state and previews one Slot Orb attempt without mutating
/// equipment or assets.
///
/// The caller owns the surrounding transaction and must acquire operation/player locks first when a
/// higher-level lifecycle requires them. The enhanced-appraisal resolver then acquires the canonical
/// `item -> structural state -> embedded enchant rows` locks and carries the authoritative +N,
/// currently unlocked Normal/Special capacities, starter/enchantable flags, and current Enhanced
/// Canonical Appraisal into this preflight.
///
/// The requested unlock must be exactly the next locked slot in its family. A request with fewer than
/// the required predecessor slots fails closed, while a request whose target is already unlocked is
/// rejected as stale/redundant. +N only establishes eligibility; this function never grants capacity.
///
/// The reused Enhanced resolver validates persisted enchant identities/levels for appraisal but, by
/// design, does not certify every existing placement/conflict/occupancy invariant. This preflight
/// therefore owns only Slot-Orb-specific eligibility/capacity/appraisal checks; a future owning
/// mutation must compose any broader current-enchant-state validation required at settlement rather
/// than treating this preview as authorization to commit.
///
/// A successful preview does not prove Orb ownership, consume the Orb/application fee, draw RNG,
/// update slot capacity, create/finalize an operation, or expose `/enchant`. Those remain atomic
/// responsibilities of the future owning Slot Orb lifecycle.
pub async fn lock_preview_slot_orb_attempt_for_owned_ordinary_equipment(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
    item_id: Uuid,
    unlock: SlotOrbUnlock,
) -> Result<SlotOrbAttemptPreview, OrdinarySlotOrbPreflightResolverError> {
    let enhanced = lock_owned_ordinary_equipment_enhanced_appraisal(tx, player_id, item_id).await?;

    if enhanced.recraft.is_starter {
        return Err(OrdinarySlotOrbPreflightResolverError::StarterEquipment);
    }
    if !enhanced.recraft.is_enchantable {
        return Err(OrdinarySlotOrbPreflightResolverError::ItemNotEnchantable);
    }

    let policy = slot_orb_policy(unlock);
    let current_capacity = match policy.family {
        SlotOrbFamily::NormalClass => enhanced.recraft.normal_enchant_slot_capacity,
        SlotOrbFamily::SpecialUniversal => enhanced.recraft.special_enchant_slot_capacity,
    };

    if current_capacity < policy.required_unlocked_slots_before_attempt {
        return Err(SlotOrbCapacityStateError::PredecessorSlotsLocked {
            family: policy.family,
            required_unlocked_slots: policy.required_unlocked_slots_before_attempt,
            current_unlocked_slots: current_capacity,
        }
        .into());
    }
    if current_capacity >= policy.target_slot_number {
        return Err(SlotOrbCapacityStateError::TargetSlotAlreadyUnlocked {
            family: policy.family,
            target_slot_number: policy.target_slot_number,
            current_unlocked_slots: current_capacity,
        }
        .into());
    }

    debug_assert_eq!(
        current_capacity, policy.required_unlocked_slots_before_attempt,
        "Slot Orb policy targets exactly the next slot"
    );

    Ok(preview_slot_orb_attempt(
        unlock,
        enhanced.recraft.upgrade_level,
        enhanced.enhanced_canonical_appraisal,
    )?)
}
