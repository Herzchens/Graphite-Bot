use graphite_core::OperationId;
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    AppliedFishingRodDurabilityState, BAIT_RACK_MAX_LEVEL, FINISHED_ENCHANT_BOOK_DEFINITION_KEY,
    FINISHED_ENCHANT_BOOK_DEFINITION_VERSION, FISHING_BAIT_ITEM_DEFINITION_VERSION, FishingArea,
    FishingBait, FishingBaitCastConsumptionAction, FishingBaitCastSettlementReceipt,
    FishingDirectBookDeliveryReceiptV1, FishingFishInstanceDeliveryReceiptV1,
    FishingNonFishOutcomeDeliveryReceiptV1, FishingRodDurabilityMutationReceiptV1,
    FishingRodDurabilityResolution, MANUAL_FISHING_RNG_PPM_SCALE, MULTI_TREASURE_MAX_LEVEL,
    MULTICATCH_MAX_LEVEL, ManualFishingMulticatchCountV1,
    ManualFishingProgressionSettlementReceiptV1, ManualFishingSchoolBaitQuantityV1,
    ManualFishingSuccessfulProgressionOutcomeV1, ManualFishingUnbreakingXWearPreviewV1,
    MulticatchCount, SHARP_HOOK_MAX_LEVEL, STRENGTHEN_MAX_LEVEL,
    direct_fishing_book_pool_membership, direct_fishing_finished_book_level_weight,
    fishing_bait_item_definition_key, manual_fishing_successful_base_aexp_v1,
    multicatch_count_policy, resolve_school_bait_quantity,
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
    Book {
        deliveries: Vec<FishingDirectBookDeliveryReceiptV1>,
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
    TreasureX,
    BaitRack,
    Multicatch,
    MultiTreasure,
    SturdyBait,
    SchoolBait,
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
    Book {
        deliveries: Vec<FishingDirectBookDeliveryReceiptV1>,
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
    Book {
        deliveries: Vec<FishingDirectBookDeliveryReceiptV1>,
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
    Book {
        deliveries: Vec<FishingDirectBookDeliveryReceiptV1>,
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
    Book {
        deliveries: Vec<FishingDirectBookDeliveryReceiptV1>,
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "branch", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ManualFishingTreasureXTerminalOutcomeV1 {
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
    Book {
        deliveries: Vec<FishingDirectBookDeliveryReceiptV1>,
        progression: ManualFishingProgressionSettlementReceiptV1,
        durability: FishingRodDurabilityMutationReceiptV1,
    },
    LineBroken {
        durability: FishingRodDurabilityMutationReceiptV1,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ManualFishingTreasureXTerminalReceiptV1 {
    pub receipt_kind: ManualFishingTerminalReceiptKindV1,
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub area: FishingArea,
    pub outcome: ManualFishingTreasureXTerminalOutcomeV1,
}

#[derive(Debug, Error)]
pub enum ManualFishingTreasureXTerminalError {
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "branch", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ManualFishingModifierTerminalOutcomeV1 {
    Fish {
        deliveries: Vec<FishingFishInstanceDeliveryReceiptV1>,
        progression: ManualFishingProgressionSettlementReceiptV1,
        durability: FishingRodDurabilityMutationReceiptV1,
    },
    NonFish {
        delivery: FishingNonFishOutcomeDeliveryReceiptV1,
        progression: ManualFishingProgressionSettlementReceiptV1,
        durability: FishingRodDurabilityMutationReceiptV1,
    },
    Book {
        deliveries: Vec<FishingDirectBookDeliveryReceiptV1>,
        progression: ManualFishingProgressionSettlementReceiptV1,
        durability: FishingRodDurabilityMutationReceiptV1,
    },
    LineBroken {
        durability: FishingRodDurabilityMutationReceiptV1,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ManualFishingBaitRackTerminalReceiptV1 {
    pub receipt_kind: ManualFishingTerminalReceiptKindV1,
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub area: FishingArea,
    pub bait_rack_level: u8,
    pub outcome: ManualFishingModifierTerminalOutcomeV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ManualFishingMulticatchTerminalReceiptV1 {
    pub receipt_kind: ManualFishingTerminalReceiptKindV1,
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub area: FishingArea,
    pub multicatch_level: u8,
    pub multicatch_count: Option<ManualFishingMulticatchCountV1>,
    pub outcome: ManualFishingModifierTerminalOutcomeV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ManualFishingMultiTreasureTerminalReceiptV1 {
    pub receipt_kind: ManualFishingTerminalReceiptKindV1,
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub area: FishingArea,
    pub multi_treasure_level: u8,
    pub outcome: ManualFishingModifierTerminalOutcomeV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ManualFishingSturdyBaitTerminalReceiptV1 {
    pub receipt_kind: ManualFishingTerminalReceiptKindV1,
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub area: FishingArea,
    pub bait_settlement: FishingBaitCastSettlementReceipt,
    pub outcome: ManualFishingModifierTerminalOutcomeV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ManualFishingSchoolBaitTerminalReceiptV1 {
    pub receipt_kind: ManualFishingTerminalReceiptKindV1,
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub area: FishingArea,
    pub bait_settlement: FishingBaitCastSettlementReceipt,
    pub school_quantity: Option<ManualFishingSchoolBaitQuantityV1>,
    pub outcome: ManualFishingModifierTerminalOutcomeV1,
}

#[derive(Debug, Error)]
pub enum ManualFishingModifierTerminalError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("terminal Fishing receipt serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("terminal Fishing receipt contains invalid modifier provenance")]
    InvalidModifierProvenance,
    #[error("terminal Fishing receipt contains child receipts for another operation/player/area")]
    ChildIntegrityMismatch,
    #[error("terminal Fishing receipt contains a non-ordinary or wrong durability consequence")]
    DurabilityIntegrityMismatch,
    #[error("terminal Fishing receipt contains invalid bait settlement provenance")]
    BaitSettlementIntegrityMismatch,
    #[error("terminal Fishing receipt contains invalid quantity RNG provenance")]
    QuantityIntegrityMismatch,
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
        ManualFishingStrengthenTerminalOutcomeV1::Book {
            deliveries,
            progression,
            durability,
        } => {
            if !validate_direct_book_children_v1(
                receipt.operation_id,
                receipt.player_id,
                deliveries,
                progression,
                Some(1),
            ) {
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
        ManualFishingSharpHookTerminalOutcomeV1::Book {
            deliveries,
            progression,
            durability,
        } => {
            if !validate_direct_book_children_v1(
                receipt.operation_id,
                receipt.player_id,
                deliveries,
                progression,
                Some(1),
            ) {
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

pub(crate) async fn finalize_manual_fishing_treasure_x_v1(
    tx: &mut Transaction<'_, Postgres>,
    receipt: &ManualFishingTreasureXTerminalReceiptV1,
) -> Result<(), ManualFishingTreasureXTerminalError> {
    validate_treasure_x_child_integrity(receipt)?;

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
        return Err(ManualFishingTreasureXTerminalError::OperationTransitionConflict);
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

fn validate_treasure_x_child_integrity(
    receipt: &ManualFishingTreasureXTerminalReceiptV1,
) -> Result<(), ManualFishingTreasureXTerminalError> {
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
                return Err(ManualFishingTreasureXTerminalError::DurabilityIntegrityMismatch);
            }
            Ok(())
        };

    match &receipt.outcome {
        ManualFishingTreasureXTerminalOutcomeV1::Fish {
            delivery,
            progression,
            durability,
        } => {
            if !matches(delivery.operation_id, delivery.player_id)
                || !matches(progression.operation_id, progression.player_id)
            {
                return Err(ManualFishingTreasureXTerminalError::ChildIntegrityMismatch);
            }
            validate_durability(
                durability,
                FishingRodDurabilityResolution::CompletedCastAttempt {
                    ordinary_event_prevented_by_unbreaking: false,
                },
            )?;
        }
        ManualFishingTreasureXTerminalOutcomeV1::NonFish {
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
                return Err(ManualFishingTreasureXTerminalError::ChildIntegrityMismatch);
            }
            validate_durability(
                durability,
                FishingRodDurabilityResolution::CompletedCastAttempt {
                    ordinary_event_prevented_by_unbreaking: false,
                },
            )?;
        }
        ManualFishingTreasureXTerminalOutcomeV1::Book {
            deliveries,
            progression,
            durability,
        } => {
            if !validate_direct_book_children_v1(
                receipt.operation_id,
                receipt.player_id,
                deliveries,
                progression,
                Some(1),
            ) {
                return Err(ManualFishingTreasureXTerminalError::ChildIntegrityMismatch);
            }
            validate_durability(
                durability,
                FishingRodDurabilityResolution::CompletedCastAttempt {
                    ordinary_event_prevented_by_unbreaking: false,
                },
            )?;
        }
        ManualFishingTreasureXTerminalOutcomeV1::LineBroken { durability } => {
            validate_durability(durability, FishingRodDurabilityResolution::LineBreak)?;
        }
    }
    Ok(())
}

pub(crate) async fn finalize_manual_fishing_bait_rack_v1(
    tx: &mut Transaction<'_, Postgres>,
    receipt: &ManualFishingBaitRackTerminalReceiptV1,
) -> Result<(), ManualFishingModifierTerminalError> {
    if !(1..=BAIT_RACK_MAX_LEVEL).contains(&receipt.bait_rack_level) {
        return Err(ManualFishingModifierTerminalError::InvalidModifierProvenance);
    }
    validate_modifier_outcome_v1(
        receipt.operation_id,
        receipt.player_id,
        receipt.area,
        &receipt.outcome,
        Some(1),
        Some(1),
    )?;
    persist_modifier_terminal_v1(tx, receipt.operation_id, receipt.player_id, receipt).await
}

pub(crate) async fn finalize_manual_fishing_multicatch_v1(
    tx: &mut Transaction<'_, Postgres>,
    receipt: &ManualFishingMulticatchTerminalReceiptV1,
) -> Result<(), ManualFishingModifierTerminalError> {
    if !(1..=MULTICATCH_MAX_LEVEL).contains(&receipt.multicatch_level) {
        return Err(ManualFishingModifierTerminalError::InvalidModifierProvenance);
    }
    let expected_fish_count = match (&receipt.outcome, receipt.multicatch_count) {
        (
            ManualFishingModifierTerminalOutcomeV1::Fish { .. }
            | ManualFishingModifierTerminalOutcomeV1::LineBroken { .. },
            Some(count),
        ) => {
            validate_multicatch_count_v1(receipt.multicatch_level, count)?;
            Some(usize::from(count.fish_count))
        }
        (
            ManualFishingModifierTerminalOutcomeV1::NonFish { .. }
            | ManualFishingModifierTerminalOutcomeV1::Book { .. },
            None,
        ) => None,
        _ => return Err(ManualFishingModifierTerminalError::QuantityIntegrityMismatch),
    };
    validate_modifier_outcome_v1(
        receipt.operation_id,
        receipt.player_id,
        receipt.area,
        &receipt.outcome,
        expected_fish_count,
        Some(1),
    )?;
    persist_modifier_terminal_v1(tx, receipt.operation_id, receipt.player_id, receipt).await
}

pub(crate) async fn finalize_manual_fishing_multi_treasure_v1(
    tx: &mut Transaction<'_, Postgres>,
    receipt: &ManualFishingMultiTreasureTerminalReceiptV1,
) -> Result<(), ManualFishingModifierTerminalError> {
    if !(1..=MULTI_TREASURE_MAX_LEVEL).contains(&receipt.multi_treasure_level) {
        return Err(ManualFishingModifierTerminalError::InvalidModifierProvenance);
    }
    validate_modifier_outcome_v1(
        receipt.operation_id,
        receipt.player_id,
        receipt.area,
        &receipt.outcome,
        Some(1),
        None,
    )?;
    persist_modifier_terminal_v1(tx, receipt.operation_id, receipt.player_id, receipt).await
}

pub(crate) async fn finalize_manual_fishing_sturdy_bait_v1(
    tx: &mut Transaction<'_, Postgres>,
    receipt: &ManualFishingSturdyBaitTerminalReceiptV1,
) -> Result<(), ManualFishingModifierTerminalError> {
    validate_bait_settlement_v1(
        receipt.operation_id,
        receipt.player_id,
        &receipt.bait_settlement,
        FishingBait::Sturdy,
    )?;
    validate_modifier_outcome_v1(
        receipt.operation_id,
        receipt.player_id,
        receipt.area,
        &receipt.outcome,
        Some(1),
        Some(1),
    )?;
    persist_modifier_terminal_v1(tx, receipt.operation_id, receipt.player_id, receipt).await
}

pub(crate) async fn finalize_manual_fishing_school_bait_v1(
    tx: &mut Transaction<'_, Postgres>,
    receipt: &ManualFishingSchoolBaitTerminalReceiptV1,
) -> Result<(), ManualFishingModifierTerminalError> {
    validate_bait_settlement_v1(
        receipt.operation_id,
        receipt.player_id,
        &receipt.bait_settlement,
        FishingBait::School,
    )?;
    let expected_fish_count = match (&receipt.outcome, receipt.school_quantity) {
        (
            ManualFishingModifierTerminalOutcomeV1::Fish { .. }
            | ManualFishingModifierTerminalOutcomeV1::LineBroken { .. },
            Some(quantity),
        ) => {
            validate_school_quantity_v1(quantity)?;
            Some(usize::from(quantity.quantity.final_fish_count()))
        }
        (
            ManualFishingModifierTerminalOutcomeV1::NonFish { .. }
            | ManualFishingModifierTerminalOutcomeV1::Book { .. },
            None,
        ) => None,
        _ => return Err(ManualFishingModifierTerminalError::QuantityIntegrityMismatch),
    };
    validate_modifier_outcome_v1(
        receipt.operation_id,
        receipt.player_id,
        receipt.area,
        &receipt.outcome,
        expected_fish_count,
        Some(1),
    )?;
    persist_modifier_terminal_v1(tx, receipt.operation_id, receipt.player_id, receipt).await
}

fn validate_modifier_outcome_v1(
    operation_id: Uuid,
    player_id: Uuid,
    area: FishingArea,
    outcome: &ManualFishingModifierTerminalOutcomeV1,
    expected_fish_count: Option<usize>,
    expected_book_count: Option<usize>,
) -> Result<(), ManualFishingModifierTerminalError> {
    let matches = |child_operation_id: Uuid, child_player_id: Uuid| {
        child_operation_id == operation_id && child_player_id == player_id
    };
    let validate_durability =
        |durability: &FishingRodDurabilityMutationReceiptV1,
         expected: FishingRodDurabilityResolution| {
            if !matches(durability.operation_id, durability.player_id)
                || durability.area != area
                || durability.resolution != expected
                || !matches!(
                    &durability.applied,
                    AppliedFishingRodDurabilityState::Ordinary { .. }
                )
            {
                return Err(ManualFishingModifierTerminalError::DurabilityIntegrityMismatch);
            }
            Ok(())
        };

    match outcome {
        ManualFishingModifierTerminalOutcomeV1::Fish {
            deliveries,
            progression,
            durability,
        } => {
            let Some(expected_fish_count) = expected_fish_count else {
                return Err(ManualFishingModifierTerminalError::QuantityIntegrityMismatch);
            };
            if deliveries.len() != expected_fish_count
                || deliveries.is_empty()
                || deliveries
                    .iter()
                    .any(|delivery| !matches(delivery.operation_id, delivery.player_id))
                || !matches(progression.operation_id, progression.player_id)
            {
                return Err(ManualFishingModifierTerminalError::ChildIntegrityMismatch);
            }
            validate_durability(
                durability,
                FishingRodDurabilityResolution::CompletedCastAttempt {
                    ordinary_event_prevented_by_unbreaking: false,
                },
            )?;
        }
        ManualFishingModifierTerminalOutcomeV1::NonFish {
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
                return Err(ManualFishingModifierTerminalError::ChildIntegrityMismatch);
            }
            validate_durability(
                durability,
                FishingRodDurabilityResolution::CompletedCastAttempt {
                    ordinary_event_prevented_by_unbreaking: false,
                },
            )?;
        }
        ManualFishingModifierTerminalOutcomeV1::Book {
            deliveries,
            progression,
            durability,
        } => {
            if !validate_direct_book_children_v1(
                operation_id,
                player_id,
                deliveries,
                progression,
                expected_book_count,
            ) {
                return Err(ManualFishingModifierTerminalError::ChildIntegrityMismatch);
            }
            validate_durability(
                durability,
                FishingRodDurabilityResolution::CompletedCastAttempt {
                    ordinary_event_prevented_by_unbreaking: false,
                },
            )?;
        }
        ManualFishingModifierTerminalOutcomeV1::LineBroken { durability } => {
            let Some(_) = expected_fish_count else {
                return Err(ManualFishingModifierTerminalError::QuantityIntegrityMismatch);
            };
            validate_durability(durability, FishingRodDurabilityResolution::LineBreak)?;
        }
    }
    Ok(())
}

fn validate_multicatch_count_v1(
    level: u8,
    count: ManualFishingMulticatchCountV1,
) -> Result<(), ManualFishingModifierTerminalError> {
    if count.level != level || count.draw_ppm >= MANUAL_FISHING_RNG_PPM_SCALE {
        return Err(ManualFishingModifierTerminalError::QuantityIntegrityMismatch);
    }
    let outcomes = [
        MulticatchCount::Single,
        MulticatchCount::Double,
        MulticatchCount::Triple,
        MulticatchCount::Quadruple,
        MulticatchCount::Quintuple,
    ];
    let mut lower = 0_u32;
    for outcome in outcomes {
        let policy = multicatch_count_policy(level, outcome)
            .map_err(|_| ManualFishingModifierTerminalError::QuantityIntegrityMismatch)?;
        let upper = lower
            .checked_add(policy.probability_ppm)
            .ok_or(ManualFishingModifierTerminalError::QuantityIntegrityMismatch)?;
        if outcome == count.outcome {
            if count.fish_count != policy.fish_count
                || count.selected_probability_ppm != policy.probability_ppm
                || !(lower..upper).contains(&count.draw_ppm)
            {
                return Err(ManualFishingModifierTerminalError::QuantityIntegrityMismatch);
            }
            return Ok(());
        }
        lower = upper;
    }
    Err(ManualFishingModifierTerminalError::QuantityIntegrityMismatch)
}

fn validate_school_quantity_v1(
    quantity: ManualFishingSchoolBaitQuantityV1,
) -> Result<(), ManualFishingModifierTerminalError> {
    if quantity.draw_ppm >= MANUAL_FISHING_RNG_PPM_SCALE || quantity.chance_ppm != 80_000 {
        return Err(ManualFishingModifierTerminalError::QuantityIntegrityMismatch);
    }
    let expected_proc = if quantity.draw_ppm < quantity.chance_ppm {
        crate::SchoolBaitProcResolution::Triggered
    } else {
        crate::SchoolBaitProcResolution::NotTriggered
    };
    if quantity.proc_resolution != expected_proc {
        return Err(ManualFishingModifierTerminalError::QuantityIntegrityMismatch);
    }
    let expected_quantity =
        resolve_school_bait_quantity(crate::FishingCatchBranch::Fish, 1, expected_proc)
            .map_err(|_| ManualFishingModifierTerminalError::QuantityIntegrityMismatch)?;
    if quantity.quantity != expected_quantity {
        return Err(ManualFishingModifierTerminalError::QuantityIntegrityMismatch);
    }
    Ok(())
}

fn validate_bait_settlement_v1(
    operation_id: Uuid,
    player_id: Uuid,
    bait_settlement: &FishingBaitCastSettlementReceipt,
    expected_bait: FishingBait,
) -> Result<(), ManualFishingModifierTerminalError> {
    if bait_settlement.operation_id != operation_id
        || bait_settlement.player_id != player_id
        || bait_settlement.plan.bait_rack_level.is_some()
        || bait_settlement.plan.consumed_bait_categories != 1
        || usize::from(bait_settlement.plan.active_baits_before_cast)
            != bait_settlement.plan.entries.len()
        || usize::from(bait_settlement.plan.auto_detached_bait_categories)
            != bait_settlement.auto_detached.len()
        || usize::from(
            bait_settlement
                .plan
                .consumed_bait_categories
                .saturating_add(bait_settlement.plan.auto_detached_bait_categories),
        ) != bait_settlement.plan.entries.len()
        || bait_settlement.consumption_receipts.len() != 1
    {
        return Err(ManualFishingModifierTerminalError::BaitSettlementIntegrityMismatch);
    }
    let mut consumed = bait_settlement
        .plan
        .entries
        .iter()
        .filter(|entry| entry.action == FishingBaitCastConsumptionAction::ConsumeForCast);
    let Some(entry) = consumed.next() else {
        return Err(ManualFishingModifierTerminalError::BaitSettlementIntegrityMismatch);
    };
    let consumption = &bait_settlement.consumption_receipts[0];
    let expected_definition_key = fishing_bait_item_definition_key(expected_bait);
    if consumed.next().is_some()
        || entry.bait != expected_bait
        || entry.units_consumed != 1
        || entry.available_units_before <= 0
        || entry.available_units_after != entry.available_units_before - 1
        || consumption.operation_id != operation_id
        || consumption.player_id != player_id
        || consumption.mutation_key
            != format!(
                "fishing:bait:consume:{}",
                expected_bait_key_v1(expected_bait)
            )
        || consumption.definition_key != expected_definition_key
        || consumption.definition_version != FISHING_BAIT_ITEM_DEFINITION_VERSION
        || consumption.quantity != 1
        || consumption.quantity_before != entry.available_units_before
        || consumption.quantity_after != entry.available_units_after
        || bait_settlement.auto_detached.iter().any(|bait| {
            !bait_settlement.plan.entries.iter().any(|plan_entry| {
                plan_entry.bait == *bait
                    && plan_entry.action == FishingBaitCastConsumptionAction::AutoDetachMissingUnit
            })
        })
    {
        return Err(ManualFishingModifierTerminalError::BaitSettlementIntegrityMismatch);
    }
    Ok(())
}

fn expected_bait_key_v1(bait: FishingBait) -> &'static str {
    match bait {
        FishingBait::School => "school",
        FishingBait::Quality => "quality",
        FishingBait::Rare => "rare",
        FishingBait::Treasure => "treasure",
        FishingBait::Sturdy => "sturdy",
    }
}

async fn persist_modifier_terminal_v1<T: Serialize>(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
    receipt: &T,
) -> Result<(), ManualFishingModifierTerminalError> {
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
    .bind(operation_id)
    .bind(TERMINAL_MUTATION_KEY)
    .bind(player_id)
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
    .bind(operation_id)
    .bind(player_id)
    .execute(&mut **tx)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(ManualFishingModifierTerminalError::OperationTransitionConflict);
    }

    sqlx::query(
        r#"
        INSERT INTO outbox_events (id, operation_id, topic, payload)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (operation_id, topic) DO NOTHING
        "#,
    )
    .bind(OperationId::new().as_uuid())
    .bind(operation_id)
    .bind(TERMINAL_OUTBOX_TOPIC)
    .bind(payload)
    .execute(&mut **tx)
    .await?;
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

fn validate_direct_book_children_v1(
    operation_id: Uuid,
    player_id: Uuid,
    deliveries: &[FishingDirectBookDeliveryReceiptV1],
    progression: &ManualFishingProgressionSettlementReceiptV1,
    expected_count: Option<usize>,
) -> bool {
    if deliveries.is_empty() || deliveries.len() > 3 {
        return false;
    }
    if let Some(expected_count) = expected_count
        && deliveries.len() != expected_count
    {
        return false;
    }
    if progression.operation_id != operation_id || progression.player_id != player_id {
        return false;
    }
    let Ok(landed_count) = u8::try_from(deliveries.len()) else {
        return false;
    };
    let Ok(expected_aexp) = manual_fishing_successful_base_aexp_v1(
        &ManualFishingSuccessfulProgressionOutcomeV1::Treasure { landed_count },
    ) else {
        return false;
    };
    if progression.base_activity_xp != expected_aexp {
        return false;
    }

    for (slot, delivery) in deliveries.iter().enumerate() {
        let Ok(slot) = u8::try_from(slot) else {
            return false;
        };
        if delivery.operation_id != operation_id
            || delivery.player_id != player_id
            || delivery.candidate.treasure_slot != slot
            || delivery.delivery.operation_id != operation_id
            || delivery.delivery.player_id != player_id
            || delivery.delivery.mutation_key != format!("fishing:book:slot:{slot}")
            || delivery.delivery.definition_key != FINISHED_ENCHANT_BOOK_DEFINITION_KEY
            || delivery.delivery.definition_version != FINISHED_ENCHANT_BOOK_DEFINITION_VERSION
            || delivery.delivery.pending != delivery.delivery.pending_delivery_id.is_some()
            || direct_fishing_book_pool_membership(delivery.candidate.enchant)
                != Some(delivery.candidate.pool)
            || delivery.candidate.level == 0
            || direct_fishing_finished_book_level_weight(
                delivery.candidate.enchant,
                delivery.candidate.level,
            )
            .map_or(true, |weight| weight == 0)
        {
            return false;
        }
    }
    true
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
        ManualFishingUnbreakingXTerminalOutcomeV1::Book {
            deliveries,
            progression,
            wear,
            durability,
        } => {
            if !validate_direct_book_children_v1(
                receipt.operation_id,
                receipt.player_id,
                deliveries,
                progression,
                Some(1),
            ) {
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
        ManualFishingUnmodifiedOrdinaryTerminalOutcomeV1::Book {
            deliveries,
            progression,
            durability,
        } => {
            if !validate_direct_book_children_v1(
                receipt.operation_id,
                receipt.player_id,
                deliveries,
                progression,
                Some(1),
            ) {
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
        ManualFishingStarterBaseTerminalOutcomeV1::Book {
            deliveries,
            progression,
        } => {
            if !validate_direct_book_children_v1(
                receipt.operation_id,
                receipt.player_id,
                deliveries,
                progression,
                Some(1),
            ) {
                return Err(ManualFishingStarterBaseTerminalError::ChildIntegrityMismatch);
            }
        }
    }
    Ok(())
}
