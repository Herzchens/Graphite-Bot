use graphite_core::OperationId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

const MAX_MUTATION_KEY_BYTES: usize = 128;
const MAX_SOURCE_BYTES: usize = 128;
const CONSUMED_EVENT_KIND: &str = "STACK_SUBCONSUMPTION_CONSUMED";

#[derive(Clone, Debug, PartialEq)]
pub struct StackConsumptionMutationRequest {
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
pub struct StackConsumptionMutationReceipt {
    pub event_id: Uuid,
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub mutation_key: String,
    pub definition_key: String,
    pub definition_version: i32,
    pub quantity: i64,
    pub quantity_before: i64,
    pub quantity_after: i64,
}

#[derive(Debug, Error)]
pub enum StackConsumptionMutationError {
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error("stack consumption quantity must be positive")]
    InvalidQuantity,
    #[error("stack definition version must be positive")]
    InvalidDefinitionVersion,
    #[error(
        "stack consumption mutation key must be a non-empty internal key of at most {MAX_MUTATION_KEY_BYTES} bytes"
    )]
    InvalidMutationKey,
    #[error("stack consumption source must be non-empty and at most {MAX_SOURCE_BYTES} bytes")]
    InvalidSource,
    #[error("stack consumption provenance must be a non-empty JSON object")]
    InvalidProvenance,
    #[error("player does not exist")]
    PlayerNotFound,
    #[error("stack consumption requires an ACTIVE account; current status is {0}")]
    AccountFrozen(String),
    #[error("owning operation does not exist")]
    OperationNotFound,
    #[error("owning operation targets a different player")]
    OperationPlayerMismatch,
    #[error("owning operation cannot accept a new stack consumption in state {0}")]
    OperationTerminal(String),
    #[error("stack definition version does not exist or is not stackable")]
    InvalidStackDefinition,
    #[error(
        "insufficient Item Bag quantity for {definition_key} v{definition_version}: available {available}, requested {requested}"
    )]
    InsufficientStack {
        definition_key: String,
        definition_version: i32,
        available: i64,
        requested: i64,
    },
    #[error("the same operation mutation key was reused with different stack consumption input")]
    MutationConflict,
    #[error("stored stack consumption mutation payload is invalid: {0}")]
    InvalidStoredMutation(Box<serde_json::Error>),
}

impl From<sqlx::Error> for StackConsumptionMutationError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct StackConsumptionMutationPayload {
    receipt: StackConsumptionMutationReceipt,
    source: String,
    provenance: Value,
}

