use graphite_items::ItemError;
use serde::Serialize;
use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::fishing_rod_state::{
    EquippedFishingRodKind, EquippedFishingRodStateError, lock_equipped_fishing_rod_state,
};
use crate::{
    FishingArea, FishingRodDurabilityPolicyError, FishingRodDurabilityPreview,
    FishingRodDurabilityResolution, preview_fishing_rod_durability,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AppliedFishingRodDurabilityState {
    StarterBasicUnbreakable {
        item_instance_id: Uuid,
        definition_key: String,
        definition_version: i32,
    },
    Ordinary {
        item_instance_id: Uuid,
        definition_key: String,
        definition_version: i32,
        preview: FishingRodDurabilityPreview,
    },
}

#[derive(Debug, Error)]
pub enum FishingRodDurabilityStateError {
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error(transparent)]
    Item(#[from] ItemError),
    #[error(transparent)]
    Policy(#[from] FishingRodDurabilityPolicyError),
    #[error("owning operation does not exist")]
    OperationNotFound,
    #[error("owning operation targets a different player")]
    OperationPlayerMismatch,
    #[error("owning operation cannot mutate Fishing Rod durability in state {0}")]
    OperationTerminal(String),
    #[error("player does not exist")]
    PlayerNotFound,
    #[error("Fishing Rod durability mutation requires an ACTIVE account; current status is {0}")]
    AccountFrozen(String),
    #[error("no Fishing Rod is currently equipped")]
    NoEquippedFishingRod,
    #[error("equipped Fishing Rod state is internally inconsistent")]
    EquippedRodIntegrityMismatch,
    #[error("Starter Basic Rod identity/state is internally inconsistent")]
    StarterBasicRodIntegrityMismatch,
    #[error("the equipped Fishing Rod is not classified as ordinary equipment")]
    NonOrdinaryFishingRod,
    #[error("ordinary Fishing Rod durability state is missing or outside the supported range")]
    InvalidOrdinaryRodDurability,
    #[error("ordinary Fishing Rod is already Broken and cannot begin a cast durability event")]
    OrdinaryRodAlreadyBroken,
    #[error(
        "resolved Fishing Rod durability expected current durability {expected}, but authoritative durability is {actual}"
    )]
    DurabilityChanged { expected: u32, actual: u32 },
    #[error("Starter Basic Rod may only be used in Starter Pool")]
    StarterBasicOutsideStarterPool,
    #[error("line break is disabled in Starter Pool")]
    LineBreakDisabledInStarterPool,
    #[error("Starter Basic Rod has no mutable durability value")]
    StarterBasicExpectedDurability,
    #[error("ordinary Fishing Rod requires an expected current durability value")]
    OrdinaryRodExpectedDurabilityMissing,
    #[error("the locked Fishing Rod durability row changed unexpectedly before write")]
    LockedStateMismatch,
}

impl From<sqlx::Error> for FishingRodDurabilityStateError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

