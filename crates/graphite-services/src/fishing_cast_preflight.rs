use graphite_progression::{ActivityXpError, lock_activity_xp_settlement_context};
use sqlx::{Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    BaitRackCapacityPolicy, BaitRackPolicyError, CanonicalEnchant, EquippedFishingRodCastSnapshot,
    EquippedFishingRodCastSnapshotError, EquippedFishingRodKind, FishingArea,
    FishingAreaAccessError, FishingAreaAccessSnapshot, bait_rack_capacity_policy,
    lock_equipped_fishing_rod_cast_snapshot, lock_or_grant_fishing_area_first_unlock,
};

#[derive(Clone, Debug, PartialEq)]
pub struct ManualFishingCastPreflight {
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub area_access: FishingAreaAccessSnapshot,
    pub rod: EquippedFishingRodCastSnapshot,
    pub bait_capacity: BaitRackCapacityPolicy,
}

#[derive(Debug, Error)]
pub enum ManualFishingCastPreflightError {
    #[error(transparent)]
    AreaAccess(#[from] FishingAreaAccessError),
    #[error(transparent)]
    ActivityXp(#[from] ActivityXpError),
    #[error(transparent)]
    Rod(#[from] EquippedFishingRodCastSnapshotError),
    #[error(transparent)]
    BaitRack(#[from] BaitRackPolicyError),
    #[error("equipped ordinary Fishing Rod is Broken and cannot start a manual cast")]
    BrokenFishingRod,
    #[error("Starter Basic Rod is restricted to Starter Pool")]
    StarterBasicRodOutsidePool,
}

/// Locks the authoritative state needed before a future manual Fishing cast resolves RNG or mutates
/// cast assets.
///
/// Permanent-area access is resolved first because a first non-default unlock already follows the
/// repository order `operation -> player -> progression -> item`. The generic Activity EXP settlement
/// prelock is then acquired before the cast-specific Rod snapshot. On a persisted/default area path it
/// introduces the progression lock before any item lock; on a first-unlock path the same progression
/// row was already locked before the qualification Rod and this call only re-enters that owned lock.
/// This keeps a later Fishing Activity EXP mutation from creating an `item -> progression` inversion.
/// The returned Activity EXP snapshot is deliberately not exposed here: PostgreSQL owns the lock, and
/// the future settlement must use the canonical keyed mutation API rather than treating a preflight
/// balance snapshot as later authority. The Rod cast snapshot then extends the held lock set with the
/// exact equipped ItemInstance, equipment slot, structural capacity row, and canonical embedded enchant
/// rows.
///
/// Existing area access remains permanent and therefore does not re-check a lower current ordinary Rod
/// tier; the one explicit per-cast exception is Starter Basic, which remains Pool-only. A consistently
/// Broken ordinary Rod is a valid low-level Rod snapshot but is rejected here before a cast can proceed.
/// Bait Rack capacity is derived from the locked canonical embedded-enchant snapshot, so downstream bait
/// planning does not need to trust Discord input or re-read mutable Rod state.
///
/// This preflight may insert the player's first permanent non-default area unlock in the caller-owned
/// transaction. Consequently the caller must roll back the transaction when this function returns an
/// error. On success the caller still owns RNG resolution, active-bait inventory/consumption, normal or
/// line-break durability consequence, Mending, CatchBag output, Fishing AEXP, cooldown, operation
/// finalization, outbox/audit effects, and commit. This function does not expose `/fish` by itself.
pub async fn lock_manual_fishing_cast_preflight(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
    area: FishingArea,
) -> Result<ManualFishingCastPreflight, ManualFishingCastPreflightError> {
    let area_access =
        lock_or_grant_fishing_area_first_unlock(tx, operation_id, player_id, area).await?;
    lock_activity_xp_settlement_context(tx, operation_id, player_id).await?;
    let rod = lock_equipped_fishing_rod_cast_snapshot(tx, player_id).await?;

    match rod.kind {
        EquippedFishingRodKind::StarterBasic if area != FishingArea::StarterPool => {
            return Err(ManualFishingCastPreflightError::StarterBasicRodOutsidePool);
        }
        EquippedFishingRodKind::Ordinary { .. } if rod.is_broken => {
            return Err(ManualFishingCastPreflightError::BrokenFishingRod);
        }
        _ => {}
    }

    let bait_rack_level = rod
        .embedded_enchants
        .iter()
        .find(|state| state.enchant == CanonicalEnchant::BaitRack)
        .map(|state| state.level);
    let bait_capacity = bait_rack_capacity_policy(bait_rack_level)?;

    Ok(ManualFishingCastPreflight {
        operation_id,
        player_id,
        area_access,
        rod,
        bait_capacity,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MAX_ACTIVE_BAIT_CATEGORY_SLOTS, NATIVE_ACTIVE_BAIT_CATEGORY_SLOTS};

    #[test]
    fn bait_capacity_without_or_with_max_bait_rack_stays_at_frozen_bounds() {
        let native = bait_rack_capacity_policy(None).unwrap();
        assert_eq!(
            native.active_bait_category_slots,
            NATIVE_ACTIVE_BAIT_CATEGORY_SLOTS
        );

        let max = bait_rack_capacity_policy(Some(3)).unwrap();
        assert_eq!(
            max.active_bait_category_slots,
            MAX_ACTIVE_BAIT_CATEGORY_SLOTS
        );
    }
}
