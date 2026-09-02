use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    CanonicalEnchant, EnchantApplyAction, EnchantApplyError, EnchantApplyPreview,
    EnchantConflictScope, EnchantSlotCapacity, EquipmentSlot, ExistingAppliedEnchant,
    OrdinaryEquipmentEnhancedResolverError, canonical_enchant_conflict_scope,
    enchant_placement_policy, lock_owned_ordinary_equipment_enhanced_appraisal,
    preview_standard_finished_book_application,
};
use graphite_core::CANONICAL_ENCHANT_COUNT;

const MAX_EQUIPPED_ARMOR_ITEMS: usize = 4;
const MAX_LOCKED_ENCHANT_ROWS: usize = CANONICAL_ENCHANT_COUNT * (MAX_EQUIPPED_ARMOR_ITEMS + 1);
const EMBEDDED_ENCHANT_ROW_QUERY_LIMIT: i64 = MAX_LOCKED_ENCHANT_ROWS as i64 + 1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EquippedArmorEnchantState {
    pub item_instance_id: Uuid,
    pub slot: EquipmentSlot,
    pub enchants: Vec<ExistingAppliedEnchant>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EquippedArmorEnchantLoadout {
    pub player_id: Uuid,
    pub items: Vec<EquippedArmorEnchantState>,
}

impl EquippedArmorEnchantLoadout {
    #[must_use]
    pub fn equipped_slot_for_item(&self, item_instance_id: Uuid) -> Option<EquipmentSlot> {
        self.items
            .iter()
            .find(|item| item.item_instance_id == item_instance_id)
            .map(|item| item.slot)
    }
}

#[derive(Debug, Error)]
pub enum EquippedArmorEnchantLoadoutError {
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error("player was not found while locking the equipped armor loadout")]
    PlayerNotFound,
    #[error("target ItemInstance was not found for this player")]
    TargetItemNotFound,
    #[error("equipped armor state is internally inconsistent for ItemInstance {0}")]
    EquipmentIntegrityMismatch(Uuid),
    #[error("equipped armor snapshot exceeded the bounded embedded-enchant row domain")]
    TooManyEmbeddedEnchantRows,
    #[error("equipped armor ItemInstance {item_instance_id} contains unknown enchant key {key}")]
    UnknownEmbeddedEnchantKey { item_instance_id: Uuid, key: String },
    #[error(
        "equipped armor ItemInstance {item_instance_id} contains {enchant:?} level {level}, outside 1..={maximum}"
    )]
    InvalidEmbeddedEnchantLevel {
        item_instance_id: Uuid,
        enchant: CanonicalEnchant,
        level: i16,
        maximum: u8,
    },
    #[error(
        "equipped armor ItemInstance {item_instance_id} contains {enchant:?}, which cannot be placed on {slot:?}"
    )]
    ExistingEnchantWrongEquipmentSlot {
        item_instance_id: Uuid,
        enchant: CanonicalEnchant,
        slot: EquipmentSlot,
    },
    #[error(
        "equipped armor ItemInstance {item_instance_id} contains conflicting enchants {left:?} and {right:?} at scope {scope:?}"
    )]
    ExistingItemConflict {
        item_instance_id: Uuid,
        left: CanonicalEnchant,
        right: CanonicalEnchant,
        scope: EnchantConflictScope,
    },
    #[error(
        "equipped armor loadout contains conflicting survival-core enchants {left:?} on {left_item_instance_id} and {right:?} on {right_item_instance_id}"
    )]
    ExistingLoadoutConflict {
        left_item_instance_id: Uuid,
        left: CanonicalEnchant,
        right_item_instance_id: Uuid,
        right: CanonicalEnchant,
    },
    #[error(
        "incoming survival-core enchant {incoming:?} conflicts with equipped {existing:?} on ItemInstance {existing_item_instance_id}"
    )]
    IncomingLoadoutConflict {
        incoming: CanonicalEnchant,
        existing_item_instance_id: Uuid,
        existing: CanonicalEnchant,
    },
}

