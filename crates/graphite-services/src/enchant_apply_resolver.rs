use sqlx::{Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    CanonicalEnchant, EnchantApplyAction, EnchantApplyError, EnchantApplyPreview,
    EnchantSlotCapacity, ExistingAppliedEnchant, OrdinaryEquipmentEnhancedResolverError,
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

#[derive(Debug, Error)]
pub enum OrdinaryEnchantApplyStateWriterError {
    #[error(transparent)]
    Preflight(#[from] OrdinaryEnchantApplyPreflightResolverError),
    #[error(
        "the resulting armor enchant state requires authoritative equipped-loadout conflict validation before mutation"
    )]
    EquippedArmorLoadoutValidationRequired,
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error(
        "the locked embedded enchant state changed unexpectedly before the standard apply write"
    )]
    LockedStateMismatch,
}

impl From<sqlx::Error> for OrdinaryEnchantApplyStateWriterError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
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

/// Mutates only the embedded-enchant row selected by one standard finished-book preflight while
/// retaining the caller-owned transaction.
///
/// This is a low-level, transaction-composable state writer rather than the owning Enchant service.
/// It first reuses the authoritative locked preflight above, so placement, slot capacity, conflicts,
/// resulting-level bounds, starter/enchantable flags, and lower/equal replacement rules are checked
/// against the same `item -> structural state -> embedded enchant rows` snapshot that remains locked
/// during the write. A new enchant inserts one canonical persistence key; a higher-level replacement
/// updates that same key without consuming another slot.
///
/// Cross-item Guardian/Nine Life/Phoenix conflicts require an equipped-armor loadout lock/validator
/// that the repository does not yet provide. Rather than accepting a caller-supplied trust flag, this
/// primitive fails closed before mutation whenever the preflight reports that validation is needed.
///
/// The return value is only the embedded-state action this primitive actually performed. It does not
/// return [`EnchantApplyPreview`], whose finished-book-consumption metadata belongs to a future owning
/// settlement rather than to this low-level writer.
///
/// This writer deliberately does not consume or mint an Enchanted Book, settle Money/AEXP, calculate
/// SoulBind appraisal top-up, create/finalize an operation, emit outbox events, or expose `/enchant`.
/// A future owning transaction must compose those asset and lifecycle steps around this primitive and
/// commit them atomically. Rolling back the caller transaction rolls back this state write as well.
pub async fn write_standard_finished_book_application_to_owned_ordinary_equipment(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
    item_id: Uuid,
    incoming_enchant: CanonicalEnchant,
    incoming_level: u8,
) -> Result<EnchantApplyAction, OrdinaryEnchantApplyStateWriterError> {
    let preview = lock_preview_standard_finished_book_application_for_owned_ordinary_equipment(
        tx,
        player_id,
        item_id,
        incoming_enchant,
        incoming_level,
    )
    .await?;

    if preview.resulting_item_requires_equipped_armor_loadout_conflict_validation {
        return Err(OrdinaryEnchantApplyStateWriterError::EquippedArmorLoadoutValidationRequired);
    }

    let persisted_key = incoming_enchant.persisted_key();
    let stored_level = i16::from(incoming_level);
    match preview.action {
        EnchantApplyAction::InsertNew => {
            let result = sqlx::query(
                r#"
                INSERT INTO item_instance_embedded_enchants (item_instance_id, enchant_key, level)
                VALUES ($1, $2, $3)
                "#,
            )
            .bind(item_id)
            .bind(persisted_key)
            .bind(stored_level)
            .execute(&mut **tx)
            .await?;
            if result.rows_affected() != 1 {
                return Err(OrdinaryEnchantApplyStateWriterError::LockedStateMismatch);
            }
        }
        EnchantApplyAction::UpgradeExisting { previous_level } => {
            let result = sqlx::query(
                r#"
                UPDATE item_instance_embedded_enchants
                   SET level = $3
                 WHERE item_instance_id = $1
                   AND enchant_key = $2
                   AND level = $4
                "#,
            )
            .bind(item_id)
            .bind(persisted_key)
            .bind(stored_level)
            .bind(i16::from(previous_level))
            .execute(&mut **tx)
            .await?;
            if result.rows_affected() != 1 {
                return Err(OrdinaryEnchantApplyStateWriterError::LockedStateMismatch);
            }
        }
    }

    Ok(preview.action)
}
