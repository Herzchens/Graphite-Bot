use super::{ItemError, item_bag_capacity_slots, item_bag_used_slots};
use graphite_core::OperationId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

const MAX_MUTATION_KEY_BYTES: usize = 128;
const MAX_SOURCE_BYTES: usize = 128;
const DELIVERED_EVENT_KIND: &str = "UNIQUE_ITEM_BAG_SUBDELIVERY_DELIVERED";
const PENDING_EVENT_KIND: &str = "UNIQUE_ITEM_BAG_SUBDELIVERY_PENDING";
const CLAIM_EVENT_KIND: &str = "UNIQUE_ITEM_BAG_PENDING_CLAIMED";
const CLAIM_MUTATION_KEY: &str = "item-bag:unique-pending-claim";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UniqueItemInstanceFlags {
    pub is_account_bound: bool,
    pub is_tradeable: bool,
    pub is_sellable: bool,
    pub is_discardable: bool,
    pub is_enchantable: bool,
    pub is_upgradeable: bool,
    pub is_unbreakable: bool,
    pub is_repairable: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UniqueItemBagDeliveryMutationRequest {
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub mutation_key: String,
    pub definition_key: String,
    pub definition_version: i32,
    pub flags: UniqueItemInstanceFlags,
    pub state: Value,
    pub source: String,
    pub provenance: Value,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UniqueItemBagDeliveryMutationReceipt {
    pub event_id: Uuid,
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub mutation_key: String,
    pub item_instance_id: Uuid,
    pub definition_key: String,
    pub definition_version: i32,
    pub pending_delivery_id: Option<Uuid>,
    pub pending: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ClaimedUniqueItemBagDelivery {
    pub pending_delivery_id: Uuid,
    pub item_instance_id: Uuid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UniqueItemBagPendingClaimReceipt {
    pub event_id: Uuid,
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub claimed: Vec<ClaimedUniqueItemBagDelivery>,
    pub blocked_pending_delivery_id: Option<Uuid>,
    pub used_slots_after: u64,
}

#[derive(Debug, Error)]
pub enum UniqueItemBagDeliveryMutationError {
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error(transparent)]
    Item(#[from] ItemError),
    #[error("unique Item Bag delivery definition version must be positive")]
    InvalidDefinitionVersion,
    #[error(
        "unique Item Bag delivery mutation key must be a non-empty internal key of at most {MAX_MUTATION_KEY_BYTES} bytes"
    )]
    InvalidMutationKey,
    #[error("unique Item Bag delivery source must be non-empty and at most {MAX_SOURCE_BYTES} bytes")]
    InvalidSource,
    #[error("unique Item Bag delivery state must be a JSON object")]
    InvalidState,
    #[error("unique Item Bag delivery provenance must be a non-empty JSON object")]
    InvalidProvenance,
    #[error("player does not exist")]
    PlayerNotFound,
    #[error("unique Item Bag delivery requires an ACTIVE account; current status is {0}")]
    AccountFrozen(String),
    #[error("owning operation does not exist")]
    OperationNotFound,
    #[error("owning operation targets a different player")]
    OperationPlayerMismatch,
    #[error("owning operation cannot accept a new unique Item Bag delivery in state {0}")]
    OperationTerminal(String),
    #[error("unique Item Bag delivery requires an exact-version non-stackable definition")]
    InvalidUniqueDefinition,
    #[error("an unresolved unique Item Bag delivery from an earlier operation must be claimed first")]
    EarlierPendingUniqueDelivery,
    #[error("the same operation mutation key was reused with different unique Item Bag input")]
    MutationConflict,
    #[error("stored unique Item Bag mutation payload is invalid: {0}")]
    InvalidStoredMutation(Box<serde_json::Error>),
    #[error("pending unique Item Bag delivery points at invalid or stale item state")]
    PendingDeliveryIntegrityMismatch,
}

impl From<sqlx::Error> for UniqueItemBagDeliveryMutationError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct UniqueItemBagDeliveryMutationPayload {
    receipt: UniqueItemBagDeliveryMutationReceipt,
    flags: UniqueItemInstanceFlags,
    state: Value,
    source: String,
    provenance: Value,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct UniqueItemBagClaimMutationPayload {
    receipt: UniqueItemBagPendingClaimReceipt,
}

/// Creates one exact-version unique ItemInstance for Item Bag delivery inside an owning transaction.
///
/// Capacity is slot-based: every unique ItemInstance consumes exactly one Item Bag slot. A full bag
/// preserves the newly created item in TEMP_OVERFLOW and links it to a PENDING delivery. New unique
/// overflow is bounded by refusing a later operation while an older unique Item Bag delivery remains
/// unresolved; one operation may still create multiple pending unique items atomically (for example,
/// a Multi Treasure Fishing cast).
pub async fn apply_unique_item_bag_delivery_mutation(
    tx: &mut Transaction<'_, Postgres>,
    request: &UniqueItemBagDeliveryMutationRequest,
) -> Result<UniqueItemBagDeliveryMutationReceipt, UniqueItemBagDeliveryMutationError> {
    validate_delivery_input(request)?;

    let operation = sqlx::query("SELECT player_id, state FROM operations WHERE id = $1 FOR UPDATE")
        .bind(request.operation_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(UniqueItemBagDeliveryMutationError::OperationNotFound)?;
    let operation_player_id: Option<Uuid> = operation.try_get("player_id")?;
    if operation_player_id.is_some_and(|stored| stored != request.player_id) {
        return Err(UniqueItemBagDeliveryMutationError::OperationPlayerMismatch);
    }
    let operation_state: String = operation.try_get("state")?;

    if let Some(row) = sqlx::query(
        r#"
        SELECT id, player_id, event_kind, payload
          FROM asset_events
         WHERE operation_id = $1
           AND mutation_key = $2
         FOR UPDATE
        "#,
    )
    .bind(request.operation_id)
    .bind(&request.mutation_key)
    .fetch_optional(&mut **tx)
    .await?
    {
        return replay_unique_item_bag_delivery(row, request);
    }

    if operation_state != "PENDING" {
        return Err(UniqueItemBagDeliveryMutationError::OperationTerminal(
            operation_state,
        ));
    }

    let player = sqlx::query(
        r#"
        SELECT p.status, s.item_bag_level
          FROM players p
          JOIN player_storage_profiles s ON s.player_id = p.id
         WHERE p.id = $1
           AND p.status <> 'DELETED'
         FOR UPDATE OF p, s
        "#,
    )
    .bind(request.player_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(UniqueItemBagDeliveryMutationError::PlayerNotFound)?;
    let status: String = player.try_get("status")?;
    if status != "ACTIVE" {
        return Err(UniqueItemBagDeliveryMutationError::AccountFrozen(status));
    }
    let item_bag_level: i64 = player.try_get("item_bag_level")?;

    let definition = sqlx::query(
        r#"
        SELECT stackable
          FROM item_definition_versions
         WHERE key = $1
           AND version = $2
         FOR SHARE
        "#,
    )
    .bind(&request.definition_key)
    .bind(request.definition_version)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(UniqueItemBagDeliveryMutationError::InvalidUniqueDefinition)?;
    let stackable: bool = definition.try_get("stackable")?;
    if stackable {
        return Err(UniqueItemBagDeliveryMutationError::InvalidUniqueDefinition);
    }

    let older_pending = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT id
          FROM pending_asset_deliveries
         WHERE player_id = $1
           AND state = 'PENDING'
           AND desired_location = 'ITEM_BAG'
           AND item_instance_id IS NOT NULL
           AND operation_id <> $2
         ORDER BY created_at, id
         LIMIT 1
         FOR UPDATE
        "#,
    )
    .bind(request.player_id)
    .bind(request.operation_id)
    .fetch_optional(&mut **tx)
    .await?;
    if older_pending.is_some() {
        return Err(UniqueItemBagDeliveryMutationError::EarlierPendingUniqueDelivery);
    }

    let capacity = item_bag_capacity_slots(item_bag_level)?;
    let used_before = item_bag_used_slots(tx, request.player_id).await?;
    let pending = used_before
        .checked_add(1)
        .ok_or(ItemError::ArithmeticOverflow)?
        > capacity;

    let item_instance_id = OperationId::new().as_uuid();
    let location = if pending {
        "TEMP_OVERFLOW"
    } else {
        "ITEM_BAG"
    };
    sqlx::query(
        r#"
        INSERT INTO item_instances (
            id, definition_key, definition_version, owner_player_id, created_by_operation_id,
            location, is_starter, is_account_bound, is_tradeable, is_sellable, is_discardable,
            is_enchantable, is_upgradeable, is_unbreakable, is_repairable, state
        )
        VALUES (
            $1, $2, $3, $4, $5, $6, FALSE, $7, $8, $9, $10, $11, $12, $13, $14, $15
        )
        "#,
    )
    .bind(item_instance_id)
    .bind(&request.definition_key)
    .bind(request.definition_version)
    .bind(request.player_id)
    .bind(request.operation_id)
    .bind(location)
    .bind(request.flags.is_account_bound)
    .bind(request.flags.is_tradeable)
    .bind(request.flags.is_sellable)
    .bind(request.flags.is_discardable)
    .bind(request.flags.is_enchantable)
    .bind(request.flags.is_upgradeable)
    .bind(request.flags.is_unbreakable)
    .bind(request.flags.is_repairable)
    .bind(request.state.clone())
    .execute(&mut **tx)
    .await?;

    let pending_delivery_id = if pending {
        let pending_delivery_id = OperationId::new().as_uuid();
        sqlx::query(
            r#"
            INSERT INTO pending_asset_deliveries (
                id, operation_id, mutation_key, player_id, definition_key, definition_version,
                quantity, desired_location, reason, item_instance_id
            )
            VALUES ($1, $2, $3, $4, $5, $6, 1, 'ITEM_BAG', 'CAPACITY', $7)
            "#,
        )
        .bind(pending_delivery_id)
        .bind(request.operation_id)
        .bind(&request.mutation_key)
        .bind(request.player_id)
        .bind(&request.definition_key)
        .bind(request.definition_version)
        .bind(item_instance_id)
        .execute(&mut **tx)
        .await?;
        Some(pending_delivery_id)
    } else {
        None
    };

    let receipt = UniqueItemBagDeliveryMutationReceipt {
        event_id: OperationId::new().as_uuid(),
        operation_id: request.operation_id,
        player_id: request.player_id,
        mutation_key: request.mutation_key.clone(),
        item_instance_id,
        definition_key: request.definition_key.clone(),
        definition_version: request.definition_version,
        pending_delivery_id,
        pending,
    };
    let payload = UniqueItemBagDeliveryMutationPayload {
        receipt: receipt.clone(),
        flags: request.flags,
        state: request.state.clone(),
        source: request.source.clone(),
        provenance: request.provenance.clone(),
    };
    let payload = serde_json::to_value(payload).map_err(|error| {
        UniqueItemBagDeliveryMutationError::InvalidStoredMutation(Box::new(error))
    })?;

    sqlx::query(
        r#"
        INSERT INTO asset_events (
            id, operation_id, mutation_key, player_id, event_kind, payload
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(receipt.event_id)
    .bind(request.operation_id)
    .bind(&request.mutation_key)
    .bind(request.player_id)
    .bind(if pending {
        PENDING_EVENT_KIND
    } else {
        DELIVERED_EVENT_KIND
    })
    .bind(payload)
    .execute(&mut **tx)
    .await?;

    Ok(receipt)
}

/// Claims item-linked PENDING Item Bag deliveries FIFO until the next unique item would exceed
/// capacity. The caller owns the operation and transaction; claim replay never moves an item twice.
pub async fn claim_pending_unique_item_bag_deliveries(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
) -> Result<UniqueItemBagPendingClaimReceipt, UniqueItemBagDeliveryMutationError> {
    let operation = sqlx::query("SELECT player_id, state FROM operations WHERE id = $1 FOR UPDATE")
        .bind(operation_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(UniqueItemBagDeliveryMutationError::OperationNotFound)?;
    let operation_player_id: Option<Uuid> = operation.try_get("player_id")?;
    if operation_player_id != Some(player_id) {
        return Err(UniqueItemBagDeliveryMutationError::OperationPlayerMismatch);
    }

    if let Some(row) = sqlx::query(
        r#"
        SELECT id, player_id, event_kind, payload
          FROM asset_events
         WHERE operation_id = $1
           AND mutation_key = $2
         FOR UPDATE
        "#,
    )
    .bind(operation_id)
    .bind(CLAIM_MUTATION_KEY)
    .fetch_optional(&mut **tx)
    .await?
    {
        let event_id: Uuid = row.try_get("id")?;
        let stored_player_id: Uuid = row.try_get("player_id")?;
        let event_kind: String = row.try_get("event_kind")?;
        let payload: Value = row.try_get("payload")?;
        let payload: UniqueItemBagClaimMutationPayload = serde_json::from_value(payload).map_err(
            |error| UniqueItemBagDeliveryMutationError::InvalidStoredMutation(Box::new(error)),
        )?;
        if stored_player_id != player_id
            || event_kind != CLAIM_EVENT_KIND
            || payload.receipt.event_id != event_id
            || payload.receipt.operation_id != operation_id
            || payload.receipt.player_id != player_id
        {
            return Err(UniqueItemBagDeliveryMutationError::MutationConflict);
        }
        return Ok(payload.receipt);
    }

    let operation_state: String = operation.try_get("state")?;
    if operation_state != "PENDING" {
        return Err(UniqueItemBagDeliveryMutationError::OperationTerminal(
            operation_state,
        ));
    }

    let player = sqlx::query(
        r#"
        SELECT p.status, s.item_bag_level
          FROM players p
          JOIN player_storage_profiles s ON s.player_id = p.id
         WHERE p.id = $1
           AND p.status <> 'DELETED'
         FOR UPDATE OF p, s
        "#,
    )
    .bind(player_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(UniqueItemBagDeliveryMutationError::PlayerNotFound)?;
    let status: String = player.try_get("status")?;
    if status != "ACTIVE" {
        return Err(UniqueItemBagDeliveryMutationError::AccountFrozen(status));
    }
    let item_bag_level: i64 = player.try_get("item_bag_level")?;
    let capacity = item_bag_capacity_slots(item_bag_level)?;
    let mut used = item_bag_used_slots(tx, player_id).await?;

    let rows = sqlx::query(
        r#"
        SELECT p.id AS pending_delivery_id,
               p.definition_key,
               p.definition_version,
               p.quantity,
               p.item_instance_id,
               i.owner_player_id,
               i.definition_key AS item_definition_key,
               i.definition_version AS item_definition_version,
               i.location
          FROM pending_asset_deliveries p
          JOIN item_instances i ON i.id = p.item_instance_id
         WHERE p.player_id = $1
           AND p.state = 'PENDING'
           AND p.desired_location = 'ITEM_BAG'
           AND p.item_instance_id IS NOT NULL
         ORDER BY p.created_at, p.id
         FOR UPDATE OF p, i
        "#,
    )
    .bind(player_id)
    .fetch_all(&mut **tx)
    .await?;

    let mut claimed = Vec::new();
    let mut blocked_pending_delivery_id = None;
    for row in rows {
        let pending_delivery_id: Uuid = row.try_get("pending_delivery_id")?;
        let item_instance_id: Uuid = row.try_get("item_instance_id")?;
        let row_player_id: Uuid = row.try_get("owner_player_id")?;
        let pending_definition_key: String = row.try_get("definition_key")?;
        let pending_definition_version: i32 = row.try_get("definition_version")?;
        let quantity: i64 = row.try_get("quantity")?;
        let item_definition_key: String = row.try_get("item_definition_key")?;
        let item_definition_version: i32 = row.try_get("item_definition_version")?;
        let location: String = row.try_get("location")?;
        if row_player_id != player_id
            || quantity != 1
            || item_definition_key != pending_definition_key
            || item_definition_version != pending_definition_version
            || location != "TEMP_OVERFLOW"
        {
            return Err(UniqueItemBagDeliveryMutationError::PendingDeliveryIntegrityMismatch);
        }
        if used
            .checked_add(1)
            .ok_or(ItemError::ArithmeticOverflow)?
            > capacity
        {
            blocked_pending_delivery_id = Some(pending_delivery_id);
            break;
        }

        let moved = sqlx::query(
            "UPDATE item_instances SET location = 'ITEM_BAG' WHERE id = $1 AND owner_player_id = $2 AND location = 'TEMP_OVERFLOW'",
        )
        .bind(item_instance_id)
        .bind(player_id)
        .execute(&mut **tx)
        .await?;
        if moved.rows_affected() != 1 {
            return Err(UniqueItemBagDeliveryMutationError::PendingDeliveryIntegrityMismatch);
        }
        let resolved = sqlx::query(
            "UPDATE pending_asset_deliveries SET state = 'CLAIMED', resolved_at = now() WHERE id = $1 AND state = 'PENDING'",
        )
        .bind(pending_delivery_id)
        .execute(&mut **tx)
        .await?;
        if resolved.rows_affected() != 1 {
            return Err(UniqueItemBagDeliveryMutationError::PendingDeliveryIntegrityMismatch);
        }
        used = used
            .checked_add(1)
            .ok_or(ItemError::ArithmeticOverflow)?;
        claimed.push(ClaimedUniqueItemBagDelivery {
            pending_delivery_id,
            item_instance_id,
        });
    }

    let receipt = UniqueItemBagPendingClaimReceipt {
        event_id: OperationId::new().as_uuid(),
        operation_id,
        player_id,
        claimed,
        blocked_pending_delivery_id,
        used_slots_after: used,
    };
    let payload = serde_json::to_value(UniqueItemBagClaimMutationPayload {
        receipt: receipt.clone(),
    })
    .map_err(|error| UniqueItemBagDeliveryMutationError::InvalidStoredMutation(Box::new(error)))?;

    sqlx::query(
        r#"
        INSERT INTO asset_events (
            id, operation_id, mutation_key, player_id, event_kind, payload
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(receipt.event_id)
    .bind(operation_id)
    .bind(CLAIM_MUTATION_KEY)
    .bind(player_id)
    .bind(CLAIM_EVENT_KIND)
    .bind(payload)
    .execute(&mut **tx)
    .await?;

    Ok(receipt)
}

fn validate_delivery_input(
    request: &UniqueItemBagDeliveryMutationRequest,
) -> Result<(), UniqueItemBagDeliveryMutationError> {
    if request.definition_version <= 0 {
        return Err(UniqueItemBagDeliveryMutationError::InvalidDefinitionVersion);
    }
    if request.mutation_key.trim().is_empty() || request.mutation_key.len() > MAX_MUTATION_KEY_BYTES
    {
        return Err(UniqueItemBagDeliveryMutationError::InvalidMutationKey);
    }
    if request.source.trim().is_empty() || request.source.len() > MAX_SOURCE_BYTES {
        return Err(UniqueItemBagDeliveryMutationError::InvalidSource);
    }
    if !request.state.is_object() {
        return Err(UniqueItemBagDeliveryMutationError::InvalidState);
    }
    let Value::Object(provenance) = &request.provenance else {
        return Err(UniqueItemBagDeliveryMutationError::InvalidProvenance);
    };
    if provenance.is_empty() {
        return Err(UniqueItemBagDeliveryMutationError::InvalidProvenance);
    }
    Ok(())
}

fn replay_unique_item_bag_delivery(
    row: sqlx::postgres::PgRow,
    request: &UniqueItemBagDeliveryMutationRequest,
) -> Result<UniqueItemBagDeliveryMutationReceipt, UniqueItemBagDeliveryMutationError> {
    let stored_event_id: Uuid = row.try_get("id")?;
    let stored_player_id: Uuid = row.try_get("player_id")?;
    let stored_event_kind: String = row.try_get("event_kind")?;
    if stored_event_kind != DELIVERED_EVENT_KIND && stored_event_kind != PENDING_EVENT_KIND {
        return Err(UniqueItemBagDeliveryMutationError::MutationConflict);
    }
    let payload: Value = row.try_get("payload")?;
    let payload: UniqueItemBagDeliveryMutationPayload =
        serde_json::from_value(payload).map_err(|error| {
            UniqueItemBagDeliveryMutationError::InvalidStoredMutation(Box::new(error))
        })?;
    let expected_event_kind = if payload.receipt.pending {
        PENDING_EVENT_KIND
    } else {
        DELIVERED_EVENT_KIND
    };
    if stored_event_id != payload.receipt.event_id
        || stored_player_id != request.player_id
        || stored_event_kind != expected_event_kind
        || payload.receipt.operation_id != request.operation_id
        || payload.receipt.player_id != request.player_id
        || payload.receipt.mutation_key != request.mutation_key
        || payload.receipt.definition_key != request.definition_key
        || payload.receipt.definition_version != request.definition_version
        || payload.receipt.pending != payload.receipt.pending_delivery_id.is_some()
        || payload.flags != request.flags
        || payload.state != request.state
        || payload.source != request.source
        || payload.provenance != request.provenance
    {
        return Err(UniqueItemBagDeliveryMutationError::MutationConflict);
    }
    Ok(payload.receipt)
}
