use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    CanonicalEnchant, EnchantApplyError, EnchantApplyPreview, EnchantSlotCapacity,
    ExistingAppliedEnchant, OrdinaryEquipmentEnhancedResolverError,
    lock_owned_ordinary_equipment_enhanced_appraisal, preview_standard_finished_book_application,
};

#[derive(Debug, Error)]
pub enum OrdinaryEnchantApplyPreflightResolverError {
    #[error(transparent)]
    Enhanced(#[from] OrdinaryEquipmentEnhancedResolverError),
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error("the locked ItemInstance/structural state disappeared during enchant preflight")]
    ItemIntegrityMismatch,
    #[error("starter equipment cannot receive enchant books")]
    StarterEquipment,
    #[error("the ItemInstance is not enchantable")]
    ItemNotEnchantable,
    #[error("persisted enchant slot capacity cannot be represented by the runtime policy type")]
    InvalidPersistedSlotCapacity,
    #[error(transparent)]
    Apply(#[from] EnchantApplyError),
}

impl From<sqlx::Error> for OrdinaryEnchantApplyPreflightResolverError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

/// Locks authoritative state and previews one standard finished-book application to owned ordinary
/// equipment without mutating any asset.
///
/// The caller owns the surrounding transaction and must acquire any operation/player locks first.
/// [`lock_owned_ordinary_equipment_enhanced_appraisal`] then acquires the canonical
/// `item -> structural state -> embedded enchant rows` locks and validates persisted enchant
/// identity/resulting-level state. This resolver performs only a read of the already-locked parent
/// and structural rows to recover the ItemInstance enchantability flags and the two persisted slot
/// capacities; it deliberately takes no additional mutable-state lock after the child rows.
///
/// A successful return is still only a preflight. It consumes no finished book, writes no enchant
/// row, spends no Money/AEXP, performs no RNG, recomputes no SoulBind top-up, and does not authorize
/// `/enchant`. The future owning mutation must keep this transaction open, validate incoming-book
/// ownership/provenance, perform any required equipped-armor loadout conflict check, revalidate all
/// mutation prerequisites, and settle the complete operation atomically/idempotently.
///
/// This function intentionally covers the ordinary-equipment path only because the repository's
/// authoritative appraisal/slot context is currently defined for ordinary equipment. It does not
/// invent enchantability or appraisal semantics for special ItemDefinitions.
pub async fn lock_preview_standard_finished_book_application_for_owned_ordinary_equipment(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
    item_id: Uuid,
    incoming_enchant: CanonicalEnchant,
    incoming_level: u8,
) -> Result<EnchantApplyPreview, OrdinaryEnchantApplyPreflightResolverError> {
    let enhanced = lock_owned_ordinary_equipment_enhanced_appraisal(tx, player_id, item_id).await?;

    // The enhanced resolver already holds both rows FOR UPDATE. Reading them again without another
    // locking clause avoids lock-order inversion after embedded-enchant child locks while recovering
    // lifecycle flags/capacity that are intentionally not part of the appraisal result.
    let state = sqlx::query(
        r#"
        SELECT i.is_starter,
               i.is_enchantable,
               s.normal_enchant_slot_capacity,
               s.special_enchant_slot_capacity
          FROM item_instances i
          JOIN item_instance_equipment_structural_state s
            ON s.item_instance_id = i.id
         WHERE i.id = $1
           AND i.owner_player_id = $2
        "#,
    )
    .bind(item_id)
    .bind(player_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(OrdinaryEnchantApplyPreflightResolverError::ItemIntegrityMismatch)?;

    let is_starter: bool = state.try_get("is_starter")?;
    if is_starter {
        return Err(OrdinaryEnchantApplyPreflightResolverError::StarterEquipment);
    }

    let is_enchantable: bool = state.try_get("is_enchantable")?;
    if !is_enchantable {
        return Err(OrdinaryEnchantApplyPreflightResolverError::ItemNotEnchantable);
    }

    let normal_slots: i16 = state.try_get("normal_enchant_slot_capacity")?;
    let special_slots: i16 = state.try_get("special_enchant_slot_capacity")?;
    let normal_slots = u8::try_from(normal_slots)
        .map_err(|_| OrdinaryEnchantApplyPreflightResolverError::InvalidPersistedSlotCapacity)?;
    let special_slots = u8::try_from(special_slots)
        .map_err(|_| OrdinaryEnchantApplyPreflightResolverError::InvalidPersistedSlotCapacity)?;
    let capacity = EnchantSlotCapacity::try_new(normal_slots, special_slots)?;

    let existing: Vec<_> = enhanced
        .embedded_enchants
        .iter()
        .map(|applied| ExistingAppliedEnchant {
            enchant: applied.enchant,
            level: applied.level,
        })
        .collect();

    Ok(preview_standard_finished_book_application(
        enhanced.recraft.slot,
        capacity,
        &existing,
        incoming_enchant,
        incoming_level,
    )?)
}
