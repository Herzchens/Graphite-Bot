use graphite_items::{ItemError, lock_owned_item_equipment_structural_state};
use serde_json::Value;
use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::equipment_appraisal::recraft_equipment_appraisal;
use crate::{
    BaseEquipmentAppraisal, CanonicalEquipmentAppraisalError, CreationRoll, CreationRollError,
    EquipmentAppraisalError, EquipmentSlot, EquipmentTier, base_equipment_appraisal,
};

#[derive(Clone, Debug, PartialEq)]
pub struct OrdinaryEquipmentRecraftAppraisal {
    pub item_instance_id: Uuid,
    pub owner_player_id: Uuid,
    pub definition_key: String,
    pub definition_version: i32,
    pub tier: EquipmentTier,
    pub slot: EquipmentSlot,
    pub base_appraisal: BaseEquipmentAppraisal,
    pub creation_roll: CreationRoll,
    pub upgrade_level: u64,
    pub recraft_appraisal: i64,
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

/// Locks and resolves the canonical structural appraisal for one owned ordinary ItemInstance.
///
/// The caller owns the surrounding transaction and must acquire any operation/player locks before
/// calling this function. The item-domain resolver then acquires the canonical ItemInstance and
/// structural-state row locks. This function performs only an unlocked read of the exact immutable
/// ItemDefinition version pinned by that locked ItemInstance, so it does not introduce another
/// mutable-state lock or invert Graphite's `operation -> player -> item -> structural state` order.
///
/// Only ordinary equipment is accepted. Tier and armor-slot metadata are derived fail-closed from
/// the pinned immutable definition; neither Discord input nor the current ItemDefinition version is
/// trusted. Special ItemDefinitions and their possible definition-specific base-appraisal override
/// path remain outside this ordinary resolver. The result intentionally exposes only
/// `RecraftAppraisal`. Embedded-enchant persistence is not authoritative yet, so this resolver does
/// not claim or synthesize an `EnhancedCanonicalAppraisal`.
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
        SELECT category, data
          FROM item_definition_versions
         WHERE key = $1
           AND version = $2
        "#,
    )
    .bind(&structural.item.definition_key)
    .bind(structural.item.definition_version)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(OrdinaryEquipmentRecraftResolverError::DefinitionIntegrityMismatch)?;

    let category: String = definition.try_get("category")?;
    let data: Value = definition.try_get("data")?;
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
        tier,
        slot,
        base_appraisal,
        creation_roll,
        upgrade_level: structural.upgrade_level,
        recraft_appraisal,
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
