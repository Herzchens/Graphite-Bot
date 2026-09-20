use graphite_items::{ItemError, lock_owned_item_ordinary_equipment_classification};
use serde::Serialize;
use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    MiningPickaxeDurabilityPolicyError, MiningPickaxeOrdinaryDurabilityPreview,
    preview_mining_pickaxe_ordinary_durability_event,
};

const STARTER_WOOD_PICKAXE_DEFINITION_KEY: &str = "equipment.pickaxe.wood.starter";

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AppliedMiningPickaxeOrdinaryDurabilityState {
    StarterWoodUnbreakable {
        item_instance_id: Uuid,
        definition_key: String,
        definition_version: i32,
    },
    Ordinary {
        item_instance_id: Uuid,
        definition_key: String,
        definition_version: i32,
        preview: MiningPickaxeOrdinaryDurabilityPreview,
    },
}

#[derive(Debug, Error)]
pub enum MiningPickaxeDurabilityStateError {
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error(transparent)]
    Item(#[from] ItemError),
    #[error(transparent)]
    Policy(#[from] MiningPickaxeDurabilityPolicyError),
    #[error("owning operation does not exist")]
    OperationNotFound,
    #[error("owning operation targets a different player")]
    OperationPlayerMismatch,
    #[error("owning operation cannot mutate Pickaxe durability in state {0}")]
    OperationTerminal(String),
    #[error("player does not exist")]
    PlayerNotFound,
    #[error("Pickaxe durability mutation requires an ACTIVE account; current status is {0}")]
    AccountFrozen(String),
    #[error("no Pickaxe is currently equipped")]
    NoEquippedPickaxe,
    #[error("equipped Pickaxe state is internally inconsistent")]
    EquippedPickaxeIntegrityMismatch,
    #[error("Starter Wood Pickaxe identity/state is internally inconsistent")]
    StarterWoodPickaxeIntegrityMismatch,
    #[error("the equipped Pickaxe is not classified as ordinary equipment")]
    NonOrdinaryPickaxe,
    #[error("ordinary Pickaxe durability state is missing or internally inconsistent")]
    InvalidOrdinaryPickaxeDurability,
    #[error("ordinary Pickaxe is already Broken and cannot receive another ordinary wear event")]
    OrdinaryPickaxeAlreadyBroken,
    #[error(
        "resolved Pickaxe durability expected current durability {expected}, but authoritative durability is {actual}"
    )]
    DurabilityChanged { expected: u32, actual: u32 },
    #[error("Starter Wood Pickaxe has no mutable durability value")]
    StarterWoodExpectedDurability,
    #[error("ordinary Pickaxe requires an expected current durability value")]
    OrdinaryPickaxeExpectedDurabilityMissing,
    #[error("the locked Pickaxe durability row changed unexpectedly before write")]
    LockedStateMismatch,
}

impl From<sqlx::Error> for MiningPickaxeDurabilityStateError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LockedEquippedPickaxeState {
    item_instance_id: Uuid,
    definition_key: String,
    definition_version: i32,
    is_starter: bool,
    is_unbreakable: bool,
    is_repairable: bool,
    is_ordinary_equipment: bool,
    current_durability: Option<i64>,
    max_durability: Option<i64>,
    is_broken: bool,
}

