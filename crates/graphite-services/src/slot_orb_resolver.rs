use sqlx::{Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::enchant_apply::validate_existing_state;
use crate::{
    EnchantApplyError, EnchantSlotCapacity, ExistingAppliedEnchant,
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
    #[error("persisted existing enchant state is invalid for Slot Orb mutation: {0}")]
    ExistingEnchantState(#[source] EnchantApplyError),
    #[error(transparent)]
    Capacity(#[from] SlotOrbCapacityStateError),
    #[error(transparent)]
    Policy(#[from] SlotOrbPolicyError),
}

#[derive(Debug, Error)]
pub enum OrdinarySlotOrbStateWriterError {
    #[error(transparent)]
    Preflight(#[from] OrdinarySlotOrbPreflightResolverError),
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error(
        "the locked Slot Orb capacity state changed unexpectedly before the successful unlock write"
    )]
    LockedStateMismatch,
}

impl From<sqlx::Error> for OrdinarySlotOrbStateWriterError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

/// Locks authoritative ordinary equipment state and previews one Slot Orb attempt without mutating
/// equipment or assets.
///
/// The caller owns the surrounding transaction and must acquire operation/player locks first when a
/// higher-level lifecycle requires them. The enhanced-appraisal resolver then acquires the canonical
/// `item -> structural state -> embedded enchant rows` locks and carries the authoritative +N,
/// currently unlocked Normal/Special capacities, starter/enchantable flags, current embedded
/// enchants, and current Enhanced Canonical Appraisal into this preflight.
///
/// Before Slot-Orb-specific checks, the persisted current enchant set is validated through the same
/// canonical placement/conflict/occupancy validator used by standard finished-book application. A
/// canonical-identity/level-valid but wrong-slot, same-item-conflicting, or over-capacity state is
/// therefore rejected instead of letting a slot unlock normalize an already-invalid item. Cross-item
/// equipped-armor validation is not introduced here because changing capacity does not change any
/// existing enchant identity or level.
///
/// The requested unlock must be exactly the next locked slot in its family. A request with fewer than
/// the required predecessor slots fails closed, while a request whose target is already unlocked is
/// rejected as stale/redundant. +N only establishes eligibility; this function never grants capacity.
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

    let capacity = EnchantSlotCapacity::try_new(
        enhanced.recraft.normal_enchant_slot_capacity,
        enhanced.recraft.special_enchant_slot_capacity,
    )
    .map_err(OrdinarySlotOrbPreflightResolverError::ExistingEnchantState)?;
    let existing: Vec<_> = enhanced
        .embedded_enchants
        .iter()
        .map(|applied| ExistingAppliedEnchant {
            enchant: applied.enchant,
            level: applied.level,
        })
        .collect();
    validate_existing_state(enhanced.recraft.slot, capacity, &existing)
        .map_err(OrdinarySlotOrbPreflightResolverError::ExistingEnchantState)?;

    let policy = slot_orb_policy(unlock);
    let current_capacity = capacity.for_family(policy.family);

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

/// Writes only the structural capacity consequence of an already-successful Slot Orb outcome.
///
/// This low-level primitive deliberately owns neither the success draw nor the attempt settlement.
/// The future owning Slot Orb lifecycle must acquire operation/player locks first, deterministically
/// resolve RNG, and call this writer only on the successful branch while atomically settling Orb
/// consumption, the application fee, operation/idempotency state, and outbox effects around it. A
/// failed Slot Orb attempt must not call this function; its capacity remains unchanged while the
/// owning lifecycle applies the frozen Orb+fee failure consequence.
///
/// The writer reruns the authoritative preflight in the caller-owned transaction, including existing
/// enchant-state validation and exact-next capacity checks, then performs a compare-and-set update on
/// only the selected slot family. Rolling back the caller transaction rolls back the capacity write.
/// It performs no RNG, Orb/inventory mutation, Money/AEXP mutation, operation finalization, or command
/// exposure by itself.
pub async fn write_successful_slot_orb_unlock_to_owned_ordinary_equipment(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
    item_id: Uuid,
    unlock: SlotOrbUnlock,
) -> Result<SlotOrbUnlock, OrdinarySlotOrbStateWriterError> {
    let preview =
        lock_preview_slot_orb_attempt_for_owned_ordinary_equipment(tx, player_id, item_id, unlock)
            .await?;

    let expected_capacity = i16::from(preview.policy.required_unlocked_slots_before_attempt);
    let target_capacity = i16::from(preview.policy.target_slot_number);
    let result = match preview.policy.family {
        SlotOrbFamily::NormalClass => {
            sqlx::query(
                r#"
                UPDATE item_instance_equipment_structural_state
                   SET normal_enchant_slot_capacity = $3
                 WHERE item_instance_id = $1
                   AND normal_enchant_slot_capacity = $2
                "#,
            )
            .bind(item_id)
            .bind(expected_capacity)
            .bind(target_capacity)
            .execute(&mut **tx)
            .await?
        }
        SlotOrbFamily::SpecialUniversal => {
            sqlx::query(
                r#"
                UPDATE item_instance_equipment_structural_state
                   SET special_enchant_slot_capacity = $3
                 WHERE item_instance_id = $1
                   AND special_enchant_slot_capacity = $2
                "#,
            )
            .bind(item_id)
            .bind(expected_capacity)
            .bind(target_capacity)
            .execute(&mut **tx)
            .await?
        }
    };

    if result.rows_affected() != 1 {
        return Err(OrdinarySlotOrbStateWriterError::LockedStateMismatch);
    }

    Ok(preview.policy.unlock)
}
