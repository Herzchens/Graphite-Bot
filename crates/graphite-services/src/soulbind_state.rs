use chrono::{DateTime, Utc};
use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    OrdinaryEquipmentEnhancedAppraisal, OrdinaryEquipmentEnhancedResolverError,
    SOULBIND_REBIND_COOLDOWN_SECONDS, SoulBindPolicyError,
    lock_owned_ordinary_equipment_enhanced_appraisal, soulbind_binding_package,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PersistedSoulBindState {
    NeverBound,
    Bound,
    Unbound { rebind_not_before: DateTime<Utc> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct OrdinaryEquipmentSoulBindStateSnapshot {
    pub equipment: OrdinaryEquipmentEnhancedAppraisal,
    pub state: PersistedSoulBindState,
    pub is_favorite: bool,
    pub is_protected: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppliedSoulBindStateTransition {
    pub previous_state: PersistedSoulBindState,
    pub new_state: PersistedSoulBindState,
    pub evaluated_at: DateTime<Utc>,
}

#[derive(Debug, Error)]
pub enum OrdinaryEquipmentSoulBindStateError {
    #[error(transparent)]
    Enhanced(#[from] OrdinaryEquipmentEnhancedResolverError),
    #[error(transparent)]
    Policy(#[from] SoulBindPolicyError),
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error("the ItemInstance is already SoulBound")]
    AlreadySoulBound,
    #[error(
        "the ItemInstance cannot be rebound before {rebind_not_before}; database time is {evaluated_at}"
    )]
    RebindCooldownActive {
        rebind_not_before: DateTime<Utc>,
        evaluated_at: DateTime<Utc>,
    },
    #[error("the ItemInstance is not currently SoulBound")]
    NotSoulBound,
    #[error("the locked SoulBind state changed unexpectedly before the resolved transition write")]
    LockedStateMismatch,
}

impl From<sqlx::Error> for OrdinaryEquipmentSoulBindStateError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

/// Locks authoritative owned ordinary-equipment appraisal, typed item control flags, and its optional
/// SoulBind child state.
///
/// Lock order is `item -> structural state -> embedded enchant rows -> SoulBind child`. The parent
/// ItemInstance is locked before the typed Favorite/Protected read and optional child lookup.
/// Migration 0019 requires every SoulBind child INSERT/UPDATE to acquire that same parent lock first,
/// so an absent child row observed here is serialized and authoritative rather than an insert race.
/// The control flags are read from the typed ItemInstance columns introduced by migration 0020;
/// similarly named keys in generic `state JSONB` are not an authority.
///
/// This resolver accepts only the canonical SoulBind equipment tiers by reusing
/// [`soulbind_binding_package`] instead of defining a second tier allowlist. It deliberately does not
/// prove account Rebirth, SoulBind Rune/material ownership, Money/AEXP affordability, decide whether
/// Favorite/Protected satisfy an operation-specific precondition, or perform command authorization.
/// Those belong to the future owning SoulBind lifecycle.
pub async fn lock_owned_ordinary_equipment_soulbind_state(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
    item_id: Uuid,
) -> Result<OrdinaryEquipmentSoulBindStateSnapshot, OrdinaryEquipmentSoulBindStateError> {
    let equipment =
        lock_owned_ordinary_equipment_enhanced_appraisal(tx, player_id, item_id).await?;
    soulbind_binding_package(equipment.recraft.tier)?;

    let control_row = sqlx::query(
        r#"
        SELECT is_favorite, is_protected
          FROM item_instances
         WHERE id = $1
           AND owner_player_id = $2
        "#,
    )
    .bind(item_id)
    .bind(player_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(OrdinaryEquipmentSoulBindStateError::LockedStateMismatch)?;
    let is_favorite: bool = control_row.try_get("is_favorite")?;
    let is_protected: bool = control_row.try_get("is_protected")?;

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
                _ => return Err(OrdinaryEquipmentSoulBindStateError::LockedStateMismatch),
            }
        }
    };

    Ok(OrdinaryEquipmentSoulBindStateSnapshot {
        equipment,
        state,
        is_favorite,
        is_protected,
    })
}

/// Writes only the persisted consequence of an already-authorized SoulBind binding.
///
/// The future owning lifecycle must acquire operation/player locks first and atomically prove
/// Rebirth >= 1, reserve/consume the tier package, SoulBind Rune, Money/AEXP and initial appraisal
/// charge, then call this primitive inside that same transaction. This function performs no asset
/// settlement and does not expose a command.
///
/// Rebinding is allowed only when no prior state exists or the persisted per-item cooldown has
/// expired at the authoritative database wall clock. The check never accepts a caller-supplied time.
/// An expired cooldown row is retained as history until this exact transition replaces it with the
/// canonical bound shape.
pub async fn write_resolved_soulbind_bind_to_owned_ordinary_equipment(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
    item_id: Uuid,
) -> Result<AppliedSoulBindStateTransition, OrdinaryEquipmentSoulBindStateError> {
    let snapshot = lock_owned_ordinary_equipment_soulbind_state(tx, player_id, item_id).await?;
    let evaluated_at = database_clock_timestamp(tx).await?;
    let previous_state = snapshot.state;

    match &previous_state {
        PersistedSoulBindState::Bound => {
            return Err(OrdinaryEquipmentSoulBindStateError::AlreadySoulBound);
        }
        PersistedSoulBindState::Unbound { rebind_not_before }
            if rebind_cooldown_active(rebind_not_before, &evaluated_at) =>
        {
            return Err(OrdinaryEquipmentSoulBindStateError::RebindCooldownActive {
                rebind_not_before: rebind_not_before.to_owned(),
                evaluated_at,
            });
        }
        PersistedSoulBindState::NeverBound | PersistedSoulBindState::Unbound { .. } => {}
    }

    let rows_affected = match &previous_state {
        PersistedSoulBindState::NeverBound => sqlx::query(
            r#"
            INSERT INTO item_instance_soulbind_state (
                item_instance_id,
                is_soulbound,
                rebind_not_before
            ) VALUES ($1, TRUE, NULL)
            "#,
        )
        .bind(item_id)
        .execute(&mut **tx)
        .await?
        .rows_affected(),
        PersistedSoulBindState::Unbound { rebind_not_before } => sqlx::query(
            r#"
            UPDATE item_instance_soulbind_state
               SET is_soulbound = TRUE,
                   rebind_not_before = NULL
             WHERE item_instance_id = $1
               AND is_soulbound = FALSE
               AND rebind_not_before = $2
            "#,
        )
        .bind(item_id)
        .bind(rebind_not_before)
        .execute(&mut **tx)
        .await?
        .rows_affected(),
        PersistedSoulBindState::Bound => unreachable!("bound state returned before mutation"),
    };

    if rows_affected != 1 {
        return Err(OrdinaryEquipmentSoulBindStateError::LockedStateMismatch);
    }

    Ok(AppliedSoulBindStateTransition {
        previous_state,
        new_state: PersistedSoulBindState::Bound,
        evaluated_at,
    })
}

