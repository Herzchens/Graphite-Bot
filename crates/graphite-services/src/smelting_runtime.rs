use chrono::{DateTime, Duration, Utc};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    ORDINARY_SMELT_SECONDS_PER_UNIT, SmeltFuelKind, SmeltingMathError, preview_single_fuel_smelting,
};

pub const ORDINARY_SMELT_MICROS_PER_UNIT: i64 = ORDINARY_SMELT_SECONDS_PER_UNIT * 1_000_000;

#[derive(Clone, Debug, PartialEq)]
pub struct SmeltingRuntimeRequest {
    pub job_id: Uuid,
    pub requested_units: i64,
    pub accepted_units: i64,
    pub fuel_kind: SmeltFuelKind,
    pub reserved_fuel_items: i64,
    pub effective_unit_micros: i64,
    pub modifier_snapshot: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SmeltingRuntimeReceipt {
    pub job_id: Uuid,
    pub requested_units: i64,
    pub accepted_units: i64,
    pub fuel_kind: SmeltFuelKind,
    pub reserved_fuel_items: i64,
    pub effective_unit_micros: i64,
    pub modifier_snapshot: Value,
    pub started_at: DateTime<Utc>,
    pub completes_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SmeltingRuntimeProgress {
    pub observed_at: DateTime<Utc>,
    pub completed_units: i64,
    pub remaining_units: i64,
    pub elapsed_work_micros: i64,
    pub current_unit_elapsed_micros: i64,
    pub finished: bool,
}

#[derive(Debug, Error)]
pub enum SmeltingRuntimeError {
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error(transparent)]
    Math(#[from] SmeltingMathError),
    #[error("requested/accepted Smelting units are invalid")]
    InvalidUnits,
    #[error("reserved Smelting fuel must be the exact minimal whole-fuel cover for accepted units")]
    InvalidFuelReservation,
    #[error("effective Smelting unit duration must be a positive microsecond value")]
    InvalidEffectiveDuration,
    #[error("Smelting modifier snapshot must be a JSON object")]
    InvalidModifierSnapshot,
    #[error("service job does not exist")]
    JobNotFound,
    #[error("owning operation/service-job identity is internally inconsistent")]
    JobIntegrityMismatch,
    #[error("player does not exist")]
    PlayerNotFound,
    #[error("a new Smelting runtime requires an ACTIVE account; current status is {0}")]
    AccountFrozen(String),
    #[error("service job kind is {0}, not SMELT")]
    WrongServiceKind(String),
    #[error("Smelting runtime cannot be newly attached while owning operation state is {0}")]
    OperationTerminal(String),
    #[error("Smelting runtime cannot be attached while service job state is {0}")]
    JobNotRunning(String),
    #[error(
        "Smelting requires exactly one INPUT reservation and one FUEL reservation matching the runtime quantities"
    )]
    ReservationShapeMismatch,
    #[error("existing Smelting runtime does not match the retried snapshot")]
    RuntimeConflict,
    #[error("stored Smelting fuel kind is invalid: {0}")]
    InvalidStoredFuelKind(String),
    #[error("Smelting runtime arithmetic exceeded the supported persistence range")]
    ArithmeticOverflow,
    #[error("Smelting timestamp is outside the supported range")]
    TimeOutOfRange,
}

impl From<sqlx::Error> for SmeltingRuntimeError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

struct JobIdentity {
    operation_id: Uuid,
    player_id: Uuid,
    policy_version: i32,
}

