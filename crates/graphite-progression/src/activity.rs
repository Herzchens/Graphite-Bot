use crate::progression::{ActivityProgress, ProgressionMathError, activity_progress};
use graphite_core::OperationId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

const MAX_MUTATION_KEY_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ActivityXpMutationKind {
    Grant,
    Spend,
    Loss,
}

impl ActivityXpMutationKind {
    const fn event_kind(self) -> &'static str {
        match self {
            Self::Grant => "ACTIVITY_XP_GRANTED",
            Self::Spend => "ACTIVITY_XP_SPENT",
            Self::Loss => "ACTIVITY_XP_LOST",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActivityXpMutationRequest {
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub mutation_key: String,
    pub kind: ActivityXpMutationKind,
    pub amount: i64,
    pub source: String,
    pub provenance: Value,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ActivityXpMutationReceipt {
    pub event_id: Uuid,
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub mutation_key: String,
    pub kind: ActivityXpMutationKind,
    pub amount: i64,
    pub source: String,
    pub before: ActivityProgress,
    pub after: ActivityProgress,
}

#[derive(Debug, Error)]
pub enum ActivityXpError {
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error(transparent)]
    Math(#[from] ProgressionMathError),
    #[error("Activity EXP mutation amount must be positive")]
    InvalidAmount,
    #[error("Activity EXP source must not be empty")]
    InvalidSource,
    #[error(
        "Activity EXP mutation key must be a non-empty internal key of at most {MAX_MUTATION_KEY_BYTES} bytes"
    )]
    InvalidMutationKey,
    #[error("Activity EXP provenance must be a non-empty JSON object")]
    InvalidProvenance,
    #[error("player does not exist")]
    PlayerNotFound,
    #[error("Activity EXP mutation requires an ACTIVE account; current status is {0}")]
    AccountFrozen(String),
    #[error("owning operation does not exist")]
    OperationNotFound,
    #[error("owning operation targets a different player")]
    OperationPlayerMismatch,
    #[error("owning operation cannot accept a new Activity EXP mutation in state {0}")]
    OperationTerminal(String),
    #[error("Activity EXP has {available} points but {requested} are required")]
    InsufficientActivityXp { available: i64, requested: i64 },
    #[error("the same operation mutation key was reused with different Activity EXP input")]
    MutationConflict,
    #[error("stored Activity EXP mutation payload is invalid: {0}")]
    InvalidStoredMutation(Box<serde_json::Error>),
}

impl From<sqlx::Error> for ActivityXpError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct ActivityMutationPayload {
    receipt: ActivityXpMutationReceipt,
    provenance: Value,
}

/// Atomically settles one already-effective integer Activity EXP mutation inside an
/// owning gameplay/service transaction.
///
/// `request.amount` is the final integer point amount after the caller has applied
/// all source-specific modifiers and caps. This function deliberately does not
/// apply Rebirth, guild, clan, event, or automation modifiers itself; doing so here
/// would make cross-system modifier provenance ambiguous and risk double
/// application.
///
/// The caller must resolve/own `request.operation_id` first. Lock order is
/// operation -> player -> progression, matching Graphite's normal mutation order.
/// The stable `mutation_key` makes a logical sub-mutation idempotent inside a
/// composite operation, so a retry can return the same receipt without applying
/// the delta twice. Outbox settlement remains the responsibility of the owning
/// operation.
pub async fn apply_activity_xp_mutation(
    tx: &mut Transaction<'_, Postgres>,
    request: &ActivityXpMutationRequest,
) -> Result<ActivityXpMutationReceipt, ActivityXpError> {
    validate_input(request)?;

    let operation = sqlx::query("SELECT player_id, state FROM operations WHERE id = $1 FOR UPDATE")
        .bind(request.operation_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(ActivityXpError::OperationNotFound)?;
    let operation_player_id: Option<Uuid> = operation.try_get("player_id")?;
    if operation_player_id.is_some_and(|stored| stored != request.player_id) {
        return Err(ActivityXpError::OperationPlayerMismatch);
    }
    let operation_state: String = operation.try_get("state")?;

    if let Some(row) = sqlx::query(
        r#"
        SELECT id, player_id, event_kind, payload
          FROM progression_events
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
        return replay_activity_mutation(row, request);
    }

    if operation_state != "PENDING" {
        return Err(ActivityXpError::OperationTerminal(operation_state));
    }

    let row = sqlx::query(
        r#"
        SELECT p.status, g.activity_xp_points
          FROM players p
          JOIN player_progression g ON g.player_id = p.id
         WHERE p.id = $1
           AND p.status <> 'DELETED'
         FOR UPDATE OF p, g
        "#,
    )
    .bind(request.player_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(ActivityXpError::PlayerNotFound)?;
    let status: String = row.try_get("status")?;
    if status != "ACTIVE" {
        return Err(ActivityXpError::AccountFrozen(status));
    }

    let points_before: i64 = row.try_get("activity_xp_points")?;
    let before = activity_progress(points_before)?;
    let points_after = activity_points_after(points_before, request.kind, request.amount)?;
    let after = activity_progress(points_after)?;

    sqlx::query(
        "UPDATE player_progression SET activity_xp_points = $1, updated_at = now() WHERE player_id = $2",
    )
    .bind(points_after)
    .bind(request.player_id)
    .execute(&mut **tx)
    .await?;

    let receipt = ActivityXpMutationReceipt {
        event_id: OperationId::new().as_uuid(),
        operation_id: request.operation_id,
        player_id: request.player_id,
        mutation_key: request.mutation_key.clone(),
        kind: request.kind,
        amount: request.amount,
        source: request.source.clone(),
        before,
        after,
    };
    let payload = ActivityMutationPayload {
        receipt: receipt.clone(),
        provenance: request.provenance.clone(),
    };
    let payload = serde_json::to_value(payload)
        .map_err(|error| ActivityXpError::InvalidStoredMutation(Box::new(error)))?;

    sqlx::query(
        r#"
        INSERT INTO progression_events (
            id, operation_id, mutation_key, player_id, event_kind, payload
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(receipt.event_id)
    .bind(request.operation_id)
    .bind(&request.mutation_key)
    .bind(request.player_id)
    .bind(request.kind.event_kind())
    .bind(payload)
    .execute(&mut **tx)
    .await?;

    Ok(receipt)
}

fn validate_input(request: &ActivityXpMutationRequest) -> Result<(), ActivityXpError> {
    if request.amount <= 0 {
        return Err(ActivityXpError::InvalidAmount);
    }
    if request.source.trim().is_empty() {
        return Err(ActivityXpError::InvalidSource);
    }
    if request.mutation_key.trim().is_empty() || request.mutation_key.len() > MAX_MUTATION_KEY_BYTES
    {
        return Err(ActivityXpError::InvalidMutationKey);
    }
    let Value::Object(fields) = &request.provenance else {
        return Err(ActivityXpError::InvalidProvenance);
    };
    if fields.is_empty() {
        return Err(ActivityXpError::InvalidProvenance);
    }
    Ok(())
}

fn activity_points_after(
    points_before: i64,
    kind: ActivityXpMutationKind,
    amount: i64,
) -> Result<i64, ActivityXpError> {
    match kind {
        ActivityXpMutationKind::Grant => {
            points_before
                .checked_add(amount)
                .ok_or(ActivityXpError::Math(
                    ProgressionMathError::ArithmeticOverflow,
                ))
        }
        ActivityXpMutationKind::Spend | ActivityXpMutationKind::Loss => {
            if points_before < amount {
                return Err(ActivityXpError::InsufficientActivityXp {
                    available: points_before,
                    requested: amount,
                });
            }
            points_before
                .checked_sub(amount)
                .ok_or(ActivityXpError::Math(
                    ProgressionMathError::ArithmeticOverflow,
                ))
        }
    }
}

fn replay_activity_mutation(
    row: sqlx::postgres::PgRow,
    request: &ActivityXpMutationRequest,
) -> Result<ActivityXpMutationReceipt, ActivityXpError> {
    let stored_event_id: Uuid = row.try_get("id")?;
    let stored_player_id: Uuid = row.try_get("player_id")?;
    let stored_event_kind: String = row.try_get("event_kind")?;
    let payload: Value = row.try_get("payload")?;
    let payload: ActivityMutationPayload = serde_json::from_value(payload)
        .map_err(|error| ActivityXpError::InvalidStoredMutation(Box::new(error)))?;

    if stored_event_id != payload.receipt.event_id
        || stored_player_id != request.player_id
        || stored_event_kind != request.kind.event_kind()
        || payload.receipt.operation_id != request.operation_id
        || payload.receipt.player_id != request.player_id
        || payload.receipt.mutation_key != request.mutation_key
        || payload.receipt.kind != request.kind
        || payload.receipt.amount != request.amount
        || payload.receipt.source != request.source
        || payload.provenance != request.provenance
    {
        return Err(ActivityXpError::MutationConflict);
    }
    Ok(payload.receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request() -> ActivityXpMutationRequest {
        ActivityXpMutationRequest {
            operation_id: Uuid::nil(),
            player_id: Uuid::nil(),
            mutation_key: "aexp:test".to_owned(),
            kind: ActivityXpMutationKind::Grant,
            amount: 1,
            source: "TEST".to_owned(),
            provenance: json!({"origin":"unit_test"}),
        }
    }

    #[test]
    fn grant_spend_and_loss_never_cross_zero() {
        assert_eq!(
            activity_points_after(100, ActivityXpMutationKind::Grant, 25).unwrap(),
            125
        );
        assert_eq!(
            activity_points_after(100, ActivityXpMutationKind::Spend, 25).unwrap(),
            75
        );
        assert_eq!(
            activity_points_after(100, ActivityXpMutationKind::Loss, 100).unwrap(),
            0
        );
        assert!(matches!(
            activity_points_after(99, ActivityXpMutationKind::Loss, 100),
            Err(ActivityXpError::InsufficientActivityXp {
                available: 99,
                requested: 100
            })
        ));
    }

    #[test]
    fn provenance_and_stable_key_are_required() {
        let mut input = request();
        input.mutation_key.clear();
        assert!(matches!(
            validate_input(&input),
            Err(ActivityXpError::InvalidMutationKey)
        ));

        let mut input = request();
        input.source.clear();
        assert!(matches!(
            validate_input(&input),
            Err(ActivityXpError::InvalidSource)
        ));

        let mut input = request();
        input.provenance = json!({});
        assert!(matches!(
            validate_input(&input),
            Err(ActivityXpError::InvalidProvenance)
        ));
    }
}
