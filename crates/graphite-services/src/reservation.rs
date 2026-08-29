use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReservationRole {
    Input,
    Fuel,
}

impl ReservationRole {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Input => "INPUT",
            Self::Fuel => "FUEL",
        }
    }

    fn from_db(value: &str) -> Result<Self, ServiceJobReservationError> {
        match value {
            "INPUT" => Ok(Self::Input),
            "FUEL" => Ok(Self::Fuel),
            _ => Err(ServiceJobReservationError::ReservationIntegrityMismatch),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StackReservationRequest {
    pub role: ReservationRole,
    pub definition_key: String,
    pub definition_version: i32,
    pub quantity: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ServiceJobReservationRequest {
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub service_kind: String,
    pub policy_version: i32,
    pub stacks: Vec<StackReservationRequest>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ServiceJobReservationReceipt {
    pub job_id: Uuid,
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub service_kind: String,
    pub policy_version: i32,
    pub state: String,
    pub stacks: Vec<StackReservationRequest>,
}

#[derive(Debug, Error)]
pub enum ServiceJobReservationError {
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error("service-job policy version must be positive")]
    InvalidPolicyVersion,
    #[error("service kind must be non-empty and canonical")]
    InvalidServiceKind,
    #[error("at least one stack reservation is required")]
    EmptyReservations,
    #[error("reservation definition key/version/quantity is invalid")]
    InvalidReservation,
    #[error("the same reservation role and definition was supplied more than once")]
    DuplicateReservation,
    #[error("operation does not exist")]
    OperationNotFound,
    #[error("operation is in terminal state {0}")]
    OperationTerminal(String),
    #[error("operation identity/player/policy does not match the service job request")]
    OperationMismatch,
    #[error("no active Graphite player exists for this job")]
    PlayerNotFound,
    #[error("service-job reservation is blocked while account status is {0}")]
    AccountFrozen(String),
    #[error(
        "insufficient Item Bag quantity for {definition_key} v{definition_version}: available {available}, requested {requested}"
    )]
    InsufficientStack {
        definition_key: String,
        definition_version: i32,
        available: i64,
        requested: i64,
    },
    #[error("service-job reservation arithmetic exceeded the supported BIGINT range")]
    ArithmeticOverflow,
    #[error("existing service job does not match the retried reservation request")]
    ReservationConflict,
    #[error("stored service-job reservation state is internally inconsistent")]
    ReservationIntegrityMismatch,
}

impl From<sqlx::Error> for ServiceJobReservationError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

/// Transaction-composable primitive that atomically moves exact stack quantities out of
/// Item Bag ownership and records immutable, per-job reservation provenance.
///
/// The caller owns the surrounding operation and transaction commit. This function never
/// commits an operation or emits an outbox event; a higher-level service command must do so
/// only after all of its canonical state is ready to commit.
pub async fn reserve_service_job_stacks(
    tx: &mut Transaction<'_, Postgres>,
    request: &ServiceJobReservationRequest,
) -> Result<ServiceJobReservationReceipt, ServiceJobReservationError> {
    let stacks = normalize_request(request)?;
    let operation = sqlx::query(
        "SELECT player_id, state, policy_version FROM operations WHERE id = $1 FOR UPDATE",
    )
    .bind(request.operation_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(ServiceJobReservationError::OperationNotFound)?;

    let operation_player: Option<Uuid> = operation.try_get("player_id")?;
    let operation_state: String = operation.try_get("state")?;
    let operation_policy: i32 = operation.try_get("policy_version")?;
    if operation_player.is_some_and(|player_id| player_id != request.player_id)
        || operation_policy != request.policy_version
    {
        return Err(ServiceJobReservationError::OperationMismatch);
    }

    if let Some(existing) = load_existing_job(tx, request.operation_id).await? {
        if operation_state != "PENDING" && operation_state != "COMMITTED" {
            return Err(ServiceJobReservationError::OperationTerminal(
                operation_state,
            ));
        }
        return validate_existing_job(tx, request, &stacks, existing).await;
    }
    if operation_state != "PENDING" {
        return Err(ServiceJobReservationError::OperationTerminal(
            operation_state,
        ));
    }

    let player =
        sqlx::query("SELECT status FROM players WHERE id = $1 AND status <> 'DELETED' FOR UPDATE")
            .bind(request.player_id)
            .fetch_optional(&mut **tx)
            .await?
            .ok_or(ServiceJobReservationError::PlayerNotFound)?;
    let status: String = player.try_get("status")?;
    if status != "ACTIVE" {
        return Err(ServiceJobReservationError::AccountFrozen(status));
    }

    let required = aggregate_by_definition(&stacks)?;
    let mut available = BTreeMap::new();
    for ((definition_key, definition_version), requested) in &required {
        let quantity = sqlx::query(
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
        .bind(definition_key)
        .bind(*definition_version)
        .fetch_optional(&mut **tx)
        .await?
        .map(|row| row.try_get("quantity"))
        .transpose()?
        .unwrap_or(0_i64);
        if quantity < *requested {
            return Err(ServiceJobReservationError::InsufficientStack {
                definition_key: definition_key.clone(),
                definition_version: *definition_version,
                available: quantity,
                requested: *requested,
            });
        }
        available.insert((definition_key.clone(), *definition_version), quantity);
    }

    let job_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO service_jobs (
            id, operation_id, player_id, service_kind, policy_version, state
        )
        VALUES ($1, $2, $3, $4, $5, 'RUNNING')
        "#,
    )
    .bind(job_id)
    .bind(request.operation_id)
    .bind(request.player_id)
    .bind(&request.service_kind)
    .bind(request.policy_version)
    .execute(&mut **tx)
    .await?;

    for ((definition_key, definition_version), requested) in &required {
        let current = available
            .get(&(definition_key.clone(), *definition_version))
            .copied()
            .ok_or(ServiceJobReservationError::ReservationIntegrityMismatch)?;
        if current == *requested {
            sqlx::query(
                "DELETE FROM item_stacks WHERE player_id = $1 AND definition_key = $2 AND definition_version = $3 AND location = 'ITEM_BAG'",
            )
            .bind(request.player_id)
            .bind(definition_key)
            .bind(*definition_version)
            .execute(&mut **tx)
            .await?;
        } else {
            sqlx::query(
                "UPDATE item_stacks SET quantity = quantity - $4, updated_at = now() WHERE player_id = $1 AND definition_key = $2 AND definition_version = $3 AND location = 'ITEM_BAG'",
            )
            .bind(request.player_id)
            .bind(definition_key)
            .bind(*definition_version)
            .bind(*requested)
            .execute(&mut **tx)
            .await?;
        }
    }

    for stack in &stacks {
        sqlx::query(
            r#"
            INSERT INTO service_job_stack_reservations (
                job_id, role, definition_key, definition_version, quantity
            )
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(job_id)
        .bind(stack.role.as_str())
        .bind(&stack.definition_key)
        .bind(stack.definition_version)
        .bind(stack.quantity)
        .execute(&mut **tx)
        .await?;
    }

    sqlx::query(
        "INSERT INTO asset_events (id, operation_id, player_id, event_kind, payload) VALUES ($1, $2, $3, 'SERVICE_JOB_STACKS_RESERVED', $4)",
    )
    .bind(Uuid::now_v7())
    .bind(request.operation_id)
    .bind(request.player_id)
    .bind(json!({
        "job_id": job_id,
        "service_kind": request.service_kind,
        "policy_version": request.policy_version,
        "stacks": stacks,
    }))
    .execute(&mut **tx)
    .await?;

    Ok(ServiceJobReservationReceipt {
        job_id,
        operation_id: request.operation_id,
        player_id: request.player_id,
        service_kind: request.service_kind.clone(),
        policy_version: request.policy_version,
        state: "RUNNING".to_owned(),
        stacks,
    })
}

fn normalize_request(
    request: &ServiceJobReservationRequest,
) -> Result<Vec<StackReservationRequest>, ServiceJobReservationError> {
    if request.policy_version <= 0 {
        return Err(ServiceJobReservationError::InvalidPolicyVersion);
    }
    if request.service_kind.is_empty() || request.service_kind.trim() != request.service_kind {
        return Err(ServiceJobReservationError::InvalidServiceKind);
    }
    if request.stacks.is_empty() {
        return Err(ServiceJobReservationError::EmptyReservations);
    }

    let mut seen = BTreeSet::new();
    let mut stacks = request.stacks.clone();
    for stack in &stacks {
        if stack.definition_key.is_empty()
            || stack.definition_key.trim() != stack.definition_key
            || stack.definition_version <= 0
            || stack.quantity <= 0
        {
            return Err(ServiceJobReservationError::InvalidReservation);
        }
        let identity = (
            stack.role,
            stack.definition_key.clone(),
            stack.definition_version,
        );
        if !seen.insert(identity) {
            return Err(ServiceJobReservationError::DuplicateReservation);
        }
    }
    stacks.sort_by(|left, right| {
        (left.role, &left.definition_key, left.definition_version).cmp(&(
            right.role,
            &right.definition_key,
            right.definition_version,
        ))
    });
    Ok(stacks)
}

fn aggregate_by_definition(
    stacks: &[StackReservationRequest],
) -> Result<BTreeMap<(String, i32), i64>, ServiceJobReservationError> {
    let mut totals = BTreeMap::new();
    for stack in stacks {
        let total = totals
            .entry((stack.definition_key.clone(), stack.definition_version))
            .or_insert(0_i64);
        *total = total
            .checked_add(stack.quantity)
            .ok_or(ServiceJobReservationError::ArithmeticOverflow)?;
    }
    Ok(totals)
}

struct ExistingJob {
    job_id: Uuid,
    player_id: Uuid,
    service_kind: String,
    policy_version: i32,
    state: String,
}

async fn load_existing_job(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
) -> Result<Option<ExistingJob>, ServiceJobReservationError> {
    let row = sqlx::query(
        "SELECT id, player_id, service_kind, policy_version, state FROM service_jobs WHERE operation_id = $1 FOR UPDATE",
    )
    .bind(operation_id)
    .fetch_optional(&mut **tx)
    .await?;
    row.map(|row| {
        Ok(ExistingJob {
            job_id: row.try_get("id")?,
            player_id: row.try_get("player_id")?,
            service_kind: row.try_get("service_kind")?,
            policy_version: row.try_get("policy_version")?,
            state: row.try_get("state")?,
        })
    })
    .transpose()
}

async fn validate_existing_job(
    tx: &mut Transaction<'_, Postgres>,
    request: &ServiceJobReservationRequest,
    expected_stacks: &[StackReservationRequest],
    existing: ExistingJob,
) -> Result<ServiceJobReservationReceipt, ServiceJobReservationError> {
    if existing.player_id != request.player_id
        || existing.service_kind != request.service_kind
        || existing.policy_version != request.policy_version
    {
        return Err(ServiceJobReservationError::ReservationConflict);
    }
    let rows = sqlx::query(
        r#"
        SELECT role, definition_key, definition_version, quantity
          FROM service_job_stack_reservations
         WHERE job_id = $1
         ORDER BY role, definition_key, definition_version
        "#,
    )
    .bind(existing.job_id)
    .fetch_all(&mut **tx)
    .await?;
    let mut stored = Vec::with_capacity(rows.len());
    for row in rows {
        let role: String = row.try_get("role")?;
        stored.push(StackReservationRequest {
            role: ReservationRole::from_db(&role)?,
            definition_key: row.try_get("definition_key")?,
            definition_version: row.try_get("definition_version")?,
            quantity: row.try_get("quantity")?,
        });
    }
    stored.sort_by(|left, right| {
        (left.role, &left.definition_key, left.definition_version).cmp(&(
            right.role,
            &right.definition_key,
            right.definition_version,
        ))
    });
    if stored != expected_stacks {
        return Err(ServiceJobReservationError::ReservationConflict);
    }

    Ok(ServiceJobReservationReceipt {
        job_id: existing.job_id,
        operation_id: request.operation_id,
        player_id: existing.player_id,
        service_kind: existing.service_kind,
        policy_version: existing.policy_version,
        state: existing.state,
        stacks: stored,
    })
}
