use super::{ItemError, item_bag_capacity_slots, item_bag_used_slots, slots_for_quantity};
use graphite_core::OperationId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

const MAX_MUTATION_KEY_BYTES: usize = 128;
const MAX_SOURCE_BYTES: usize = 128;
const DELIVERED_EVENT_KIND: &str = "STACK_SUBDELIVERY_DELIVERED";
const PENDING_EVENT_KIND: &str = "STACK_SUBDELIVERY_PENDING";

#[derive(Clone, Debug, PartialEq)]
pub struct StackDeliveryMutationRequest {
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub mutation_key: String,
    pub definition_key: String,
    pub definition_version: i32,
    pub quantity: i64,
    pub source: String,
    pub provenance: Value,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StackDeliveryMutationReceipt {
    pub event_id: Uuid,
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub mutation_key: String,
    pub definition_key: String,
    pub definition_version: i32,
    pub quantity: i64,
    pub pending_delivery_id: Option<Uuid>,
    pub pending: bool,
}

#[derive(Debug, Error)]
pub enum StackDeliveryMutationError {
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error(transparent)]
    Item(#[from] ItemError),
    #[error("stack delivery quantity must be positive")]
    InvalidQuantity,
    #[error("stack definition version must be positive")]
    InvalidDefinitionVersion,
    #[error(
        "stack delivery mutation key must be a non-empty internal key of at most {MAX_MUTATION_KEY_BYTES} bytes"
    )]
    InvalidMutationKey,
    #[error("stack delivery source must be non-empty and at most {MAX_SOURCE_BYTES} bytes")]
    InvalidSource,
    #[error("stack delivery provenance must be a non-empty JSON object")]
    InvalidProvenance,
    #[error("player does not exist")]
    PlayerNotFound,
    #[error("stack delivery requires an ACTIVE account; current status is {0}")]
    AccountFrozen(String),
    #[error("owning operation does not exist")]
    OperationNotFound,
    #[error("owning operation targets a different player")]
    OperationPlayerMismatch,
    #[error("owning operation cannot accept a new stack delivery in state {0}")]
    OperationTerminal(String),
    #[error("stack definition version does not exist or is not stackable")]
    InvalidStackDefinition,
    #[error("the same operation mutation key was reused with different stack delivery input")]
    MutationConflict,
    #[error("stored stack delivery mutation payload is invalid: {0}")]
    InvalidStoredMutation(Box<serde_json::Error>),
}

impl From<sqlx::Error> for StackDeliveryMutationError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct StackDeliveryMutationPayload {
    receipt: StackDeliveryMutationReceipt,
    source: String,
    provenance: Value,
}

