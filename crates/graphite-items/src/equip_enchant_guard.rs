use graphite_core::{
    CANONICAL_ENCHANT_COUNT, CanonicalEnchant, EnchantConflictScope,
    canonical_enchant_conflict_scope,
};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::ItemError;

const MAX_EQUIPPED_ARMOR_ITEMS: usize = 4;
const MAX_PROSPECTIVE_ARMOR_ENCHANT_ROWS: usize =
    CANONICAL_ENCHANT_COUNT * MAX_EQUIPPED_ARMOR_ITEMS;
const PROSPECTIVE_ARMOR_ENCHANT_ROW_QUERY_LIMIT: i64 =
    MAX_PROSPECTIVE_ARMOR_ENCHANT_ROWS as i64 + 1;

/// Validates the only frozen cross-item enchant invariant before an armor equip changes membership.
///
/// The caller already owns the player lock, which serializes same-player Equip/Unequip and the
/// authoritative Enchant writer. This helper then locks current armor-slot membership and the
/// prospective loadout's embedded-enchant rows in deterministic order. The currently equipped item
/// in `target_slot` is deliberately excluded because a successful equip displaces it atomically.
///
/// This lower-layer guard owns no enchant gameplay policy beyond the shared Core vocabulary: it
/// exact-parses persisted identities and rejects only pairs classified as
/// `EquippedArmorLoadout`. Resulting levels, equipment placement, slot capacity, appraisal, and
/// acquisition remain with their higher-layer owners.
pub(crate) async fn lock_validate_prospective_equipped_armor_enchant_conflicts(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
    target_item_id: Uuid,
    target_slot: &str,
) -> Result<(), ItemError> {
    if !is_armor_slot(target_slot) {
        return Ok(());
    }

    let slot_rows = sqlx::query(
        r#"
        SELECT slot, item_instance_id
          FROM equipment_slots
         WHERE player_id = $1
           AND slot IN ('ARMOR_HELMET', 'ARMOR_CHEST', 'ARMOR_LEGS', 'ARMOR_BOOTS')
         ORDER BY item_instance_id
         FOR UPDATE
        "#,
    )
    .bind(player_id)
    .fetch_all(&mut **tx)
    .await?;

    if slot_rows.len() > MAX_EQUIPPED_ARMOR_ITEMS {
        return Err(ItemError::EquipmentIntegrityMismatch);
    }

    let mut prospective_item_ids = Vec::with_capacity(MAX_EQUIPPED_ARMOR_ITEMS);
    for row in slot_rows {
        let slot: String = row.try_get("slot")?;
        let item_instance_id: Uuid = row.try_get("item_instance_id")?;
        if slot != target_slot {
            prospective_item_ids.push(item_instance_id);
        }
    }
    prospective_item_ids.push(target_item_id);
    prospective_item_ids.sort_unstable();
    prospective_item_ids.dedup();

    if prospective_item_ids.len() > MAX_EQUIPPED_ARMOR_ITEMS {
        return Err(ItemError::EquipmentIntegrityMismatch);
    }

    let rows = sqlx::query(
        r#"
        SELECT item_instance_id, enchant_key
          FROM item_instance_embedded_enchants
         WHERE item_instance_id = ANY($1::uuid[])
         ORDER BY item_instance_id, enchant_key
         LIMIT $2
         FOR UPDATE
        "#,
    )
    .bind(&prospective_item_ids)
    .bind(PROSPECTIVE_ARMOR_ENCHANT_ROW_QUERY_LIMIT)
    .fetch_all(&mut **tx)
    .await?;

    if rows.len() > MAX_PROSPECTIVE_ARMOR_ENCHANT_ROWS {
        return Err(ItemError::EquippedArmorEnchantRowsExceeded);
    }

    let mut seen = Vec::with_capacity(rows.len());
    for row in rows {
        let item_instance_id: Uuid = row.try_get("item_instance_id")?;
        let key: String = row.try_get("enchant_key")?;
        let enchant = CanonicalEnchant::from_persisted_key(&key).ok_or_else(|| {
            ItemError::UnknownEmbeddedEnchantKey {
                item_instance_id,
                key: key.clone(),
            }
        })?;

        for (previous_item_instance_id, previous_enchant) in &seen {
            if canonical_enchant_conflict_scope(*previous_enchant, enchant)
                == Some(EnchantConflictScope::EquippedArmorLoadout)
            {
                return Err(ItemError::EquippedArmorEnchantConflict {
                    left_item_instance_id: *previous_item_instance_id,
                    left: *previous_enchant,
                    right_item_instance_id: item_instance_id,
                    right: enchant,
                });
            }
        }
        seen.push((item_instance_id, enchant));
    }

    Ok(())
}

const fn is_armor_slot(slot: &str) -> bool {
    matches!(
        slot.as_bytes(),
        b"ARMOR_HELMET" | b"ARMOR_CHEST" | b"ARMOR_LEGS" | b"ARMOR_BOOTS"
    )
}
