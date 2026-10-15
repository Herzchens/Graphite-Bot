use sqlx::{Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::fishing_unmodified_line_stage::resolve_manual_unmodified_line_stage_v1;

use crate::{
    AppliedFishingRodDurabilityState, CanonicalEnchant, EquipmentTier, EquippedFishingRodKind,
    FishingArea, FishingBait, FishingBaitCastConsumptionAction, FishingBaitCastConsumptionEntry,
    FishingBaitCastSettlementReceipt, FishingBaitStateError, FishingCatchBranch,
    FishingCatchBranchPreCapWeightsV1, FishingDirectBookCandidateV1, FishingDirectBookRollError,
    FishingFishInstanceDeliveryError, FishingFishInstanceDeliveryReceiptV1,
    FishingFishInstanceDeliveryRequestV1, FishingNonFishOutcomeDeliveryReceiptV1,
    FishingNonFishPayloadDeliveryError, FishingNonFishRollError, FishingResolvedNonFishOutcomeV1,
    FishingRodDurabilityMutationError, FishingRodDurabilityMutationReceiptV1,
    FishingRodDurabilityResolution, ManualFishingCastPreflight, ManualFishingCastPreflightError,
    ManualFishingProgressionSettlementError, ManualFishingProgressionSettlementReceiptV1,
    ManualFishingRngDrawError, ManualFishingSuccessfulProgressionOutcomeV1,
    ManualFishingUnmodifiedCapabilityError, ManualFishingUnmodifiedCapabilityPreviewV1,
    ManualFishingUnmodifiedFishCandidateError, ManualFishingUnmodifiedLineStageError,
    ManualFishingUnmodifiedLineStageV1, apply_operation_fishing_rod_durability_v1,
    deliver_fishing_fish_instance_v1, deliver_fishing_non_fish_outcome_v1,
    fishing_catch_branch_pre_cap_weights_v1, lock_manual_fishing_cast_preflight,
    preview_manual_fishing_unmodified_fish_candidate_v1,
    preview_manual_unmodified_fish_capability_v1, resolve_manual_fishing_direct_book_candidate_v1,
    resolve_manual_fishing_non_fish_outcome_v1, resolve_manual_fishing_unmodified_base_branch_v1,
    settle_active_fishing_baits_for_cast, settle_manual_fishing_successful_progression_v1,
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
    DirectBookRoll(#[from] FishingDirectBookRollError),
    #[error("the actual cast branch is not Treasure")]
    BookRequiresTreasureBranch,
    #[error("the actual Treasure category is not Enchant Book")]
    BookRequiresSelectedEnchantBook,

    #[error(transparent)]
    CatchBranchRng(#[from] ManualFishingRngDrawError),
    #[error(transparent)]
    NonFishDelivery(#[from] FishingNonFishPayloadDeliveryError),
    #[error(transparent)]
    SuccessfulProgression(#[from] ManualFishingProgressionSettlementError),
    #[error(transparent)]
    UnmodifiedFish(#[from] ManualFishingUnmodifiedFishCandidateError),
    #[error("the locked cast contains modifiers requiring the shared capped Fish resolver")]
    ModifiedFishRequiresSharedComposer,
    #[error(transparent)]
    FishDelivery(#[from] FishingFishInstanceDeliveryError),
    #[error("unmodified Starter Pool provisional fish delivery requires Starter Pool")]
    ProvisionalFishOutsideStarterPool,
    #[error(
        "baseline Starter Basic outcome staging requires Starter Pool with Starter Basic equipped"
    )]
    StarterBaselineRequiresStarterBasic,
    #[error(transparent)]
    UnmodifiedCapability(#[from] ManualFishingUnmodifiedCapabilityError),
    #[error(transparent)]
    UnmodifiedLineStage(#[from] ManualFishingUnmodifiedLineStageError),
    #[error(transparent)]
    RodDurability(#[from] FishingRodDurabilityMutationError),
}

/// Provisional Junk or non-book Treasure delivery and reward children in a single
/// uncommitted cast transaction. These receipts are NOT a terminal catch result:
/// the final catch-branch roll and rod consequences have not been resolved.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingManualFishingNonFishReceiptV1 {
    pub outcome: FishingResolvedNonFishOutcomeV1,
    pub delivery: FishingNonFishOutcomeDeliveryReceiptV1,
    pub progression: ManualFishingProgressionSettlementReceiptV1,
}

/// Internal, rollback-only resolution of the complete *baseline Starter Basic* branch:
/// its deterministic Fish/Junk/non-book Treasure asset and successful XP children.
/// Rod wear is physically unavailable on the unbreakable Starter Basic, but operation
/// result, audit, outbox and command integration are still absent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PendingManualFishingStarterBaseOutcomeV1 {
    Fish {
        delivery: FishingFishInstanceDeliveryReceiptV1,
        progression: ManualFishingProgressionSettlementReceiptV1,
    },
    NonFish {
        staged: PendingManualFishingNonFishReceiptV1,
    },
    DirectBookPending {
        candidate: FishingDirectBookCandidateV1,
    },
}

/// Rollback-only selected baseline cast outcome with its associated Rod consequence.
///
/// A survived over-cap line is *not* a landed Fish. Its exact rational catch/escape
/// roll is not yet frozen into a semantic RNG resolver, so no Fish/XP/wear is staged
/// for that branch by this API. All variants remain non-committable until the
/// operation result/audit/outbox and remaining modifiers/Mending are composed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PendingManualFishingUnmodifiedSelectedOutcomeV1 {
    LandedFish {
        delivery: FishingFishInstanceDeliveryReceiptV1,
        progression: ManualFishingProgressionSettlementReceiptV1,
        durability: AppliedFishingRodDurabilityState,
    },
    NonFish {
        staged: PendingManualFishingNonFishReceiptV1,
        durability: AppliedFishingRodDurabilityState,
    },
    LineBroken {
        durability: AppliedFishingRodDurabilityState,
    },
    OverCapCatchPending {
        line_stage: ManualFishingUnmodifiedLineStageV1,
    },
    DirectBookPending {
        candidate: FishingDirectBookCandidateV1,
    },
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
    staged_durability: Option<FishingRodDurabilityMutationReceiptV1>,
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

    /// Previews a direct Enchant Book identity only if this *unmodified* operation's
    /// actual CatchBranch and TreasureResult RNG have both selected Enchant Book.
    ///
    /// Only slot 0 is eligible with no Multi Treasure enchant. This does not
    /// materialize a Book ItemInstance, grant rewards or authorize cast commit.
    pub fn preview_unmodified_selected_direct_book_v1(
        &self,
    ) -> Result<FishingDirectBookCandidateV1, ManualFishingCastOwnerError> {
        if self.preview_unmodified_base_branch_v1()? != FishingCatchBranch::Treasure {
            return Err(ManualFishingCastOwnerError::BookRequiresTreasureBranch);
        }
        match self.preview_non_fish_candidate_v1(FishingCatchBranch::Treasure) {
            Err(ManualFishingCastOwnerError::NonFishRoll(
                FishingNonFishRollError::EnchantBookRequiresDedicatedResolver,
            )) => Ok(resolve_manual_fishing_direct_book_candidate_v1(
                self.preflight.rng,
                0,
            )?),
            Ok(_) => Err(ManualFishingCastOwnerError::BookRequiresSelectedEnchantBook),
            Err(other) => Err(other),
        }
    }

    /// Stages one provisional Junk/non-book Treasure candidate, its canonical stack
    /// outputs and exact successful-progression children under this same locked transaction.
    ///
    /// The passed branch is NOT authoritative until the future full cast owner resolves
    /// final capped catch weights and the CatchBranch RNG domain. In particular this
    /// method never commits, draws branch RNG, or resolves ordinary Rod wear/Mending.
    /// Direct Enchant Book outcomes fail closed before issuing any payload.
    ///
    /// Same-operation repeated staging reuses the underlying mutation-key receipts.
    /// Rollback removes bait use, pending overflow, assets and both XP children together.
    pub async fn stage_non_fish_candidate_v1(
        &mut self,
        candidate: FishingCatchBranch,
    ) -> Result<PendingManualFishingNonFishReceiptV1, ManualFishingCastOwnerError> {
        let outcome = self.preview_non_fish_candidate_v1(candidate)?;
        let delivery = deliver_fishing_non_fish_outcome_v1(
            &mut self.tx,
            self.preflight.operation_id,
            self.preflight.player_id,
            &outcome,
        )
        .await?;
        let progression_outcome = match &outcome {
            FishingResolvedNonFishOutcomeV1::Junk { .. } => {
                ManualFishingSuccessfulProgressionOutcomeV1::Junk
            }
            FishingResolvedNonFishOutcomeV1::Treasure { payloads, .. } => {
                ManualFishingSuccessfulProgressionOutcomeV1::Treasure {
                    landed_count: u8::try_from(payloads.len())
                        .expect("canonical Treasure count never exceeds three"),
                }
            }
        };
        let progression = settle_manual_fishing_successful_progression_v1(
            &mut self.tx,
            self.preflight.operation_id,
            self.preflight.player_id,
            &progression_outcome,
        )
        .await?;
        Ok(PendingManualFishingNonFishReceiptV1 {
            outcome,
            delivery,
            progression,
        })
    }

    /// Selects the real versioned CatchBranch RNG outcome only for a cast whose
    /// authoritative locked Rod/bait state has no Fishing modifiers. This is
    /// still a provisional branch until all cast consequences are finalized.
    pub fn preview_unmodified_base_branch_v1(
        &self,
    ) -> Result<FishingCatchBranch, ManualFishingCastOwnerError> {
        self.ensure_unmodified_cast_v1()?;
        Ok(resolve_manual_fishing_unmodified_base_branch_v1(
            self.preflight.rng,
        )?)
    }

    fn ensure_unmodified_cast_v1(&self) -> Result<(), ManualFishingCastOwnerError> {
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
        Ok(())
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
        self.ensure_unmodified_cast_v1()?;
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

    /// Resolves the actual zero-modifier CatchBranch RNG result, then stages its
    /// matching unique Fish or non-book Junk/Treasure and exact successful XP under
    /// the same still-uncommittable transaction.
    ///
    /// This path is deliberately restricted to the unbreakable Starter Basic Rod
    /// in Starter Pool, where Fish physically lands without line risk, and to casts
    /// with no active bait or enchants. Direct Enchant Book outcomes return a pending identity without issuing
    /// any asset, XP or Rod wear until the separate Book settlement is composed.
    ///
    /// A same-operation repeated call replays the original item/XP child receipts.
    /// The pending owner has no commit method: these staged effects roll back unless
    /// the future final lifecycle also implements operation/audit/outbox settlement.
    pub async fn stage_unmodified_starter_base_outcome_v1(
        &mut self,
    ) -> Result<PendingManualFishingStarterBaseOutcomeV1, ManualFishingCastOwnerError> {
        if self.preflight.area_access.area != FishingArea::StarterPool
            || self.preflight.rod.kind != EquippedFishingRodKind::StarterBasic
        {
            return Err(ManualFishingCastOwnerError::StarterBaselineRequiresStarterBasic);
        }
        let selected = self.preview_unmodified_base_branch_v1()?;
        match selected {
            FishingCatchBranch::Fish => {
                let delivery = self.stage_unmodified_starter_pool_fish_v1().await?;
                let progression = settle_manual_fishing_successful_progression_v1(
                    &mut self.tx,
                    self.preflight.operation_id,
                    self.preflight.player_id,
                    &ManualFishingSuccessfulProgressionOutcomeV1::Fish {
                        landed_rarities: vec![delivery.rarity],
                    },
                )
                .await?;
                Ok(PendingManualFishingStarterBaseOutcomeV1::Fish {
                    delivery,
                    progression,
                })
            }
            FishingCatchBranch::Junk | FishingCatchBranch::Treasure => {
                match self.stage_non_fish_candidate_v1(selected).await {
                    Ok(staged) => Ok(PendingManualFishingStarterBaseOutcomeV1::NonFish { staged }),
                    Err(ManualFishingCastOwnerError::NonFishRoll(
                        FishingNonFishRollError::EnchantBookRequiresDedicatedResolver,
                    )) => Ok(
                        PendingManualFishingStarterBaseOutcomeV1::DirectBookPending {
                            candidate: self.preview_unmodified_selected_direct_book_v1()?,
                        },
                    ),
                    Err(other) => Err(other),
                }
            }
        }
    }

    /// Stages the Rod consequence of a pinned zero-modifier cast with no user-supplied
    /// outcome or Unbreaking flag. A selected Fish branch resolves LineBreak first;
    /// any surviving/within-capability Fish or non-Fish branch consumes the one
    /// ordinary completed-attempt wear event (Starter Basic stays unbreakable).
    ///
    /// The operation-keyed asset event enforces durable replay, while the transaction-owner
    /// cache also avoids redundant event queries within one attempt. The still-uncommittable transaction
    /// rolls this child back with every other provisional cast mutation.
    pub async fn stage_unmodified_rod_durability_v1(
        &mut self,
    ) -> Result<AppliedFishingRodDurabilityState, ManualFishingCastOwnerError> {
        self.ensure_unmodified_cast_v1()?;
        if let Some(staged) = &self.staged_durability {
            return Ok(staged.applied.clone());
        }
        let branch = self.preview_unmodified_base_branch_v1()?;
        let resolution = if branch == FishingCatchBranch::Fish
            && matches!(
                self.preview_unmodified_line_stage_v1(0)?,
                ManualFishingUnmodifiedLineStageV1::LineBroken { .. }
            ) {
            FishingRodDurabilityResolution::LineBreak
        } else {
            FishingRodDurabilityResolution::CompletedCastAttempt {
                ordinary_event_prevented_by_unbreaking: false,
            }
        };
        let receipt = apply_operation_fishing_rod_durability_v1(
            &mut self.tx,
            self.preflight.operation_id,
            self.preflight.player_id,
            self.preflight.area_access.area,
            self.preflight.rod.current_durability,
            resolution,
        )
        .await?;
        let applied = receipt.applied.clone();
        self.staged_durability = Some(receipt);
        Ok(applied)
    }

    /// Composes the actual pinned zero-modifier branch and its physically resolved
    /// Fish/Junk/non-book Treasure output, exact successful XP and one Rod event.
    ///
    /// The line-break path destroys the Rod without issuing Fish or successful XP.
    /// A survived over-cap Fish stays pending with no output/durability/XP until
    /// the rational catch/escape RNG stage has an approved versioned mapping.
    /// Direct Enchant Book Treasure remains identity-only, without assets or XP.
    ///
    /// This transaction-owned staging method cannot commit the operation or
    /// expose an application adapter; repeated calls replay the same child receipts.
    pub async fn stage_unmodified_selected_outcome_v1(
        &mut self,
    ) -> Result<PendingManualFishingUnmodifiedSelectedOutcomeV1, ManualFishingCastOwnerError> {
        let branch = self.preview_unmodified_base_branch_v1()?;
        match branch {
            FishingCatchBranch::Fish => {
                match self.preview_unmodified_line_stage_v1(0)? {
                    ManualFishingUnmodifiedLineStageV1::LineBroken { .. } => {
                        let durability = self.stage_unmodified_rod_durability_v1().await?;
                        Ok(PendingManualFishingUnmodifiedSelectedOutcomeV1::LineBroken {
                            durability,
                        })
                    }
                    line_stage @ ManualFishingUnmodifiedLineStageV1::LineSurvivedOverCapCatchPending { .. } => {
                        Ok(PendingManualFishingUnmodifiedSelectedOutcomeV1::OverCapCatchPending {
                            line_stage,
                        })
                    }
                    ManualFishingUnmodifiedLineStageV1::LandedWithoutLineRisk => {
                        let request = self.preview_unmodified_fish_candidate_v1(0)?;
                        let delivery = deliver_fishing_fish_instance_v1(
                            &mut self.tx,
                            self.preflight.operation_id,
                            self.preflight.player_id,
                            request,
                        )
                        .await?;
                        let progression = settle_manual_fishing_successful_progression_v1(
                            &mut self.tx,
                            self.preflight.operation_id,
                            self.preflight.player_id,
                            &ManualFishingSuccessfulProgressionOutcomeV1::Fish {
                                landed_rarities: vec![delivery.rarity],
                            },
                        )
                        .await?;
                        let durability = self.stage_unmodified_rod_durability_v1().await?;
                        Ok(PendingManualFishingUnmodifiedSelectedOutcomeV1::LandedFish {
                            delivery,
                            progression,
                            durability,
                        })
                    }
                }
            }
            FishingCatchBranch::Junk | FishingCatchBranch::Treasure => {
                match self.stage_non_fish_candidate_v1(branch).await {
                    Ok(staged) => {
                        let durability = self.stage_unmodified_rod_durability_v1().await?;
                        Ok(PendingManualFishingUnmodifiedSelectedOutcomeV1::NonFish {
                            staged,
                            durability,
                        })
                    }
                    Err(ManualFishingCastOwnerError::NonFishRoll(
                        FishingNonFishRollError::EnchantBookRequiresDedicatedResolver,
                    )) => Ok(PendingManualFishingUnmodifiedSelectedOutcomeV1::DirectBookPending {
                        candidate: self.preview_unmodified_selected_direct_book_v1()?,
                    }),
                    Err(other) => Err(other),
                }
            }
        }
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
        staged_durability: None,
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
