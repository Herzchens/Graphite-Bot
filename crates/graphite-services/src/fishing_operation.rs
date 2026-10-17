use blake3::Hasher;
use graphite_core::{OperationId, RootSeed};
use serde_json::Value;
use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    FishingArea, MANUAL_FISHING_OPERATION_KIND, MANUAL_FISHING_OPERATION_POLICY_VERSION,
    ManualFishingStarterBaseTerminalReceiptV1, ManualFishingStrengthenTerminalReceiptV1,
    ManualFishingUnbreakingXTerminalReceiptV1, ManualFishingUnmodifiedOrdinaryTerminalReceiptV1,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManualFishingOperationV1 {
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub area: FishingArea,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ManualFishingOperationResolutionV1 {
    Pending(ManualFishingOperationV1),
    CommittedStarterBase(Box<ManualFishingStarterBaseTerminalReceiptV1>),
    CommittedUnmodifiedOrdinary(Box<ManualFishingUnmodifiedOrdinaryTerminalReceiptV1>),
    CommittedStrengthen(Box<ManualFishingStrengthenTerminalReceiptV1>),
    CommittedUnbreakingX(Box<ManualFishingUnbreakingXTerminalReceiptV1>),
}

#[derive(Debug, Error)]
pub enum ManualFishingOperationError {
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error("Discord snowflake is outside the signed database range")]
    SnowflakeOutOfRange,
    #[error("player does not exist")]
    PlayerNotFound,
    #[error("Fishing operation disappeared after insert/conflict resolution")]
    OperationMissingAfterInsert,
    #[error("external request key was reused with different Fishing request provenance")]
    IdempotencyConflict,
    #[error("committed Fishing operation has no typed result")]
    MissingOperationResult,
    #[error("stored Fishing operation result is not a valid supported typed receipt: {0}")]
    InvalidOperationResult(Box<serde_json::Error>),
    #[error("stored Fishing operation result has unsupported receipt kind {0}")]
    UnsupportedOperationResultKind(String),
    #[error("stored Fishing operation state does not match its typed committed receipt")]
    StoredOperationIntegrityMismatch,
    #[error("Fishing operation cannot be resumed in state {0}")]
    OperationTerminal(String),
}

impl From<sqlx::Error> for ManualFishingOperationError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

/// Resolves one manual-Fishing request identity inside the caller transaction.
///
/// A matching PENDING operation returns its original operation/player/area and
/// leaves preflight to the owning cast service. The only currently supported
/// COMMITTED replay is the typed Starter-Pool/Starter-Basic v1 receipt. Replay
/// validates operation id, player id, area, actor, kind, policy version and the
/// domain-separated request hash before exposing the receipt.
///
/// This function performs no gameplay mutation and commits nothing.
pub async fn resolve_manual_fishing_operation_v1(
    tx: &mut Transaction<'_, Postgres>,
    discord_user_id: u64,
    external_request_key: &str,
    area: FishingArea,
) -> Result<ManualFishingOperationResolutionV1, ManualFishingOperationError> {
    let actor = i64::try_from(discord_user_id)
        .map_err(|_| ManualFishingOperationError::SnowflakeOutOfRange)?;
    let request_hash = manual_fishing_request_hash_v1(area);

    if let Some(row) = select_operation(tx, external_request_key).await? {
        return validate_existing_operation(tx, row, actor, area, &request_hash).await;
    }

    let player_id = load_player_id(tx, actor).await?;
    let proposed_operation_id = OperationId::new().as_uuid();
    let rng_root = RootSeed::generate();
    sqlx::query(
        r#"
        INSERT INTO operations (
            id, external_request_key, actor_discord_user_id, player_id,
            kind, state, policy_version, request_hash, rng_root
        )
        VALUES ($1, $2, $3, $4, $5, 'PENDING', $6, $7, $8)
        ON CONFLICT (external_request_key) DO NOTHING
        "#,
    )
    .bind(proposed_operation_id)
    .bind(external_request_key)
    .bind(actor)
    .bind(player_id)
    .bind(MANUAL_FISHING_OPERATION_KIND)
    .bind(MANUAL_FISHING_OPERATION_POLICY_VERSION)
    .bind(request_hash.as_slice())
    .bind(rng_root.as_bytes().as_slice())
    .execute(&mut **tx)
    .await?;

    let row = select_operation(tx, external_request_key)
        .await?
        .ok_or(ManualFishingOperationError::OperationMissingAfterInsert)?;
    validate_existing_operation(tx, row, actor, area, &request_hash).await
}