/// Writes only the persisted consequence of an already-authorized SoulBind removal.
///
/// The future owning lifecycle must first prove the item is eligible to unbind, is unprotected and
/// unfavorited, and atomically settle the frozen 20% current-enhanced-appraisal Money fee. This
/// primitive owns none of those asset/precondition checks; it only requires that the authoritative
/// locked item is currently SoulBound.
///
/// The mutation statement samples PostgreSQL `clock_timestamp()` once and persists the new per-item
/// cooldown as exactly seven days after that same instant. This intentionally does not use
/// `CURRENT_TIMESTAMP`, because PostgreSQL defines that as transaction-start time and a long-lived
/// owning transaction would otherwise shorten the effective cooldown. No binding resource is
/// refunded and no scheduler is required: an unbound row may remain after its timestamp passes, and
/// the binding transition simply treats it as eligible at/after that instant.
pub async fn write_resolved_soulbind_unbind_to_owned_ordinary_equipment(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
    item_id: Uuid,
) -> Result<AppliedSoulBindStateTransition, OrdinaryEquipmentSoulBindStateError> {
    let snapshot = lock_owned_ordinary_equipment_soulbind_state(tx, player_id, item_id).await?;
    if snapshot.state != PersistedSoulBindState::Bound {
        return Err(OrdinaryEquipmentSoulBindStateError::NotSoulBound);
    }

    let row = sqlx::query(
        r#"
        WITH timing AS MATERIALIZED (
            SELECT clock_timestamp() AS evaluated_at
        )
        UPDATE item_instance_soulbind_state AS soulbind
           SET is_soulbound = FALSE,
               rebind_not_before = timing.evaluated_at
                   + make_interval(secs => ($2::BIGINT)::DOUBLE PRECISION)
          FROM timing
         WHERE soulbind.item_instance_id = $1
           AND soulbind.is_soulbound = TRUE
           AND soulbind.rebind_not_before IS NULL
        RETURNING timing.evaluated_at, soulbind.rebind_not_before
        "#,
    )
    .bind(item_id)
    .bind(SOULBIND_REBIND_COOLDOWN_SECONDS)
    .fetch_optional(&mut **tx)
    .await?;

    let Some(row) = row else {
        return Err(OrdinaryEquipmentSoulBindStateError::LockedStateMismatch);
    };
    let evaluated_at: DateTime<Utc> = row.try_get("evaluated_at")?;
    let rebind_not_before: DateTime<Utc> = row.try_get("rebind_not_before")?;

    let new_state = PersistedSoulBindState::Unbound { rebind_not_before };
    Ok(AppliedSoulBindStateTransition {
        previous_state: PersistedSoulBindState::Bound,
        new_state,
        evaluated_at,
    })
}

fn rebind_cooldown_active(rebind_not_before: &DateTime<Utc>, evaluated_at: &DateTime<Utc>) -> bool {
    rebind_not_before > evaluated_at
}

async fn database_clock_timestamp(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<DateTime<Utc>, OrdinaryEquipmentSoulBindStateError> {
    let row = sqlx::query("SELECT clock_timestamp() AS evaluated_at")
        .fetch_one(&mut **tx)
        .await?;
    Ok(row.try_get("evaluated_at")?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EquipmentTier;

    #[test]
    fn tier_gate_reuses_only_endgame_soulbind_packages() {
        for tier in [EquipmentTier::Netherite, EquipmentTier::Graphite] {
            assert!(soulbind_binding_package(tier).is_ok());
        }
        for tier in [
            EquipmentTier::StarterLeather,
            EquipmentTier::Wood,
            EquipmentTier::Stone,
            EquipmentTier::Copper,
            EquipmentTier::Gold,
            EquipmentTier::Iron,
            EquipmentTier::Diamond,
            EquipmentTier::Obsidian,
        ] {
            assert_eq!(
                soulbind_binding_package(tier),
                Err(SoulBindPolicyError::IneligibleTier)
            );
        }
    }

    #[test]
    fn cooldown_boundary_is_eligible() {
        let boundary = DateTime::parse_from_rfc3339("2026-09-03T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(!rebind_cooldown_active(&boundary, &boundary));

        let before = DateTime::parse_from_rfc3339("2026-09-02T23:59:59Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(rebind_cooldown_active(&boundary, &before));
    }
}
