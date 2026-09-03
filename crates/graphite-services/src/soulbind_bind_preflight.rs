use chrono::{DateTime, Utc};
use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    OrdinaryEquipmentSoulBindStateError, OrdinaryEquipmentSoulBindStateSnapshot,
    PersistedSoulBindState, SoulBindBindingPreview, SoulBindPolicyError,
    lock_owned_ordinary_equipment_soulbind_state, preview_soulbind_binding,
};

#[derive(Clone, Debug, PartialEq)]
pub struct OrdinarySoulBindBindPreflight {
    pub snapshot: OrdinaryEquipmentSoulBindStateSnapshot,
    pub preview: SoulBindBindingPreview,
    pub evaluated_at: DateTime<Utc>,
}

#[derive(Debug, Error)]
pub enum OrdinarySoulBindBindPreflightError {
    #[error(transparent)]
    State(#[from] OrdinaryEquipmentSoulBindStateError),
    #[error(transparent)]
    Policy(#[from] SoulBindPolicyError),
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error("player does not exist")]
    PlayerNotFound,
    #[error("SoulBind binding requires an ACTIVE account; current status is {0}")]
    AccountFrozen(String),
    #[error("persisted Rebirth count is outside the supported non-negative range: {0}")]
    InvalidRebirthCount(i64),
}

impl From<sqlx::Error> for OrdinarySoulBindBindPreflightError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

/// Locks and preflights the authoritative state required to bind SoulBind to one owned ordinary
/// equipment ItemInstance without consuming assets or mutating SoulBind state.
///
/// This boundary deliberately acquires the player row before item state so the authoritative
/// `rebirth_count` and account status cannot race a concurrent Rebirth/freeze transition while the
/// later item appraisal and SoulBind child are resolved. The complete local lock order is therefore
/// `player -> item -> structural state -> embedded enchant rows -> SoulBind child`. A higher-level
/// owning lifecycle must acquire its operation and any earlier cross-domain balance/progression
/// locks before calling this function; it must not pre-lock the target item and then enter here.
///
/// The ordinary-equipment SoulBind resolver proves exact pinned ordinary classification, the
/// Netherite/Graphite tier allowlist, current Enhanced Canonical Appraisal, and persisted per-item
/// SoulBind state. PostgreSQL `clock_timestamp()` is sampled only after those locks are retained, so
/// an unbound item is eligible exactly at/after its persisted seven-day cooldown boundary. The
/// existing pure SoulBind policy then enforces Rebirth >= 1 and derives the frozen tier package plus
/// initial `ceil(60% × EnhancedCanonicalAppraisal)` protection charge from authoritative inputs.
///
/// A successful return is still read-only. It proves no SoulBind Rune/material ownership, does not
/// spend Money or Activity EXP, does not write Bound state, does not create/finalize an operation or
/// outbox event, and does not expose a Discord command. The future owner must keep this transaction
/// open, settle every frozen package leg atomically, and only then invoke the already-resolved bind
/// state writer.
pub async fn lock_preview_soulbind_bind_for_owned_ordinary_equipment(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
    item_id: Uuid,
) -> Result<OrdinarySoulBindBindPreflight, OrdinarySoulBindBindPreflightError> {
    let rebirth_count = lock_active_rebirth_count(tx, player_id).await?;
    let snapshot = lock_owned_ordinary_equipment_soulbind_state(tx, player_id, item_id).await?;
    let evaluated_at = database_clock_timestamp(tx).await?;

    match &snapshot.state {
        PersistedSoulBindState::Bound => {
            return Err(OrdinaryEquipmentSoulBindStateError::AlreadySoulBound.into());
        }
        PersistedSoulBindState::Unbound { rebind_not_before }
            if rebind_not_before > &evaluated_at =>
        {
            return Err(OrdinaryEquipmentSoulBindStateError::RebindCooldownActive {
                rebind_not_before: rebind_not_before.to_owned(),
                evaluated_at,
            }
            .into());
        }
        PersistedSoulBindState::NeverBound | PersistedSoulBindState::Unbound { .. } => {}
    }

    let preview = preview_soulbind_binding(
        snapshot.equipment.recraft.tier,
        true,
        rebirth_count,
        snapshot.equipment.enhanced_canonical_appraisal,
    )?;

    Ok(OrdinarySoulBindBindPreflight {
        snapshot,
        preview,
        evaluated_at,
    })
}

async fn lock_active_rebirth_count(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
) -> Result<u64, OrdinarySoulBindBindPreflightError> {
    let row = sqlx::query(
        "SELECT status, rebirth_count FROM players WHERE id = $1 AND status <> 'DELETED' FOR UPDATE",
    )
    .bind(player_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(OrdinarySoulBindBindPreflightError::PlayerNotFound)?;

    let status: String = row.try_get("status")?;
    if status != "ACTIVE" {
        return Err(OrdinarySoulBindBindPreflightError::AccountFrozen(status));
    }

    let persisted: i64 = row.try_get("rebirth_count")?;
    u64::try_from(persisted)
        .map_err(|_| OrdinarySoulBindBindPreflightError::InvalidRebirthCount(persisted))
}

async fn database_clock_timestamp(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<DateTime<Utc>, OrdinarySoulBindBindPreflightError> {
    let row = sqlx::query("SELECT clock_timestamp() AS evaluated_at")
        .fetch_one(&mut **tx)
        .await?;
    Ok(row.try_get("evaluated_at")?)
}