/// Creates or resumes one PENDING manual-Fishing operation.
///
/// This compatibility/preflight helper intentionally refuses a successfully
/// decoded COMMITTED receipt. Adapter-facing code that needs retry replay should
/// use `resolve_manual_fishing_operation_v1` or the higher cast-request resolver.
pub async fn lock_or_create_manual_fishing_operation_v1(
    tx: &mut Transaction<'_, Postgres>,
    discord_user_id: u64,
    external_request_key: &str,
    area: FishingArea,
) -> Result<ManualFishingOperationV1, ManualFishingOperationError> {
    match resolve_manual_fishing_operation_v1(tx, discord_user_id, external_request_key, area)
        .await?
    {
        ManualFishingOperationResolutionV1::Pending(operation) => Ok(operation),
        ManualFishingOperationResolutionV1::CommittedStarterBase(_)
        | ManualFishingOperationResolutionV1::CommittedUnmodifiedOrdinary(_)
        | ManualFishingOperationResolutionV1::CommittedStrengthen(_)
        | ManualFishingOperationResolutionV1::CommittedUnbreakingX(_) => Err(
            ManualFishingOperationError::OperationTerminal("COMMITTED".to_owned()),
        ),
    }
}

async fn select_operation(
    tx: &mut Transaction<'_, Postgres>,
    external_request_key: &str,
) -> Result<Option<sqlx::postgres::PgRow>, ManualFishingOperationError> {
    Ok(sqlx::query(
        r#"
        SELECT id, actor_discord_user_id, player_id, kind, state,
               policy_version, request_hash, result
          FROM operations
         WHERE external_request_key = $1
         FOR UPDATE
        "#,
    )
    .bind(external_request_key)
    .fetch_optional(&mut **tx)
    .await?)
}

