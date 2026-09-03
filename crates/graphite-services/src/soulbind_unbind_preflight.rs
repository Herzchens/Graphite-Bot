use graphite_economy::{
    WalletSpendError, WalletSpendReceipt, WalletSpendRequest, apply_wallet_spend,
    lock_new_wallet_spend_context,
};
use serde_json::json;
use sqlx::{Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    AppliedSoulBindStateTransition, OrdinaryEquipmentSoulBindStateError,
    OrdinaryEquipmentSoulBindStateSnapshot, PersistedSoulBindState, SoulBindPolicyError,
    SoulBindUnbindPreview, lock_owned_ordinary_equipment_soulbind_state, preview_soulbind_unbind,
    write_resolved_soulbind_unbind_to_owned_ordinary_equipment,
};

const SOULBIND_UNBIND_WALLET_SOURCE: &str = "SOULBIND_UNBIND";

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
    #[error(transparent)]
    Wallet(#[from] WalletSpendError),
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

impl OrdinarySoulBindUnbindPreflight {
    /// Atomically settles the already-frozen stateful consequences of removing SoulBind from one
    /// owned ordinary equipment ItemInstance inside a caller-owned operation transaction.
    ///
    /// The lock sequence is deliberately
    /// `operation -> player/balance -> item -> structural state -> embedded enchants -> SoulBind child`.
    /// The Wallet context is locked before resolving the mutable item appraisal so the later Money
    /// debit cannot create an `item -> balance` lock-order inversion. The authoritative unbind
    /// preflight then freezes current Enhanced Canonical Appraisal, verifies Bound plus cleared
    /// Favorite/Protected state, and derives the exact 20% Wallet fee. That fee is settled through
    /// `graphite-economy`'s immutable double-entry Wallet sink before the already-resolved SoulBind
    /// state writer records the seven-day rebind cooldown. The ledger provenance records the exact
    /// appraisal, fee, cooldown, control prerequisites, and the frozen no-binding-resource-refund
    /// policy. Every step shares this one SQL transaction, so a caller rollback restores both Money
    /// and SoulBind state.
    ///
    /// This is still a transaction-composable Phase 6 primitive, not the live `/unbind` lifecycle.
    /// It does not create/finalize the operation, emit an outbox event, auto-pull from Bank, refund
    /// prior binding resources, alter Favorite/Protected, or expose a Discord command. The
    /// higher-level operation owner must finalize and commit only after all required canonical
    /// effects are ready.
    pub async fn settle_for_owned_ordinary_equipment(
        tx: &mut Transaction<'_, Postgres>,
        operation_id: Uuid,
        player_id: Uuid,
        item_id: Uuid,
    ) -> Result<
        (Self, WalletSpendReceipt, AppliedSoulBindStateTransition),
        OrdinarySoulBindUnbindPreflightError,
    > {
        let _locked_wallet = lock_new_wallet_spend_context(tx, operation_id, player_id).await?;

        let preflight =
            lock_preview_soulbind_unbind_for_owned_ordinary_equipment(tx, player_id, item_id)
                .await?;

        let wallet_spend = apply_wallet_spend(
            tx,
            &WalletSpendRequest {
                operation_id,
                player_id,
                amount: preflight.preview.money_fee,
                source: SOULBIND_UNBIND_WALLET_SOURCE.to_owned(),
                provenance: json!({
                    "service": SOULBIND_UNBIND_WALLET_SOURCE,
                    "item_instance_id": item_id,
                    "current_enhanced_appraisal": preflight.preview.current_enhanced_appraisal,
                    "money_fee": preflight.preview.money_fee,
                    "rebind_cooldown_seconds": preflight.preview.rebind_cooldown_seconds,
                    "refunds_binding_resources": preflight.preview.refunds_binding_resources,
                    "requires_unprotected": preflight.preview.requires_unprotected,
                    "requires_unfavorited": preflight.preview.requires_unfavorited,
                }),
            },
        )
        .await?;

        let state_transition =
            write_resolved_soulbind_unbind_to_owned_ordinary_equipment(tx, player_id, item_id)
                .await?;

        Ok((preflight, wallet_spend, state_transition))
    }
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
