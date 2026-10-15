use blake3::Hasher;
use graphite_core::{OperationId, RootSeed};
use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{FishingArea, MANUAL_FISHING_OPERATION_KIND, MANUAL_FISHING_OPERATION_POLICY_VERSION};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManualFishingOperationV1 {
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub area: FishingArea,
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
    #[error("Fishing operation cannot be resumed in state {0}")]
    OperationTerminal(String),
}

impl From<sqlx::Error> for ManualFishingOperationError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

/// Creates or resumes one PENDING manual-Fishing operation inside the caller's
/// transaction.
///
/// The external request key owns retry identity. The domain-separated request
/// hash covers the selected canonical Fishing area; actor, operation kind,
/// policy version and bound player are validated independently from persisted
/// columns. A duplicate PENDING request returns the exact original operation
/// and RNG root remains untouched. COMMITTED replay belongs to the future typed
/// terminal-result owner, so this pre-terminal resolver fails closed rather than
/// exposing opaque JSON as a gameplay receipt.
///
/// This function commits nothing. Its caller must keep operation creation,
/// Fishing preflight and eventual settlement in the same transaction or roll
/// the operation row back on failure.
pub async fn lock_or_create_manual_fishing_operation_v1(
    tx: &mut Transaction<'_, Postgres>,
    discord_user_id: u64,
    external_request_key: &str,
    area: FishingArea,
) -> Result<ManualFishingOperationV1, ManualFishingOperationError> {
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

async fn select_operation(
    tx: &mut Transaction<'_, Postgres>,
    external_request_key: &str,
) -> Result<Option<sqlx::postgres::PgRow>, ManualFishingOperationError> {
    Ok(sqlx::query(
        r#"
        SELECT id, actor_discord_user_id, player_id, kind, state,
               policy_version, request_hash
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
) -> Result<ManualFishingOperationV1, ManualFishingOperationError> {
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
    if state != "PENDING" {
        return Err(ManualFishingOperationError::OperationTerminal(state));
    }

    let player_id = load_player_id(tx, actor).await?;
    if stored_player_id != Some(player_id) {
        return Err(ManualFishingOperationError::IdempotencyConflict);
    }
    Ok(ManualFishingOperationV1 {
        operation_id,
        player_id,
        area,
    })
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
