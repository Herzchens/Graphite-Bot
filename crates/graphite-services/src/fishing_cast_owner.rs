use sqlx::{Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::fishing_unmodified_line_stage::resolve_manual_unmodified_line_stage_v1;

use crate::{
    CanonicalEnchant, EquipmentTier, EquippedFishingRodKind, FishingArea, FishingBait,
    FishingBaitCastConsumptionAction, FishingBaitCastConsumptionEntry,
    FishingBaitCastSettlementReceipt, FishingBaitStateError, FishingCatchBranch,
    FishingCatchBranchPreCapWeightsV1, FishingFishInstanceDeliveryError,
    FishingFishInstanceDeliveryReceiptV1, FishingFishInstanceDeliveryRequestV1,
    FishingNonFishRollError, FishingResolvedNonFishOutcomeV1, ManualFishingCastPreflight,
    ManualFishingCastPreflightError, ManualFishingUnmodifiedCapabilityError,
    ManualFishingUnmodifiedCapabilityPreviewV1, ManualFishingUnmodifiedFishCandidateError,
    ManualFishingUnmodifiedLineStageError, ManualFishingUnmodifiedLineStageV1,
    deliver_fishing_fish_instance_v1, fishing_catch_branch_pre_cap_weights_v1,
    lock_manual_fishing_cast_preflight, preview_manual_fishing_unmodified_fish_candidate_v1,
    preview_manual_unmodified_fish_capability_v1, resolve_manual_fishing_non_fish_outcome_v1,
    settle_active_fishing_baits_for_cast,
};

#[derive(Debug, Error)]
pub enum ManualFishingCastOwnerError {
    #[error(transparent)]
    Preflight(#[from] ManualFishingCastPreflightError),
    #[error(transparent)]
    Bait(#[from] FishingBaitStateError),
    #[error(transparent)]
    NonFishRoll(#[from] FishingNonFishRollError),
    #[error(transparent)]
    UnmodifiedFish(#[from] ManualFishingUnmodifiedFishCandidateError),
    #[error("the locked cast contains modifiers requiring the shared capped Fish resolver")]
    ModifiedFishRequiresSharedComposer,
    #[error(transparent)]
    FishDelivery(#[from] FishingFishInstanceDeliveryError),
    #[error("unmodified Starter Pool provisional fish delivery requires Starter Pool")]
    ProvisionalFishOutsideStarterPool,
    #[error(transparent)]
    UnmodifiedCapability(#[from] ManualFishingUnmodifiedCapabilityError),
    #[error(transparent)]
    UnmodifiedLineStage(#[from] ManualFishingUnmodifiedLineStageError),
}

/// Owns an uncommitted manual Fishing cast transaction.
///
/// There is intentionally no commit or transaction-extraction method until the entire cast
/// lifecycle (all outcome branches, rod, progression, result, audit and outbox) is composed.
/// Dropping this value rolls the PostgreSQL transaction back, including any first-area unlock
/// or bait consumption performed during preparation. The caller can explicitly roll back.
#[must_use = "an unfinished manual Fishing cast must be rolled back"]
pub struct PendingManualFishingCastV1<'c> {
    tx: Transaction<'c, Postgres>,
    preflight: ManualFishingCastPreflight,
    bait: FishingBaitCastSettlementReceipt,
    pre_cap_branch_weights: FishingCatchBranchPreCapWeightsV1,
}

impl PendingManualFishingCastV1<'_> {
    #[must_use]
    pub fn operation_id(&self) -> Uuid {
        self.preflight.operation_id
    }

    #[must_use]
    pub fn player_id(&self) -> Uuid {
        self.preflight.player_id
    }

    #[must_use]
    pub fn preflight(&self) -> &ManualFishingCastPreflight {
        &self.preflight
    }

    #[must_use]
    pub fn bait_settlement(&self) -> &FishingBaitCastSettlementReceipt {
        &self.bait
    }

    /// Frozen Gold Rod + consumed Treasure Bait contributions before shared Fishing caps.
    ///
    /// This is not yet the final branch weight vector. In particular, other enchant/modifier
    /// sources must be composed before any `CatchBranch` RNG draw is authorized.
    #[must_use]
    pub fn pre_cap_branch_weights(&self) -> FishingCatchBranchPreCapWeightsV1 {
        self.pre_cap_branch_weights
    }

    /// Previews deterministic non-Fish payload selection from the locked operation RNG root.
    ///
    /// The branch argument is only a *provisional candidate*, not proof of a completed
    /// catch-branch roll. This method never delivers assets or finalizes an operation.
    /// The full cast resolver must choose the branch from final capped weights before any
    /// result can become authoritative. An absent Multi Treasure enchant gives one payload.
    pub fn preview_non_fish_candidate_v1(
        &self,
        candidate: FishingCatchBranch,
    ) -> Result<FishingResolvedNonFishOutcomeV1, ManualFishingCastOwnerError> {
        let multi_treasure_level = self
            .preflight
            .rod
            .embedded_enchants
            .iter()
            .find(|state| state.enchant == CanonicalEnchant::MultiTreasure)
            .map(|state| state.level);
        Ok(resolve_manual_fishing_non_fish_outcome_v1(
            self.preflight.rng,
            candidate,
            multi_treasure_level,
        )?)
    }

    /// Deterministically previews one base FishInstance candidate from an unmodified cast.
    ///
    /// An active bait, Gold side-grade or any embedded Rod enchant requires later shared
    /// composition; that case fails closed rather than bypassing a modifier. No assets are
    /// issued, and Fish capability/terminal consequences still require the full cast owner.
    pub fn preview_unmodified_fish_candidate_v1(
        &self,
        fish_slot: u8,
    ) -> Result<FishingFishInstanceDeliveryRequestV1, ManualFishingCastOwnerError> {
        if self.bait.plan.consumed_bait_categories != 0
            || !self.preflight.rod.embedded_enchants.is_empty()
            || matches!(
                self.preflight.rod.kind,
                EquippedFishingRodKind::Ordinary {
                    tier: EquipmentTier::Gold
                }
            )
        {
            return Err(ManualFishingCastOwnerError::ModifiedFishRequiresSharedComposer);
        }
        Ok(preview_manual_fishing_unmodified_fish_candidate_v1(
            self.preflight.rng,
            self.preflight.area_access.area,
            fish_slot,
        )?)
    }

    /// Previews physical capability for a candidate generated from this same locked
    /// unmodified operation. Over-cap stages remain unresolved and never sample RNG here.
    pub fn preview_unmodified_fish_capability_v1(
        &self,
        fish_slot: u8,
    ) -> Result<ManualFishingUnmodifiedCapabilityPreviewV1, ManualFishingCastOwnerError> {
        let candidate = self.preview_unmodified_fish_candidate_v1(fish_slot)?;
        Ok(preview_manual_unmodified_fish_capability_v1(
            self.preflight.area_access.area,
            self.preflight.rod.kind,
            candidate.numeric,
        )?)
    }

    /// Resolves the pinned LineBreak PPM stage of an unmodified candidate, never a
    /// finished cast. Over-cap line survival still requires catch/escape resolution.
    pub fn preview_unmodified_line_stage_v1(
        &self,
        fish_slot: u8,
    ) -> Result<ManualFishingUnmodifiedLineStageV1, ManualFishingCastOwnerError> {
        let capability = self.preview_unmodified_fish_capability_v1(fish_slot)?;
        Ok(resolve_manual_unmodified_line_stage_v1(
            self.preflight.rng,
            capability,
        )?)
    }

    /// Stages the single unmodified Starter Pool Fish candidate in the *still uncommittable*
    /// cast transaction. Starter Pool Fish always lands at the capability stage, so the
    /// physical line-break stage is bypassed. This call does NOT choose the catch branch;
    /// it is a provisional internal composition step, not a completed Fishing cast.
    ///
    /// With no Multicatch/School Bait permitted by the unmodified guard, only fish slot 0
    /// is eligible. The shared unique delivery child ensures repeated staging cannot mint
    /// another ItemInstance. The entire candidate vanishes on pending-owner rollback.
    pub async fn stage_unmodified_starter_pool_fish_v1(
        &mut self,
    ) -> Result<FishingFishInstanceDeliveryReceiptV1, ManualFishingCastOwnerError> {
        if self.preflight.area_access.area != FishingArea::StarterPool {
            return Err(ManualFishingCastOwnerError::ProvisionalFishOutsideStarterPool);
        }
        if self.preview_unmodified_fish_capability_v1(0)?
            != ManualFishingUnmodifiedCapabilityPreviewV1::StarterPoolGuaranteedLanded
        {
            return Err(ManualFishingCastOwnerError::ProvisionalFishOutsideStarterPool);
        }
        let request = self.preview_unmodified_fish_candidate_v1(0)?;
        Ok(deliver_fishing_fish_instance_v1(
            &mut self.tx,
            self.preflight.operation_id,
            self.preflight.player_id,
            request,
        )
        .await?)
    }

    pub async fn rollback(self) -> Result<(), sqlx::Error> {
        self.tx.rollback().await
    }
}

fn locked_pre_cap_modifiers(
    rod: EquippedFishingRodKind,
    consumed: &[FishingBaitCastConsumptionEntry],
) -> (bool, bool) {
    let treasure_bait_active = consumed.iter().any(|entry| {
        entry.bait == FishingBait::Treasure
            && entry.action == FishingBaitCastConsumptionAction::ConsumeForCast
    });
    let ordinary_gold_rod_active = matches!(
        rod,
        EquippedFishingRodKind::Ordinary {
            tier: EquipmentTier::Gold
        }
    );
    (treasure_bait_active, ordinary_gold_rod_active)
}

/// Begins the stateful manual cast in an *owned* caller-provided PostgreSQL transaction.
///
/// This composes the pinned operation RNG/preflight/locks and actual active bait consumption
/// under one transaction owner, deriving the known pre-cap branch modifiers from locked
/// canonical Rod state and the settled bait receipt, never caller-supplied gameplay flags.
///
/// While result/terminal/outbox integration is not implemented, this function can prepare a
/// cast but deliberately cannot commit it. Its opaque pending owner prevents accidental live
/// bait consumption or permanent-area unlocking without a fully settled cast operation.
pub async fn begin_manual_fishing_cast_v1<'c>(
    mut tx: Transaction<'c, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
    area: FishingArea,
) -> Result<PendingManualFishingCastV1<'c>, ManualFishingCastOwnerError> {
    let preflight =
        lock_manual_fishing_cast_preflight(&mut tx, operation_id, player_id, area).await?;
    let bait = settle_active_fishing_baits_for_cast(&mut tx, operation_id, player_id).await?;
    let (treasure_bait_active, ordinary_gold_rod_active) =
        locked_pre_cap_modifiers(preflight.rod.kind, &bait.plan.entries);

    Ok(PendingManualFishingCastV1 {
        tx,
        preflight,
        bait,
        pre_cap_branch_weights: fishing_catch_branch_pre_cap_weights_v1(
            treasure_bait_active,
            ordinary_gold_rod_active,
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FishingBaitCategory;

    fn treasure_bait_entry(
        action: FishingBaitCastConsumptionAction,
    ) -> FishingBaitCastConsumptionEntry {
        FishingBaitCastConsumptionEntry {
            bait: FishingBait::Treasure,
            category: FishingBaitCategory::Treasure,
            available_units_before: 1,
            units_consumed: 1,
            available_units_after: 0,
            action,
        }
    }

    #[test]
    fn only_consumed_treasure_bait_and_true_ordinary_gold_rod_modify_the_bucket() {
        let gold_rod = EquippedFishingRodKind::Ordinary {
            tier: EquipmentTier::Gold,
        };
        let (bait, gold) = locked_pre_cap_modifiers(
            gold_rod,
            &[treasure_bait_entry(
                FishingBaitCastConsumptionAction::ConsumeForCast,
            )],
        );
        assert_eq!((bait, gold), (true, true));

        let weights = fishing_catch_branch_pre_cap_weights_v1(bait, gold);
        assert_eq!(
            (weights.fish, weights.junk, weights.treasure),
            (70_400, 6_120, 3_703)
        );

        let (bait, gold) = locked_pre_cap_modifiers(
            EquippedFishingRodKind::StarterBasic,
            &[treasure_bait_entry(
                FishingBaitCastConsumptionAction::AutoDetachMissingUnit,
            )],
        );
        assert_eq!((bait, gold), (false, false));
        let weights = fishing_catch_branch_pre_cap_weights_v1(bait, gold);
        assert_eq!(
            (weights.fish, weights.junk, weights.treasure),
            (70_400, 6_800, 2_800)
        );
    }
}
