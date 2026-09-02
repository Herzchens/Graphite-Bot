use graphite_items::{ItemError, lock_owned_item_equipment_structural_state};
use serde_json::Value;
use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::equipment_appraisal::recraft_equipment_appraisal;
use crate::{
    BaseEquipmentAppraisal, CanonicalBookAppraisal, CanonicalEnchant,
    CanonicalEquipmentAppraisalError, CreationRoll, CreationRollError,
    EmbeddedEnchantAppraisalInput, EnchantAppraisalError, EquipmentAppraisalError, EquipmentSlot,
    EquipmentTier, base_equipment_appraisal, canonical_book_appraisal, embedded_enchant_value,
    enchant_catalog_policy,
};
use graphite_core::CANONICAL_ENCHANT_COUNT;

const EMBEDDED_ENCHANT_ROW_QUERY_LIMIT: i64 = CANONICAL_ENCHANT_COUNT as i64 + 1;

#[derive(Clone, Debug, PartialEq)]
pub struct OrdinaryEquipmentRecraftAppraisal {
    pub item_instance_id: Uuid,
    pub owner_player_id: Uuid,
    pub definition_key: String,
    pub definition_version: i32,
    pub is_starter: bool,
    pub is_enchantable: bool,
    pub is_upgradeable: bool,
    pub tier: EquipmentTier,
    pub slot: EquipmentSlot,
    pub base_appraisal: BaseEquipmentAppraisal,
    pub creation_roll: CreationRoll,
    pub upgrade_level: u64,
    pub normal_enchant_slot_capacity: u8,
    pub special_enchant_slot_capacity: u8,
    pub recraft_appraisal: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedEmbeddedEnchantAppraisal {
    pub enchant: CanonicalEnchant,
    pub level: u8,
    pub book_appraisal: CanonicalBookAppraisal,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OrdinaryEquipmentEnhancedAppraisal {
    pub recraft: OrdinaryEquipmentRecraftAppraisal,
    pub embedded_enchants: Vec<ResolvedEmbeddedEnchantAppraisal>,
    pub embedded_enchant_value: i64,
    pub enhanced_canonical_appraisal: i64,
}

#[derive(Debug, Error)]
pub enum OrdinaryEquipmentRecraftResolverError {
    #[error(transparent)]
    Item(#[from] ItemError),
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error("the ItemInstance is not classified as ordinary equipment")]
    NotOrdinaryEquipment,
    #[error("the pinned ordinary equipment definition has invalid or unsupported tier metadata")]
    InvalidTierMetadata,
    #[error("the pinned ordinary equipment definition has invalid equipment-slot metadata")]
    InvalidSlotMetadata,
    #[error(
        "the pinned ordinary equipment definition uses a tier/slot combination unavailable in current-v1"
    )]
    InvalidTierSlotCombination,
    #[error(
        "the pinned ItemDefinition version disappeared while its ItemInstance still references it"
    )]
    DefinitionIntegrityMismatch,
    #[error(transparent)]
    CreationRoll(#[from] CreationRollError),
    #[error(transparent)]
    BaseAppraisal(#[from] EquipmentAppraisalError),
    #[error(transparent)]
    CanonicalAppraisal(#[from] CanonicalEquipmentAppraisalError),
}

impl From<sqlx::Error> for OrdinaryEquipmentRecraftResolverError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

#[derive(Debug, Error)]
pub enum OrdinaryEquipmentEnhancedResolverError {
    #[error(transparent)]
    Recraft(#[from] OrdinaryEquipmentRecraftResolverError),
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error("persisted embedded enchant key is not canonical: {0}")]
    UnknownEmbeddedEnchantKey(String),
    #[error(
        "persisted embedded enchant {enchant:?} has invalid resulting level {level}; maximum is {max_level}"
    )]
    InvalidEmbeddedEnchantLevel {
        enchant: CanonicalEnchant,
        level: i16,
        max_level: u8,
    },
    #[error("embedded enchant row count exceeds the canonical catalog cardinality")]
    TooManyEmbeddedEnchantRows,
    #[error(transparent)]
    EnchantAppraisal(#[from] EnchantAppraisalError),
    #[error("enhanced canonical appraisal arithmetic exceeded supported integer bounds")]
    ArithmeticOverflow,
}

impl From<sqlx::Error> for OrdinaryEquipmentEnhancedResolverError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

/// Locks and resolves the canonical structural appraisal for one owned ordinary ItemInstance.
///
/// The caller owns the surrounding transaction and must acquire any operation/player locks before
/// calling this function. The item-domain resolver then acquires the canonical ItemInstance and
/// structural-state row locks. This function performs only an unlocked read joining the exact
/// immutable ItemDefinition version to that already-locked ItemInstance, so it can carry the parent
/// starter/enchantable/upgradeable flags forward without introducing another mutable-state lock or
/// inverting Graphite's `operation -> player -> item -> structural state` order.
///
/// Only ordinary equipment is accepted. Tier and armor-slot metadata are derived fail-closed from
/// the pinned immutable definition; neither Discord input nor the current ItemDefinition version is
/// trusted. The result also carries the parent mutation-capability flags and the two slot capacities
/// from the authoritative locked snapshot so downstream lifecycle owners do not issue ad-hoc reads.
/// Special ItemDefinitions and their possible definition-specific base-appraisal override path remain
/// outside this ordinary resolver. Callers that need embedded-enchant value should use
/// [`lock_owned_ordinary_equipment_enhanced_appraisal`].
pub async fn lock_owned_ordinary_equipment_recraft_appraisal(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
    item_id: Uuid,
) -> Result<OrdinaryEquipmentRecraftAppraisal, OrdinaryEquipmentRecraftResolverError> {
    let structural = lock_owned_item_equipment_structural_state(tx, player_id, item_id).await?;
    if !structural.item.is_ordinary_equipment {
        return Err(OrdinaryEquipmentRecraftResolverError::NotOrdinaryEquipment);
    }

    let definition = sqlx::query(
        r#"
        SELECT d.category,
               d.data,
               i.is_starter,
               i.is_enchantable,
               i.is_upgradeable
          FROM item_definition_versions d
          JOIN item_instances i
            ON i.definition_key = d.key
           AND i.definition_version = d.version
         WHERE d.key = $1
           AND d.version = $2
           AND i.id = $3
           AND i.owner_player_id = $4
        "#,
    )
    .bind(&structural.item.definition_key)
    .bind(structural.item.definition_version)
    .bind(item_id)
    .bind(player_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(OrdinaryEquipmentRecraftResolverError::DefinitionIntegrityMismatch)?;

    let category: String = definition.try_get("category")?;
    let data: Value = definition.try_get("data")?;
    let is_starter: bool = definition.try_get("is_starter")?;
    let is_enchantable: bool = definition.try_get("is_enchantable")?;
    let is_upgradeable: bool = definition.try_get("is_upgradeable")?;
    let tier = ordinary_equipment_tier(&data)?;
    let slot = ordinary_equipment_slot(&category, &data)?;
    validate_ordinary_tier_slot(tier, slot)?;
    let base_appraisal = base_equipment_appraisal(tier, slot, None)?;
    let creation_roll = CreationRoll::new(
        structural.creation_roll_numerator,
        structural.creation_roll_denominator,
    )?;
    let recraft_appraisal =
        recraft_equipment_appraisal(base_appraisal, creation_roll, structural.upgrade_level)?;

    Ok(OrdinaryEquipmentRecraftAppraisal {
        item_instance_id: structural.item.item_instance_id,
        owner_player_id: structural.item.owner_player_id,
        definition_key: structural.item.definition_key,
        definition_version: structural.item.definition_version,
        is_starter,
        is_enchantable,
        is_upgradeable,
        tier,
        slot,
        base_appraisal,
        creation_roll,
        upgrade_level: structural.upgrade_level,
        normal_enchant_slot_capacity: structural.normal_enchant_slot_capacity,
        special_enchant_slot_capacity: structural.special_enchant_slot_capacity,
        recraft_appraisal,
    })
}

/// Locks and resolves embedded-enchant appraisal on top of the authoritative ordinary Recraft value.
///
/// Lock order is `item -> structural state -> embedded enchant rows`; operation/player locks, when
/// required by a stateful owner, must already have been acquired by the caller. Embedded rows are
/// locked in deterministic `enchant_key` order and the query is hard-bounded to the canonical
/// catalog cardinality plus one sentinel row, so malformed persistence cannot turn this request-time
/// resolver into an unbounded scan.
///
/// Persisted keys are mapped through [`CanonicalEnchant::from_persisted_key`] and each resulting
/// level is checked against the frozen per-enchant ceiling before its appraisal class is derived from
/// [`enchant_catalog_policy`]. Unknown keys and impossible fixed-level states fail closed. This
/// function intentionally does not certify slot-family occupancy, equipment compatibility, or
/// conflict legality; those are lifecycle invariants for the future Enchant mutation owner. It does
/// not mutate ItemInstance, enchant, Money, AEXP, or slot state.
pub async fn lock_owned_ordinary_equipment_enhanced_appraisal(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
    item_id: Uuid,
) -> Result<OrdinaryEquipmentEnhancedAppraisal, OrdinaryEquipmentEnhancedResolverError> {
    let recraft = lock_owned_ordinary_equipment_recraft_appraisal(tx, player_id, item_id).await?;

    let rows = sqlx::query(
        r#"
        SELECT enchant_key, level
          FROM item_instance_embedded_enchants
         WHERE item_instance_id = $1
         ORDER BY enchant_key
         LIMIT $2
         FOR UPDATE
        "#,
    )
    .bind(item_id)
    .bind(EMBEDDED_ENCHANT_ROW_QUERY_LIMIT)
    .fetch_all(&mut **tx)
    .await?;

    if rows.len() > CANONICAL_ENCHANT_COUNT {
        return Err(OrdinaryEquipmentEnhancedResolverError::TooManyEmbeddedEnchantRows);
    }

    let mut resolved = Vec::with_capacity(rows.len());
    let mut appraisal_inputs = Vec::with_capacity(rows.len());
    for row in rows {
        let persisted_key: String = row.try_get("enchant_key")?;
        let stored_level: i16 = row.try_get("level")?;
        let enchant = CanonicalEnchant::from_persisted_key(&persisted_key).ok_or_else(|| {
            OrdinaryEquipmentEnhancedResolverError::UnknownEmbeddedEnchantKey(persisted_key.clone())
        })?;
        let max_level = crate::canonical_enchant_max_resulting_level(enchant);
        let level = u8::try_from(stored_level).map_err(|_| {
            OrdinaryEquipmentEnhancedResolverError::InvalidEmbeddedEnchantLevel {
                enchant,
                level: stored_level,
                max_level,
            }
        })?;
        if level == 0 || level > max_level {
            return Err(
                OrdinaryEquipmentEnhancedResolverError::InvalidEmbeddedEnchantLevel {
                    enchant,
                    level: stored_level,
                    max_level,
                },
            );
        }

        let appraisal_class = enchant_catalog_policy(enchant).appraisal_class;
        let book_appraisal = canonical_book_appraisal(appraisal_class, level)?;
        appraisal_inputs.push(EmbeddedEnchantAppraisalInput {
            class: appraisal_class,
            level,
        });
        resolved.push(ResolvedEmbeddedEnchantAppraisal {
            enchant,
            level,
            book_appraisal,
        });
    }

    let embedded_enchant_value = embedded_enchant_value(&appraisal_inputs)?;
    let enhanced_canonical_appraisal = recraft
        .recraft_appraisal
        .checked_add(embedded_enchant_value)
        .ok_or(OrdinaryEquipmentEnhancedResolverError::ArithmeticOverflow)?;

    Ok(OrdinaryEquipmentEnhancedAppraisal {
        recraft,
        embedded_enchants: resolved,
        embedded_enchant_value,
        enhanced_canonical_appraisal,
    })
}

fn ordinary_equipment_tier(
    data: &Value,
) -> Result<EquipmentTier, OrdinaryEquipmentRecraftResolverError> {
    match data.get("tier").and_then(Value::as_str) {
        Some("WOOD") => Ok(EquipmentTier::Wood),
        Some("STONE") => Ok(EquipmentTier::Stone),
        Some("COPPER") => Ok(EquipmentTier::Copper),
        Some("GOLD") => Ok(EquipmentTier::Gold),
        Some("IRON") => Ok(EquipmentTier::Iron),
        Some("DIAMOND") => Ok(EquipmentTier::Diamond),
        Some("OBSIDIAN") => Ok(EquipmentTier::Obsidian),
        Some("NETHERITE") => Ok(EquipmentTier::Netherite),
        Some("GRAPHITE") => Ok(EquipmentTier::Graphite),
        _ => Err(OrdinaryEquipmentRecraftResolverError::InvalidTierMetadata),
    }
}

fn ordinary_equipment_slot(
    category: &str,
    data: &Value,
) -> Result<EquipmentSlot, OrdinaryEquipmentRecraftResolverError> {
    match category {
        "PICKAXE" => Ok(EquipmentSlot::Pickaxe),
        "SWORD" => Ok(EquipmentSlot::Sword),
        "FISHING_ROD" => Ok(EquipmentSlot::FishingRod),
        "ARMOR" => match data.get("slot").and_then(Value::as_str) {
            Some("ARMOR_HELMET") => Ok(EquipmentSlot::Helmet),
            Some("ARMOR_CHEST") => Ok(EquipmentSlot::Chestplate),
            Some("ARMOR_LEGS") => Ok(EquipmentSlot::Leggings),
            Some("ARMOR_BOOTS") => Ok(EquipmentSlot::Boots),
            _ => Err(OrdinaryEquipmentRecraftResolverError::InvalidSlotMetadata),
        },
        _ => Err(OrdinaryEquipmentRecraftResolverError::InvalidSlotMetadata),
    }
}

fn validate_ordinary_tier_slot(
    tier: EquipmentTier,
    slot: EquipmentSlot,
) -> Result<(), OrdinaryEquipmentRecraftResolverError> {
    if tier == EquipmentTier::Gold
        && matches!(
            slot,
            EquipmentSlot::Helmet
                | EquipmentSlot::Chestplate
                | EquipmentSlot::Leggings
                | EquipmentSlot::Boots
        )
    {
        return Err(OrdinaryEquipmentRecraftResolverError::InvalidTierSlotCombination);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ordinary_tier_metadata_is_explicit_and_fail_closed() {
        let expected = [
            ("WOOD", EquipmentTier::Wood),
            ("STONE", EquipmentTier::Stone),
            ("COPPER", EquipmentTier::Copper),
            ("GOLD", EquipmentTier::Gold),
            ("IRON", EquipmentTier::Iron),
            ("DIAMOND", EquipmentTier::Diamond),
            ("OBSIDIAN", EquipmentTier::Obsidian),
            ("NETHERITE", EquipmentTier::Netherite),
            ("GRAPHITE", EquipmentTier::Graphite),
        ];
        for (raw, tier) in expected {
            assert_eq!(
                ordinary_equipment_tier(&json!({"tier": raw})).unwrap(),
                tier
            );
        }

        for invalid in [json!({}), json!({"tier": "LEATHER"}), json!({"tier": 7})] {
            assert!(matches!(
                ordinary_equipment_tier(&invalid),
                Err(OrdinaryEquipmentRecraftResolverError::InvalidTierMetadata)
            ));
        }
    }

    #[test]
    fn ordinary_slot_metadata_reuses_the_canonical_equipment_categories() {
        assert_eq!(
            ordinary_equipment_slot("PICKAXE", &json!({})).unwrap(),
            EquipmentSlot::Pickaxe
        );
        assert_eq!(
            ordinary_equipment_slot("SWORD", &json!({})).unwrap(),
            EquipmentSlot::Sword
        );
        assert_eq!(
            ordinary_equipment_slot("FISHING_ROD", &json!({})).unwrap(),
            EquipmentSlot::FishingRod
        );
        let armor = [
            ("ARMOR_HELMET", EquipmentSlot::Helmet),
            ("ARMOR_CHEST", EquipmentSlot::Chestplate),
            ("ARMOR_LEGS", EquipmentSlot::Leggings),
            ("ARMOR_BOOTS", EquipmentSlot::Boots),
        ];
        for (raw, slot) in armor {
            assert_eq!(
                ordinary_equipment_slot("ARMOR", &json!({"slot": raw})).unwrap(),
                slot
            );
        }

        for (category, data) in [
            ("TOTEM", json!({})),
            ("ARMOR", json!({})),
            ("ARMOR", json!({"slot": "ARMOR_UNKNOWN"})),
        ] {
            assert!(matches!(
                ordinary_equipment_slot(category, &data),
                Err(OrdinaryEquipmentRecraftResolverError::InvalidSlotMetadata)
            ));
        }
    }

    #[test]
    fn current_v1_rejects_gold_armor_even_though_the_generic_table_is_mathematically_defined() {
        assert!(validate_ordinary_tier_slot(EquipmentTier::Gold, EquipmentSlot::Pickaxe).is_ok());
        for slot in [
            EquipmentSlot::Helmet,
            EquipmentSlot::Chestplate,
            EquipmentSlot::Leggings,
            EquipmentSlot::Boots,
        ] {
            assert!(matches!(
                validate_ordinary_tier_slot(EquipmentTier::Gold, slot),
                Err(OrdinaryEquipmentRecraftResolverError::InvalidTierSlotCombination)
            ));
        }
    }
}
