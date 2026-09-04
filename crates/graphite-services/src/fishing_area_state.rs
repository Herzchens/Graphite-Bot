use chrono::{DateTime, Utc};
use graphite_items::{ItemError, lock_owned_item_ordinary_equipment_classification};
use graphite_progression::{ProgressionMathError, account_level};
use serde_json::Value;
use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    EquipmentTier, FishingArea, FishingAreaFirstUnlockPreview, FishingAreaPolicyError,
    FishingRodForUnlock, preview_first_fishing_area_unlock,
};

const STARTER_BASIC_ROD_DEFINITION_KEY: &str = "equipment.rod.basic.starter";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FishingAreaAccessOrigin {
    StarterPoolDefault,
    Persisted,
    NewlyUnlocked,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FishingAreaAccessSnapshot {
    pub player_id: Uuid,
    pub area: FishingArea,
    pub origin: FishingAreaAccessOrigin,
    pub granted_by_operation_id: Option<Uuid>,
    pub unlocked_at: Option<DateTime<Utc>>,
    pub first_unlock_preview: Option<FishingAreaFirstUnlockPreview>,
}

#[derive(Debug, Error)]
pub enum FishingAreaAccessError {
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error(transparent)]
    Item(#[from] ItemError),
    #[error(transparent)]
    ProgressionMath(#[from] ProgressionMathError),
    #[error(transparent)]
    Policy(#[from] FishingAreaPolicyError),
    #[error("owning operation does not exist")]
    OperationNotFound,
    #[error("owning operation targets a different player")]
    OperationPlayerMismatch,
    #[error("owning operation cannot grant Fishing area access in state {0}")]
    OperationTerminal(String),
    #[error("player does not exist")]
    PlayerNotFound,
    #[error("Fishing area first unlock requires an ACTIVE account; current status is {0}")]
    AccountFrozen(String),
    #[error("persisted Rebirth count is outside the supported non-negative range: {0}")]
    InvalidRebirthCount(i64),
    #[error("player progression state is missing")]
    ProgressionStateMissing,
    #[error("no Fishing Rod is currently equipped")]
    NoEquippedFishingRod,
    #[error("equipped Fishing Rod state is internally inconsistent")]
    EquippedRodIntegrityMismatch,
    #[error("Starter Basic Rod identity/state is internally inconsistent")]
    StarterBasicRodIntegrityMismatch,
    #[error("the equipped Fishing Rod is not classified as ordinary equipment")]
    NonOrdinaryFishingRod,
    #[error("the equipped ordinary Fishing Rod has invalid or unsupported tier metadata")]
    InvalidOrdinaryRodTierMetadata,
    #[error("non-default Fishing area has no persistence key")]
    InvalidAreaPersistenceMapping,
    #[error("Fishing area first-unlock requirements are not satisfied")]
    FirstUnlockRequirementsNotMet {
        preview: FishingAreaFirstUnlockPreview,
    },
}

impl From<sqlx::Error> for FishingAreaAccessError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

/// Resolves permanent Fishing-area access inside a caller-owned gameplay transaction and, when the
/// requested non-default area has never been unlocked, grants that first unlock if the current
/// authoritative progression and equipped-Rod snapshot satisfies the frozen policy.
///
/// This primitive is intentionally operation-composable rather than a standalone `/unlock` action.
/// It acquires locks in `operation -> player -> progression -> item` order and records the operation
/// that first granted a non-default area. The future cast owner remains responsible for its own
/// operation result, RNG, bait/durability/output settlement, audit/outbox payload, and commit.
///
/// Existing persisted access is checked before current Account Level/Rebirth/Rod qualification. Once
/// an area has been unlocked, later Rebirth or equipment changes therefore cannot re-lock it. This
/// does not authorize a future cast with the Starter Basic Rod outside Starter Pool: Pool-only Rod
/// capability remains a separate per-cast rule. Starter Pool itself is implicit default access and
/// never consumes a persistence row.
///
/// The player row serializes same-account progression/equipment mutations while this snapshot is
/// resolved. The equipped ItemInstance is additionally locked through `graphite-items`; ordinary
/// classification comes from the exact immutable ItemDefinition version pinned by that item. Neither
/// Discord input nor the mutable current ItemDefinition is trusted as Rod authority.
pub async fn lock_or_grant_fishing_area_first_unlock(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
    area: FishingArea,
) -> Result<FishingAreaAccessSnapshot, FishingAreaAccessError> {
    lock_pending_player_operation(tx, operation_id, player_id).await?;
    let rebirth_count = lock_active_player(tx, player_id).await?;

    if area == FishingArea::StarterPool {
        return Ok(FishingAreaAccessSnapshot {
            player_id,
            area,
            origin: FishingAreaAccessOrigin::StarterPoolDefault,
            granted_by_operation_id: None,
            unlocked_at: None,
            first_unlock_preview: None,
        });
    }

    let area_key =
        persisted_area_key(area).ok_or(FishingAreaAccessError::InvalidAreaPersistenceMapping)?;
    if let Some((granted_by_operation_id, unlocked_at)) =
        persisted_unlock(tx, player_id, area_key).await?
    {
        return Ok(FishingAreaAccessSnapshot {
            player_id,
            area,
            origin: FishingAreaAccessOrigin::Persisted,
            granted_by_operation_id: Some(granted_by_operation_id),
            unlocked_at: Some(unlocked_at),
            first_unlock_preview: None,
        });
    }

    let account_xp = lock_account_xp(tx, player_id).await?;
    let account_level = u32::from(account_level(account_xp)?);
    let rebirth = rebirth_for_first_unlock_policy(rebirth_count)?;
    let rod = lock_equipped_rod_for_first_unlock(tx, player_id).await?;
    let preview = preview_first_fishing_area_unlock(area, account_level, rebirth, rod)?;
    if !preview.eligible_for_first_unlock {
        return Err(FishingAreaAccessError::FirstUnlockRequirementsNotMet { preview });
    }

    let unlocked_at: DateTime<Utc> = sqlx::query_scalar(
        r#"
        INSERT INTO player_fishing_area_unlocks (
            player_id, area, granted_by_operation_id, unlocked_at
        )
        VALUES ($1, $2, $3, clock_timestamp())
        RETURNING unlocked_at
        "#,
    )
    .bind(player_id)
    .bind(area_key)
    .bind(operation_id)
    .fetch_one(&mut **tx)
    .await?;

    Ok(FishingAreaAccessSnapshot {
        player_id,
        area,
        origin: FishingAreaAccessOrigin::NewlyUnlocked,
        granted_by_operation_id: Some(operation_id),
        unlocked_at: Some(unlocked_at),
        first_unlock_preview: Some(preview),
    })
}

async fn lock_pending_player_operation(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
) -> Result<(), FishingAreaAccessError> {
    let row = sqlx::query("SELECT player_id, state FROM operations WHERE id = $1 FOR UPDATE")
        .bind(operation_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(FishingAreaAccessError::OperationNotFound)?;
    let operation_player_id: Option<Uuid> = row.try_get("player_id")?;
    if operation_player_id != Some(player_id) {
        return Err(FishingAreaAccessError::OperationPlayerMismatch);
    }
    let state: String = row.try_get("state")?;
    if state != "PENDING" {
        return Err(FishingAreaAccessError::OperationTerminal(state));
    }
    Ok(())
}

async fn lock_active_player(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
) -> Result<i64, FishingAreaAccessError> {
    let row = sqlx::query(
        "SELECT status, rebirth_count FROM players WHERE id = $1 AND status <> 'DELETED' FOR UPDATE",
    )
    .bind(player_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(FishingAreaAccessError::PlayerNotFound)?;
    let status: String = row.try_get("status")?;
    if status != "ACTIVE" {
        return Err(FishingAreaAccessError::AccountFrozen(status));
    }
    row.try_get("rebirth_count").map_err(Into::into)
}

async fn persisted_unlock(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
    area_key: &str,
) -> Result<Option<(Uuid, DateTime<Utc>)>, FishingAreaAccessError> {
    sqlx::query_as(
        "SELECT granted_by_operation_id, unlocked_at FROM player_fishing_area_unlocks WHERE player_id = $1 AND area = $2",
    )
    .bind(player_id)
    .bind(area_key)
    .fetch_optional(&mut **tx)
    .await
    .map_err(Into::into)
}

async fn lock_account_xp(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
) -> Result<i64, FishingAreaAccessError> {
    sqlx::query_scalar("SELECT account_xp FROM player_progression WHERE player_id = $1 FOR UPDATE")
        .bind(player_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(FishingAreaAccessError::ProgressionStateMissing)
}

async fn lock_equipped_rod_for_first_unlock(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
) -> Result<FishingRodForUnlock, FishingAreaAccessError> {
    // The player row is already locked, so canonical equip/unequip owners for this player cannot
    // change the slot while this transaction resolves it. Avoid taking the slot lock before the item
    // lock, preserving the existing item -> equipment-slot ordering used by equip mutation.
    let item_id: Uuid = sqlx::query_scalar(
        "SELECT item_instance_id FROM equipment_slots WHERE player_id = $1 AND slot = 'FISHING_ROD'",
    )
    .bind(player_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(FishingAreaAccessError::NoEquippedFishingRod)?;

    let classification =
        lock_owned_item_ordinary_equipment_classification(tx, player_id, item_id).await?;
    let row = sqlx::query(
        r#"
        SELECT i.location, i.is_starter, d.category, d.data
          FROM item_instances i
          JOIN item_definition_versions d
            ON d.key = i.definition_key
           AND d.version = i.definition_version
         WHERE i.id = $1
           AND i.owner_player_id = $2
           AND i.definition_key = $3
           AND i.definition_version = $4
        "#,
    )
    .bind(item_id)
    .bind(player_id)
    .bind(&classification.definition_key)
    .bind(classification.definition_version)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(FishingAreaAccessError::EquippedRodIntegrityMismatch)?;

    let location: String = row.try_get("location")?;
    let is_starter: bool = row.try_get("is_starter")?;
    let category: String = row.try_get("category")?;
    let data: Value = row.try_get("data")?;
    if location != "EQUIPPED" || category != "FISHING_ROD" {
        return Err(FishingAreaAccessError::EquippedRodIntegrityMismatch);
    }

    if classification.definition_key == STARTER_BASIC_ROD_DEFINITION_KEY {
        if !is_starter || classification.is_ordinary_equipment {
            return Err(FishingAreaAccessError::StarterBasicRodIntegrityMismatch);
        }
        return Ok(FishingRodForUnlock::StarterBasic);
    }

    if is_starter {
        return Err(FishingAreaAccessError::StarterBasicRodIntegrityMismatch);
    }
    if !classification.is_ordinary_equipment {
        return Err(FishingAreaAccessError::NonOrdinaryFishingRod);
    }

    let tier = ordinary_rod_tier_from_definition(&data)
        .ok_or(FishingAreaAccessError::InvalidOrdinaryRodTierMetadata)?;
    Ok(FishingRodForUnlock::Ordinary(tier))
}

fn ordinary_rod_tier_from_definition(data: &Value) -> Option<EquipmentTier> {
    match data.get("tier").and_then(Value::as_str) {
        Some("WOOD") => Some(EquipmentTier::Wood),
        Some("STONE") => Some(EquipmentTier::Stone),
        Some("COPPER") => Some(EquipmentTier::Copper),
        Some("GOLD") => Some(EquipmentTier::Gold),
        Some("IRON") => Some(EquipmentTier::Iron),
        Some("DIAMOND") => Some(EquipmentTier::Diamond),
        Some("OBSIDIAN") => Some(EquipmentTier::Obsidian),
        Some("NETHERITE") => Some(EquipmentTier::Netherite),
        Some("GRAPHITE") => Some(EquipmentTier::Graphite),
        _ => None,
    }
}

fn rebirth_for_first_unlock_policy(rebirth_count: i64) -> Result<u32, FishingAreaAccessError> {
    if rebirth_count < 0 {
        return Err(FishingAreaAccessError::InvalidRebirthCount(rebirth_count));
    }
    Ok(u32::try_from(rebirth_count).unwrap_or(u32::MAX))
}

const fn persisted_area_key(area: FishingArea) -> Option<&'static str> {
    match area {
        FishingArea::StarterPool => None,
        FishingArea::River => Some("RIVER"),
        FishingArea::Lake => Some("LAKE"),
        FishingArea::Coast => Some("COAST"),
        FishingArea::DeepSea => Some("DEEP_SEA"),
        FishingArea::Abyss => Some("ABYSS"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn persisted_area_keys_cover_only_non_default_areas() {
        assert_eq!(persisted_area_key(FishingArea::StarterPool), None);
        assert_eq!(persisted_area_key(FishingArea::River), Some("RIVER"));
        assert_eq!(persisted_area_key(FishingArea::Lake), Some("LAKE"));
        assert_eq!(persisted_area_key(FishingArea::Coast), Some("COAST"));
        assert_eq!(persisted_area_key(FishingArea::DeepSea), Some("DEEP_SEA"));
        assert_eq!(persisted_area_key(FishingArea::Abyss), Some("ABYSS"));
    }

    #[test]
    fn ordinary_rod_tier_metadata_is_exact_and_fail_closed() {
        let cases = [
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
        for (raw, expected) in cases {
            assert_eq!(
                ordinary_rod_tier_from_definition(&json!({"tier": raw})),
                Some(expected)
            );
        }
        for invalid in [
            json!({}),
            json!({"tier": "LEATHER"}),
            json!({"tier": "wood"}),
            json!({"tier": 7}),
        ] {
            assert_eq!(ordinary_rod_tier_from_definition(&invalid), None);
        }
    }

    #[test]
    fn very_large_non_negative_rebirth_counts_preserve_the_threshold_semantics() {
        assert_eq!(rebirth_for_first_unlock_policy(0).unwrap(), 0);
        assert_eq!(rebirth_for_first_unlock_policy(1).unwrap(), 1);
        assert_eq!(
            rebirth_for_first_unlock_policy(i64::from(u32::MAX) + 1).unwrap(),
            u32::MAX
        );
        assert!(matches!(
            rebirth_for_first_unlock_policy(-1),
            Err(FishingAreaAccessError::InvalidRebirthCount(-1))
        ));
    }
}