/// Applies one already-resolved ordinary Pickaxe durability event inside a caller-owned Mining
/// transaction.
///
/// The primitive locks in `operation -> player -> ItemInstance -> equipment slot` order and
/// re-resolves the currently equipped Pickaxe from authoritative versioned item state. The exact
/// system-bound Starter Wood Pickaxe is an explicit unbreakable no-op. Every other Pickaxe must be
/// classified as ordinary equipment and carry a consistent mutable durability representation.
///
/// `expected_current_durability` is the optimistic token from the caller's still-open authoritative
/// expedition snapshot. A mismatch fails closed so re-entry into a still-PENDING operation cannot
/// silently consume a second durability point. The operation owner must still intercept COMMITTED
/// replay before calling this primitive and must finalize result/audit/outbox state atomically in the
/// same transaction.
///
/// This primitive intentionally handles only the normal one-point Pickaxe durability event after the
/// caller has already resolved whether Unbreaking prevented it. It does **not** apply Nuke
/// destruction: the frozen Nuke consequence also requires `NUKE_BURNOUT`, whose canonical persisted
/// representation is not currently frozen in the repository. Persisting only the zero-durability
/// half of that consequence would create invalid partial state. It also does not draw RNG, apply
/// Mending/Master, settle pressure/ore/AEXP, finalize the operation, or expose `/mine`.
pub async fn apply_resolved_equipped_pickaxe_ordinary_durability_event(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
    expected_current_durability: Option<u32>,
    ordinary_event_prevented_by_unbreaking: bool,
) -> Result<AppliedMiningPickaxeOrdinaryDurabilityState, MiningPickaxeDurabilityStateError> {
    lock_pending_player_operation(tx, operation_id, player_id).await?;
    lock_active_player(tx, player_id).await?;
    let pickaxe = lock_equipped_pickaxe_state(tx, player_id).await?;

    if pickaxe.definition_key == STARTER_WOOD_PICKAXE_DEFINITION_KEY {
        if !pickaxe.is_starter
            || !pickaxe.is_unbreakable
            || pickaxe.is_repairable
            || pickaxe.is_ordinary_equipment
            || pickaxe.current_durability.is_some()
            || pickaxe.max_durability.is_some()
            || pickaxe.is_broken
        {
            return Err(MiningPickaxeDurabilityStateError::StarterWoodPickaxeIntegrityMismatch);
        }
        if expected_current_durability.is_some() {
            return Err(MiningPickaxeDurabilityStateError::StarterWoodExpectedDurability);
        }
        return Ok(
            AppliedMiningPickaxeOrdinaryDurabilityState::StarterWoodUnbreakable {
                item_instance_id: pickaxe.item_instance_id,
                definition_key: pickaxe.definition_key,
                definition_version: pickaxe.definition_version,
            },
        );
    }

    if pickaxe.is_starter {
        return Err(MiningPickaxeDurabilityStateError::StarterWoodPickaxeIntegrityMismatch);
    }
    if !pickaxe.is_ordinary_equipment {
        return Err(MiningPickaxeDurabilityStateError::NonOrdinaryPickaxe);
    }

    let current = pickaxe
        .current_durability
        .and_then(|value| u32::try_from(value).ok())
        .ok_or(MiningPickaxeDurabilityStateError::InvalidOrdinaryPickaxeDurability)?;
    let maximum = pickaxe
        .max_durability
        .and_then(|value| u32::try_from(value).ok())
        .ok_or(MiningPickaxeDurabilityStateError::InvalidOrdinaryPickaxeDurability)?;

    if maximum == 0 || current > maximum {
        return Err(MiningPickaxeDurabilityStateError::InvalidOrdinaryPickaxeDurability);
    }
    if current == 0 {
        return if pickaxe.is_broken {
            Err(MiningPickaxeDurabilityStateError::OrdinaryPickaxeAlreadyBroken)
        } else {
            Err(MiningPickaxeDurabilityStateError::InvalidOrdinaryPickaxeDurability)
        };
    }
    if pickaxe.is_broken {
        return Err(MiningPickaxeDurabilityStateError::InvalidOrdinaryPickaxeDurability);
    }

    let expected = expected_current_durability
        .ok_or(MiningPickaxeDurabilityStateError::OrdinaryPickaxeExpectedDurabilityMissing)?;
    if expected != current {
        return Err(MiningPickaxeDurabilityStateError::DurabilityChanged {
            expected,
            actual: current,
        });
    }

    let preview = preview_mining_pickaxe_ordinary_durability_event(
        current,
        maximum,
        true,
        ordinary_event_prevented_by_unbreaking,
    )?;

    if preview.resulting_durability != current {
        let updated = sqlx::query(
            r#"
            UPDATE item_instances
               SET current_durability = $1,
                   is_broken = $2
             WHERE id = $3
               AND owner_player_id = $4
               AND location = 'EQUIPPED'
               AND current_durability = $5
               AND max_durability = $6
               AND is_broken = FALSE
            "#,
        )
        .bind(i64::from(preview.resulting_durability))
        .bind(preview.resulting_durability == 0)
        .bind(pickaxe.item_instance_id)
        .bind(player_id)
        .bind(i64::from(current))
        .bind(i64::from(maximum))
        .execute(&mut **tx)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(MiningPickaxeDurabilityStateError::LockedStateMismatch);
        }
    }

    Ok(AppliedMiningPickaxeOrdinaryDurabilityState::Ordinary {
        item_instance_id: pickaxe.item_instance_id,
        definition_key: pickaxe.definition_key,
        definition_version: pickaxe.definition_version,
        preview,
    })
}

