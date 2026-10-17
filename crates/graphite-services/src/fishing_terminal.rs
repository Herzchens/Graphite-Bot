use graphite_core::OperationId;
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    AppliedFishingRodDurabilityState, FishingArea, FishingFishInstanceDeliveryReceiptV1,
    FishingNonFishOutcomeDeliveryReceiptV1, FishingRodDurabilityMutationReceiptV1,
    FishingRodDurabilityResolution, ManualFishingProgressionSettlementReceiptV1,
    ManualFishingUnbreakingXWearPreviewV1, SHARP_HOOK_MAX_LEVEL, STRENGTHEN_MAX_LEVEL,
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ManualFishingTerminalReceiptKindV1 {
    UnmodifiedOrdinary,
    UnbreakingX,
    Strengthen,
    SharpHook,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "branch", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ManualFishingUnmodifiedOrdinaryTerminalOutcomeV1 {
    Fish {
        delivery: FishingFishInstanceDeliveryReceiptV1,
        progression: ManualFishingProgressionSettlementReceiptV1,
        durability: FishingRodDurabilityMutationReceiptV1,
    },
    NonFish {
        delivery: FishingNonFishOutcomeDeliveryReceiptV1,
        progression: ManualFishingProgressionSettlementReceiptV1,
        durability: FishingRodDurabilityMutationReceiptV1,
    },
    LineBroken {
        durability: FishingRodDurabilityMutationReceiptV1,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ManualFishingUnmodifiedOrdinaryTerminalReceiptV1 {
    pub receipt_kind: ManualFishingTerminalReceiptKindV1,
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub area: FishingArea,
    pub outcome: ManualFishingUnmodifiedOrdinaryTerminalOutcomeV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "branch", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ManualFishingUnbreakingXTerminalOutcomeV1 {
    Fish {
        delivery: FishingFishInstanceDeliveryReceiptV1,
        progression: ManualFishingProgressionSettlementReceiptV1,
        wear: ManualFishingUnbreakingXWearPreviewV1,
        durability: FishingRodDurabilityMutationReceiptV1,
    },
    NonFish {
        delivery: FishingNonFishOutcomeDeliveryReceiptV1,
        progression: ManualFishingProgressionSettlementReceiptV1,
        wear: ManualFishingUnbreakingXWearPreviewV1,
        durability: FishingRodDurabilityMutationReceiptV1,
    },
    LineBroken {
        durability: FishingRodDurabilityMutationReceiptV1,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ManualFishingUnbreakingXTerminalReceiptV1 {
    pub receipt_kind: ManualFishingTerminalReceiptKindV1,
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub area: FishingArea,
    pub outcome: ManualFishingUnbreakingXTerminalOutcomeV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "branch", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ManualFishingStrengthenTerminalOutcomeV1 {
    Fish {
        delivery: FishingFishInstanceDeliveryReceiptV1,
        progression: ManualFishingProgressionSettlementReceiptV1,
        durability: FishingRodDurabilityMutationReceiptV1,
    },
    NonFish {
        delivery: FishingNonFishOutcomeDeliveryReceiptV1,
        progression: ManualFishingProgressionSettlementReceiptV1,
        durability: FishingRodDurabilityMutationReceiptV1,
    },
    LineBroken {
        durability: FishingRodDurabilityMutationReceiptV1,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ManualFishingStrengthenTerminalReceiptV1 {
    pub receipt_kind: ManualFishingTerminalReceiptKindV1,
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub area: FishingArea,
    pub strengthen_level: u8,
    pub outcome: ManualFishingStrengthenTerminalOutcomeV1,
}

#[derive(Debug, Error)]
pub enum ManualFishingStrengthenTerminalError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("terminal Fishing receipt serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("terminal Fishing receipt contains an invalid Strengthen level")]
    InvalidStrengthenLevel,
    #[error("terminal Fishing receipt contains child receipts for another operation/player/area")]
    ChildIntegrityMismatch,
    #[error("terminal Fishing receipt contains a non-ordinary or wrong durability consequence")]
    DurabilityIntegrityMismatch,
    #[error("manual Fishing operation was not PENDING at terminal transition")]
    OperationTransitionConflict,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "branch", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ManualFishingSharpHookTerminalOutcomeV1 {
    Fish {
        delivery: FishingFishInstanceDeliveryReceiptV1,
        progression: ManualFishingProgressionSettlementReceiptV1,
        durability: FishingRodDurabilityMutationReceiptV1,
    },
    NonFish {
        delivery: FishingNonFishOutcomeDeliveryReceiptV1,
        progression: ManualFishingProgressionSettlementReceiptV1,
        durability: FishingRodDurabilityMutationReceiptV1,
    },
    LineBroken {
        durability: FishingRodDurabilityMutationReceiptV1,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ManualFishingSharpHookTerminalReceiptV1 {
    pub receipt_kind: ManualFishingTerminalReceiptKindV1,
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub area: FishingArea,
    pub sharp_hook_level: u8,
    pub outcome: ManualFishingSharpHookTerminalOutcomeV1,
}

#[derive(Debug, Error)]
pub enum ManualFishingSharpHookTerminalError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("terminal Fishing receipt serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("terminal Fishing receipt contains an invalid SharpHook level")]
    InvalidSharpHookLevel,
    #[error("terminal Fishing receipt contains child receipts for another operation/player/area")]
    ChildIntegrityMismatch,
    #[error("terminal Fishing receipt contains a non-ordinary or wrong durability consequence")]
    DurabilityIntegrityMismatch,
    #[error("manual Fishing operation was not PENDING at terminal transition")]
    OperationTransitionConflict,
}

#[derive(Debug, Error)]
pub enum ManualFishingUnbreakingXTerminalError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("terminal Fishing receipt serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("terminal Fishing receipt contains child receipts for another operation/player/area")]
    ChildIntegrityMismatch,
    #[error("terminal Fishing receipt contains invalid Unbreaking X wear or durability state")]
    DurabilityIntegrityMismatch,
    #[error("manual Fishing operation was not PENDING at terminal transition")]
    OperationTransitionConflict,
}

#[derive(Debug, Error)]
pub enum ManualFishingUnmodifiedOrdinaryTerminalError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("terminal Fishing receipt serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("terminal Fishing receipt contains child receipts for another operation/player/area")]
    ChildIntegrityMismatch,
    #[error("terminal Fishing receipt contains a non-ordinary or wrong durability consequence")]
    DurabilityIntegrityMismatch,
    #[error("manual Fishing operation was not PENDING at terminal transition")]
    OperationTransitionConflict,
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

pub(crate) async fn finalize_manual_fishing_unmodified_ordinary_v1(
    tx: &mut Transaction<'_, Postgres>,
    receipt: &ManualFishingUnmodifiedOrdinaryTerminalReceiptV1,
) -> Result<(), ManualFishingUnmodifiedOrdinaryTerminalError> {
    validate_unmodified_ordinary_child_integrity(receipt)?;

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
        return Err(ManualFishingUnmodifiedOrdinaryTerminalError::OperationTransitionConflict);
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

pub(crate) async fn finalize_manual_fishing_strengthen_v1(
    tx: &mut Transaction<'_, Postgres>,
    receipt: &ManualFishingStrengthenTerminalReceiptV1,
) -> Result<(), ManualFishingStrengthenTerminalError> {
    validate_strengthen_child_integrity(receipt)?;

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
        return Err(ManualFishingStrengthenTerminalError::OperationTransitionConflict);
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

fn validate_strengthen_child_integrity(
    receipt: &ManualFishingStrengthenTerminalReceiptV1,
) -> Result<(), ManualFishingStrengthenTerminalError> {
    if !(1..=STRENGTHEN_MAX_LEVEL).contains(&receipt.strengthen_level) {
        return Err(ManualFishingStrengthenTerminalError::InvalidStrengthenLevel);
    }
    let matches = |operation_id: Uuid, player_id: Uuid| {
        operation_id == receipt.operation_id && player_id == receipt.player_id
    };
    let validate_durability =
        |durability: &FishingRodDurabilityMutationReceiptV1,
         expected: FishingRodDurabilityResolution| {
            if !matches(durability.operation_id, durability.player_id)
                || durability.area != receipt.area
                || durability.resolution != expected
                || !matches!(
                    &durability.applied,
                    AppliedFishingRodDurabilityState::Ordinary { .. }
                )
            {
                return Err(ManualFishingStrengthenTerminalError::DurabilityIntegrityMismatch);
            }
            Ok(())
        };

    match &receipt.outcome {
        ManualFishingStrengthenTerminalOutcomeV1::Fish {
            delivery,
            progression,
            durability,
        } => {
            if !matches(delivery.operation_id, delivery.player_id)
                || !matches(progression.operation_id, progression.player_id)
            {
                return Err(ManualFishingStrengthenTerminalError::ChildIntegrityMismatch);
            }
            validate_durability(
                durability,
                FishingRodDurabilityResolution::CompletedCastAttempt {
                    ordinary_event_prevented_by_unbreaking: false,
                },
            )?;
        }
        ManualFishingStrengthenTerminalOutcomeV1::NonFish {
            delivery,
            progression,
            durability,
        } => {
            if !matches(progression.operation_id, progression.player_id)
                || delivery
                    .deliveries
                    .iter()
                    .any(|row| !matches(row.delivery.operation_id, row.delivery.player_id))
            {
                return Err(ManualFishingStrengthenTerminalError::ChildIntegrityMismatch);
            }
            validate_durability(
                durability,
                FishingRodDurabilityResolution::CompletedCastAttempt {
                    ordinary_event_prevented_by_unbreaking: false,
                },
            )?;
        }
        ManualFishingStrengthenTerminalOutcomeV1::LineBroken { durability } => {
            validate_durability(durability, FishingRodDurabilityResolution::LineBreak)?;
        }
    }
    Ok(())
}

pub(crate) async fn finalize_manual_fishing_sharp_hook_v1(
    tx: &mut Transaction<'_, Postgres>,
    receipt: &ManualFishingSharpHookTerminalReceiptV1,
) -> Result<(), ManualFishingSharpHookTerminalError> {
    validate_sharp_hook_child_integrity(receipt)?;

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
        return Err(ManualFishingSharpHookTerminalError::OperationTransitionConflict);
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

fn validate_sharp_hook_child_integrity(
    receipt: &ManualFishingSharpHookTerminalReceiptV1,
) -> Result<(), ManualFishingSharpHookTerminalError> {
    if !(1..=SHARP_HOOK_MAX_LEVEL).contains(&receipt.sharp_hook_level) {
        return Err(ManualFishingSharpHookTerminalError::InvalidSharpHookLevel);
    }
    let matches = |operation_id: Uuid, player_id: Uuid| {
        operation_id == receipt.operation_id && player_id == receipt.player_id
    };
    let validate_durability =
        |durability: &FishingRodDurabilityMutationReceiptV1,
         expected: FishingRodDurabilityResolution| {
            if !matches(durability.operation_id, durability.player_id)
                || durability.area != receipt.area
                || durability.resolution != expected
                || !matches!(
                    &durability.applied,
                    AppliedFishingRodDurabilityState::Ordinary { .. }
                )
            {
                return Err(ManualFishingSharpHookTerminalError::DurabilityIntegrityMismatch);
            }
            Ok(())
        };

    match &receipt.outcome {
        ManualFishingSharpHookTerminalOutcomeV1::Fish {
            delivery,
            progression,
            durability,
        } => {
            if !matches(delivery.operation_id, delivery.player_id)
                || !matches(progression.operation_id, progression.player_id)
            {
                return Err(ManualFishingSharpHookTerminalError::ChildIntegrityMismatch);
            }
            validate_durability(
                durability,
                FishingRodDurabilityResolution::CompletedCastAttempt {
                    ordinary_event_prevented_by_unbreaking: false,
                },
            )?;
        }
        ManualFishingSharpHookTerminalOutcomeV1::NonFish {
            delivery,
            progression,
            durability,
        } => {
            if !matches(progression.operation_id, progression.player_id)
                || delivery
                    .deliveries
                    .iter()
                    .any(|row| !matches(row.delivery.operation_id, row.delivery.player_id))
            {
                return Err(ManualFishingSharpHookTerminalError::ChildIntegrityMismatch);
            }
            validate_durability(
                durability,
                FishingRodDurabilityResolution::CompletedCastAttempt {
                    ordinary_event_prevented_by_unbreaking: false,
                },
            )?;
        }
        ManualFishingSharpHookTerminalOutcomeV1::LineBroken { durability } => {
            validate_durability(durability, FishingRodDurabilityResolution::LineBreak)?;
        }
    }
    Ok(())
}

pub(crate) async fn finalize_manual_fishing_unbreaking_x_v1(
    tx: &mut Transaction<'_, Postgres>,
    receipt: &ManualFishingUnbreakingXTerminalReceiptV1,
) -> Result<(), ManualFishingUnbreakingXTerminalError> {
    validate_unbreaking_x_child_integrity(receipt)?;

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
        return Err(ManualFishingUnbreakingXTerminalError::OperationTransitionConflict);
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

fn validate_unbreaking_x_child_integrity(
    receipt: &ManualFishingUnbreakingXTerminalReceiptV1,
) -> Result<(), ManualFishingUnbreakingXTerminalError> {
    let matches = |operation_id: Uuid, player_id: Uuid| {
        operation_id == receipt.operation_id && player_id == receipt.player_id
    };
    let validate_durability =
        |durability: &FishingRodDurabilityMutationReceiptV1,
         wear: Option<ManualFishingUnbreakingXWearPreviewV1>| {
            if !matches(durability.operation_id, durability.player_id)
                || durability.area != receipt.area
                || !matches!(
                    &durability.applied,
                    AppliedFishingRodDurabilityState::Ordinary { .. }
                )
            {
                return Err(ManualFishingUnbreakingXTerminalError::DurabilityIntegrityMismatch);
            }
            match wear {
                Some(wear) => {
                    if wear.chance_ppm != 200_000
                        || wear.prevented != (wear.draw_ppm < wear.chance_ppm)
                        || durability.resolution
                            != (FishingRodDurabilityResolution::CompletedCastAttempt {
                                ordinary_event_prevented_by_unbreaking: wear.prevented,
                            })
                    {
                        return Err(
                            ManualFishingUnbreakingXTerminalError::DurabilityIntegrityMismatch,
                        );
                    }
                }
                None => {
                    if durability.resolution != FishingRodDurabilityResolution::LineBreak {
                        return Err(
                            ManualFishingUnbreakingXTerminalError::DurabilityIntegrityMismatch,
                        );
                    }
                }
            }
            Ok(())
        };

    match &receipt.outcome {
        ManualFishingUnbreakingXTerminalOutcomeV1::Fish {
            delivery,
            progression,
            wear,
            durability,
        } => {
            if !matches(delivery.operation_id, delivery.player_id)
                || !matches(progression.operation_id, progression.player_id)
            {
                return Err(ManualFishingUnbreakingXTerminalError::ChildIntegrityMismatch);
            }
            validate_durability(durability, Some(*wear))?;
        }
        ManualFishingUnbreakingXTerminalOutcomeV1::NonFish {
            delivery,
            progression,
            wear,
            durability,
        } => {
            if !matches(progression.operation_id, progression.player_id)
                || delivery
                    .deliveries
                    .iter()
                    .any(|row| !matches(row.delivery.operation_id, row.delivery.player_id))
            {
                return Err(ManualFishingUnbreakingXTerminalError::ChildIntegrityMismatch);
            }
            validate_durability(durability, Some(*wear))?;
        }
        ManualFishingUnbreakingXTerminalOutcomeV1::LineBroken { durability } => {
            validate_durability(durability, None)?;
        }
    }
    Ok(())
}

fn validate_unmodified_ordinary_child_integrity(
    receipt: &ManualFishingUnmodifiedOrdinaryTerminalReceiptV1,
) -> Result<(), ManualFishingUnmodifiedOrdinaryTerminalError> {
    let matches = |operation_id: Uuid, player_id: Uuid| {
        operation_id == receipt.operation_id && player_id == receipt.player_id
    };
    let validate_durability =
        |durability: &FishingRodDurabilityMutationReceiptV1,
         expected_resolution: FishingRodDurabilityResolution| {
            if !matches(durability.operation_id, durability.player_id)
                || durability.area != receipt.area
                || durability.resolution != expected_resolution
                || !matches!(
                    &durability.applied,
                    AppliedFishingRodDurabilityState::Ordinary { .. }
                )
            {
                return Err(
                    ManualFishingUnmodifiedOrdinaryTerminalError::DurabilityIntegrityMismatch,
                );
            }
            Ok(())
        };

    match &receipt.outcome {
        ManualFishingUnmodifiedOrdinaryTerminalOutcomeV1::Fish {
            delivery,
            progression,
            durability,
        } => {
            if !matches(delivery.operation_id, delivery.player_id)
                || !matches(progression.operation_id, progression.player_id)
            {
                return Err(ManualFishingUnmodifiedOrdinaryTerminalError::ChildIntegrityMismatch);
            }
            validate_durability(
                durability,
                FishingRodDurabilityResolution::CompletedCastAttempt {
                    ordinary_event_prevented_by_unbreaking: false,
                },
            )?;
        }
        ManualFishingUnmodifiedOrdinaryTerminalOutcomeV1::NonFish {
            delivery,
            progression,
            durability,
        } => {
            if !matches(progression.operation_id, progression.player_id)
                || delivery
                    .deliveries
                    .iter()
                    .any(|row| !matches(row.delivery.operation_id, row.delivery.player_id))
            {
                return Err(ManualFishingUnmodifiedOrdinaryTerminalError::ChildIntegrityMismatch);
            }
            validate_durability(
                durability,
                FishingRodDurabilityResolution::CompletedCastAttempt {
                    ordinary_event_prevented_by_unbreaking: false,
                },
            )?;
        }
        ManualFishingUnmodifiedOrdinaryTerminalOutcomeV1::LineBroken { durability } => {
            validate_durability(durability, FishingRodDurabilityResolution::LineBreak)?;
        }
    }
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
