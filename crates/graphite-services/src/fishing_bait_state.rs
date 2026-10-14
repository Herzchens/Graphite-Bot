use graphite_core::OperationId;
use graphite_items::{
    StackConsumptionMutationError, StackConsumptionMutationReceipt,
    StackConsumptionMutationRequest, apply_stack_consumption_mutation,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    ActiveFishingBaitInventory, BaitRackPolicyError, CanonicalEnchant,
    EquippedFishingRodCastSnapshot, EquippedFishingRodCastSnapshotError, FishingBait,
    FishingBaitCastConsumptionAction, FishingBaitCastConsumptionError,
    FishingBaitCastConsumptionPlan, bait_rack_capacity_policy, fishing_bait_item_definition_key,
    lock_equipped_fishing_rod_cast_snapshot, plan_fishing_bait_cast_consumption,
};

pub const FISHING_BAIT_ITEM_DEFINITION_VERSION: i32 = 1;

const SELECTION_MUTATION_KEY: &str = "fishing:bait:selection";
const SELECTION_EVENT_KIND: &str = "FISHING_BAIT_SELECTION_REPLACED";
const CAST_SETTLEMENT_MUTATION_KEY: &str = "fishing:bait:cast";
const CAST_SETTLEMENT_EVENT_KIND: &str = "FISHING_BAIT_CAST_SETTLED";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PersistedActiveFishingBait {
    pub bait: FishingBait,
    pub definition_key: String,
    pub definition_version: i32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FishingBaitSelectionMutationReceipt {
    pub event_id: Uuid,
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub previous: Vec<PersistedActiveFishingBait>,
    pub active: Vec<PersistedActiveFishingBait>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FishingBaitCastSettlementReceipt {
    pub event_id: Uuid,
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub plan: FishingBaitCastConsumptionPlan,
    pub consumption_receipts: Vec<StackConsumptionMutationReceipt>,
    pub auto_detached: Vec<FishingBait>,
}

#[derive(Debug, Error)]
pub enum FishingBaitStateError {
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error(transparent)]
    Rod(#[from] EquippedFishingRodCastSnapshotError),
    #[error(transparent)]
    BaitRack(#[from] BaitRackPolicyError),
    #[error(transparent)]
    Plan(#[from] FishingBaitCastConsumptionError),
    #[error(transparent)]
    StackConsumption(#[from] StackConsumptionMutationError),
    #[error("owning operation does not exist")]
    OperationNotFound,
    #[error("owning operation targets a different player")]
    OperationPlayerMismatch,
    #[error("owning operation cannot mutate Fishing bait state in state {0}")]
    OperationTerminal(String),
    #[error("player does not exist")]
    PlayerNotFound,
    #[error("Fishing bait state mutation requires an ACTIVE account; current status is {0}")]
    AccountFrozen(String),
    #[error("Fishing Rod is Broken and cannot settle cast bait")]
    BrokenFishingRod,
    #[error(
        "active bait selection has {selected} categories but the equipped Rod permits only {capacity}"
    )]
    ActiveCategoryCapacityExceeded { selected: usize, capacity: u8 },
    #[error("Fishing bait {0:?} appears more than once in the requested active selection")]
    DuplicateActiveBait(FishingBait),
    #[error("persisted Fishing bait kind is not canonical: {0}")]
    UnknownPersistedBaitKind(String),
    #[error(
        "persisted Fishing bait {bait:?} uses unsupported ItemDefinition version {version}; expected {FISHING_BAIT_ITEM_DEFINITION_VERSION}"
    )]
    UnsupportedBaitDefinitionVersion { bait: FishingBait, version: i32 },
    #[error("persisted Fishing bait selection is internally inconsistent")]
    PersistedSelectionIntegrityMismatch,
    #[error("the same operation bait-selection mutation was reused with different input")]
    SelectionMutationConflict,
    #[error("stored Fishing bait event payload is invalid: {0}")]
    InvalidStoredEvent(Box<serde_json::Error>),
    #[error("the stored Fishing bait cast settlement is internally inconsistent")]
    CastSettlementConflict,
}

impl From<sqlx::Error> for FishingBaitStateError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct FishingBaitSelectionEvent {
    requested: Vec<FishingBait>,
    receipt: FishingBaitSelectionMutationReceipt,
}

/// Replaces the persisted active Fishing-bait selection inside a caller-owned transaction.
///
/// The owning operation must already target this player. The current equipped Rod is locked and its
/// authoritative Bait Rack level defines the active-category capacity. Selection identity is the
/// canonical bait kind plus the exact v1 bait ItemDefinition; request-provided definition keys or
/// versions are never accepted.
///
/// This mutation intentionally does not require the selected bait to be present in Item Bag. The
/// canonical cast rule is that a missing active bait auto-detaches rather than blocking the cast, so
/// possession is resolved at cast settlement. Replacing the selection is mutation-key idempotent and
/// audited through asset events; the higher-level owner still finalizes the operation/outbox.
pub async fn replace_active_fishing_baits(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
    requested: &[FishingBait],
) -> Result<FishingBaitSelectionMutationReceipt, FishingBaitStateError> {
    validate_requested_selection(requested)?;

    let operation_state = lock_target_operation(tx, operation_id, player_id).await?;
    if let Some(receipt) = replay_selection_mutation(tx, operation_id, player_id, requested).await?
    {
        return Ok(receipt);
    }
    if operation_state != "PENDING" {
        return Err(FishingBaitStateError::OperationTerminal(operation_state));
    }

    lock_active_player(tx, player_id).await?;
    let rod = lock_equipped_fishing_rod_cast_snapshot(tx, player_id).await?;
    let bait_rack_level = bait_rack_level(&rod);
    let capacity = bait_rack_capacity_policy(bait_rack_level)?;
    if requested.len() > usize::from(capacity.active_bait_category_slots) {
        return Err(FishingBaitStateError::ActiveCategoryCapacityExceeded {
            selected: requested.len(),
            capacity: capacity.active_bait_category_slots,
        });
    }

    let previous = lock_persisted_selection(tx, player_id).await?;

    sqlx::query("DELETE FROM player_active_fishing_baits WHERE player_id = $1")
        .bind(player_id)
        .execute(&mut **tx)
        .await?;

    let mut active = Vec::with_capacity(requested.len());
    for bait in requested {
        let definition_key = fishing_bait_item_definition_key(*bait);
        sqlx::query(
            r#"
            INSERT INTO player_active_fishing_baits (
                player_id, bait_kind, definition_key, definition_version,
                activated_by_operation_id
            )
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(player_id)
        .bind(bait_persisted_key(*bait))
        .bind(definition_key)
        .bind(FISHING_BAIT_ITEM_DEFINITION_VERSION)
        .bind(operation_id)
        .execute(&mut **tx)
        .await?;

        active.push(PersistedActiveFishingBait {
            bait: *bait,
            definition_key: definition_key.to_owned(),
            definition_version: FISHING_BAIT_ITEM_DEFINITION_VERSION,
        });
    }

    let receipt = FishingBaitSelectionMutationReceipt {
        event_id: OperationId::new().as_uuid(),
        operation_id,
        player_id,
        previous,
        active,
    };
    let payload = serde_json::to_value(FishingBaitSelectionEvent {
        requested: requested.to_vec(),
        receipt: receipt.clone(),
    })
    .map_err(|error| FishingBaitStateError::InvalidStoredEvent(Box::new(error)))?;

    insert_asset_event(
        tx,
        receipt.event_id,
        operation_id,
        player_id,
        SELECTION_MUTATION_KEY,
        SELECTION_EVENT_KIND,
        payload,
    )
    .await?;

    Ok(receipt)
}

/// Consumes every currently active bait that exists in Item Bag and auto-detaches every missing bait.
///
/// The function re-locks the current Rod and persisted active selection inside the caller transaction.
/// The owning player lock makes the selected Item Bag quantities stable for planning; the shared
/// stack-consumption primitive then acquires each definition/stack lock in its canonical order for the
/// actual one-unit decrement. A bait
/// that has no unit at cast start is removed from the active selection and recorded in the cast
/// settlement receipt. Consuming the final available unit does not detach immediately; it becomes
/// missing and auto-detaches on the next cast, matching the pure consumption policy.
///
/// A Rod swap that makes the persisted selection exceed current capacity fails closed rather than
/// silently choosing which bait effect to discard. The entire bait settlement has its own replay
/// event, so re-entering the same operation cannot consume a second unit. The caller still owns the
/// rest of the Fishing outcome and must commit bait, reward, durability, progression, operation
/// result, audit, and outbox atomically.
pub async fn settle_active_fishing_baits_for_cast(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
) -> Result<FishingBaitCastSettlementReceipt, FishingBaitStateError> {
    let operation_state = lock_target_operation(tx, operation_id, player_id).await?;
    if let Some(receipt) = replay_cast_settlement(tx, operation_id, player_id).await? {
        return Ok(receipt);
    }
    if operation_state != "PENDING" {
        return Err(FishingBaitStateError::OperationTerminal(operation_state));
    }

    lock_active_player(tx, player_id).await?;
    let rod = lock_equipped_fishing_rod_cast_snapshot(tx, player_id).await?;
    if rod.is_broken {
        return Err(FishingBaitStateError::BrokenFishingRod);
    }

    let selection = lock_persisted_selection(tx, player_id).await?;
    let mut inventory = Vec::with_capacity(selection.len());
    for selected in &selection {
        if selected.definition_version != FISHING_BAIT_ITEM_DEFINITION_VERSION {
            return Err(FishingBaitStateError::UnsupportedBaitDefinitionVersion {
                bait: selected.bait,
                version: selected.definition_version,
            });
        }
        let expected_key = fishing_bait_item_definition_key(selected.bait);
        if selected.definition_key != expected_key {
            return Err(FishingBaitStateError::PersistedSelectionIntegrityMismatch);
        }

        let available_units: i64 = sqlx::query_scalar(
            r#"
            SELECT quantity
              FROM item_stacks
             WHERE player_id = $1
               AND definition_key = $2
               AND definition_version = $3
               AND location = 'ITEM_BAG'
            "#,
        )
        .bind(player_id)
        .bind(&selected.definition_key)
        .bind(selected.definition_version)
        .fetch_optional(&mut **tx)
        .await?
        .unwrap_or(0);

        inventory.push(ActiveFishingBaitInventory {
            bait: selected.bait,
            available_units,
        });
    }

    let plan = plan_fishing_bait_cast_consumption(bait_rack_level(&rod), &inventory)?;
    let mut consumption_receipts = Vec::with_capacity(usize::from(plan.consumed_bait_categories));
    let mut auto_detached = Vec::with_capacity(usize::from(plan.auto_detached_bait_categories));

    for entry in &plan.entries {
        let selected = selection
            .iter()
            .find(|row| row.bait == entry.bait)
            .ok_or(FishingBaitStateError::PersistedSelectionIntegrityMismatch)?;

        match entry.action {
            FishingBaitCastConsumptionAction::ConsumeForCast => {
                let receipt = apply_stack_consumption_mutation(
                    tx,
                    &StackConsumptionMutationRequest {
                        operation_id,
                        player_id,
                        mutation_key: format!(
                            "fishing:bait:consume:{}",
                            bait_persisted_key(entry.bait).to_ascii_lowercase()
                        ),
                        definition_key: selected.definition_key.clone(),
                        definition_version: selected.definition_version,
                        quantity: i64::from(entry.units_consumed),
                        source: "MANUAL_FISHING".to_owned(),
                        provenance: json!({
                            "bait": bait_persisted_key(entry.bait),
                            "purpose": "CAST_CONSUMPTION"
                        }),
                    },
                )
                .await?;
                consumption_receipts.push(receipt);
            }
            FishingBaitCastConsumptionAction::AutoDetachMissingUnit => {
                let rows_affected = sqlx::query(
                    r#"
                    DELETE FROM player_active_fishing_baits
                     WHERE player_id = $1
                       AND bait_kind = $2
                       AND definition_key = $3
                       AND definition_version = $4
                    "#,
                )
                .bind(player_id)
                .bind(bait_persisted_key(entry.bait))
                .bind(&selected.definition_key)
                .bind(selected.definition_version)
                .execute(&mut **tx)
                .await?
                .rows_affected();
                if rows_affected != 1 {
                    return Err(FishingBaitStateError::PersistedSelectionIntegrityMismatch);
                }
                auto_detached.push(entry.bait);
            }
        }
    }

    let receipt = FishingBaitCastSettlementReceipt {
        event_id: OperationId::new().as_uuid(),
        operation_id,
        player_id,
        plan,
        consumption_receipts,
        auto_detached,
    };
    let payload = serde_json::to_value(&receipt)
        .map_err(|error| FishingBaitStateError::InvalidStoredEvent(Box::new(error)))?;
    insert_asset_event(
        tx,
        receipt.event_id,
        operation_id,
        player_id,
        CAST_SETTLEMENT_MUTATION_KEY,
        CAST_SETTLEMENT_EVENT_KIND,
        payload,
    )
    .await?;

    Ok(receipt)
}

fn validate_requested_selection(requested: &[FishingBait]) -> Result<(), FishingBaitStateError> {
    for (index, bait) in requested.iter().enumerate() {
        if requested[..index].contains(bait) {
            return Err(FishingBaitStateError::DuplicateActiveBait(*bait));
        }
    }
    Ok(())
}

fn bait_rack_level(rod: &EquippedFishingRodCastSnapshot) -> Option<u8> {
    rod.embedded_enchants
        .iter()
        .find(|state| state.enchant == CanonicalEnchant::BaitRack)
        .map(|state| state.level)
}

async fn lock_target_operation(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
) -> Result<String, FishingBaitStateError> {
    let row = sqlx::query("SELECT player_id, state FROM operations WHERE id = $1 FOR UPDATE")
        .bind(operation_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(FishingBaitStateError::OperationNotFound)?;
    let operation_player_id: Option<Uuid> = row.try_get("player_id")?;
    if operation_player_id != Some(player_id) {
        return Err(FishingBaitStateError::OperationPlayerMismatch);
    }
    Ok(row.try_get("state")?)
}

async fn lock_active_player(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
) -> Result<(), FishingBaitStateError> {
    let status: String = sqlx::query_scalar(
        "SELECT status FROM players WHERE id = $1 AND status <> 'DELETED' FOR UPDATE",
    )
    .bind(player_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(FishingBaitStateError::PlayerNotFound)?;
    if status != "ACTIVE" {
        return Err(FishingBaitStateError::AccountFrozen(status));
    }
    Ok(())
}

async fn lock_persisted_selection(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
) -> Result<Vec<PersistedActiveFishingBait>, FishingBaitStateError> {
    let rows = sqlx::query(
        r#"
        SELECT bait_kind, definition_key, definition_version
          FROM player_active_fishing_baits
         WHERE player_id = $1
         ORDER BY bait_kind
         FOR UPDATE
        "#,
    )
    .bind(player_id)
    .fetch_all(&mut **tx)
    .await?;

    let mut selection = Vec::with_capacity(rows.len());
    for row in rows {
        let raw_kind: String = row.try_get("bait_kind")?;
        let bait = bait_from_persisted_key(&raw_kind)
            .ok_or_else(|| FishingBaitStateError::UnknownPersistedBaitKind(raw_kind.clone()))?;
        let definition_key: String = row.try_get("definition_key")?;
        let definition_version: i32 = row.try_get("definition_version")?;
        if definition_key != fishing_bait_item_definition_key(bait) {
            return Err(FishingBaitStateError::PersistedSelectionIntegrityMismatch);
        }
        selection.push(PersistedActiveFishingBait {
            bait,
            definition_key,
            definition_version,
        });
    }
    Ok(selection)
}

async fn replay_selection_mutation(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
    requested: &[FishingBait],
) -> Result<Option<FishingBaitSelectionMutationReceipt>, FishingBaitStateError> {
    let Some(row) = load_asset_event(tx, operation_id, SELECTION_MUTATION_KEY).await? else {
        return Ok(None);
    };
    let event_id: Uuid = row.try_get("id")?;
    let stored_player_id: Uuid = row.try_get("player_id")?;
    let event_kind: String = row.try_get("event_kind")?;
    let payload: Value = row.try_get("payload")?;
    let event: FishingBaitSelectionEvent = serde_json::from_value(payload)
        .map_err(|error| FishingBaitStateError::InvalidStoredEvent(Box::new(error)))?;

    if event_kind != SELECTION_EVENT_KIND
        || event_id != event.receipt.event_id
        || stored_player_id != player_id
        || event.receipt.operation_id != operation_id
        || event.receipt.player_id != player_id
        || event.requested != requested
    {
        return Err(FishingBaitStateError::SelectionMutationConflict);
    }
    Ok(Some(event.receipt))
}

async fn replay_cast_settlement(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
) -> Result<Option<FishingBaitCastSettlementReceipt>, FishingBaitStateError> {
    let Some(row) = load_asset_event(tx, operation_id, CAST_SETTLEMENT_MUTATION_KEY).await? else {
        return Ok(None);
    };
    let event_id: Uuid = row.try_get("id")?;
    let stored_player_id: Uuid = row.try_get("player_id")?;
    let event_kind: String = row.try_get("event_kind")?;
    let payload: Value = row.try_get("payload")?;
    let receipt: FishingBaitCastSettlementReceipt = serde_json::from_value(payload)
        .map_err(|error| FishingBaitStateError::InvalidStoredEvent(Box::new(error)))?;

    if event_kind != CAST_SETTLEMENT_EVENT_KIND
        || event_id != receipt.event_id
        || stored_player_id != player_id
        || receipt.operation_id != operation_id
        || receipt.player_id != player_id
    {
        return Err(FishingBaitStateError::CastSettlementConflict);
    }
    Ok(Some(receipt))
}

async fn load_asset_event(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    mutation_key: &str,
) -> Result<Option<sqlx::postgres::PgRow>, FishingBaitStateError> {
    Ok(sqlx::query(
        r#"
        SELECT id, player_id, event_kind, payload
          FROM asset_events
         WHERE operation_id = $1
           AND mutation_key = $2
         FOR UPDATE
        "#,
    )
    .bind(operation_id)
    .bind(mutation_key)
    .fetch_optional(&mut **tx)
    .await?)
}

async fn insert_asset_event(
    tx: &mut Transaction<'_, Postgres>,
    event_id: Uuid,
    operation_id: Uuid,
    player_id: Uuid,
    mutation_key: &str,
    event_kind: &str,
    payload: Value,
) -> Result<(), FishingBaitStateError> {
    sqlx::query(
        r#"
        INSERT INTO asset_events (
            id, operation_id, mutation_key, player_id, event_kind, payload
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(event_id)
    .bind(operation_id)
    .bind(mutation_key)
    .bind(player_id)
    .bind(event_kind)
    .bind(payload)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

const fn bait_persisted_key(bait: FishingBait) -> &'static str {
    match bait {
        FishingBait::School => "SCHOOL",
        FishingBait::Quality => "QUALITY",
        FishingBait::Rare => "RARE",
        FishingBait::Treasure => "TREASURE",
        FishingBait::Sturdy => "STURDY",
    }
}

fn bait_from_persisted_key(key: &str) -> Option<FishingBait> {
    match key {
        "SCHOOL" => Some(FishingBait::School),
        "QUALITY" => Some(FishingBait::Quality),
        "RARE" => Some(FishingBait::Rare),
        "TREASURE" => Some(FishingBait::Treasure),
        "STURDY" => Some(FishingBait::Sturdy),
        _ => None,
    }
}