async fn lock_equipped_pickaxe_state(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
) -> Result<LockedEquippedPickaxeState, MiningPickaxeDurabilityStateError> {
    let item_id: Uuid = sqlx::query_scalar(
        "SELECT item_instance_id FROM equipment_slots WHERE player_id = $1 AND slot = 'PICKAXE'",
    )
    .bind(player_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(MiningPickaxeDurabilityStateError::NoEquippedPickaxe)?;

    let classification =
        lock_owned_item_ordinary_equipment_classification(tx, player_id, item_id).await?;
    let row = sqlx::query(
        r#"
        SELECT i.location,
               i.is_starter,
               i.is_unbreakable,
               i.is_repairable,
               i.current_durability,
               i.max_durability,
               i.is_broken,
               d.category
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
    .ok_or(MiningPickaxeDurabilityStateError::EquippedPickaxeIntegrityMismatch)?;

    let slot_item_id: Uuid = sqlx::query_scalar(
        "SELECT item_instance_id FROM equipment_slots WHERE player_id = $1 AND slot = 'PICKAXE' FOR UPDATE",
    )
    .bind(player_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(MiningPickaxeDurabilityStateError::EquippedPickaxeIntegrityMismatch)?;
    if slot_item_id != item_id {
        return Err(MiningPickaxeDurabilityStateError::EquippedPickaxeIntegrityMismatch);
    }

    let location: String = row.try_get("location")?;
    let category: String = row.try_get("category")?;
    if location != "EQUIPPED" || category != "PICKAXE" {
        return Err(MiningPickaxeDurabilityStateError::EquippedPickaxeIntegrityMismatch);
    }

    Ok(LockedEquippedPickaxeState {
        item_instance_id: item_id,
        definition_key: classification.definition_key,
        definition_version: classification.definition_version,
        is_starter: row.try_get("is_starter")?,
        is_unbreakable: row.try_get("is_unbreakable")?,
        is_repairable: row.try_get("is_repairable")?,
        is_ordinary_equipment: classification.is_ordinary_equipment,
        current_durability: row.try_get("current_durability")?,
        max_durability: row.try_get("max_durability")?,
        is_broken: row.try_get("is_broken")?,
    })
}

async fn lock_pending_player_operation(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
) -> Result<(), MiningPickaxeDurabilityStateError> {
    let row = sqlx::query("SELECT player_id, state FROM operations WHERE id = $1 FOR UPDATE")
        .bind(operation_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(MiningPickaxeDurabilityStateError::OperationNotFound)?;
    let operation_player_id: Option<Uuid> = row.try_get("player_id")?;
    if operation_player_id != Some(player_id) {
        return Err(MiningPickaxeDurabilityStateError::OperationPlayerMismatch);
    }
    let state: String = row.try_get("state")?;
    if state != "PENDING" {
        return Err(MiningPickaxeDurabilityStateError::OperationTerminal(state));
    }
    Ok(())
}

async fn lock_active_player(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
) -> Result<(), MiningPickaxeDurabilityStateError> {
    let status: String = sqlx::query_scalar(
        "SELECT status FROM players WHERE id = $1 AND status <> 'DELETED' FOR UPDATE",
    )
    .bind(player_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(MiningPickaxeDurabilityStateError::PlayerNotFound)?;
    if status != "ACTIVE" {
        return Err(MiningPickaxeDurabilityStateError::AccountFrozen(status));
    }
    Ok(())
}
