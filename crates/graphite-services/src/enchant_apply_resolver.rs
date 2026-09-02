use sqlx::{Postgres, Transaction};
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
    #[error("starter equipment cannot receive enchant books")]
    StarterEquipment,
    #[error("the ItemInstance is not enchantable")]
    ItemNotEnchantable,
    #[error(transparent)]
    Apply(#[from] EnchantApplyError),
}

/// Locks authoritative state and previews one standard finished-book application to owned ordinary
/// equipment without mutating any asset.
///
/// The caller owns the surrounding transaction and must acquire any operation/player locks first.
/// [`lock_owned_ordinary_equipment_enhanced_appraisal`] acquires the canonical
/// `item -> structural state -> embedded enchant rows` locks, validates persisted enchant
/// identity/resulting-level state, and carries forward the ItemInstance enchantability flags plus the
/// two persisted slot capacities resolved before the child-row lock stage. This resolver therefore
/// performs no ad-hoc persistence read after the authoritative locked snapshot has been assembled.
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

    if enhanced.recraft.is_starter {
        return Err(OrdinaryEnchantApplyPreflightResolverError::StarterEquipment);
    }
    if !enhanced.recraft.is_enchantable {
        return Err(OrdinaryEnchantApplyPreflightResolverError::ItemNotEnchantable);
    }

    let capacity = EnchantSlotCapacity::try_new(
        enhanced.recraft.normal_enchant_slot_capacity,
        enhanced.recraft.special_enchant_slot_capacity,
    )?;
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