/// Attaches the immutable, time/progress snapshot for an already-reserved SMELT service job.
///
/// The caller owns the surrounding Confirm transaction. The effective unit duration and
/// modifier snapshot are inputs from canonical modifier evaluation at Confirm; this layer
/// deliberately does not invent or recompute a speed bucket. Exact Item reservations must
/// already exist for the job and are revalidated before the runtime snapshot is inserted.
/// A new snapshot can only be attached while the owning operation is still PENDING and the
/// account remains ACTIVE; matching snapshots remain replayable after COMMITTED/freeze.
pub async fn attach_smelting_job_runtime(
    tx: &mut Transaction<'_, Postgres>,
    request: &SmeltingRuntimeRequest,
) -> Result<SmeltingRuntimeReceipt, SmeltingRuntimeError> {
    validate_runtime_request(request)?;

    // service_jobs identity is immutable at the database layer. Read it without a row lock so
    // every mutation path can then acquire locks in Graphite's deterministic order:
    // operation -> player -> service_job -> domain rows.
    let identity = load_job_identity(tx, request.job_id).await?;

    let operation = sqlx::query(
        "SELECT player_id, policy_version, state FROM operations WHERE id = $1 FOR UPDATE",
    )
    .bind(identity.operation_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(SmeltingRuntimeError::JobIntegrityMismatch)?;
    let operation_player_id: Option<Uuid> = operation.try_get("player_id")?;
    let operation_policy_version: i32 = operation.try_get("policy_version")?;
    let operation_state: String = operation.try_get("state")?;
    if operation_player_id.is_some_and(|player_id| player_id != identity.player_id)
        || operation_policy_version != identity.policy_version
    {
        return Err(SmeltingRuntimeError::JobIntegrityMismatch);
    }
    if operation_state == "COMMITTED" && operation_player_id != Some(identity.player_id) {
        return Err(SmeltingRuntimeError::JobIntegrityMismatch);
    }

    let player = sqlx::query("SELECT status FROM players WHERE id = $1 FOR UPDATE")
        .bind(identity.player_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(SmeltingRuntimeError::PlayerNotFound)?;
    let player_status: String = player.try_get("status")?;

    let job = sqlx::query(
        "SELECT operation_id, player_id, policy_version, service_kind, state FROM service_jobs WHERE id = $1 FOR UPDATE",
    )
    .bind(request.job_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(SmeltingRuntimeError::JobNotFound)?;
    let locked_operation_id: Uuid = job.try_get("operation_id")?;
    let locked_player_id: Uuid = job.try_get("player_id")?;
    let locked_policy_version: i32 = job.try_get("policy_version")?;
    if locked_operation_id != identity.operation_id
        || locked_player_id != identity.player_id
        || locked_policy_version != identity.policy_version
    {
        return Err(SmeltingRuntimeError::JobIntegrityMismatch);
    }
    let service_kind: String = job.try_get("service_kind")?;
    if service_kind != "SMELT" {
        return Err(SmeltingRuntimeError::WrongServiceKind(service_kind));
    }
    let job_state: String = job.try_get("state")?;

    if let Some(existing) = load_runtime_row(tx, request.job_id).await? {
        if operation_state != "PENDING" && operation_state != "COMMITTED" {
            return Err(SmeltingRuntimeError::OperationTerminal(operation_state));
        }
        return validate_runtime_replay(existing, request);
    }
    if operation_state != "PENDING" {
        return Err(SmeltingRuntimeError::OperationTerminal(operation_state));
    }
    if player_status != "ACTIVE" {
        return Err(SmeltingRuntimeError::AccountFrozen(player_status));
    }
    if job_state != "RUNNING" {
        return Err(SmeltingRuntimeError::JobNotRunning(job_state));
    }

    validate_reservation_shape(tx, request).await?;

    let started_at: DateTime<Utc> = sqlx::query("SELECT clock_timestamp() AS now")
        .fetch_one(&mut **tx)
        .await?
        .try_get("now")?;
    let total_micros = request
        .accepted_units
        .checked_mul(request.effective_unit_micros)
        .ok_or(SmeltingRuntimeError::ArithmeticOverflow)?;
    let completes_at = started_at
        .checked_add_signed(Duration::microseconds(total_micros))
        .ok_or(SmeltingRuntimeError::TimeOutOfRange)?;

    sqlx::query(
        r#"
        INSERT INTO smelting_job_runtimes (
            job_id, requested_units, accepted_units, fuel_kind, reserved_fuel_items,
            effective_unit_micros, modifier_snapshot, started_at, completes_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
    )
    .bind(request.job_id)
    .bind(request.requested_units)
    .bind(request.accepted_units)
    .bind(fuel_kind_database_value(request.fuel_kind))
    .bind(request.reserved_fuel_items)
    .bind(request.effective_unit_micros)
    .bind(&request.modifier_snapshot)
    .bind(started_at)
    .bind(completes_at)
    .execute(&mut **tx)
    .await?;

    Ok(SmeltingRuntimeReceipt {
        job_id: request.job_id,
        requested_units: request.requested_units,
        accepted_units: request.accepted_units,
        fuel_kind: request.fuel_kind,
        reserved_fuel_items: request.reserved_fuel_items,
        effective_unit_micros: request.effective_unit_micros,
        modifier_snapshot: request.modifier_snapshot.clone(),
        started_at,
        completes_at,
    })
}

pub async fn load_smelting_job_runtime(
    pool: &PgPool,
    job_id: Uuid,
) -> Result<Option<SmeltingRuntimeReceipt>, SmeltingRuntimeError> {
    let row = sqlx::query(
        r#"
        SELECT job_id, requested_units, accepted_units, fuel_kind, reserved_fuel_items,
               effective_unit_micros, modifier_snapshot, started_at, completes_at
          FROM smelting_job_runtimes
         WHERE job_id = $1
        "#,
    )
    .bind(job_id)
    .fetch_optional(pool)
    .await?;
    row.map(runtime_row_to_receipt).transpose()
}

/// O(1) wall-clock projection: no per-job ticker or periodic progress writes are required.
/// Partial current-unit work is intentionally not counted as a completed unit.
pub fn project_smelting_runtime_progress(
    runtime: &SmeltingRuntimeReceipt,
    observed_at: DateTime<Utc>,
) -> Result<SmeltingRuntimeProgress, SmeltingRuntimeError> {
    if runtime.accepted_units <= 0 || runtime.effective_unit_micros <= 0 {
        return Err(SmeltingRuntimeError::RuntimeConflict);
    }
    let total_micros = runtime
        .accepted_units
        .checked_mul(runtime.effective_unit_micros)
        .ok_or(SmeltingRuntimeError::ArithmeticOverflow)?;

    let elapsed_work_micros = if observed_at <= runtime.started_at {
        0
    } else if observed_at >= runtime.completes_at {
        total_micros
    } else {
        observed_at
            .signed_duration_since(runtime.started_at)
            .num_microseconds()
            .ok_or(SmeltingRuntimeError::TimeOutOfRange)?
            .clamp(0, total_micros)
    };
    let completed_units =
        (elapsed_work_micros / runtime.effective_unit_micros).min(runtime.accepted_units);
    let finished = completed_units == runtime.accepted_units;
    let current_unit_elapsed_micros = if finished {
        0
    } else {
        elapsed_work_micros % runtime.effective_unit_micros
    };

    Ok(SmeltingRuntimeProgress {
        observed_at,
        completed_units,
        remaining_units: runtime
            .accepted_units
            .checked_sub(completed_units)
            .ok_or(SmeltingRuntimeError::ArithmeticOverflow)?,
        elapsed_work_micros,
        current_unit_elapsed_micros,
        finished,
    })
}

async fn load_job_identity(
    tx: &mut Transaction<'_, Postgres>,
    job_id: Uuid,
) -> Result<JobIdentity, SmeltingRuntimeError> {
    let row = sqlx::query(
        "SELECT operation_id, player_id, policy_version FROM service_jobs WHERE id = $1",
    )
    .bind(job_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(SmeltingRuntimeError::JobNotFound)?;
    Ok(JobIdentity {
        operation_id: row.try_get("operation_id")?,
        player_id: row.try_get("player_id")?,
        policy_version: row.try_get("policy_version")?,
    })
}

fn validate_runtime_request(request: &SmeltingRuntimeRequest) -> Result<(), SmeltingRuntimeError> {
    if request.requested_units <= 0
        || request.accepted_units <= 0
        || request.accepted_units > request.requested_units
    {
        return Err(SmeltingRuntimeError::InvalidUnits);
    }
    if request.effective_unit_micros <= 0 {
        return Err(SmeltingRuntimeError::InvalidEffectiveDuration);
    }
    if !request.modifier_snapshot.is_object() {
        return Err(SmeltingRuntimeError::InvalidModifierSnapshot);
    }
    if request.reserved_fuel_items <= 0 {
        return Err(SmeltingRuntimeError::InvalidFuelReservation);
    }

    let preview = preview_single_fuel_smelting(
        request.accepted_units,
        request.accepted_units,
        request.fuel_kind,
        request.reserved_fuel_items,
    )?;
    if !preview.confirmable
        || preview.processable_units != request.accepted_units
        || preview.fuel_items_to_reserve != request.reserved_fuel_items
    {
        return Err(SmeltingRuntimeError::InvalidFuelReservation);
    }
    request
        .accepted_units
        .checked_mul(request.effective_unit_micros)
        .ok_or(SmeltingRuntimeError::ArithmeticOverflow)?;
    Ok(())
}

async fn validate_reservation_shape(
    tx: &mut Transaction<'_, Postgres>,
    request: &SmeltingRuntimeRequest,
) -> Result<(), SmeltingRuntimeError> {
    let rows = sqlx::query(
        "SELECT role, quantity FROM service_job_stack_reservations WHERE job_id = $1 ORDER BY role, definition_key, definition_version",
    )
    .bind(request.job_id)
    .fetch_all(&mut **tx)
    .await?;
    if rows.len() != 2 {
        return Err(SmeltingRuntimeError::ReservationShapeMismatch);
    }

    let mut input_quantity = None;
    let mut fuel_quantity = None;
    for row in rows {
        let role: String = row.try_get("role")?;
        let quantity: i64 = row.try_get("quantity")?;
        match role.as_str() {
            "INPUT" if input_quantity.is_none() => input_quantity = Some(quantity),
            "FUEL" if fuel_quantity.is_none() => fuel_quantity = Some(quantity),
            _ => return Err(SmeltingRuntimeError::ReservationShapeMismatch),
        }
    }
    if input_quantity != Some(request.accepted_units)
        || fuel_quantity != Some(request.reserved_fuel_items)
    {
        return Err(SmeltingRuntimeError::ReservationShapeMismatch);
    }
    Ok(())
}

async fn load_runtime_row(
    tx: &mut Transaction<'_, Postgres>,
    job_id: Uuid,
) -> Result<Option<sqlx::postgres::PgRow>, SmeltingRuntimeError> {
    Ok(sqlx::query(
        r#"
        SELECT job_id, requested_units, accepted_units, fuel_kind, reserved_fuel_items,
               effective_unit_micros, modifier_snapshot, started_at, completes_at
          FROM smelting_job_runtimes
         WHERE job_id = $1
         FOR UPDATE
        "#,
    )
    .bind(job_id)
    .fetch_optional(&mut **tx)
    .await?)
}

fn validate_runtime_replay(
    row: sqlx::postgres::PgRow,
    request: &SmeltingRuntimeRequest,
) -> Result<SmeltingRuntimeReceipt, SmeltingRuntimeError> {
    let stored = runtime_row_to_receipt(row)?;
    if stored.job_id != request.job_id
        || stored.requested_units != request.requested_units
        || stored.accepted_units != request.accepted_units
        || stored.fuel_kind != request.fuel_kind
        || stored.reserved_fuel_items != request.reserved_fuel_items
        || stored.effective_unit_micros != request.effective_unit_micros
        || stored.modifier_snapshot != request.modifier_snapshot
    {
        return Err(SmeltingRuntimeError::RuntimeConflict);
    }
    Ok(stored)
}

fn runtime_row_to_receipt(
    row: sqlx::postgres::PgRow,
) -> Result<SmeltingRuntimeReceipt, SmeltingRuntimeError> {
    let fuel_kind: String = row.try_get("fuel_kind")?;
    Ok(SmeltingRuntimeReceipt {
        job_id: row.try_get("job_id")?,
        requested_units: row.try_get("requested_units")?,
        accepted_units: row.try_get("accepted_units")?,
        fuel_kind: fuel_kind_from_database(&fuel_kind)?,
        reserved_fuel_items: row.try_get("reserved_fuel_items")?,
        effective_unit_micros: row.try_get("effective_unit_micros")?,
        modifier_snapshot: row.try_get("modifier_snapshot")?,
        started_at: row.try_get("started_at")?,
        completes_at: row.try_get("completes_at")?,
    })
}

const fn fuel_kind_database_value(kind: SmeltFuelKind) -> &'static str {
    match kind {
        SmeltFuelKind::Coal => "COAL",
        SmeltFuelKind::WoodLog => "WOOD_LOG",
    }
}

fn fuel_kind_from_database(value: &str) -> Result<SmeltFuelKind, SmeltingRuntimeError> {
    match value {
        "COAL" => Ok(SmeltFuelKind::Coal),
        "WOOD_LOG" => Ok(SmeltFuelKind::WoodLog),
        _ => Err(SmeltingRuntimeError::InvalidStoredFuelKind(
            value.to_owned(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use serde_json::json;

    use super::*;

    fn runtime() -> SmeltingRuntimeReceipt {
        let started_at = DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap();
        SmeltingRuntimeReceipt {
            job_id: Uuid::nil(),
            requested_units: 10,
            accepted_units: 8,
            fuel_kind: SmeltFuelKind::Coal,
            reserved_fuel_items: 1,
            effective_unit_micros: ORDINARY_SMELT_MICROS_PER_UNIT,
            modifier_snapshot: json!({}),
            started_at,
            completes_at: started_at + TimeDelta::seconds(160),
        }
    }

    #[test]
    fn progress_is_floor_based_clamped_and_tickless() {
        let runtime = runtime();
        for (micros, completed, current, finished) in [
            (-1, 0, 0, false),
            (0, 0, 0, false),
            (19_999_999, 0, 19_999_999, false),
            (20_000_000, 1, 0, false),
            (39_999_999, 1, 19_999_999, false),
            (159_999_999, 7, 19_999_999, false),
            (160_000_000, 8, 0, true),
            (500_000_000, 8, 0, true),
        ] {
            let observed = runtime.started_at + TimeDelta::microseconds(micros);
            let progress = project_smelting_runtime_progress(&runtime, observed).unwrap();
            assert_eq!(progress.completed_units, completed);
            assert_eq!(progress.current_unit_elapsed_micros, current);
            assert_eq!(progress.finished, finished);
            assert_eq!(
                progress.completed_units + progress.remaining_units,
                runtime.accepted_units
            );
        }
    }

    #[test]
    fn runtime_validation_rejects_under_and_over_reserved_fuel() {
        let base = SmeltingRuntimeRequest {
            job_id: Uuid::nil(),
            requested_units: 8,
            accepted_units: 8,
            fuel_kind: SmeltFuelKind::Coal,
            reserved_fuel_items: 1,
            effective_unit_micros: ORDINARY_SMELT_MICROS_PER_UNIT,
            modifier_snapshot: json!({}),
        };
        validate_runtime_request(&base).unwrap();

        let mut under = base.clone();
        under.reserved_fuel_items = 0;
        assert!(matches!(
            validate_runtime_request(&under),
            Err(SmeltingRuntimeError::InvalidFuelReservation)
        ));

        let mut over = base;
        over.reserved_fuel_items = 2;
        assert!(matches!(
            validate_runtime_request(&over),
            Err(SmeltingRuntimeError::InvalidFuelReservation)
        ));
    }
}
