use sqlx::{Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    CanonicalEnchant, OrdinaryEquipmentEnhancedResolverError,
    lock_owned_ordinary_equipment_enhanced_appraisal,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemovedEmbeddedEnchant {
    pub enchant: CanonicalEnchant,
    pub level: u8,
}

#[derive(Debug, Error)]
pub enum EnchantRemovalStateWriterError {
    #[error(transparent)]
    Enhanced(#[from] OrdinaryEquipmentEnhancedResolverError),
    #[error("selected embedded enchant {0:?} is not present on the ItemInstance")]
    SelectedEnchantNotFound(CanonicalEnchant),
    #[error(
        "selected embedded enchant {enchant:?} expected level {expected_level}, but authoritative level is {actual_level}"
    )]
    SelectedEnchantLevelChanged {
        enchant: CanonicalEnchant,
        expected_level: u8,
        actual_level: u8,
    },
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error("the locked embedded enchant row changed unexpectedly before exact removal")]
    LockedStateMismatch,
}

impl From<sqlx::Error> for EnchantRemovalStateWriterError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

/// Removes exactly one already-proven-removable embedded enchant from owned ordinary equipment.
///
/// This is a low-level, transaction-composable state writer. The caller must establish removability
/// before calling it; the active specification does not freeze a complete canonical removability
/// classifier, so this function deliberately cannot decide that policy itself. A future owning
/// Remove/Recover lifecycle must also acquire operation/player locks first when required by that
/// lifecycle.
///
/// The authoritative enhanced-equipment resolver locks the ItemInstance, structural state, and all
/// embedded-enchant rows in deterministic order, exact-parses every persisted enchant identity and
/// level, and keeps those locks for the caller transaction. The selected identity and expected level
/// are then rechecked against that locked snapshot before one compare-and-delete statement removes
/// the row. A stale level therefore fails before mutation, while a concurrent row change after the
/// snapshot is caught by the compare-and-delete affected-row check.
///
/// Removing an enchant can only reduce the active constraint set, so this primitive does not invent
/// an Equip-style prospective loadout validation. It also does not price or charge the NPC service,
/// consume a Blank Enchant Book, produce a recovered book, resolve recovery RNG, compose multi-remove
/// semantics, finalize an operation/outbox event, or expose `/enchant`. Rolling back the caller
/// transaction restores the removed row.
pub async fn write_exact_enchant_removal_after_removability_check(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
    item_id: Uuid,
    enchant: CanonicalEnchant,
    expected_level: u8,
) -> Result<RemovedEmbeddedEnchant, EnchantRemovalStateWriterError> {
    let enhanced = lock_owned_ordinary_equipment_enhanced_appraisal(tx, player_id, item_id).await?;
    let selected = enhanced
        .embedded_enchants
        .iter()
        .find(|applied| applied.enchant == enchant)
        .ok_or(EnchantRemovalStateWriterError::SelectedEnchantNotFound(
            enchant,
        ))?;

    if selected.level != expected_level {
        return Err(
            EnchantRemovalStateWriterError::SelectedEnchantLevelChanged {
                enchant,
                expected_level,
                actual_level: selected.level,
            },
        );
    }

    let result = sqlx::query(
        r#"
        DELETE FROM item_instance_embedded_enchants
         WHERE item_instance_id = $1
           AND enchant_key = $2
           AND level = $3
        "#,
    )
    .bind(item_id)
    .bind(enchant.persisted_key())
    .bind(i16::from(expected_level))
    .execute(&mut **tx)
    .await?;

    if result.rows_affected() != 1 {
        return Err(EnchantRemovalStateWriterError::LockedStateMismatch);
    }

    Ok(RemovedEmbeddedEnchant {
        enchant,
        level: expected_level,
    })
}
