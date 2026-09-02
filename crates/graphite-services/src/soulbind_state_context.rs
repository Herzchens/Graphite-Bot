use chrono::{DateTime, Utc};
use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    OrdinaryEquipmentEnhancedAppraisal, OrdinaryEquipmentEnhancedResolverError,
    lock_owned_ordinary_equipment_enhanced_appraisal,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PersistedSoulBindState {
    NeverBound,
    Bound,
    Unbound {
        rebind_not_before: DateTime<Utc>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LockedOrdinarySoulBindContext {
    pub player_id: Uuid,
    pub rebirth_count: u64,
    pub equipment: OrdinaryEquipmentEnhancedAppraisal,
    pub state: PersistedSoulBindState,
}

#[derive(Debug, Error)]
pub enum OrdinarySoulBindContextError {
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error("player was not found while locking the SoulBind mutation context")]
    PlayerNotFound,
    #[error("SoulBind mutation requires an ACTIVE account; current status is {0}")]
    AccountNotMutable(String),
    #[error("persisted rebirth count is outside the supported non-negative integer domain")]
    InvalidRebirthCount,
    #[error(transparent)]
    Enhanced(#[from] OrdinaryEquipmentEnhancedResolverError),
    #[error("persisted SoulBind state violates the canonical bound/cooldown shape")]
    InvalidPersistedSoulBindState,
}

impl From<sqlx::Error> for OrdinarySoulBindContextError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

/// Locks all authoritative state required to make a later ordinary-equipment SoulBind mutation
/// decision without trusting a cache, Discord input, or an unlocked read model.
///
/// The caller owns the surrounding transaction and must acquire any operation lock before entering
/// this boundary. This function deliberately acquires the player row itself before delegating to the
/// ordinary Enhanced appraisal resolver, preserving the canonical
/// `operation -> player -> item -> structural state -> embedded enchant rows -> SoulBind child` order.
/// The caller must therefore not pre-lock the target item before calling this function.
///
/// Only ACTIVE accounts are admitted to this mutation-oriented context. The player's authoritative
/// Rebirth count is converted to the same non-negative `u64` domain used by the pure SoulBind policy.
/// The ordinary equipment resolver then locks and validates the current canonical Enhanced appraisal.
/// Finally, the optional normalized SoulBind child row is locked. Since database triggers require all
/// SoulBind child INSERT/UPDATE writes to lock the parent ItemInstance first, an absent child observed
/// while that parent lock is held is an authoritative `NeverBound` state rather than a write-skew gap.
///
/// This resolver does not decide binding/unbinding eligibility, compare the rebind timestamp with a
/// clock, consume assets, charge Money/AEXP, mutate SoulBind state, or expose a command. A future
/// owning lifecycle must keep this transaction open and compose those already-authorized consequences
/// atomically/idempotently around this locked context.
pub async fn lock_owned_ordinary_soulbind_context_for_mutation(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
    item_id: Uuid,
) -> Result<LockedOrdinarySoulBindContext, OrdinarySoulBindContextError> {
    let player = sqlx::query(
        r#"
        SELECT status, rebirth_count
          FROM players
         WHERE id = $1
           AND status <> 'DELETED'
         FOR UPDATE
        "#,
    )
    .bind(player_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(OrdinarySoulBindContextError::PlayerNotFound)?;

    let status: String = player.try_get("status")?;
    if status != "ACTIVE" {
        return Err(OrdinarySoulBindContextError::AccountNotMutable(status));
    }
    let persisted_rebirth_count: i64 = player.try_get("rebirth_count")?;
    let rebirth_count = u64::try_from(persisted_rebirth_count)
        .map_err(|_| OrdinarySoulBindContextError::InvalidRebirthCount)?;

    let equipment =
        lock_owned_ordinary_equipment_enhanced_appraisal(tx, player_id, item_id).await?;

    let row = sqlx::query(
        r#"
        SELECT is_soulbound, rebind_not_before
          FROM item_instance_soulbind_state
         WHERE item_instance_id = $1
         FOR UPDATE
        "#,
    )
    .bind(item_id)
    .fetch_optional(&mut **tx)
    .await?;

    let state = match row {
        None => PersistedSoulBindState::NeverBound,
        Some(row) => {
            let is_soulbound: bool = row.try_get("is_soulbound")?;
            let rebind_not_before: Option<DateTime<Utc>> = row.try_get("rebind_not_before")?;
            match (is_soulbound, rebind_not_before) {
                (true, None) => PersistedSoulBindState::Bound,
                (false, Some(rebind_not_before)) => {
                    PersistedSoulBindState::Unbound { rebind_not_before }
                }
                _ => return Err(OrdinarySoulBindContextError::InvalidPersistedSoulBindState),
            }
        }
    };

    Ok(LockedOrdinarySoulBindContext {
        player_id,
        rebirth_count,
        equipment,
        state,
    })
}
