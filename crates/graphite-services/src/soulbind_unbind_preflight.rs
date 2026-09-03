use sqlx::{Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    OrdinaryEquipmentSoulBindStateError, OrdinaryEquipmentSoulBindStateSnapshot,
    PersistedSoulBindState, SoulBindPolicyError, SoulBindUnbindPreview,
    lock_owned_ordinary_equipment_soulbind_state, preview_soulbind_unbind,
};

#[derive(Clone, Debug, PartialEq)]
pub struct OrdinarySoulBindUnbindPreflight {
    pub snapshot: OrdinaryEquipmentSoulBindStateSnapshot,
    pub preview: SoulBindUnbindPreview,
}

#[derive(Debug, Error)]
pub enum OrdinarySoulBindUnbindPreflightError {
    #[error(transparent)]
    State(#[from] OrdinaryEquipmentSoulBindStateError),
    #[error(transparent)]
    Policy(#[from] SoulBindPolicyError),
    #[error("the ItemInstance is not currently SoulBound")]
    NotSoulBound,
    #[error(
        "SoulBind removal requires Favorite and Protected to be cleared; is_favorite={is_favorite}, is_protected={is_protected}"
    )]
    ControlFlagsSet {
        is_favorite: bool,
        is_protected: bool,
    },
}

/// Locks and preflights the frozen SoulBind removal requirements for one owned ordinary equipment
/// ItemInstance without settling Money or mutating SoulBind state.
///
/// The reused SoulBind snapshot retains the canonical
/// `item -> structural state -> embedded enchant rows -> SoulBind child` lock chain and carries the
/// authoritative typed Favorite/Protected flags plus current Enhanced Canonical Appraisal. Removal
/// is eligible only when that locked state is currently SoulBound and both control flags are clear.
/// The frozen 20% Money fee, seven-day rebind cooldown metadata, and no-refund policy are then
/// derived by the existing pure [`preview_soulbind_unbind`] authority.
///
/// This is a read-only prerequisite for the future owning unbind transaction. It does not debit
/// Money, finalize an operation/outbox event, write the cooldown transition, alter Favorite or
/// Protected state, or expose a command. A caller that later settles removal must keep this same
/// transaction open and perform settlement before invoking the already-resolved unbind writer.
pub async fn lock_preview_soulbind_unbind_for_owned_ordinary_equipment(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
    item_id: Uuid,
) -> Result<OrdinarySoulBindUnbindPreflight, OrdinarySoulBindUnbindPreflightError> {
    let snapshot = lock_owned_ordinary_equipment_soulbind_state(tx, player_id, item_id).await?;

    if snapshot.state != PersistedSoulBindState::Bound {
        return Err(OrdinarySoulBindUnbindPreflightError::NotSoulBound);
    }

    if snapshot.is_favorite || snapshot.is_protected {
        return Err(OrdinarySoulBindUnbindPreflightError::ControlFlagsSet {
            is_favorite: snapshot.is_favorite,
            is_protected: snapshot.is_protected,
        });
    }

    let preview = preview_soulbind_unbind(snapshot.equipment.enhanced_canonical_appraisal)?;

    Ok(OrdinarySoulBindUnbindPreflight { snapshot, preview })
}