impl From<sqlx::Error> for EquippedArmorEnchantLoadoutError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

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
    #[error(transparent)]
    Loadout(#[from] EquippedArmorEnchantLoadoutError),
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

/// Locks the player, the target ItemInstance, every currently equipped armor ItemInstance, their
/// structural rows when present, and their embedded-enchant rows in deterministic key order.
///
/// This lock boundary exists specifically so a future Enchant owner does not acquire sibling armor
/// after it has already locked the target. Same-player equip/unequip mutations also lock the player
/// first, so the player row serializes loadout membership while the item/enchant snapshot is built.
/// The target is included in the sorted item-lock set even when it is not equipped.
///
/// The returned loadout validates exact persisted enchant identities, resulting-level ceilings,
/// body-part placement, same-item conflicts, and the only currently frozen cross-item conflict scope:
/// Guardian/Nine Life/Phoenix. It does not validate slot capacity, appraisal, book ownership, or any
/// mutation consequence. Those remain with their owning per-item/service policies.
pub async fn lock_validate_equipped_armor_enchant_loadout_for_owned_target(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
    target_item_id: Uuid,
) -> Result<EquippedArmorEnchantLoadout, EquippedArmorEnchantLoadoutError> {
    let player =
        sqlx::query("SELECT id FROM players WHERE id = $1 AND status <> 'DELETED' FOR UPDATE")
            .bind(player_id)
            .fetch_optional(&mut **tx)
            .await?;
    if player.is_none() {
        return Err(EquippedArmorEnchantLoadoutError::PlayerNotFound);
    }

    let slot_rows = sqlx::query(
        r#"
        SELECT slot, item_instance_id
          FROM equipment_slots
         WHERE player_id = $1
           AND slot IN ('ARMOR_HELMET', 'ARMOR_CHEST', 'ARMOR_LEGS', 'ARMOR_BOOTS')
         ORDER BY item_instance_id
        "#,
    )
    .bind(player_id)
    .fetch_all(&mut **tx)
    .await?;

    let mut armor_items = Vec::with_capacity(slot_rows.len());
    let mut item_ids = Vec::with_capacity(slot_rows.len() + 1);
    for row in slot_rows {
        let item_instance_id: Uuid = row.try_get("item_instance_id")?;
        let persisted_slot: String = row.try_get("slot")?;
        let slot = armor_slot_from_persisted(&persisted_slot).ok_or(
            EquippedArmorEnchantLoadoutError::EquipmentIntegrityMismatch(item_instance_id),
        )?;
        armor_items.push((item_instance_id, slot));
        item_ids.push(item_instance_id);
    }
    item_ids.push(target_item_id);
    item_ids.sort_unstable();
    item_ids.dedup();

    let item_rows = sqlx::query(
        r#"
        SELECT id, owner_player_id, location
          FROM item_instances
         WHERE id = ANY($1::uuid[])
         ORDER BY id
         FOR UPDATE
        "#,
    )
    .bind(&item_ids)
    .fetch_all(&mut **tx)
    .await?;
    if item_rows.len() != item_ids.len() {
        return Err(EquippedArmorEnchantLoadoutError::TargetItemNotFound);
    }

    for (row, expected_id) in item_rows.iter().zip(item_ids.iter()) {
        let item_instance_id: Uuid = row.try_get("id")?;
        if item_instance_id != *expected_id {
            return Err(
                EquippedArmorEnchantLoadoutError::EquipmentIntegrityMismatch(item_instance_id),
            );
        }
        let owner_player_id: Uuid = row.try_get("owner_player_id")?;
        if owner_player_id != player_id {
            if item_instance_id == target_item_id {
                return Err(EquippedArmorEnchantLoadoutError::TargetItemNotFound);
            }
            return Err(
                EquippedArmorEnchantLoadoutError::EquipmentIntegrityMismatch(item_instance_id),
            );
        }
        if armor_items
            .iter()
            .any(|(armor_item_id, _)| *armor_item_id == item_instance_id)
        {
            let location: String = row.try_get("location")?;
            if location != "EQUIPPED" {
                return Err(
                    EquippedArmorEnchantLoadoutError::EquipmentIntegrityMismatch(item_instance_id),
                );
            }
        }
    }

    sqlx::query(
        r#"
        SELECT item_instance_id
          FROM item_instance_equipment_structural_state
         WHERE item_instance_id = ANY($1::uuid[])
         ORDER BY item_instance_id
         FOR UPDATE
        "#,
    )
    .bind(&item_ids)
    .fetch_all(&mut **tx)
    .await?;

    let enchant_rows = sqlx::query(
        r#"
        SELECT item_instance_id, enchant_key, level
          FROM item_instance_embedded_enchants
         WHERE item_instance_id = ANY($1::uuid[])
         ORDER BY item_instance_id, enchant_key
         LIMIT $2
         FOR UPDATE
        "#,
    )
    .bind(&item_ids)
    .bind(EMBEDDED_ENCHANT_ROW_QUERY_LIMIT)
    .fetch_all(&mut **tx)
    .await?;
    if enchant_rows.len() > MAX_LOCKED_ENCHANT_ROWS {
        return Err(EquippedArmorEnchantLoadoutError::TooManyEmbeddedEnchantRows);
    }

    let mut items: Vec<_> = armor_items
        .into_iter()
        .map(|(item_instance_id, slot)| EquippedArmorEnchantState {
            item_instance_id,
            slot,
            enchants: Vec::new(),
        })
        .collect();

    for row in enchant_rows {
        let item_instance_id: Uuid = row.try_get("item_instance_id")?;
        let Some(item) = items
            .iter_mut()
            .find(|item| item.item_instance_id == item_instance_id)
        else {
            continue;
        };
        let persisted_key: String = row.try_get("enchant_key")?;
        let enchant = CanonicalEnchant::from_persisted_key(&persisted_key).ok_or_else(|| {
            EquippedArmorEnchantLoadoutError::UnknownEmbeddedEnchantKey {
                item_instance_id,
                key: persisted_key.clone(),
            }
        })?;
        let stored_level: i16 = row.try_get("level")?;
        let maximum = crate::canonical_enchant_max_resulting_level(enchant);
        let level = u8::try_from(stored_level).map_err(|_| {
            EquippedArmorEnchantLoadoutError::InvalidEmbeddedEnchantLevel {
                item_instance_id,
                enchant,
                level: stored_level,
                maximum,
            }
        })?;
        if level == 0 || level > maximum {
            return Err(
                EquippedArmorEnchantLoadoutError::InvalidEmbeddedEnchantLevel {
                    item_instance_id,
                    enchant,
                    level: stored_level,
                    maximum,
                },
            );
        }
        if !enchant_placement_policy(enchant).applies_to(item.slot) {
            return Err(
                EquippedArmorEnchantLoadoutError::ExistingEnchantWrongEquipmentSlot {
                    item_instance_id,
                    enchant,
                    slot: item.slot,
                },
            );
        }
        item.enchants
            .push(ExistingAppliedEnchant { enchant, level });
    }

    validate_existing_equipped_armor_conflicts(&items)?;

    Ok(EquippedArmorEnchantLoadout { player_id, items })
}

fn validate_existing_equipped_armor_conflicts(
    items: &[EquippedArmorEnchantState],
) -> Result<(), EquippedArmorEnchantLoadoutError> {
    for item in items {
        for (index, applied) in item.enchants.iter().enumerate() {
            for previous in &item.enchants[..index] {
                if let Some(scope) =
                    canonical_enchant_conflict_scope(previous.enchant, applied.enchant)
                {
                    return Err(EquippedArmorEnchantLoadoutError::ExistingItemConflict {
                        item_instance_id: item.item_instance_id,
                        left: previous.enchant,
                        right: applied.enchant,
                        scope,
                    });
                }
            }
        }
    }

    for left_index in 0..items.len() {
        for right in &items[left_index + 1..] {
            let left = &items[left_index];
            for left_enchant in &left.enchants {
                for right_enchant in &right.enchants {
                    if canonical_enchant_conflict_scope(left_enchant.enchant, right_enchant.enchant)
                        == Some(EnchantConflictScope::EquippedArmorLoadout)
                    {
                        return Err(EquippedArmorEnchantLoadoutError::ExistingLoadoutConflict {
                            left_item_instance_id: left.item_instance_id,
                            left: left_enchant.enchant,
                            right_item_instance_id: right.item_instance_id,
                            right: right_enchant.enchant,
                        });
                    }
                }
            }
        }
    }

    Ok(())
}

fn validate_incoming_against_equipped_armor_loadout(
    loadout: &EquippedArmorEnchantLoadout,
    target_item_id: Uuid,
    incoming_enchant: CanonicalEnchant,
) -> Result<(), EquippedArmorEnchantLoadoutError> {
    for item in &loadout.items {
        if item.item_instance_id == target_item_id {
            continue;
        }
        for existing in &item.enchants {
            if canonical_enchant_conflict_scope(incoming_enchant, existing.enchant)
                == Some(EnchantConflictScope::EquippedArmorLoadout)
            {
                return Err(EquippedArmorEnchantLoadoutError::IncomingLoadoutConflict {
                    incoming: incoming_enchant,
                    existing_item_instance_id: item.item_instance_id,
                    existing: existing.enchant,
                });
            }
        }
    }
    Ok(())
}

const fn armor_slot_from_persisted(slot: &str) -> Option<EquipmentSlot> {
    match slot.as_bytes() {
        b"ARMOR_HELMET" => Some(EquipmentSlot::Helmet),
        b"ARMOR_CHEST" => Some(EquipmentSlot::Chestplate),
        b"ARMOR_LEGS" => Some(EquipmentSlot::Leggings),
        b"ARMOR_BOOTS" => Some(EquipmentSlot::Boots),
        _ => None,
    }
}

/// Mutates only the embedded-enchant row selected by one standard finished-book preflight while
/// retaining the caller-owned transaction.
///
/// This is a low-level, transaction-composable state writer rather than the owning Enchant service.
/// It first locks the player plus the target/equipped-armor set in deterministic order, then reuses
/// the ordinary authoritative preflight. This makes equipped Guardian/Nine Life/Phoenix mutations
/// safe against current sibling armor while preserving the canonical player-before-item lock order.
/// Existing loadout conflicts fail closed before mutation.
///
/// For a loadout-scoped incoming enchant, an equipped target is validated against current sibling
/// armor before mutation. An unequipped target may retain dormant survival-core state because the
/// authoritative Equip path revalidates the complete prospective armor loadout before changing
/// membership. Existing active-loadout conflicts still fail closed here before any write.
///
/// The return value is only the embedded-state action this primitive actually performed. It does not
/// return [`EnchantApplyPreview`], whose finished-book-consumption metadata belongs to a future owning
/// settlement rather than to this low-level writer.
///
/// This writer deliberately does not consume or mint an Enchanted Book, settle Money/AEXP, calculate
/// SoulBind appraisal top-up, create/finalize an operation, emit outbox events, or expose `/enchant`.
/// A future owning transaction must acquire any operation lock before calling this writer and compose
/// the remaining asset/lifecycle steps atomically. Rolling back the caller transaction rolls back this
/// state write as well.
pub async fn write_standard_finished_book_application_to_owned_ordinary_equipment(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
    item_id: Uuid,
    incoming_enchant: CanonicalEnchant,
    incoming_level: u8,
) -> Result<EnchantApplyAction, OrdinaryEnchantApplyStateWriterError> {
    let loadout =
        lock_validate_equipped_armor_enchant_loadout_for_owned_target(tx, player_id, item_id)
            .await?;
    let preview = lock_preview_standard_finished_book_application_for_owned_ordinary_equipment(
        tx,
        player_id,
        item_id,
        incoming_enchant,
        incoming_level,
    )
    .await?;

    if preview.resulting_item_requires_equipped_armor_loadout_conflict_validation
        && loadout.equipped_slot_for_item(item_id).is_some()
    {
        validate_incoming_against_equipped_armor_loadout(&loadout, item_id, incoming_enchant)?;
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