/// Atomically consumes one exact-version stack quantity from Item Bag ownership inside an owning
/// gameplay/service transaction.
///
/// The caller owns `request.operation_id`, the surrounding transaction, the final operation
/// transition, and operation-level outbox emission. Lock order is `operation -> player -> item
/// definition -> item stack`. `mutation_key` identifies one logical consumption leg inside a
/// composite operation, allowing an immediate service such as SoulBind to consume several fixed
/// package stacks without child operations or double-consuming any leg on replay.
///
/// Definition identity is version-pinned. The immutable historical definition row must exist and be
/// stackable; inactive/current-version status is deliberately irrelevant because an owning
/// transaction may already have resolved a historical exact-version asset. Consumption never uses
/// pending-delivery overflow: it only removes quantity that is authoritatively present in
/// `ITEM_BAG` and fails closed otherwise.
pub async fn apply_stack_consumption_mutation(
    tx: &mut Transaction<'_, Postgres>,
    request: &StackConsumptionMutationRequest,
) -> Result<StackConsumptionMutationReceipt, StackConsumptionMutationError> {
    validate_input(request)?;

    let operation = sqlx::query("SELECT player_id, state FROM operations WHERE id = $1 FOR UPDATE")
        .bind(request.operation_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(StackConsumptionMutationError::OperationNotFound)?;
    let operation_player_id: Option<Uuid> = operation.try_get("player_id")?;
    if operation_player_id.is_some_and(|stored| stored != request.player_id) {
        return Err(StackConsumptionMutationError::OperationPlayerMismatch);
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
        return replay_stack_consumption(row, request);
    }

    if operation_state != "PENDING" {
        return Err(StackConsumptionMutationError::OperationTerminal(
            operation_state,
        ));
    }

    let player =
        sqlx::query("SELECT status FROM players WHERE id = $1 AND status <> 'DELETED' FOR UPDATE")
            .bind(request.player_id)
            .fetch_optional(&mut **tx)
            .await?
            .ok_or(StackConsumptionMutationError::PlayerNotFound)?;
    let status: String = player.try_get("status")?;
    if status != "ACTIVE" {
        return Err(StackConsumptionMutationError::AccountFrozen(status));
    }

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
    .ok_or(StackConsumptionMutationError::InvalidStackDefinition)?;
    let stackable: bool = definition.try_get("stackable")?;
    if !stackable {
        return Err(StackConsumptionMutationError::InvalidStackDefinition);
    }

    let quantity_before: i64 = sqlx::query(
        r#"
        SELECT quantity
          FROM item_stacks
         WHERE player_id = $1
           AND definition_key = $2
           AND definition_version = $3
           AND location = 'ITEM_BAG'
         FOR UPDATE
        "#,
    )
    .bind(request.player_id)
    .bind(&request.definition_key)
    .bind(request.definition_version)
    .fetch_optional(&mut **tx)
    .await?
    .map(|row| row.try_get("quantity"))
    .transpose()?
    .unwrap_or(0_i64);

    if quantity_before < request.quantity {
        return Err(StackConsumptionMutationError::InsufficientStack {
            definition_key: request.definition_key.clone(),
            definition_version: request.definition_version,
            available: quantity_before,
            requested: request.quantity,
        });
    }
    let quantity_after = quantity_before.checked_sub(request.quantity).ok_or(
        StackConsumptionMutationError::InsufficientStack {
            definition_key: request.definition_key.clone(),
            definition_version: request.definition_version,
            available: quantity_before,
            requested: request.quantity,
        },
    )?;

    let rows_affected = if quantity_after == 0 {
        sqlx::query(
            "DELETE FROM item_stacks WHERE player_id = $1 AND definition_key = $2 AND definition_version = $3 AND location = 'ITEM_BAG' AND quantity = $4",
        )
        .bind(request.player_id)
        .bind(&request.definition_key)
        .bind(request.definition_version)
        .bind(quantity_before)
        .execute(&mut **tx)
        .await?
        .rows_affected()
    } else {
        sqlx::query(
            "UPDATE item_stacks SET quantity = $1, updated_at = now() WHERE player_id = $2 AND definition_key = $3 AND definition_version = $4 AND location = 'ITEM_BAG' AND quantity = $5",
        )
        .bind(quantity_after)
        .bind(request.player_id)
        .bind(&request.definition_key)
        .bind(request.definition_version)
        .bind(quantity_before)
        .execute(&mut **tx)
        .await?
        .rows_affected()
    };
    if rows_affected != 1 {
        return Err(StackConsumptionMutationError::MutationConflict);
    }

    let receipt = StackConsumptionMutationReceipt {
        event_id: OperationId::new().as_uuid(),
        operation_id: request.operation_id,
        player_id: request.player_id,
        mutation_key: request.mutation_key.clone(),
        definition_key: request.definition_key.clone(),
        definition_version: request.definition_version,
        quantity: request.quantity,
        quantity_before,
        quantity_after,
    };
    let payload = StackConsumptionMutationPayload {
        receipt: receipt.clone(),
        source: request.source.clone(),
        provenance: request.provenance.clone(),
    };
    let payload = serde_json::to_value(payload)
        .map_err(|error| StackConsumptionMutationError::InvalidStoredMutation(Box::new(error)))?;

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
    .bind(CONSUMED_EVENT_KIND)
    .bind(payload)
    .execute(&mut **tx)
    .await?;

    Ok(receipt)
}

fn validate_input(
    request: &StackConsumptionMutationRequest,
) -> Result<(), StackConsumptionMutationError> {
    if request.quantity <= 0 {
        return Err(StackConsumptionMutationError::InvalidQuantity);
    }
    if request.definition_version <= 0 {
        return Err(StackConsumptionMutationError::InvalidDefinitionVersion);
    }
    if request.mutation_key.trim().is_empty() || request.mutation_key.len() > MAX_MUTATION_KEY_BYTES
    {
        return Err(StackConsumptionMutationError::InvalidMutationKey);
    }
    if request.source.trim().is_empty() || request.source.len() > MAX_SOURCE_BYTES {
        return Err(StackConsumptionMutationError::InvalidSource);
    }
    let Value::Object(fields) = &request.provenance else {
        return Err(StackConsumptionMutationError::InvalidProvenance);
    };
    if fields.is_empty() {
        return Err(StackConsumptionMutationError::InvalidProvenance);
    }
    Ok(())
}

fn replay_stack_consumption(
    row: sqlx::postgres::PgRow,
    request: &StackConsumptionMutationRequest,
) -> Result<StackConsumptionMutationReceipt, StackConsumptionMutationError> {
    let stored_event_id: Uuid = row.try_get("id")?;
    let stored_player_id: Uuid = row.try_get("player_id")?;
    let stored_event_kind: String = row.try_get("event_kind")?;
    if stored_event_kind != CONSUMED_EVENT_KIND {
        return Err(StackConsumptionMutationError::MutationConflict);
    }

    let payload: Value = row.try_get("payload")?;
    let payload: StackConsumptionMutationPayload = serde_json::from_value(payload)
        .map_err(|error| StackConsumptionMutationError::InvalidStoredMutation(Box::new(error)))?;
    if stored_event_id != payload.receipt.event_id
        || stored_player_id != request.player_id
        || payload.receipt.operation_id != request.operation_id
        || payload.receipt.player_id != request.player_id
        || payload.receipt.mutation_key != request.mutation_key
        || payload.receipt.definition_key != request.definition_key
        || payload.receipt.definition_version != request.definition_version
        || payload.receipt.quantity != request.quantity
        || payload.receipt.quantity_before < payload.receipt.quantity
        || payload.receipt.quantity_after
            != payload
                .receipt
                .quantity_before
                .checked_sub(payload.receipt.quantity)
                .ok_or(StackConsumptionMutationError::MutationConflict)?
        || payload.source != request.source
        || payload.provenance != request.provenance
    {
        return Err(StackConsumptionMutationError::MutationConflict);
    }

    Ok(payload.receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request() -> StackConsumptionMutationRequest {
        StackConsumptionMutationRequest {
            operation_id: Uuid::nil(),
            player_id: Uuid::nil(),
            mutation_key: "stack-consume:test".to_owned(),
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
        input.quantity = 0;
        assert!(matches!(
            validate_input(&input),
            Err(StackConsumptionMutationError::InvalidQuantity)
        ));

        let mut input = request();
        input.definition_version = 0;
        assert!(matches!(
            validate_input(&input),
            Err(StackConsumptionMutationError::InvalidDefinitionVersion)
        ));

        let mut input = request();
        input.mutation_key.clear();
        assert!(matches!(
            validate_input(&input),
            Err(StackConsumptionMutationError::InvalidMutationKey)
        ));

        let mut input = request();
        input.source.clear();
        assert!(matches!(
            validate_input(&input),
            Err(StackConsumptionMutationError::InvalidSource)
        ));

        let mut input = request();
        input.provenance = json!({});
        assert!(matches!(
            validate_input(&input),
            Err(StackConsumptionMutationError::InvalidProvenance)
        ));
    }
}
