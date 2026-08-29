use sqlx::{PgPool, Row};
use thiserror::Error;
use uuid::Uuid;

use crate::{SmeltingRuntimeError, SmeltingRuntimeReceipt, load_smelting_job_runtime};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReservedStackIdentity {
    pub definition_key: String,
    pub definition_version: i32,
    pub quantity: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SmeltingSettlementContext {
    pub job_id: Uuid,
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub policy_version: i32,
    pub runtime: SmeltingRuntimeReceipt,
    pub input: ReservedStackIdentity,
    pub fuel: ReservedStackIdentity,
}

#[derive(Debug, Error)]
pub enum SmeltingSettlementContextError {
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error(transparent)]
    Runtime(#[from] SmeltingRuntimeError),
    #[error("service job kind is {0}, not SMELT")]
    WrongServiceKind(String),
    #[error("SMELT service job has no immutable runtime snapshot")]
    MissingRuntime,
    #[error("SMELT service job reservation provenance is internally inconsistent")]
    ReservationIntegrityMismatch,
}

impl From<sqlx::Error> for SmeltingSettlementContextError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

/// Loads the immutable asset/runtime identity needed by a future Smelting terminal settlement.
///
/// This function is deliberately read-only. It does not treat the returned context as authority
/// for mutable job state and therefore does not expose `service_jobs.state`; a terminal mutation
/// owner must lock/revalidate the job and its owning operation before applying any asset or
/// progression effects. Service-job identity, reservation provenance, and the attached Smelting
/// runtime are immutable once created, so they are safe to assemble here without inventing a
/// second source of truth.
///
/// The exact definition key/version pairs come from the already-reserved Item Stacks. No
/// content-to-ItemDefinition mapping is inferred here.
pub async fn load_smelting_settlement_context(
    pool: &PgPool,
    job_id: Uuid,
) -> Result<Option<SmeltingSettlementContext>, SmeltingSettlementContextError> {
    let job = sqlx::query(
        "SELECT operation_id, player_id, service_kind, policy_version FROM service_jobs WHERE id = $1",
    )
    .bind(job_id)
    .fetch_optional(pool)
    .await?;
    let Some(job) = job else {
        return Ok(None);
    };

    let service_kind: String = job.try_get("service_kind")?;
    if service_kind != "SMELT" {
        return Err(SmeltingSettlementContextError::WrongServiceKind(
            service_kind,
        ));
    }

    let rows = sqlx::query(
        r#"
        SELECT role, definition_key, definition_version, quantity
          FROM service_job_stack_reservations
         WHERE job_id = $1
         ORDER BY role, definition_key, definition_version
        "#,
    )
    .bind(job_id)
    .fetch_all(pool)
    .await?;
    if rows.len() != 2 {
        return Err(SmeltingSettlementContextError::ReservationIntegrityMismatch);
    }

    let mut input = None;
    let mut fuel = None;
    for row in rows {
        let role: String = row.try_get("role")?;
        let stack = ReservedStackIdentity {
            definition_key: row.try_get("definition_key")?,
            definition_version: row.try_get("definition_version")?,
            quantity: row.try_get("quantity")?,
        };
        match role.as_str() {
            "INPUT" if input.is_none() => input = Some(stack),
            "FUEL" if fuel.is_none() => fuel = Some(stack),
            _ => {
                return Err(SmeltingSettlementContextError::ReservationIntegrityMismatch);
            }
        }
    }
    let input = input.ok_or(SmeltingSettlementContextError::ReservationIntegrityMismatch)?;
    let fuel = fuel.ok_or(SmeltingSettlementContextError::ReservationIntegrityMismatch)?;

    let runtime = load_smelting_job_runtime(pool, job_id)
        .await?
        .ok_or(SmeltingSettlementContextError::MissingRuntime)?;
    if input.quantity != runtime.accepted_units || fuel.quantity != runtime.reserved_fuel_items {
        return Err(SmeltingSettlementContextError::ReservationIntegrityMismatch);
    }

    Ok(Some(SmeltingSettlementContext {
        job_id,
        operation_id: job.try_get("operation_id")?,
        player_id: job.try_get("player_id")?,
        policy_version: job.try_get("policy_version")?,
        runtime,
        input,
        fuel,
    }))
}