/// Applies one already-resolved durability consequence to the currently equipped Fishing Rod inside
/// a caller-owned gameplay transaction.
///
/// This is deliberately a transaction-composable state primitive, not the `/fish` owner. It locks in
/// `operation -> player -> ItemInstance -> equipment slot` order, requires the owning operation to be
/// PENDING for exactly this player, and re-resolves the currently equipped Rod through the shared
/// authoritative Rod-state bridge. The ItemInstance is classified from the exact immutable
/// ItemDefinition version pinned by that instance; request-provided Rod identity or ordinary/special
/// flags are never trusted.
///
/// `expected_current_durability` is the optimistic state token from the caller's already-resolved
/// cast snapshot. Ordinary Rods compare it against the locked authoritative value before applying the
/// pure durability policy, so a stale/re-entered PENDING operation cannot silently consume a second
/// durability point. The higher-level operation owner must still intercept COMMITTED replay before
/// re-entering this primitive and must finalize its operation/result/outbox in the same transaction.
///
/// Starter Basic is the separate system-bound unbreakable Rod. It is accepted only in Starter Pool,
/// requires the canonical NULL durability representation, and produces an explicit no-op receipt.
/// Starter Pool rejects a line-break consequence for every Rod because that event is disabled there.
/// Ordinary Rods must be operational (`current_durability > 0` and not Broken) before a cast event can
/// be applied. Reaching zero durability, including through line break, atomically sets `is_broken`.
///
/// This primitive does not calculate line-break probability, draw RNG, decide Unbreaking, consume
/// bait, apply Mending, grant AEXP, settle CatchBag output, finalize the operation, emit asset/audit
/// events, or expose a command. Rolling back the surrounding transaction rolls back the durability
/// mutation as well.
pub async fn apply_resolved_equipped_fishing_rod_durability(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
    area: FishingArea,
    expected_current_durability: Option<u32>,
    resolution: FishingRodDurabilityResolution,
) -> Result<AppliedFishingRodDurabilityState, FishingRodDurabilityStateError> {
    lock_pending_player_operation(tx, operation_id, player_id).await?;
    lock_active_player(tx, player_id).await?;
    let rod = lock_equipped_fishing_rod_state(tx, player_id)
        .await
        .map_err(map_equipped_rod_state_error)?;

    if matches!(resolution, FishingRodDurabilityResolution::LineBreak)
        && area == FishingArea::StarterPool
    {
        return Err(FishingRodDurabilityStateError::LineBreakDisabledInStarterPool);
    }

    if rod.kind == EquippedFishingRodKind::StarterBasic {
        if !rod.is_starter
            || !rod.is_unbreakable
            || rod.is_repairable
            || rod.current_durability.is_some()
            || rod.max_durability.is_some()
            || rod.is_broken
        {
            return Err(FishingRodDurabilityStateError::StarterBasicRodIntegrityMismatch);
        }
        if area != FishingArea::StarterPool {
            return Err(FishingRodDurabilityStateError::StarterBasicOutsideStarterPool);
        }
        if expected_current_durability.is_some() {
            return Err(FishingRodDurabilityStateError::StarterBasicExpectedDurability);
        }
        return Ok(AppliedFishingRodDurabilityState::StarterBasicUnbreakable {
            item_instance_id: rod.item_instance_id,
            definition_key: rod.definition_key,
            definition_version: rod.definition_version,
        });
    }

    let current = rod
        .current_durability
        .and_then(|value| u32::try_from(value).ok())
        .ok_or(FishingRodDurabilityStateError::InvalidOrdinaryRodDurability)?;
    let maximum = rod
        .max_durability
        .and_then(|value| u32::try_from(value).ok())
        .ok_or(FishingRodDurabilityStateError::InvalidOrdinaryRodDurability)?;
    if current == 0 || rod.is_broken {
        return Err(FishingRodDurabilityStateError::OrdinaryRodAlreadyBroken);
    }
    let expected = expected_current_durability
        .ok_or(FishingRodDurabilityStateError::OrdinaryRodExpectedDurabilityMissing)?;
    if expected != current {
        return Err(FishingRodDurabilityStateError::DurabilityChanged {
            expected,
            actual: current,
        });
    }

    let preview = preview_fishing_rod_durability(current, maximum, true, resolution)?;
    if preview.resulting_durability != current {
        let result = sqlx::query(
            r#"
            UPDATE item_instances
               SET current_durability = $1,
                   is_broken = $2
             WHERE id = $3
               AND owner_player_id = $4
               AND location = 'EQUIPPED'
               AND current_durability = $5
               AND max_durability = $6
               AND is_broken = FALSE
            "#,
        )
        .bind(i64::from(preview.resulting_durability))
        .bind(preview.resulting_durability == 0)
        .bind(rod.item_instance_id)
        .bind(player_id)
        .bind(i64::from(current))
        .bind(i64::from(maximum))
        .execute(&mut **tx)
        .await?;
        if result.rows_affected() != 1 {
            return Err(FishingRodDurabilityStateError::LockedStateMismatch);
        }
    }

    Ok(AppliedFishingRodDurabilityState::Ordinary {
        item_instance_id: rod.item_instance_id,
        definition_key: rod.definition_key,
        definition_version: rod.definition_version,
        preview,
    })
}

fn map_equipped_rod_state_error(error: EquippedFishingRodStateError) -> FishingRodDurabilityStateError {
    match error {
        EquippedFishingRodStateError::Database(error) => {
            FishingRodDurabilityStateError::Database(error)
        }
        EquippedFishingRodStateError::Item(error) => FishingRodDurabilityStateError::Item(error),
        EquippedFishingRodStateError::NoEquippedFishingRod => {
            FishingRodDurabilityStateError::NoEquippedFishingRod
        }
        EquippedFishingRodStateError::EquippedRodIntegrityMismatch => {
            FishingRodDurabilityStateError::EquippedRodIntegrityMismatch
        }
        EquippedFishingRodStateError::StarterBasicRodIntegrityMismatch => {
            FishingRodDurabilityStateError::StarterBasicRodIntegrityMismatch
        }
        EquippedFishingRodStateError::NonOrdinaryFishingRod => {
            FishingRodDurabilityStateError::NonOrdinaryFishingRod
        }
        EquippedFishingRodStateError::InvalidOrdinaryRodTierMetadata => {
            FishingRodDurabilityStateError::EquippedRodIntegrityMismatch
        }
    }
}

async fn lock_pending_player_operation(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
) -> Result<(), FishingRodDurabilityStateError> {
    let row = sqlx::query("SELECT player_id, state FROM operations WHERE id = $1 FOR UPDATE")
        .bind(operation_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(FishingRodDurabilityStateError::OperationNotFound)?;
    let operation_player_id: Option<Uuid> = row.try_get("player_id")?;
    if operation_player_id != Some(player_id) {
        return Err(FishingRodDurabilityStateError::OperationPlayerMismatch);
    }
    let state: String = row.try_get("state")?;
    if state != "PENDING" {
        return Err(FishingRodDurabilityStateError::OperationTerminal(state));
    }
    Ok(())
}

async fn lock_active_player(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
) -> Result<(), FishingRodDurabilityStateError> {
    let status: String = sqlx::query_scalar(
        "SELECT status FROM players WHERE id = $1 AND status <> 'DELETED' FOR UPDATE",
    )
    .bind(player_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(FishingRodDurabilityStateError::PlayerNotFound)?;
    if status != "ACTIVE" {
        return Err(FishingRodDurabilityStateError::AccountFrozen(status));
    }
    Ok(())
}
