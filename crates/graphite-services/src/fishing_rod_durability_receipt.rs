use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    AppliedFishingRodDurabilityState, FishingArea, FishingRodDurabilityResolution,
    FishingRodDurabilityStateError, apply_resolved_equipped_fishing_rod_durability,
};

const MUTATION_KEY: &str = "fishing:rod:durability";
const EVENT_KIND: &str = "FISHING_ROD_DURABILITY_APPLIED";

/// Canonical persisted single-event record for one operation's Rod consequence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FishingRodDurabilityMutationReceiptV1 {
    pub event_id: Uuid,
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub area: FishingArea,
    pub expected_current_durability: Option<u32>,
    pub resolution: FishingRodDurabilityResolution,
    pub applied: AppliedFishingRodDurabilityState,
}

#[derive(Debug, Error)]
pub enum FishingRodDurabilityMutationError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    State(#[from] FishingRodDurabilityStateError),
    #[error("owning operation does not exist")]
    OperationNotFound,
    #[error("owning operation belongs to another player")]
    OperationPlayerMismatch,
    #[error("owning operation cannot stage new Rod wear in state {0}")]
    OperationTerminal(String),
    #[error("stored Fishing Rod event receipt is malformed: {0}")]
    InvalidStoredEvent(#[from] serde_json::Error),
    #[error("the same operation has a conflicting Rod wear receipt or input")]
    MutationConflict,
}

/// Persist exactly one durability consequence per cast operation.
///
/// The operation row is locked first. Replaying an existing receipt is legal
/// after COMMITTED even if the Rod is subsequently repaired/unequipped; a mismatch
/// in area, expected state, resolution or event provenance fails closed. If no
/// receipt exists, only PENDING may call the existing authoritative Rod writer.
/// The mutation and event are in the same caller-owned PostgreSQL transaction.
/// Callers must abort their transaction on any error; this never finalizes the cast.
pub async fn apply_operation_fishing_rod_durability_v1(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
    area: FishingArea,
    expected_current_durability: Option<u32>,
    resolution: FishingRodDurabilityResolution,
) -> Result<FishingRodDurabilityMutationReceiptV1, FishingRodDurabilityMutationError> {
    let row = sqlx::query("SELECT player_id, state FROM operations WHERE id = $1 FOR UPDATE")
        .bind(operation_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(FishingRodDurabilityMutationError::OperationNotFound)?;
    let owner: Option<Uuid> = row.try_get("player_id")?;
    if owner != Some(player_id) {
        return Err(FishingRodDurabilityMutationError::OperationPlayerMismatch);
    }
    let state: String = row.try_get("state")?;

    if let Some(event) = sqlx::query(
        "SELECT id, player_id, event_kind, payload FROM asset_events WHERE operation_id = $1 AND mutation_key = $2 FOR UPDATE",
    )
    .bind(operation_id)
    .bind(MUTATION_KEY)
    .fetch_optional(&mut **tx)
    .await?
    {
        let event_id: Uuid = event.try_get("id")?;
        let recorded_player: Uuid = event.try_get("player_id")?;
        let kind: String = event.try_get("event_kind")?;
        let payload: Value = event.try_get("payload")?;
        let receipt: FishingRodDurabilityMutationReceiptV1 =
            serde_json::from_value(payload)?;
        if (state != "PENDING" && state != "COMMITTED")
            || event_id != receipt.event_id
            || recorded_player != player_id
            || kind != EVENT_KIND
            || receipt.operation_id != operation_id
            || receipt.player_id != player_id
            || receipt.area != area
            || receipt.expected_current_durability != expected_current_durability
            || receipt.resolution != resolution
        {
            return Err(FishingRodDurabilityMutationError::MutationConflict);
        }
        return Ok(receipt);
    }
    if state != "PENDING" {
        return Err(FishingRodDurabilityMutationError::OperationTerminal(state));
    }

    let applied = apply_resolved_equipped_fishing_rod_durability(
        tx,
        operation_id,
        player_id,
        area,
        expected_current_durability,
        resolution,
    )
    .await?;
    let receipt = FishingRodDurabilityMutationReceiptV1 {
        event_id: Uuid::now_v7(),
        operation_id,
        player_id,
        area,
        expected_current_durability,
        resolution,
        applied,
    };
    let payload = serde_json::to_value(&receipt)?;
    sqlx::query(
        "INSERT INTO asset_events (id, operation_id, mutation_key, player_id, event_kind, payload) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(receipt.event_id)
    .bind(operation_id)
    .bind(MUTATION_KEY)
    .bind(player_id)
    .bind(EVENT_KIND)
    .bind(payload)
    .execute(&mut **tx)
    .await?;
    Ok(receipt)
}
