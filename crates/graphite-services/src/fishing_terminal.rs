use graphite_core::OperationId;
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    FishingArea, FishingFishInstanceDeliveryReceiptV1, FishingNonFishOutcomeDeliveryReceiptV1,
    ManualFishingProgressionSettlementReceiptV1,
};

const TERMINAL_MUTATION_KEY: &str = "fishing:cast:terminal";
const TERMINAL_EVENT_KIND: &str = "MANUAL_FISHING_CAST_COMMITTED";
const TERMINAL_OUTBOX_TOPIC: &str = "fishing.cast.completed";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "branch", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ManualFishingStarterBaseTerminalOutcomeV1 {
    Fish {
        delivery: FishingFishInstanceDeliveryReceiptV1,
        progression: ManualFishingProgressionSettlementReceiptV1,
    },
    NonFish {
        delivery: FishingNonFishOutcomeDeliveryReceiptV1,
        progression: ManualFishingProgressionSettlementReceiptV1,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ManualFishingStarterBaseTerminalReceiptV1 {
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub area: FishingArea,
    pub outcome: ManualFishingStarterBaseTerminalOutcomeV1,
}

#[derive(Debug, Error)]
pub enum ManualFishingStarterBaseTerminalError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("terminal Fishing receipt serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Starter Basic terminal receipt requires Starter Pool")]
    WrongArea,
    #[error("terminal Fishing receipt contains child receipts for another operation or player")]
    ChildIntegrityMismatch,
    #[error("manual Fishing operation was not PENDING at terminal transition")]
    OperationTransitionConflict,
}

pub(crate) async fn finalize_manual_fishing_starter_base_v1(
    tx: &mut Transaction<'_, Postgres>,
    receipt: &ManualFishingStarterBaseTerminalReceiptV1,
) -> Result<(), ManualFishingStarterBaseTerminalError> {
    if receipt.area != FishingArea::StarterPool {
        return Err(ManualFishingStarterBaseTerminalError::WrongArea);
    }
    validate_child_integrity(receipt)?;

    let payload = serde_json::to_value(receipt)?;
    sqlx::query(
        r#"
        INSERT INTO asset_events (
            id, operation_id, mutation_key, player_id, event_kind, payload
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(OperationId::new().as_uuid())
    .bind(receipt.operation_id)
    .bind(TERMINAL_MUTATION_KEY)
    .bind(receipt.player_id)
    .bind(TERMINAL_EVENT_KIND)
    .bind(payload.clone())
    .execute(&mut **tx)
    .await?;

    let updated = sqlx::query(
        r#"
        UPDATE operations
           SET state = 'COMMITTED',
               result = $1,
               committed_at = clock_timestamp()
         WHERE id = $2
           AND player_id = $3
           AND state = 'PENDING'
        "#,
    )
    .bind(payload.clone())
    .bind(receipt.operation_id)
    .bind(receipt.player_id)
    .execute(&mut **tx)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(ManualFishingStarterBaseTerminalError::OperationTransitionConflict);
    }

    sqlx::query(
        r#"
        INSERT INTO outbox_events (id, operation_id, topic, payload)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (operation_id, topic) DO NOTHING
        "#,
    )
    .bind(OperationId::new().as_uuid())
    .bind(receipt.operation_id)
    .bind(TERMINAL_OUTBOX_TOPIC)
    .bind(payload)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn validate_child_integrity(
    receipt: &ManualFishingStarterBaseTerminalReceiptV1,
) -> Result<(), ManualFishingStarterBaseTerminalError> {
    let matches = |operation_id: Uuid, player_id: Uuid| {
        operation_id == receipt.operation_id && player_id == receipt.player_id
    };
    match &receipt.outcome {
        ManualFishingStarterBaseTerminalOutcomeV1::Fish {
            delivery,
            progression,
        } => {
            if !matches(delivery.operation_id, delivery.player_id)
                || !matches(progression.operation_id, progression.player_id)
            {
                return Err(ManualFishingStarterBaseTerminalError::ChildIntegrityMismatch);
            }
        }
        ManualFishingStarterBaseTerminalOutcomeV1::NonFish {
            delivery,
            progression,
        } => {
            if !matches(progression.operation_id, progression.player_id)
                || delivery
                    .deliveries
                    .iter()
                    .any(|row| !matches(row.delivery.operation_id, row.delivery.player_id))
            {
                return Err(ManualFishingStarterBaseTerminalError::ChildIntegrityMismatch);
            }
        }
    }
    Ok(())
}