/// Applies one exact-version Item Bag stack delivery inside an owning transaction.
///
/// The caller owns `request.operation_id`, the surrounding transaction, the final operation
/// transition, and any operation-level outbox event. Lock order is operation -> player/storage ->
/// item stacks. `mutation_key` identifies one logical sub-delivery inside a composite operation, so
/// one Smelting settlement can safely return raw input, return unopened fuel, and deliver output
/// without creating child operations or double-applying any stack on retry.
///
/// Definition identity is version-pinned. This intentionally accepts an immutable historical
/// `item_definition_versions` row even when a newer version is current or the definition has since
/// been inactivated; settlement must return exactly the asset version that was reserved. Capacity
/// overflow becomes a keyed `pending_asset_deliveries` row rather than silently dropping assets.
pub async fn apply_stack_delivery_mutation(
    tx: &mut Transaction<'_, Postgres>,
    request: &StackDeliveryMutationRequest,
) -> Result<StackDeliveryMutationReceipt, StackDeliveryMutationError> {
    validate_input(request)?;

    let operation = sqlx::query("SELECT player_id, state FROM operations WHERE id = $1 FOR UPDATE")
        .bind(request.operation_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(StackDeliveryMutationError::OperationNotFound)?;
    let operation_player_id: Option<Uuid> = operation.try_get("player_id")?;
    if operation_player_id.is_some_and(|stored| stored != request.player_id) {
        return Err(StackDeliveryMutationError::OperationPlayerMismatch);
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
        return replay_stack_delivery(row, request);
    }

    if operation_state != "PENDING" {
        return Err(StackDeliveryMutationError::OperationTerminal(
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
    .ok_or(StackDeliveryMutationError::PlayerNotFound)?;
    let status: String = player.try_get("status")?;
    if status != "ACTIVE" {
        return Err(StackDeliveryMutationError::AccountFrozen(status));
    }
    let item_bag_level: i64 = player.try_get("item_bag_level")?;

    let definition = sqlx::query(
        r#"
        SELECT stackable, stack_limit
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
    .ok_or(StackDeliveryMutationError::InvalidStackDefinition)?;
    let stackable: bool = definition.try_get("stackable")?;
    let stack_limit: Option<i64> = definition.try_get("stack_limit")?;
    if !stackable {
        return Err(StackDeliveryMutationError::InvalidStackDefinition);
    }
    let Some(stack_limit) = stack_limit.filter(|value| *value > 0) else {
        return Err(StackDeliveryMutationError::InvalidStackDefinition);
    };

    let capacity = item_bag_capacity_slots(item_bag_level)?;
    let used_before = item_bag_used_slots(tx, request.player_id).await?;
    let existing_quantity: i64 = sqlx::query(
        r#"
        SELECT quantity
          FROM item_stacks
         WHERE player_id = $1
           AND definition_key = $2
           AND definition_version = $3
           AND location = 'ITEM_BAG'
        "#,
    )
    .bind(request.player_id)
    .bind(&request.definition_key)
    .bind(request.definition_version)
    .fetch_optional(&mut **tx)
    .await?
    .map(|row| row.try_get("quantity"))
    .transpose()?
    .unwrap_or(0);

    let before_slots = slots_for_quantity(existing_quantity, stack_limit)?;
    let combined_quantity = existing_quantity
        .checked_add(request.quantity)
        .ok_or(ItemError::ArithmeticOverflow)?;
    let after_slots = slots_for_quantity(combined_quantity, stack_limit)?;
    let projected_used = used_before
        .checked_sub(before_slots)
        .and_then(|value| value.checked_add(after_slots))
        .ok_or(ItemError::ArithmeticOverflow)?;
    let pending = projected_used > capacity;

    let pending_delivery_id = if pending {
        let pending_delivery_id = OperationId::new().as_uuid();
        sqlx::query(
            r#"
            INSERT INTO pending_asset_deliveries (
                id, operation_id, mutation_key, player_id, definition_key, definition_version,
                quantity, desired_location, reason
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, 'ITEM_BAG', 'CAPACITY')
            "#,
        )
        .bind(pending_delivery_id)
        .bind(request.operation_id)
        .bind(&request.mutation_key)
        .bind(request.player_id)
        .bind(&request.definition_key)
        .bind(request.definition_version)
        .bind(request.quantity)
        .execute(&mut **tx)
        .await?;
        Some(pending_delivery_id)
    } else {
        sqlx::query(
            r#"
            INSERT INTO item_stacks (
                player_id, definition_key, definition_version, location, quantity
            )
            VALUES ($1, $2, $3, 'ITEM_BAG', $4)
            ON CONFLICT (player_id, definition_key, definition_version, location)
            DO UPDATE SET quantity = item_stacks.quantity + EXCLUDED.quantity,
                          updated_at = now()
            "#,
        )
        .bind(request.player_id)
        .bind(&request.definition_key)
        .bind(request.definition_version)
        .bind(request.quantity)
        .execute(&mut **tx)
        .await?;
        None
    };

    let receipt = StackDeliveryMutationReceipt {
        event_id: OperationId::new().as_uuid(),
        operation_id: request.operation_id,
        player_id: request.player_id,
        mutation_key: request.mutation_key.clone(),
        definition_key: request.definition_key.clone(),
        definition_version: request.definition_version,
        quantity: request.quantity,
        pending_delivery_id,
        pending,
    };
    let payload = StackDeliveryMutationPayload {
        receipt: receipt.clone(),
        source: request.source.clone(),
        provenance: request.provenance.clone(),
    };
    let payload = serde_json::to_value(payload)
        .map_err(|error| StackDeliveryMutationError::InvalidStoredMutation(Box::new(error)))?;

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

fn validate_input(
    request: &StackDeliveryMutationRequest,
) -> Result<(), StackDeliveryMutationError> {
    if request.quantity <= 0 {
        return Err(StackDeliveryMutationError::InvalidQuantity);
    }
    if request.definition_version <= 0 {
        return Err(StackDeliveryMutationError::InvalidDefinitionVersion);
    }
    if request.mutation_key.trim().is_empty() || request.mutation_key.len() > MAX_MUTATION_KEY_BYTES
    {
        return Err(StackDeliveryMutationError::InvalidMutationKey);
    }
    if request.source.trim().is_empty() || request.source.len() > MAX_SOURCE_BYTES {
        return Err(StackDeliveryMutationError::InvalidSource);
    }
    let Value::Object(fields) = &request.provenance else {
        return Err(StackDeliveryMutationError::InvalidProvenance);
    };
    if fields.is_empty() {
        return Err(StackDeliveryMutationError::InvalidProvenance);
    }
    Ok(())
}

fn replay_stack_delivery(
    row: sqlx::postgres::PgRow,
    request: &StackDeliveryMutationRequest,
) -> Result<StackDeliveryMutationReceipt, StackDeliveryMutationError> {
    let stored_event_id: Uuid = row.try_get("id")?;
    let stored_player_id: Uuid = row.try_get("player_id")?;
    let stored_event_kind: String = row.try_get("event_kind")?;
    if stored_event_kind != DELIVERED_EVENT_KIND && stored_event_kind != PENDING_EVENT_KIND {
        return Err(StackDeliveryMutationError::MutationConflict);
    }

    let payload: Value = row.try_get("payload")?;
    let payload: StackDeliveryMutationPayload = serde_json::from_value(payload)
        .map_err(|error| StackDeliveryMutationError::InvalidStoredMutation(Box::new(error)))?;
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
        || payload.receipt.quantity != request.quantity
        || payload.receipt.pending != payload.receipt.pending_delivery_id.is_some()
        || payload.source != request.source
        || payload.provenance != request.provenance
    {
        return Err(StackDeliveryMutationError::MutationConflict);
    }

    Ok(payload.receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request() -> StackDeliveryMutationRequest {
        StackDeliveryMutationRequest {
            operation_id: Uuid::nil(),
            player_id: Uuid::nil(),
            mutation_key: "stack:test".to_owned(),
            definition_key: "resource.test".to_owned(),
            definition_version: 1,
            quantity: 1,
            source: "TEST".to_owned(),
            provenance: json!({"origin":"unit_test"}),
        }
    }

    #[test]
    fn mutation_identity_inputs_are_required() {
        let mut input = request();
        input.mutation_key.clear();
        assert!(matches!(
            validate_input(&input),
            Err(StackDeliveryMutationError::InvalidMutationKey)
        ));

        let mut input = request();
        input.source.clear();
        assert!(matches!(
            validate_input(&input),
            Err(StackDeliveryMutationError::InvalidSource)
        ));

        let mut input = request();
        input.provenance = json!({});
        assert!(matches!(
            validate_input(&input),
            Err(StackDeliveryMutationError::InvalidProvenance)
        ));

        let mut input = request();
        input.definition_version = 0;
        assert!(matches!(
            validate_input(&input),
            Err(StackDeliveryMutationError::InvalidDefinitionVersion)
        ));
    }
}