async fn validate_existing_operation(
    tx: &mut Transaction<'_, Postgres>,
    row: sqlx::postgres::PgRow,
    actor: i64,
    area: FishingArea,
    request_hash: &[u8; 32],
) -> Result<ManualFishingOperationResolutionV1, ManualFishingOperationError> {
    let operation_id: Uuid = row.try_get("id")?;
    let stored_actor: i64 = row.try_get("actor_discord_user_id")?;
    let stored_player_id: Option<Uuid> = row.try_get("player_id")?;
    let kind: String = row.try_get("kind")?;
    let state: String = row.try_get("state")?;
    let policy_version: i32 = row.try_get("policy_version")?;
    let stored_hash: Vec<u8> = row.try_get("request_hash")?;

    if stored_actor != actor
        || kind != MANUAL_FISHING_OPERATION_KIND
        || policy_version != MANUAL_FISHING_OPERATION_POLICY_VERSION
        || stored_hash.as_slice() != request_hash.as_slice()
    {
        return Err(ManualFishingOperationError::IdempotencyConflict);
    }

    if state == "COMMITTED" {
        let value: Option<Value> = row.try_get("result")?;
        let value = value.ok_or(ManualFishingOperationError::MissingOperationResult)?;
        if let Some(kind) = value.get("receipt_kind").and_then(Value::as_str) {
            return match kind {
                "UNMODIFIED_ORDINARY" => {
                    let receipt: ManualFishingUnmodifiedOrdinaryTerminalReceiptV1 =
                        serde_json::from_value(value).map_err(|error| {
                            ManualFishingOperationError::InvalidOperationResult(Box::new(error))
                        })?;
                    if receipt.operation_id != operation_id
                        || stored_player_id != Some(receipt.player_id)
                        || receipt.area != area
                    {
                        return Err(ManualFishingOperationError::StoredOperationIntegrityMismatch);
                    }
                    Ok(
                        ManualFishingOperationResolutionV1::CommittedUnmodifiedOrdinary(Box::new(
                            receipt,
                        )),
                    )
                }
                "STRENGTHEN" => {
                    let receipt: ManualFishingStrengthenTerminalReceiptV1 =
                        serde_json::from_value(value).map_err(|error| {
                            ManualFishingOperationError::InvalidOperationResult(Box::new(error))
                        })?;
                    if receipt.operation_id != operation_id
                        || stored_player_id != Some(receipt.player_id)
                        || receipt.area != area
                    {
                        return Err(ManualFishingOperationError::StoredOperationIntegrityMismatch);
                    }
                    Ok(ManualFishingOperationResolutionV1::CommittedStrengthen(
                        Box::new(receipt),
                    ))
                }
                "UNBREAKING_X" => {
                    let receipt: ManualFishingUnbreakingXTerminalReceiptV1 =
                        serde_json::from_value(value).map_err(|error| {
                            ManualFishingOperationError::InvalidOperationResult(Box::new(error))
                        })?;
                    if receipt.operation_id != operation_id
                        || stored_player_id != Some(receipt.player_id)
                        || receipt.area != area
                    {
                        return Err(ManualFishingOperationError::StoredOperationIntegrityMismatch);
                    }
                    Ok(ManualFishingOperationResolutionV1::CommittedUnbreakingX(
                        Box::new(receipt),
                    ))
                }
                _ => Err(ManualFishingOperationError::UnsupportedOperationResultKind(
                    kind.to_owned(),
                )),
            };
        }

        let receipt: ManualFishingStarterBaseTerminalReceiptV1 = serde_json::from_value(value)
            .map_err(|error| {
                ManualFishingOperationError::InvalidOperationResult(Box::new(error))
            })?;
        if receipt.operation_id != operation_id
            || stored_player_id != Some(receipt.player_id)
            || receipt.area != area
        {
            return Err(ManualFishingOperationError::StoredOperationIntegrityMismatch);
        }
        return Ok(ManualFishingOperationResolutionV1::CommittedStarterBase(
            Box::new(receipt),
        ));
    }
    if state != "PENDING" {
        return Err(ManualFishingOperationError::OperationTerminal(state));
    }

    let player_id = load_player_id(tx, actor).await?;
    if stored_player_id != Some(player_id) {
        return Err(ManualFishingOperationError::IdempotencyConflict);
    }
    Ok(ManualFishingOperationResolutionV1::Pending(
        ManualFishingOperationV1 {
            operation_id,
            player_id,
            area,
        },
    ))
}

async fn load_player_id(
    tx: &mut Transaction<'_, Postgres>,
    actor: i64,
) -> Result<Uuid, ManualFishingOperationError> {
    sqlx::query_scalar("SELECT id FROM players WHERE discord_user_id = $1 AND status <> 'DELETED'")
        .bind(actor)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(ManualFishingOperationError::PlayerNotFound)
}

#[must_use]
pub fn manual_fishing_request_hash_v1(area: FishingArea) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(b"graphite.operation.manual-fishing.v1\0");
    hasher.update(&[area_protocol_byte(area)]);
    *hasher.finalize().as_bytes()
}

const fn area_protocol_byte(area: FishingArea) -> u8 {
    match area {
        FishingArea::StarterPool => 0,
        FishingArea::River => 1,
        FishingArea::Lake => 2,
        FishingArea::Coast => 3,
        FishingArea::DeepSea => 4,
        FishingArea::Abyss => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_hash_is_stable_and_area_scoped() {
        for area in [
            FishingArea::StarterPool,
            FishingArea::River,
            FishingArea::Lake,
            FishingArea::Coast,
            FishingArea::DeepSea,
            FishingArea::Abyss,
        ] {
            assert_eq!(
                manual_fishing_request_hash_v1(area),
                manual_fishing_request_hash_v1(area)
            );
        }
        assert_ne!(
            manual_fishing_request_hash_v1(FishingArea::StarterPool),
            manual_fishing_request_hash_v1(FishingArea::River)
        );
    }
}
