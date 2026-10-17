use sqlx::{Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::fishing_terminal::{
    finalize_manual_fishing_sharp_hook_v1, finalize_manual_fishing_starter_base_v1,
    finalize_manual_fishing_strengthen_v1, finalize_manual_fishing_unbreaking_x_v1,
    finalize_manual_fishing_unmodified_ordinary_v1,
};
use crate::fishing_unmodified_line_stage::{
    resolve_manual_sturdy_bait_line_stage_v1, resolve_manual_unmodified_line_stage_v1,
};

use crate::{
    ActionSpeedBonus, ActionSpeedPolicyError, AppliedFishingRodDurabilityState,
    BAIT_RACK_MAX_LEVEL, CanonicalEnchant, EquipmentTier, EquippedFishingRodKind, FishingArea,
    FishingBait, FishingBaitCastConsumptionAction, FishingBaitCastConsumptionEntry,
    FishingBaitCastSettlementReceipt, FishingBaitStateError, FishingCatchBranch,
    FishingCatchBranchPreCapWeightsV1, FishingDirectBookCandidateV1, FishingDirectBookRollError,
    FishingFishInstanceDeliveryError, FishingFishInstanceDeliveryReceiptV1,
    FishingFishInstanceDeliveryRequestV1, FishingNonFishOutcomeDeliveryReceiptV1,
    FishingNonFishPayloadDeliveryError, FishingNonFishRollError, FishingOverCapError,
    FishingResolvedNonFishOutcomeV1, FishingRodDurabilityMutationError,
    FishingRodDurabilityMutationReceiptV1, FishingRodDurabilityResolution, FishingRodEnchant,
    FishingRodLevelXEffect, FishingRodLevelXPolicyError, MULTI_TREASURE_MAX_LEVEL,
    MULTICATCH_MAX_LEVEL, ManualFishingCastPreflight, ManualFishingCastPreflightError,
    ManualFishingMulticatchCountV1, ManualFishingMulticatchRollError, ManualFishingOperationError,
    ManualFishingOperationResolutionV1, ManualFishingProgressionSettlementError,
    ManualFishingProgressionSettlementReceiptV1, ManualFishingRngDrawError,
    ManualFishingSchoolBaitError, ManualFishingSchoolBaitQuantityV1,
    ManualFishingSharpHookTerminalError, ManualFishingSharpHookTerminalOutcomeV1,
    ManualFishingSharpHookTerminalReceiptV1, ManualFishingStarterBaseTerminalError,
    ManualFishingStarterBaseTerminalOutcomeV1, ManualFishingStarterBaseTerminalReceiptV1,
    ManualFishingStrengthenTerminalError, ManualFishingStrengthenTerminalOutcomeV1,
    ManualFishingStrengthenTerminalReceiptV1, ManualFishingSuccessfulProgressionOutcomeV1,
    ManualFishingTerminalReceiptKindV1, ManualFishingTreasureXBranchError,
    ManualFishingUnbreakingXError, ManualFishingUnbreakingXTerminalError,
    ManualFishingUnbreakingXTerminalOutcomeV1, ManualFishingUnbreakingXTerminalReceiptV1,
    ManualFishingUnbreakingXWearPreviewV1, ManualFishingUnmodifiedCapabilityError,
    ManualFishingUnmodifiedCapabilityPreviewV1, ManualFishingUnmodifiedFishCandidateError,
    ManualFishingUnmodifiedLineStageError, ManualFishingUnmodifiedLineStageV1,
    ManualFishingUnmodifiedOrdinaryTerminalError, ManualFishingUnmodifiedOrdinaryTerminalOutcomeV1,
    ManualFishingUnmodifiedOrdinaryTerminalReceiptV1, ManualMineFishCooldownPolicy,
    OverCapCatchChancePolicy, SHARP_HOOK_MAX_LEVEL, STRENGTHEN_MAX_LEVEL,
    apply_operation_fishing_rod_durability_v1, canonical_enchant_max_resulting_level,
    deliver_fishing_fish_instance_v1, deliver_fishing_non_fish_outcome_v1,
    fishing_catch_branch_pre_cap_weights_v1, fishing_rod_level_x_policy,
    lock_manual_fishing_cast_preflight, lock_or_create_manual_fishing_operation_v1,
    manual_mine_fish_cooldown_policy, preview_manual_fishing_unmodified_fish_candidate_v1,
    preview_manual_multicatch_fish_capability_v1, preview_manual_school_bait_fish_capability_v1,
    preview_manual_strengthen_fish_capability_v1, preview_manual_sturdy_bait_fish_capability_v1,
    preview_manual_unmodified_fish_capability_v1, preview_over_cap_catch_chance,
    resolve_manual_fishing_direct_book_candidate_v1,
    resolve_manual_fishing_multi_treasure_count_v1, resolve_manual_fishing_multicatch_count_v1,
    resolve_manual_fishing_non_fish_outcome_v1, resolve_manual_fishing_operation_v1,
    resolve_manual_fishing_school_bait_quantity_v1, resolve_manual_fishing_treasure_x_branch_v1,
    resolve_manual_fishing_unbreaking_x_wear_v1, resolve_manual_fishing_unmodified_base_branch_v1,
    settle_active_fishing_baits_for_cast, settle_manual_fishing_successful_progression_v1,
};

#[derive(Debug, Error)]
pub enum ManualFishingCastOwnerError {
    #[error(transparent)]
    Operation(#[from] ManualFishingOperationError),
    #[error(transparent)]
    StarterTerminal(#[from] ManualFishingStarterBaseTerminalError),
    #[error(transparent)]
    UnmodifiedOrdinaryTerminal(#[from] ManualFishingUnmodifiedOrdinaryTerminalError),
    #[error(transparent)]
    StrengthenTerminal(#[from] ManualFishingStrengthenTerminalError),
    #[error("survived over-cap Fish cannot commit from the Strengthen terminal path")]
    StrengthenTerminalOverCapPending,
    #[error("direct Enchant Book cannot commit from the Strengthen terminal path")]
    StrengthenTerminalDirectBookPending,
    #[error("resolved Strengthen terminal outcome is missing its operation-keyed Rod receipt")]
    StrengthenTerminalDurabilityMissing,
    #[error("resolved Strengthen terminal durability disagrees with its persisted receipt")]
    StrengthenTerminalDurabilityMismatch,
    #[error(transparent)]
    SharpHookTerminal(#[from] ManualFishingSharpHookTerminalError),
    #[error("survived over-cap Fish cannot commit from the SharpHook terminal path")]
    SharpHookTerminalOverCapPending,
    #[error("direct Enchant Book cannot commit from the SharpHook terminal path")]
    SharpHookTerminalDirectBookPending,
    #[error("resolved SharpHook terminal outcome is missing its operation-keyed Rod receipt")]
    SharpHookTerminalDurabilityMissing,
    #[error("resolved SharpHook terminal durability disagrees with its persisted receipt")]
    SharpHookTerminalDurabilityMismatch,
    #[error(transparent)]
    UnbreakingXTerminal(#[from] ManualFishingUnbreakingXTerminalError),
    #[error("survived over-cap Fish cannot commit from the Unbreaking X terminal path")]
    UnbreakingXTerminalOverCapPending,
    #[error("direct Enchant Book cannot commit from the Unbreaking X terminal path")]
    UnbreakingXTerminalDirectBookPending,
    #[error("resolved Unbreaking X terminal outcome is missing its operation-keyed Rod receipt")]
    UnbreakingXTerminalDurabilityMissing,
    #[error("resolved Unbreaking X terminal durability disagrees with its persisted receipt")]
    UnbreakingXTerminalDurabilityMismatch,
    #[error(
        "unmodified ordinary terminal path requires an ordinary non-Gold Rod with no bait or enchants"
    )]
    UnmodifiedOrdinaryTerminalRequiresOrdinaryRod,
    #[error("survived over-cap Fish cannot commit until exact catch/escape RNG is frozen")]
    UnmodifiedOrdinaryTerminalOverCapPending,
    #[error("direct Enchant Book cannot commit until unique Book delivery is implemented")]
    UnmodifiedOrdinaryTerminalDirectBookPending,
    #[error(
        "resolved ordinary terminal outcome is missing its operation-keyed Rod durability receipt"
    )]
    UnmodifiedOrdinaryTerminalDurabilityMissing,
    #[error("resolved ordinary terminal durability state disagrees with its persisted receipt")]
    UnmodifiedOrdinaryTerminalDurabilityMismatch,
    #[error(
        "direct Enchant Book delivery is not implemented, so this Starter Basic cast cannot commit"
    )]
    StarterTerminalDirectBookPending,
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
    #[error(transparent)]
    UnbreakingX(#[from] ManualFishingUnbreakingXError),
    #[error(
        "Unbreaking X baseline path requires one ordinary non-Gold Rod with only Unbreaking X and no consumed bait"
    )]
    UnbreakingXRequiresExactLoadout,
    #[error(
        "Strengthen baseline path requires one ordinary non-Gold Rod with only Strengthen I-X and no consumed bait"
    )]
    StrengthenRequiresExactLoadout,
    #[error(transparent)]
    SharpHookOverCap(#[from] FishingOverCapError),
    #[error(
        "SharpHook baseline path requires one ordinary non-Gold Rod with only SharpHook I-X and no consumed bait"
    )]
    SharpHookRequiresExactLoadout,
    #[error(
        "Sturdy Bait baseline path requires one ordinary non-Gold Rod with no enchants and exactly one consumed Sturdy Bait"
    )]
    SturdyBaitRequiresExactLoadout,
    #[error(transparent)]
    SchoolBait(#[from] ManualFishingSchoolBaitError),
    #[error(
        "School Bait baseline path requires one ordinary non-Gold Rod with no enchants and exactly one consumed School Bait"
    )]
    SchoolBaitRequiresExactLoadout,
    #[error("School Bait quantity proc is only eligible after the selected catch branch is Fish")]
    SchoolBaitRequiresFishBranch,
    #[error("SharpHook capability and line-stage resolution disagree")]
    SharpHookCapabilityStageMismatch,
    #[error(
        "Bait Rack baseline path requires one ordinary non-Gold Rod with only Bait Rack I-III and no consumed bait"
    )]
    BaitRackRequiresExactLoadout,
    #[error(transparent)]
    ActionSpeed(#[from] ActionSpeedPolicyError),
    #[error(transparent)]
    RodLevelX(#[from] FishingRodLevelXPolicyError),
    #[error(
        "Lure X baseline path requires one ordinary non-Gold Rod with only Lure X and no consumed bait"
    )]
    LureXRequiresExactLoadout,
    #[error("Lure X scalar policy is inconsistent with the shared Fishing speed bucket contract")]
    LureXPolicyMismatch,
    #[error(transparent)]
    MulticatchRoll(#[from] ManualFishingMulticatchRollError),
    #[error(
        "Multicatch baseline path requires one ordinary non-Gold Rod with only Multicatch I-X and no consumed bait"
    )]
    MulticatchRequiresExactLoadout,
    #[error(
        "Multi Treasure baseline path requires one ordinary non-Gold Rod with only Multi Treasure I-X and no consumed bait"
    )]
    MultiTreasureRequiresExactLoadout,
    #[error(transparent)]
    TreasureXBranch(#[from] ManualFishingTreasureXBranchError),
    #[error(
        "Treasure X path requires one ordinary non-Gold Rod with only Treasure X and no consumed bait"
    )]
    TreasureXRequiresExactLoadout,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PendingManualFishingTreasureXSelectedOutcomeV1 {
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PendingManualFishingMultiTreasureSelectedOutcomeV1 {
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
    DirectBooksPending {
        count: u8,
        candidates: Vec<FishingDirectBookCandidateV1>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PendingManualFishingMulticatchSelectedOutcomeV1 {
    LandedFish {
        count: ManualFishingMulticatchCountV1,
        deliveries: Vec<FishingFishInstanceDeliveryReceiptV1>,
        progression: ManualFishingProgressionSettlementReceiptV1,
        durability: AppliedFishingRodDurabilityState,
    },
    NonFish {
        staged: PendingManualFishingNonFishReceiptV1,
        durability: AppliedFishingRodDurabilityState,
    },
    LineBroken {
        count: ManualFishingMulticatchCountV1,
        durability: AppliedFishingRodDurabilityState,
    },
    OverCapCatchPending {
        count: ManualFishingMulticatchCountV1,
        line_stage: ManualFishingUnmodifiedLineStageV1,
    },
    DirectBookPending {
        candidate: FishingDirectBookCandidateV1,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PendingManualFishingBaitRackSelectedOutcomeV1 {
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PendingManualFishingLureXSelectedOutcomeV1 {
    LandedFish {
        delivery: FishingFishInstanceDeliveryReceiptV1,
        progression: ManualFishingProgressionSettlementReceiptV1,
        durability: AppliedFishingRodDurabilityState,
        cooldown: ManualMineFishCooldownPolicy,
    },
    NonFish {
        staged: PendingManualFishingNonFishReceiptV1,
        durability: AppliedFishingRodDurabilityState,
        cooldown: ManualMineFishCooldownPolicy,
    },
    LineBroken {
        durability: AppliedFishingRodDurabilityState,
        cooldown: ManualMineFishCooldownPolicy,
    },
    OverCapCatchPending {
        line_stage: ManualFishingUnmodifiedLineStageV1,
        cooldown: ManualMineFishCooldownPolicy,
    },
    DirectBookPending {
        candidate: FishingDirectBookCandidateV1,
        cooldown: ManualMineFishCooldownPolicy,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PendingManualFishingSchoolBaitSelectedOutcomeV1 {
    LandedFish {
        quantity: ManualFishingSchoolBaitQuantityV1,
        deliveries: Vec<FishingFishInstanceDeliveryReceiptV1>,
        progression: ManualFishingProgressionSettlementReceiptV1,
        durability: AppliedFishingRodDurabilityState,
    },
    NonFish {
        staged: PendingManualFishingNonFishReceiptV1,
        durability: AppliedFishingRodDurabilityState,
    },
    LineBroken {
        quantity: ManualFishingSchoolBaitQuantityV1,
        durability: AppliedFishingRodDurabilityState,
    },
    OverCapCatchPending {
        quantity: ManualFishingSchoolBaitQuantityV1,
        line_stage: ManualFishingUnmodifiedLineStageV1,
    },
    DirectBookPending {
        candidate: FishingDirectBookCandidateV1,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PendingManualFishingSturdyBaitSelectedOutcomeV1 {
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PendingManualFishingSharpHookSelectedOutcomeV1 {
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
        catch_chance: OverCapCatchChancePolicy,
    },
    DirectBookPending {
        candidate: FishingDirectBookCandidateV1,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PendingManualFishingStrengthenSelectedOutcomeV1 {
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PendingManualFishingUnbreakingXSelectedOutcomeV1 {
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
pub enum ManualFishingCastRequestResolutionV1<'c> {
    Pending(Box<PendingManualFishingCastV1<'c>>),
    CommittedStarterBase(Box<ManualFishingStarterBaseTerminalReceiptV1>),
    CommittedUnmodifiedOrdinary(Box<ManualFishingUnmodifiedOrdinaryTerminalReceiptV1>),
    CommittedStrengthen(Box<ManualFishingStrengthenTerminalReceiptV1>),
    CommittedSharpHook(Box<ManualFishingSharpHookTerminalReceiptV1>),
    CommittedUnbreakingX(Box<ManualFishingUnbreakingXTerminalReceiptV1>),
}

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

    /// Previews a direct Enchant Book identity for the narrow Unbreaking-X-only
    /// baseline when the actual branch and Treasure result select a Book.
    pub fn preview_unbreaking_x_selected_direct_book_v1(
        &self,
    ) -> Result<FishingDirectBookCandidateV1, ManualFishingCastOwnerError> {
        if self.preview_unbreaking_x_base_branch_v1()? != FishingCatchBranch::Treasure {
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

    fn ensure_treasure_x_only_cast_v1(&self) -> Result<(), ManualFishingCastOwnerError> {
        let EquippedFishingRodKind::Ordinary { tier } = self.preflight.rod.kind else {
            return Err(ManualFishingCastOwnerError::TreasureXRequiresExactLoadout);
        };
        if tier == EquipmentTier::Gold || self.bait.plan.consumed_bait_categories != 0 {
            return Err(ManualFishingCastOwnerError::TreasureXRequiresExactLoadout);
        }
        let [embedded] = self.preflight.rod.embedded_enchants.as_slice() else {
            return Err(ManualFishingCastOwnerError::TreasureXRequiresExactLoadout);
        };
        if embedded.enchant != CanonicalEnchant::FishingRodTreasure
            || embedded.level
                != canonical_enchant_max_resulting_level(CanonicalEnchant::FishingRodTreasure)
        {
            return Err(ManualFishingCastOwnerError::TreasureXRequiresExactLoadout);
        }
        Ok(())
    }

    pub fn preview_treasure_x_branch_v1(
        &self,
    ) -> Result<FishingCatchBranch, ManualFishingCastOwnerError> {
        self.ensure_treasure_x_only_cast_v1()?;
        Ok(resolve_manual_fishing_treasure_x_branch_v1(
            self.preflight.rng,
        )?)
    }

    fn preview_treasure_x_fish_candidate_v1(
        &self,
    ) -> Result<FishingFishInstanceDeliveryRequestV1, ManualFishingCastOwnerError> {
        self.ensure_treasure_x_only_cast_v1()?;
        Ok(preview_manual_fishing_unmodified_fish_candidate_v1(
            self.preflight.rng,
            self.preflight.area_access.area,
            0,
        )?)
    }

    pub fn preview_treasure_x_line_stage_v1(
        &self,
    ) -> Result<ManualFishingUnmodifiedLineStageV1, ManualFishingCastOwnerError> {
        let candidate = self.preview_treasure_x_fish_candidate_v1()?;
        let capability = preview_manual_unmodified_fish_capability_v1(
            self.preflight.area_access.area,
            self.preflight.rod.kind,
            candidate.numeric,
        )?;
        Ok(resolve_manual_unmodified_line_stage_v1(
            self.preflight.rng,
            capability,
        )?)
    }

    pub fn preview_treasure_x_selected_direct_book_v1(
        &self,
    ) -> Result<FishingDirectBookCandidateV1, ManualFishingCastOwnerError> {
        if self.preview_treasure_x_branch_v1()? != FishingCatchBranch::Treasure {
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

    pub async fn stage_treasure_x_rod_durability_v1(
        &mut self,
    ) -> Result<AppliedFishingRodDurabilityState, ManualFishingCastOwnerError> {
        self.ensure_treasure_x_only_cast_v1()?;
        if let Some(staged) = &self.staged_durability {
            return Ok(staged.applied.clone());
        }
        let branch = self.preview_treasure_x_branch_v1()?;
        let resolution = if branch == FishingCatchBranch::Fish
            && matches!(
                self.preview_treasure_x_line_stage_v1()?,
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

    fn multi_treasure_only_level_v1(&self) -> Result<u8, ManualFishingCastOwnerError> {
        let EquippedFishingRodKind::Ordinary { tier } = self.preflight.rod.kind else {
            return Err(ManualFishingCastOwnerError::MultiTreasureRequiresExactLoadout);
        };
        if tier == EquipmentTier::Gold || self.bait.plan.consumed_bait_categories != 0 {
            return Err(ManualFishingCastOwnerError::MultiTreasureRequiresExactLoadout);
        }
        let [embedded] = self.preflight.rod.embedded_enchants.as_slice() else {
            return Err(ManualFishingCastOwnerError::MultiTreasureRequiresExactLoadout);
        };
        if embedded.enchant != CanonicalEnchant::MultiTreasure
            || !(1..=MULTI_TREASURE_MAX_LEVEL).contains(&embedded.level)
        {
            return Err(ManualFishingCastOwnerError::MultiTreasureRequiresExactLoadout);
        }
        Ok(embedded.level)
    }

    pub fn preview_multi_treasure_base_branch_v1(
        &self,
    ) -> Result<FishingCatchBranch, ManualFishingCastOwnerError> {
        self.multi_treasure_only_level_v1()?;
        Ok(resolve_manual_fishing_unmodified_base_branch_v1(
            self.preflight.rng,
        )?)
    }

    fn preview_multi_treasure_fish_candidate_v1(
        &self,
    ) -> Result<FishingFishInstanceDeliveryRequestV1, ManualFishingCastOwnerError> {
        self.multi_treasure_only_level_v1()?;
        Ok(preview_manual_fishing_unmodified_fish_candidate_v1(
            self.preflight.rng,
            self.preflight.area_access.area,
            0,
        )?)
    }

    pub fn preview_multi_treasure_line_stage_v1(
        &self,
    ) -> Result<ManualFishingUnmodifiedLineStageV1, ManualFishingCastOwnerError> {
        let candidate = self.preview_multi_treasure_fish_candidate_v1()?;
        let capability = preview_manual_unmodified_fish_capability_v1(
            self.preflight.area_access.area,
            self.preflight.rod.kind,
            candidate.numeric,
        )?;
        Ok(resolve_manual_unmodified_line_stage_v1(
            self.preflight.rng,
            capability,
        )?)
    }

    pub fn preview_multi_treasure_selected_direct_books_v1(
        &self,
    ) -> Result<(u8, Vec<FishingDirectBookCandidateV1>), ManualFishingCastOwnerError> {
        let level = self.multi_treasure_only_level_v1()?;
        if self.preview_multi_treasure_base_branch_v1()? != FishingCatchBranch::Treasure {
            return Err(ManualFishingCastOwnerError::BookRequiresTreasureBranch);
        }
        match self.preview_non_fish_candidate_v1(FishingCatchBranch::Treasure) {
            Err(ManualFishingCastOwnerError::NonFishRoll(
                FishingNonFishRollError::EnchantBookRequiresDedicatedResolver,
            )) => {
                let count =
                    resolve_manual_fishing_multi_treasure_count_v1(self.preflight.rng, level)?;
                let mut candidates = Vec::with_capacity(usize::from(count));
                for treasure_slot in 0..count {
                    candidates.push(resolve_manual_fishing_direct_book_candidate_v1(
                        self.preflight.rng,
                        treasure_slot,
                    )?);
                }
                Ok((count, candidates))
            }
            Ok(_) => Err(ManualFishingCastOwnerError::BookRequiresSelectedEnchantBook),
            Err(other) => Err(other),
        }
    }

    pub async fn stage_multi_treasure_rod_durability_v1(
        &mut self,
    ) -> Result<AppliedFishingRodDurabilityState, ManualFishingCastOwnerError> {
        self.multi_treasure_only_level_v1()?;
        if let Some(staged) = &self.staged_durability {
            return Ok(staged.applied.clone());
        }
        let branch = self.preview_multi_treasure_base_branch_v1()?;
        let resolution = if branch == FishingCatchBranch::Fish
            && matches!(
                self.preview_multi_treasure_line_stage_v1()?,
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

    fn multicatch_only_level_v1(&self) -> Result<u8, ManualFishingCastOwnerError> {
        let EquippedFishingRodKind::Ordinary { tier } = self.preflight.rod.kind else {
            return Err(ManualFishingCastOwnerError::MulticatchRequiresExactLoadout);
        };
        if tier == EquipmentTier::Gold || self.bait.plan.consumed_bait_categories != 0 {
            return Err(ManualFishingCastOwnerError::MulticatchRequiresExactLoadout);
        }
        let [embedded] = self.preflight.rod.embedded_enchants.as_slice() else {
            return Err(ManualFishingCastOwnerError::MulticatchRequiresExactLoadout);
        };
        if embedded.enchant != CanonicalEnchant::Multicatch
            || !(1..=MULTICATCH_MAX_LEVEL).contains(&embedded.level)
        {
            return Err(ManualFishingCastOwnerError::MulticatchRequiresExactLoadout);
        }
        Ok(embedded.level)
    }

    pub fn preview_multicatch_base_branch_v1(
        &self,
    ) -> Result<FishingCatchBranch, ManualFishingCastOwnerError> {
        self.multicatch_only_level_v1()?;
        Ok(resolve_manual_fishing_unmodified_base_branch_v1(
            self.preflight.rng,
        )?)
    }

    pub fn preview_multicatch_count_v1(
        &self,
    ) -> Result<ManualFishingMulticatchCountV1, ManualFishingCastOwnerError> {
        let level = self.multicatch_only_level_v1()?;
        Ok(resolve_manual_fishing_multicatch_count_v1(
            self.preflight.rng,
            level,
        )?)
    }

    pub fn preview_multicatch_fish_candidates_v1(
        &self,
    ) -> Result<Vec<FishingFishInstanceDeliveryRequestV1>, ManualFishingCastOwnerError> {
        let count = self.preview_multicatch_count_v1()?;
        let mut candidates = Vec::with_capacity(usize::from(count.fish_count));
        for fish_slot in 0..count.fish_count {
            candidates.push(preview_manual_fishing_unmodified_fish_candidate_v1(
                self.preflight.rng,
                self.preflight.area_access.area,
                fish_slot,
            )?);
        }
        Ok(candidates)
    }

    pub fn preview_multicatch_fish_capability_v1(
        &self,
    ) -> Result<ManualFishingUnmodifiedCapabilityPreviewV1, ManualFishingCastOwnerError> {
        let candidates = self.preview_multicatch_fish_candidates_v1()?;
        let numeric = candidates
            .iter()
            .map(|candidate| candidate.numeric)
            .collect::<Vec<_>>();
        Ok(preview_manual_multicatch_fish_capability_v1(
            self.preflight.area_access.area,
            self.preflight.rod.kind,
            &numeric,
        )?)
    }

    pub fn preview_multicatch_line_stage_v1(
        &self,
    ) -> Result<ManualFishingUnmodifiedLineStageV1, ManualFishingCastOwnerError> {
        let capability = self.preview_multicatch_fish_capability_v1()?;
        Ok(resolve_manual_unmodified_line_stage_v1(
            self.preflight.rng,
            capability,
        )?)
    }

    pub fn preview_multicatch_selected_direct_book_v1(
        &self,
    ) -> Result<FishingDirectBookCandidateV1, ManualFishingCastOwnerError> {
        if self.preview_multicatch_base_branch_v1()? != FishingCatchBranch::Treasure {
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

    pub async fn stage_multicatch_rod_durability_v1(
        &mut self,
    ) -> Result<AppliedFishingRodDurabilityState, ManualFishingCastOwnerError> {
        self.multicatch_only_level_v1()?;
        if let Some(staged) = &self.staged_durability {
            return Ok(staged.applied.clone());
        }
        let branch = self.preview_multicatch_base_branch_v1()?;
        let resolution = if branch == FishingCatchBranch::Fish
            && matches!(
                self.preview_multicatch_line_stage_v1()?,
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

    fn ensure_lure_x_only_cast_v1(&self) -> Result<(), ManualFishingCastOwnerError> {
        let EquippedFishingRodKind::Ordinary { tier } = self.preflight.rod.kind else {
            return Err(ManualFishingCastOwnerError::LureXRequiresExactLoadout);
        };
        if tier == EquipmentTier::Gold || self.bait.plan.consumed_bait_categories != 0 {
            return Err(ManualFishingCastOwnerError::LureXRequiresExactLoadout);
        }
        let [embedded] = self.preflight.rod.embedded_enchants.as_slice() else {
            return Err(ManualFishingCastOwnerError::LureXRequiresExactLoadout);
        };
        if embedded.enchant != CanonicalEnchant::Lure
            || embedded.level != canonical_enchant_max_resulting_level(CanonicalEnchant::Lure)
        {
            return Err(ManualFishingCastOwnerError::LureXRequiresExactLoadout);
        }
        Ok(())
    }

    /// Resolves the exact Lure-X-only manual Fishing cooldown through the existing
    /// shared action-speed bucket and canonical Mine/Fish floor.
    pub fn preview_lure_x_cooldown_v1(
        &self,
    ) -> Result<ManualMineFishCooldownPolicy, ManualFishingCastOwnerError> {
        self.ensure_lure_x_only_cast_v1()?;
        let policy = fishing_rod_level_x_policy(FishingRodEnchant::Lure)?;
        let FishingRodLevelXEffect::Lure {
            action_speed_rating_percent,
            uses_shared_fishing_speed_bucket,
        } = policy.effect
        else {
            return Err(ManualFishingCastOwnerError::LureXPolicyMismatch);
        };
        if !uses_shared_fishing_speed_bucket {
            return Err(ManualFishingCastOwnerError::LureXPolicyMismatch);
        }
        let bonus = ActionSpeedBonus::new(u64::from(action_speed_rating_percent), 100)?;
        Ok(manual_mine_fish_cooldown_policy(&[bonus])?)
    }

    pub fn preview_lure_x_base_branch_v1(
        &self,
    ) -> Result<FishingCatchBranch, ManualFishingCastOwnerError> {
        self.ensure_lure_x_only_cast_v1()?;
        Ok(resolve_manual_fishing_unmodified_base_branch_v1(
            self.preflight.rng,
        )?)
    }

    fn preview_lure_x_fish_candidate_v1(
        &self,
        fish_slot: u8,
    ) -> Result<FishingFishInstanceDeliveryRequestV1, ManualFishingCastOwnerError> {
        self.ensure_lure_x_only_cast_v1()?;
        Ok(preview_manual_fishing_unmodified_fish_candidate_v1(
            self.preflight.rng,
            self.preflight.area_access.area,
            fish_slot,
        )?)
    }

    pub fn preview_lure_x_line_stage_v1(
        &self,
        fish_slot: u8,
    ) -> Result<ManualFishingUnmodifiedLineStageV1, ManualFishingCastOwnerError> {
        let candidate = self.preview_lure_x_fish_candidate_v1(fish_slot)?;
        let capability = preview_manual_unmodified_fish_capability_v1(
            self.preflight.area_access.area,
            self.preflight.rod.kind,
            candidate.numeric,
        )?;
        Ok(resolve_manual_unmodified_line_stage_v1(
            self.preflight.rng,
            capability,
        )?)
    }

    pub fn preview_lure_x_selected_direct_book_v1(
        &self,
    ) -> Result<FishingDirectBookCandidateV1, ManualFishingCastOwnerError> {
        if self.preview_lure_x_base_branch_v1()? != FishingCatchBranch::Treasure {
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

    pub async fn stage_lure_x_rod_durability_v1(
        &mut self,
    ) -> Result<AppliedFishingRodDurabilityState, ManualFishingCastOwnerError> {
        self.ensure_lure_x_only_cast_v1()?;
        if let Some(staged) = &self.staged_durability {
            return Ok(staged.applied.clone());
        }
        let branch = self.preview_lure_x_base_branch_v1()?;
        let resolution = if branch == FishingCatchBranch::Fish
            && matches!(
                self.preview_lure_x_line_stage_v1(0)?,
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

    fn bait_rack_only_level_v1(&self) -> Result<u8, ManualFishingCastOwnerError> {
        let EquippedFishingRodKind::Ordinary { tier } = self.preflight.rod.kind else {
            return Err(ManualFishingCastOwnerError::BaitRackRequiresExactLoadout);
        };
        if tier == EquipmentTier::Gold || self.bait.plan.consumed_bait_categories != 0 {
            return Err(ManualFishingCastOwnerError::BaitRackRequiresExactLoadout);
        }
        let [embedded] = self.preflight.rod.embedded_enchants.as_slice() else {
            return Err(ManualFishingCastOwnerError::BaitRackRequiresExactLoadout);
        };
        if embedded.enchant != CanonicalEnchant::BaitRack
            || !(1..=BAIT_RACK_MAX_LEVEL).contains(&embedded.level)
        {
            return Err(ManualFishingCastOwnerError::BaitRackRequiresExactLoadout);
        }
        Ok(embedded.level)
    }

    pub fn preview_bait_rack_base_branch_v1(
        &self,
    ) -> Result<FishingCatchBranch, ManualFishingCastOwnerError> {
        self.bait_rack_only_level_v1()?;
        Ok(resolve_manual_fishing_unmodified_base_branch_v1(
            self.preflight.rng,
        )?)
    }

    fn preview_bait_rack_fish_candidate_v1(
        &self,
        fish_slot: u8,
    ) -> Result<FishingFishInstanceDeliveryRequestV1, ManualFishingCastOwnerError> {
        self.bait_rack_only_level_v1()?;
        Ok(preview_manual_fishing_unmodified_fish_candidate_v1(
            self.preflight.rng,
            self.preflight.area_access.area,
            fish_slot,
        )?)
    }

    pub fn preview_bait_rack_line_stage_v1(
        &self,
        fish_slot: u8,
    ) -> Result<ManualFishingUnmodifiedLineStageV1, ManualFishingCastOwnerError> {
        let candidate = self.preview_bait_rack_fish_candidate_v1(fish_slot)?;
        let capability = preview_manual_unmodified_fish_capability_v1(
            self.preflight.area_access.area,
            self.preflight.rod.kind,
            candidate.numeric,
        )?;
        Ok(resolve_manual_unmodified_line_stage_v1(
            self.preflight.rng,
            capability,
        )?)
    }

    pub fn preview_bait_rack_selected_direct_book_v1(
        &self,
    ) -> Result<FishingDirectBookCandidateV1, ManualFishingCastOwnerError> {
        if self.preview_bait_rack_base_branch_v1()? != FishingCatchBranch::Treasure {
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

    pub async fn stage_bait_rack_rod_durability_v1(
        &mut self,
    ) -> Result<AppliedFishingRodDurabilityState, ManualFishingCastOwnerError> {
        self.bait_rack_only_level_v1()?;
        if let Some(staged) = &self.staged_durability {
            return Ok(staged.applied.clone());
        }
        let branch = self.preview_bait_rack_base_branch_v1()?;
        let resolution = if branch == FishingCatchBranch::Fish
            && matches!(
                self.preview_bait_rack_line_stage_v1(0)?,
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

    fn ensure_school_bait_only_cast_v1(&self) -> Result<(), ManualFishingCastOwnerError> {
        let EquippedFishingRodKind::Ordinary { tier } = self.preflight.rod.kind else {
            return Err(ManualFishingCastOwnerError::SchoolBaitRequiresExactLoadout);
        };
        if tier == EquipmentTier::Gold || !self.preflight.rod.embedded_enchants.is_empty() {
            return Err(ManualFishingCastOwnerError::SchoolBaitRequiresExactLoadout);
        }
        if self.bait.plan.consumed_bait_categories != 1 {
            return Err(ManualFishingCastOwnerError::SchoolBaitRequiresExactLoadout);
        }
        let mut consumed = self
            .bait
            .plan
            .entries
            .iter()
            .filter(|entry| entry.action == FishingBaitCastConsumptionAction::ConsumeForCast);
        let Some(entry) = consumed.next() else {
            return Err(ManualFishingCastOwnerError::SchoolBaitRequiresExactLoadout);
        };
        if entry.bait != FishingBait::School || consumed.next().is_some() {
            return Err(ManualFishingCastOwnerError::SchoolBaitRequiresExactLoadout);
        }
        Ok(())
    }

    pub fn preview_school_bait_base_branch_v1(
        &self,
    ) -> Result<FishingCatchBranch, ManualFishingCastOwnerError> {
        self.ensure_school_bait_only_cast_v1()?;
        Ok(resolve_manual_fishing_unmodified_base_branch_v1(
            self.preflight.rng,
        )?)
    }

    pub fn preview_school_bait_quantity_v1(
        &self,
    ) -> Result<ManualFishingSchoolBaitQuantityV1, ManualFishingCastOwnerError> {
        if self.preview_school_bait_base_branch_v1()? != FishingCatchBranch::Fish {
            return Err(ManualFishingCastOwnerError::SchoolBaitRequiresFishBranch);
        }
        Ok(resolve_manual_fishing_school_bait_quantity_v1(
            self.preflight.rng,
            1,
        )?)
    }

    pub fn preview_school_bait_fish_candidates_v1(
        &self,
    ) -> Result<Vec<FishingFishInstanceDeliveryRequestV1>, ManualFishingCastOwnerError> {
        let quantity = self.preview_school_bait_quantity_v1()?;
        let fish_count = quantity.quantity.final_fish_count();
        let mut candidates = Vec::with_capacity(usize::from(fish_count));
        for fish_slot in 0..fish_count {
            candidates.push(preview_manual_fishing_unmodified_fish_candidate_v1(
                self.preflight.rng,
                self.preflight.area_access.area,
                fish_slot,
            )?);
        }
        Ok(candidates)
    }

    pub fn preview_school_bait_fish_capability_v1(
        &self,
    ) -> Result<ManualFishingUnmodifiedCapabilityPreviewV1, ManualFishingCastOwnerError> {
        let candidates = self.preview_school_bait_fish_candidates_v1()?;
        let numeric = candidates
            .iter()
            .map(|candidate| candidate.numeric)
            .collect::<Vec<_>>();
        Ok(preview_manual_school_bait_fish_capability_v1(
            self.preflight.area_access.area,
            self.preflight.rod.kind,
            &numeric,
        )?)
    }

    pub fn preview_school_bait_line_stage_v1(
        &self,
    ) -> Result<ManualFishingUnmodifiedLineStageV1, ManualFishingCastOwnerError> {
        let capability = self.preview_school_bait_fish_capability_v1()?;
        Ok(resolve_manual_unmodified_line_stage_v1(
            self.preflight.rng,
            capability,
        )?)
    }

    pub fn preview_school_bait_selected_direct_book_v1(
        &self,
    ) -> Result<FishingDirectBookCandidateV1, ManualFishingCastOwnerError> {
        if self.preview_school_bait_base_branch_v1()? != FishingCatchBranch::Treasure {
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

    pub async fn stage_school_bait_rod_durability_v1(
        &mut self,
    ) -> Result<AppliedFishingRodDurabilityState, ManualFishingCastOwnerError> {
        self.ensure_school_bait_only_cast_v1()?;
        if let Some(staged) = &self.staged_durability {
            return Ok(staged.applied.clone());
        }
        let branch = self.preview_school_bait_base_branch_v1()?;
        let resolution = if branch == FishingCatchBranch::Fish
            && matches!(
                self.preview_school_bait_line_stage_v1()?,
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

    fn ensure_sturdy_bait_only_cast_v1(&self) -> Result<(), ManualFishingCastOwnerError> {
        let EquippedFishingRodKind::Ordinary { tier } = self.preflight.rod.kind else {
            return Err(ManualFishingCastOwnerError::SturdyBaitRequiresExactLoadout);
        };
        if tier == EquipmentTier::Gold || !self.preflight.rod.embedded_enchants.is_empty() {
            return Err(ManualFishingCastOwnerError::SturdyBaitRequiresExactLoadout);
        }
        if self.bait.plan.consumed_bait_categories != 1 {
            return Err(ManualFishingCastOwnerError::SturdyBaitRequiresExactLoadout);
        }
        let mut consumed = self
            .bait
            .plan
            .entries
            .iter()
            .filter(|entry| entry.action == FishingBaitCastConsumptionAction::ConsumeForCast);
        let Some(entry) = consumed.next() else {
            return Err(ManualFishingCastOwnerError::SturdyBaitRequiresExactLoadout);
        };
        if entry.bait != FishingBait::Sturdy || consumed.next().is_some() {
            return Err(ManualFishingCastOwnerError::SturdyBaitRequiresExactLoadout);
        }
        Ok(())
    }

    pub fn preview_sturdy_bait_base_branch_v1(
        &self,
    ) -> Result<FishingCatchBranch, ManualFishingCastOwnerError> {
        self.ensure_sturdy_bait_only_cast_v1()?;
        Ok(resolve_manual_fishing_unmodified_base_branch_v1(
            self.preflight.rng,
        )?)
    }

    fn preview_sturdy_bait_fish_candidate_v1(
        &self,
        fish_slot: u8,
    ) -> Result<FishingFishInstanceDeliveryRequestV1, ManualFishingCastOwnerError> {
        self.ensure_sturdy_bait_only_cast_v1()?;
        Ok(preview_manual_fishing_unmodified_fish_candidate_v1(
            self.preflight.rng,
            self.preflight.area_access.area,
            fish_slot,
        )?)
    }

    pub fn preview_sturdy_bait_fish_capability_v1(
        &self,
        fish_slot: u8,
    ) -> Result<ManualFishingUnmodifiedCapabilityPreviewV1, ManualFishingCastOwnerError> {
        let candidate = self.preview_sturdy_bait_fish_candidate_v1(fish_slot)?;
        Ok(preview_manual_sturdy_bait_fish_capability_v1(
            self.preflight.area_access.area,
            self.preflight.rod.kind,
            candidate.numeric,
        )?)
    }

    pub fn preview_sturdy_bait_line_stage_v1(
        &self,
        fish_slot: u8,
    ) -> Result<ManualFishingUnmodifiedLineStageV1, ManualFishingCastOwnerError> {
        let capability = self.preview_sturdy_bait_fish_capability_v1(fish_slot)?;
        Ok(resolve_manual_sturdy_bait_line_stage_v1(
            self.preflight.rng,
            capability,
        )?)
    }

    pub fn preview_sturdy_bait_selected_direct_book_v1(
        &self,
    ) -> Result<FishingDirectBookCandidateV1, ManualFishingCastOwnerError> {
        if self.preview_sturdy_bait_base_branch_v1()? != FishingCatchBranch::Treasure {
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

    pub async fn stage_sturdy_bait_rod_durability_v1(
        &mut self,
    ) -> Result<AppliedFishingRodDurabilityState, ManualFishingCastOwnerError> {
        self.ensure_sturdy_bait_only_cast_v1()?;
        if let Some(staged) = &self.staged_durability {
            return Ok(staged.applied.clone());
        }
        let branch = self.preview_sturdy_bait_base_branch_v1()?;
        let resolution = if branch == FishingCatchBranch::Fish
            && matches!(
                self.preview_sturdy_bait_line_stage_v1(0)?,
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

    fn sharp_hook_only_level_v1(&self) -> Result<u8, ManualFishingCastOwnerError> {
        let EquippedFishingRodKind::Ordinary { tier } = self.preflight.rod.kind else {
            return Err(ManualFishingCastOwnerError::SharpHookRequiresExactLoadout);
        };
        if tier == EquipmentTier::Gold || self.bait.plan.consumed_bait_categories != 0 {
            return Err(ManualFishingCastOwnerError::SharpHookRequiresExactLoadout);
        }
        let [embedded] = self.preflight.rod.embedded_enchants.as_slice() else {
            return Err(ManualFishingCastOwnerError::SharpHookRequiresExactLoadout);
        };
        if embedded.enchant != CanonicalEnchant::SharpHook
            || !(1..=SHARP_HOOK_MAX_LEVEL).contains(&embedded.level)
        {
            return Err(ManualFishingCastOwnerError::SharpHookRequiresExactLoadout);
        }
        Ok(embedded.level)
    }

    pub fn preview_sharp_hook_base_branch_v1(
        &self,
    ) -> Result<FishingCatchBranch, ManualFishingCastOwnerError> {
        self.sharp_hook_only_level_v1()?;
        Ok(resolve_manual_fishing_unmodified_base_branch_v1(
            self.preflight.rng,
        )?)
    }

    fn preview_sharp_hook_fish_candidate_v1(
        &self,
        fish_slot: u8,
    ) -> Result<FishingFishInstanceDeliveryRequestV1, ManualFishingCastOwnerError> {
        self.sharp_hook_only_level_v1()?;
        Ok(preview_manual_fishing_unmodified_fish_candidate_v1(
            self.preflight.rng,
            self.preflight.area_access.area,
            fish_slot,
        )?)
    }

    pub fn preview_sharp_hook_fish_capability_v1(
        &self,
        fish_slot: u8,
    ) -> Result<ManualFishingUnmodifiedCapabilityPreviewV1, ManualFishingCastOwnerError> {
        let level = self.sharp_hook_only_level_v1()?;
        let candidate = self.preview_sharp_hook_fish_candidate_v1(fish_slot)?;
        let baseline = preview_manual_unmodified_fish_capability_v1(
            self.preflight.area_access.area,
            self.preflight.rod.kind,
            candidate.numeric,
        )?;
        Ok(match baseline {
            ManualFishingUnmodifiedCapabilityPreviewV1::OverCapPending {
                ratio,
                line_break,
                ..
            } => ManualFishingUnmodifiedCapabilityPreviewV1::OverCapPending {
                ratio,
                line_break,
                catch_chance: preview_over_cap_catch_chance(ratio, Some(level))?,
            },
            other => other,
        })
    }

    pub fn preview_sharp_hook_line_stage_v1(
        &self,
        fish_slot: u8,
    ) -> Result<ManualFishingUnmodifiedLineStageV1, ManualFishingCastOwnerError> {
        let capability = self.preview_sharp_hook_fish_capability_v1(fish_slot)?;
        Ok(resolve_manual_unmodified_line_stage_v1(
            self.preflight.rng,
            capability,
        )?)
    }

    pub fn preview_sharp_hook_selected_direct_book_v1(
        &self,
    ) -> Result<FishingDirectBookCandidateV1, ManualFishingCastOwnerError> {
        if self.preview_sharp_hook_base_branch_v1()? != FishingCatchBranch::Treasure {
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

    pub async fn stage_sharp_hook_rod_durability_v1(
        &mut self,
    ) -> Result<AppliedFishingRodDurabilityState, ManualFishingCastOwnerError> {
        self.sharp_hook_only_level_v1()?;
        if let Some(staged) = &self.staged_durability {
            return Ok(staged.applied.clone());
        }
        let branch = self.preview_sharp_hook_base_branch_v1()?;
        let resolution = if branch == FishingCatchBranch::Fish
            && matches!(
                self.preview_sharp_hook_line_stage_v1(0)?,
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

    fn strengthen_only_level_v1(&self) -> Result<u8, ManualFishingCastOwnerError> {
        let EquippedFishingRodKind::Ordinary { tier } = self.preflight.rod.kind else {
            return Err(ManualFishingCastOwnerError::StrengthenRequiresExactLoadout);
        };
        if tier == EquipmentTier::Gold || self.bait.plan.consumed_bait_categories != 0 {
            return Err(ManualFishingCastOwnerError::StrengthenRequiresExactLoadout);
        }
        let [embedded] = self.preflight.rod.embedded_enchants.as_slice() else {
            return Err(ManualFishingCastOwnerError::StrengthenRequiresExactLoadout);
        };
        if embedded.enchant != CanonicalEnchant::Strengthen
            || !(1..=STRENGTHEN_MAX_LEVEL).contains(&embedded.level)
        {
            return Err(ManualFishingCastOwnerError::StrengthenRequiresExactLoadout);
        }
        Ok(embedded.level)
    }

    pub fn preview_strengthen_base_branch_v1(
        &self,
    ) -> Result<FishingCatchBranch, ManualFishingCastOwnerError> {
        self.strengthen_only_level_v1()?;
        Ok(resolve_manual_fishing_unmodified_base_branch_v1(
            self.preflight.rng,
        )?)
    }

    fn preview_strengthen_fish_candidate_v1(
        &self,
        fish_slot: u8,
    ) -> Result<FishingFishInstanceDeliveryRequestV1, ManualFishingCastOwnerError> {
        self.strengthen_only_level_v1()?;
        Ok(preview_manual_fishing_unmodified_fish_candidate_v1(
            self.preflight.rng,
            self.preflight.area_access.area,
            fish_slot,
        )?)
    }

    pub fn preview_strengthen_fish_capability_v1(
        &self,
        fish_slot: u8,
    ) -> Result<ManualFishingUnmodifiedCapabilityPreviewV1, ManualFishingCastOwnerError> {
        let level = self.strengthen_only_level_v1()?;
        let candidate = self.preview_strengthen_fish_candidate_v1(fish_slot)?;
        Ok(preview_manual_strengthen_fish_capability_v1(
            self.preflight.area_access.area,
            self.preflight.rod.kind,
            candidate.numeric,
            level,
        )?)
    }

    pub fn preview_strengthen_line_stage_v1(
        &self,
        fish_slot: u8,
    ) -> Result<ManualFishingUnmodifiedLineStageV1, ManualFishingCastOwnerError> {
        let capability = self.preview_strengthen_fish_capability_v1(fish_slot)?;
        Ok(resolve_manual_unmodified_line_stage_v1(
            self.preflight.rng,
            capability,
        )?)
    }

    pub fn preview_strengthen_selected_direct_book_v1(
        &self,
    ) -> Result<FishingDirectBookCandidateV1, ManualFishingCastOwnerError> {
        if self.preview_strengthen_base_branch_v1()? != FishingCatchBranch::Treasure {
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

    pub async fn stage_strengthen_rod_durability_v1(
        &mut self,
    ) -> Result<AppliedFishingRodDurabilityState, ManualFishingCastOwnerError> {
        self.strengthen_only_level_v1()?;
        if let Some(staged) = &self.staged_durability {
            return Ok(staged.applied.clone());
        }
        let branch = self.preview_strengthen_base_branch_v1()?;
        let resolution = if branch == FishingCatchBranch::Fish
            && matches!(
                self.preview_strengthen_line_stage_v1(0)?,
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

    fn ensure_unbreaking_x_only_cast_v1(&self) -> Result<(), ManualFishingCastOwnerError> {
        let EquippedFishingRodKind::Ordinary { tier } = self.preflight.rod.kind else {
            return Err(ManualFishingCastOwnerError::UnbreakingXRequiresExactLoadout);
        };
        if tier == EquipmentTier::Gold || self.bait.plan.consumed_bait_categories != 0 {
            return Err(ManualFishingCastOwnerError::UnbreakingXRequiresExactLoadout);
        }
        let [embedded] = self.preflight.rod.embedded_enchants.as_slice() else {
            return Err(ManualFishingCastOwnerError::UnbreakingXRequiresExactLoadout);
        };
        if embedded.enchant != CanonicalEnchant::Unbreaking
            || embedded.level != canonical_enchant_max_resulting_level(CanonicalEnchant::Unbreaking)
        {
            return Err(ManualFishingCastOwnerError::UnbreakingXRequiresExactLoadout);
        }
        Ok(())
    }

    /// Resolves the base Fish/Junk/Treasure branch for the narrow Unbreaking-X-only
    /// loadout. Unbreaking X affects only the ordinary Rod wear event, never branch
    /// weights or Fish identity, so the canonical 176/17/7 branch draw remains exact.
    pub fn preview_unbreaking_x_base_branch_v1(
        &self,
    ) -> Result<FishingCatchBranch, ManualFishingCastOwnerError> {
        self.ensure_unbreaking_x_only_cast_v1()?;
        Ok(resolve_manual_fishing_unmodified_base_branch_v1(
            self.preflight.rng,
        )?)
    }

    fn preview_unbreaking_x_fish_candidate_v1(
        &self,
        fish_slot: u8,
    ) -> Result<FishingFishInstanceDeliveryRequestV1, ManualFishingCastOwnerError> {
        self.ensure_unbreaking_x_only_cast_v1()?;
        Ok(preview_manual_fishing_unmodified_fish_candidate_v1(
            self.preflight.rng,
            self.preflight.area_access.area,
            fish_slot,
        )?)
    }

    pub fn preview_unbreaking_x_line_stage_v1(
        &self,
        fish_slot: u8,
    ) -> Result<ManualFishingUnmodifiedLineStageV1, ManualFishingCastOwnerError> {
        let candidate = self.preview_unbreaking_x_fish_candidate_v1(fish_slot)?;
        let capability = preview_manual_unmodified_fish_capability_v1(
            self.preflight.area_access.area,
            self.preflight.rod.kind,
            candidate.numeric,
        )?;
        Ok(resolve_manual_unmodified_line_stage_v1(
            self.preflight.rng,
            capability,
        )?)
    }

    /// Resolves the exact operation-pinned 20% Unbreaking X prevention draw for the
    /// one normal durability event. This is never consulted for a line break.
    pub fn preview_unbreaking_x_ordinary_wear_v1(
        &self,
    ) -> Result<ManualFishingUnbreakingXWearPreviewV1, ManualFishingCastOwnerError> {
        self.ensure_unbreaking_x_only_cast_v1()?;
        Ok(resolve_manual_fishing_unbreaking_x_wear_v1(
            self.preflight.rng,
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

    /// Stages exactly one durability consequence for the narrow Unbreaking-X-only
    /// baseline path. Line-break destruction bypasses Unbreaking entirely; every
    /// other completed attempt uses the versioned Unbreaking wear domain.
    pub async fn stage_unbreaking_x_rod_durability_v1(
        &mut self,
    ) -> Result<AppliedFishingRodDurabilityState, ManualFishingCastOwnerError> {
        self.ensure_unbreaking_x_only_cast_v1()?;
        if let Some(staged) = &self.staged_durability {
            return Ok(staged.applied.clone());
        }
        let branch = self.preview_unbreaking_x_base_branch_v1()?;
        let resolution = if branch == FishingCatchBranch::Fish
            && matches!(
                self.preview_unbreaking_x_line_stage_v1(0)?,
                ManualFishingUnmodifiedLineStageV1::LineBroken { .. }
            ) {
            FishingRodDurabilityResolution::LineBreak
        } else {
            let wear = self.preview_unbreaking_x_ordinary_wear_v1()?;
            FishingRodDurabilityResolution::CompletedCastAttempt {
                ordinary_event_prevented_by_unbreaking: wear.prevented,
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

    /// Composes the selected branch for an exact Strengthen-only Rod loadout.
    ///
    /// Strengthen changes only the exact capability ratio. Branch/Fish identity,
    /// non-Fish payloads, successful progression and ordinary wear remain their
    /// canonical baseline owners. A line break stages only Rod destruction; survived
    /// over-cap catch and direct Book delivery remain explicitly pending.
    /// Composes the selected branch for the exact Treasure-X-only loadout.
    ///
    /// Treasure X changes only the outer Treasure relative weight. Fish identity,
    /// capability, internal Treasure results, progression and ordinary Rod wear
    /// keep their existing owners. Combined modifier/cap cases remain fail-closed.
    pub async fn stage_treasure_x_selected_outcome_v1(
        &mut self,
    ) -> Result<PendingManualFishingTreasureXSelectedOutcomeV1, ManualFishingCastOwnerError> {
        let branch = self.preview_treasure_x_branch_v1()?;
        match branch {
            FishingCatchBranch::Fish => match self.preview_treasure_x_line_stage_v1()? {
                ManualFishingUnmodifiedLineStageV1::LineBroken { .. } => {
                    let durability = self.stage_treasure_x_rod_durability_v1().await?;
                    Ok(PendingManualFishingTreasureXSelectedOutcomeV1::LineBroken {
                        durability,
                    })
                }
                line_stage @ ManualFishingUnmodifiedLineStageV1::LineSurvivedOverCapCatchPending {
                    ..
                } => Ok(
                    PendingManualFishingTreasureXSelectedOutcomeV1::OverCapCatchPending {
                        line_stage,
                    },
                ),
                ManualFishingUnmodifiedLineStageV1::LandedWithoutLineRisk => {
                    let request = self.preview_treasure_x_fish_candidate_v1()?;
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
                    let durability = self.stage_treasure_x_rod_durability_v1().await?;
                    Ok(PendingManualFishingTreasureXSelectedOutcomeV1::LandedFish {
                        delivery,
                        progression,
                        durability,
                    })
                }
            },
            FishingCatchBranch::Junk | FishingCatchBranch::Treasure => {
                match self.stage_non_fish_candidate_v1(branch).await {
                    Ok(staged) => {
                        let durability = self.stage_treasure_x_rod_durability_v1().await?;
                        Ok(PendingManualFishingTreasureXSelectedOutcomeV1::NonFish {
                            staged,
                            durability,
                        })
                    }
                    Err(ManualFishingCastOwnerError::NonFishRoll(
                        FishingNonFishRollError::EnchantBookRequiresDedicatedResolver,
                    )) => Ok(
                        PendingManualFishingTreasureXSelectedOutcomeV1::DirectBookPending {
                            candidate: self.preview_treasure_x_selected_direct_book_v1()?,
                        },
                    ),
                    Err(other) => Err(other),
                }
            }
        }
    }

    /// Composes the selected baseline branch for the exact Multi-Treasure-only loadout.
    ///
    /// Multi Treasure never changes the outer branch or Fish identity. Non-book
    /// Treasure repetition is already resolved by the shared payload owner. When
    /// the selected category is Enchant Book, this path retains independently
    /// rerolled identity candidates for every repeated slot but issues no Book yet.
    pub async fn stage_multi_treasure_selected_outcome_v1(
        &mut self,
    ) -> Result<PendingManualFishingMultiTreasureSelectedOutcomeV1, ManualFishingCastOwnerError>
    {
        let branch = self.preview_multi_treasure_base_branch_v1()?;
        match branch {
            FishingCatchBranch::Fish => match self.preview_multi_treasure_line_stage_v1()? {
                ManualFishingUnmodifiedLineStageV1::LineBroken { .. } => {
                    let durability = self.stage_multi_treasure_rod_durability_v1().await?;
                    Ok(PendingManualFishingMultiTreasureSelectedOutcomeV1::LineBroken {
                        durability,
                    })
                }
                line_stage @ ManualFishingUnmodifiedLineStageV1::LineSurvivedOverCapCatchPending { .. } => {
                    Ok(PendingManualFishingMultiTreasureSelectedOutcomeV1::OverCapCatchPending {
                        line_stage,
                    })
                }
                ManualFishingUnmodifiedLineStageV1::LandedWithoutLineRisk => {
                    let request = self.preview_multi_treasure_fish_candidate_v1()?;
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
                    let durability = self.stage_multi_treasure_rod_durability_v1().await?;
                    Ok(PendingManualFishingMultiTreasureSelectedOutcomeV1::LandedFish {
                        delivery,
                        progression,
                        durability,
                    })
                }
            },
            FishingCatchBranch::Junk | FishingCatchBranch::Treasure => {
                match self.stage_non_fish_candidate_v1(branch).await {
                    Ok(staged) => {
                        let durability = self.stage_multi_treasure_rod_durability_v1().await?;
                        Ok(PendingManualFishingMultiTreasureSelectedOutcomeV1::NonFish {
                            staged,
                            durability,
                        })
                    }
                    Err(ManualFishingCastOwnerError::NonFishRoll(
                        FishingNonFishRollError::EnchantBookRequiresDedicatedResolver,
                    )) => {
                        let (count, candidates) =
                            self.preview_multi_treasure_selected_direct_books_v1()?;
                        Ok(PendingManualFishingMultiTreasureSelectedOutcomeV1::DirectBooksPending {
                            count,
                            candidates,
                        })
                    }
                    Err(other) => Err(other),
                }
            }
        }
    }

    /// Composes the selected baseline branch for the exact Multicatch-only loadout.
    ///
    /// Multicatch count is operation-pinned and every fish slot independently rolls
    /// its full FishInstance identity. Aggregate CatchLoad determines capability and
    /// line-break. A landed multi-fish result settles all fish before one progression
    /// receipt and exactly one ordinary Rod durability event.
    pub async fn stage_multicatch_selected_outcome_v1(
        &mut self,
    ) -> Result<PendingManualFishingMulticatchSelectedOutcomeV1, ManualFishingCastOwnerError> {
        let branch = self.preview_multicatch_base_branch_v1()?;
        match branch {
            FishingCatchBranch::Fish => {
                let count = self.preview_multicatch_count_v1()?;
                match self.preview_multicatch_line_stage_v1()? {
                    ManualFishingUnmodifiedLineStageV1::LineBroken { .. } => {
                        let durability = self.stage_multicatch_rod_durability_v1().await?;
                        Ok(PendingManualFishingMulticatchSelectedOutcomeV1::LineBroken {
                            count,
                            durability,
                        })
                    }
                    line_stage @ ManualFishingUnmodifiedLineStageV1::LineSurvivedOverCapCatchPending { .. } => {
                        Ok(PendingManualFishingMulticatchSelectedOutcomeV1::OverCapCatchPending {
                            count,
                            line_stage,
                        })
                    }
                    ManualFishingUnmodifiedLineStageV1::LandedWithoutLineRisk => {
                        let requests = self.preview_multicatch_fish_candidates_v1()?;
                        let mut deliveries = Vec::with_capacity(requests.len());
                        for request in requests {
                            deliveries.push(
                                deliver_fishing_fish_instance_v1(
                                    &mut self.tx,
                                    self.preflight.operation_id,
                                    self.preflight.player_id,
                                    request,
                                )
                                .await?,
                            );
                        }
                        let landed_rarities =
                            deliveries.iter().map(|delivery| delivery.rarity).collect();
                        let progression = settle_manual_fishing_successful_progression_v1(
                            &mut self.tx,
                            self.preflight.operation_id,
                            self.preflight.player_id,
                            &ManualFishingSuccessfulProgressionOutcomeV1::Fish {
                                landed_rarities,
                            },
                        )
                        .await?;
                        let durability = self.stage_multicatch_rod_durability_v1().await?;
                        Ok(PendingManualFishingMulticatchSelectedOutcomeV1::LandedFish {
                            count,
                            deliveries,
                            progression,
                            durability,
                        })
                    }
                }
            }
            FishingCatchBranch::Junk | FishingCatchBranch::Treasure => {
                match self.stage_non_fish_candidate_v1(branch).await {
                    Ok(staged) => {
                        let durability = self.stage_multicatch_rod_durability_v1().await?;
                        Ok(PendingManualFishingMulticatchSelectedOutcomeV1::NonFish {
                            staged,
                            durability,
                        })
                    }
                    Err(ManualFishingCastOwnerError::NonFishRoll(
                        FishingNonFishRollError::EnchantBookRequiresDedicatedResolver,
                    )) => Ok(
                        PendingManualFishingMulticatchSelectedOutcomeV1::DirectBookPending {
                            candidate: self.preview_multicatch_selected_direct_book_v1()?,
                        },
                    ),
                    Err(other) => Err(other),
                }
            }
        }
    }

    /// Composes the selected baseline catch for the exact Lure-X-only loadout.
    ///
    /// Lure changes only the shared action-speed rating, so branch/Fish identity,
    /// capability and durability remain baseline. The returned cooldown is a pure
    /// exact projection; this still-uncommittable owner does not persist cooldown.
    pub async fn stage_lure_x_selected_outcome_v1(
        &mut self,
    ) -> Result<PendingManualFishingLureXSelectedOutcomeV1, ManualFishingCastOwnerError> {
        let cooldown = self.preview_lure_x_cooldown_v1()?;
        let branch = self.preview_lure_x_base_branch_v1()?;
        match branch {
            FishingCatchBranch::Fish => match self.preview_lure_x_line_stage_v1(0)? {
                ManualFishingUnmodifiedLineStageV1::LineBroken { .. } => {
                    let durability = self.stage_lure_x_rod_durability_v1().await?;
                    Ok(PendingManualFishingLureXSelectedOutcomeV1::LineBroken {
                        durability,
                        cooldown,
                    })
                }
                line_stage @ ManualFishingUnmodifiedLineStageV1::LineSurvivedOverCapCatchPending {
                    ..
                } => Ok(PendingManualFishingLureXSelectedOutcomeV1::OverCapCatchPending {
                    line_stage,
                    cooldown,
                }),
                ManualFishingUnmodifiedLineStageV1::LandedWithoutLineRisk => {
                    let request = self.preview_lure_x_fish_candidate_v1(0)?;
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
                    let durability = self.stage_lure_x_rod_durability_v1().await?;
                    Ok(PendingManualFishingLureXSelectedOutcomeV1::LandedFish {
                        delivery,
                        progression,
                        durability,
                        cooldown,
                    })
                }
            },
            FishingCatchBranch::Junk | FishingCatchBranch::Treasure => {
                match self.stage_non_fish_candidate_v1(branch).await {
                    Ok(staged) => {
                        let durability = self.stage_lure_x_rod_durability_v1().await?;
                        Ok(PendingManualFishingLureXSelectedOutcomeV1::NonFish {
                            staged,
                            durability,
                            cooldown,
                        })
                    }
                    Err(ManualFishingCastOwnerError::NonFishRoll(
                        FishingNonFishRollError::EnchantBookRequiresDedicatedResolver,
                    )) => Ok(PendingManualFishingLureXSelectedOutcomeV1::DirectBookPending {
                        candidate: self.preview_lure_x_selected_direct_book_v1()?,
                        cooldown,
                    }),
                    Err(other) => Err(other),
                }
            }
        }
    }

    /// Composes the selected baseline branch for the exact Bait-Rack-only loadout.
    ///
    /// Bait Rack changes only active-bait capacity. With zero consumed bait it
    /// cannot alter branch, Fish identity, capability or ordinary Rod wear.
    pub async fn stage_bait_rack_selected_outcome_v1(
        &mut self,
    ) -> Result<PendingManualFishingBaitRackSelectedOutcomeV1, ManualFishingCastOwnerError> {
        self.bait_rack_only_level_v1()?;
        let branch = self.preview_bait_rack_base_branch_v1()?;
        match branch {
            FishingCatchBranch::Fish => match self.preview_bait_rack_line_stage_v1(0)? {
                ManualFishingUnmodifiedLineStageV1::LineBroken { .. } => {
                    let durability = self.stage_bait_rack_rod_durability_v1().await?;
                    Ok(PendingManualFishingBaitRackSelectedOutcomeV1::LineBroken {
                        durability,
                    })
                }
                line_stage @ ManualFishingUnmodifiedLineStageV1::LineSurvivedOverCapCatchPending { .. } => {
                    Ok(PendingManualFishingBaitRackSelectedOutcomeV1::OverCapCatchPending {
                        line_stage,
                    })
                }
                ManualFishingUnmodifiedLineStageV1::LandedWithoutLineRisk => {
                    let request = self.preview_bait_rack_fish_candidate_v1(0)?;
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
                    let durability = self.stage_bait_rack_rod_durability_v1().await?;
                    Ok(PendingManualFishingBaitRackSelectedOutcomeV1::LandedFish {
                        delivery,
                        progression,
                        durability,
                    })
                }
            },
            FishingCatchBranch::Junk | FishingCatchBranch::Treasure => {
                match self.stage_non_fish_candidate_v1(branch).await {
                    Ok(staged) => {
                        let durability = self.stage_bait_rack_rod_durability_v1().await?;
                        Ok(PendingManualFishingBaitRackSelectedOutcomeV1::NonFish {
                            staged,
                            durability,
                        })
                    }
                    Err(ManualFishingCastOwnerError::NonFishRoll(
                        FishingNonFishRollError::EnchantBookRequiresDedicatedResolver,
                    )) => Ok(PendingManualFishingBaitRackSelectedOutcomeV1::DirectBookPending {
                        candidate: self.preview_bait_rack_selected_direct_book_v1()?,
                    }),
                    Err(other) => Err(other),
                }
            }
        }
    }

    /// Composes the selected baseline branch after exactly one School Bait was
    /// consumed. The School proc is sampled only for a Fish branch, then every
    /// resulting same-area fish slot is independently resolved before aggregate
    /// capability and one Rod durability event.
    pub async fn stage_school_bait_selected_outcome_v1(
        &mut self,
    ) -> Result<PendingManualFishingSchoolBaitSelectedOutcomeV1, ManualFishingCastOwnerError> {
        let branch = self.preview_school_bait_base_branch_v1()?;
        match branch {
            FishingCatchBranch::Fish => {
                let quantity = self.preview_school_bait_quantity_v1()?;
                match self.preview_school_bait_line_stage_v1()? {
                    ManualFishingUnmodifiedLineStageV1::LineBroken { .. } => {
                        let durability = self.stage_school_bait_rod_durability_v1().await?;
                        Ok(PendingManualFishingSchoolBaitSelectedOutcomeV1::LineBroken {
                            quantity,
                            durability,
                        })
                    }
                    line_stage @ ManualFishingUnmodifiedLineStageV1::LineSurvivedOverCapCatchPending {
                        ..
                    } => Ok(
                        PendingManualFishingSchoolBaitSelectedOutcomeV1::OverCapCatchPending {
                            quantity,
                            line_stage,
                        },
                    ),
                    ManualFishingUnmodifiedLineStageV1::LandedWithoutLineRisk => {
                        let requests = self.preview_school_bait_fish_candidates_v1()?;
                        let mut deliveries = Vec::with_capacity(requests.len());
                        for request in requests {
                            deliveries.push(
                                deliver_fishing_fish_instance_v1(
                                    &mut self.tx,
                                    self.preflight.operation_id,
                                    self.preflight.player_id,
                                    request,
                                )
                                .await?,
                            );
                        }
                        let landed_rarities =
                            deliveries.iter().map(|delivery| delivery.rarity).collect();
                        let progression = settle_manual_fishing_successful_progression_v1(
                            &mut self.tx,
                            self.preflight.operation_id,
                            self.preflight.player_id,
                            &ManualFishingSuccessfulProgressionOutcomeV1::Fish {
                                landed_rarities,
                            },
                        )
                        .await?;
                        let durability = self.stage_school_bait_rod_durability_v1().await?;
                        Ok(PendingManualFishingSchoolBaitSelectedOutcomeV1::LandedFish {
                            quantity,
                            deliveries,
                            progression,
                            durability,
                        })
                    }
                }
            }
            FishingCatchBranch::Junk | FishingCatchBranch::Treasure => {
                match self.stage_non_fish_candidate_v1(branch).await {
                    Ok(staged) => {
                        let durability = self.stage_school_bait_rod_durability_v1().await?;
                        Ok(PendingManualFishingSchoolBaitSelectedOutcomeV1::NonFish {
                            staged,
                            durability,
                        })
                    }
                    Err(ManualFishingCastOwnerError::NonFishRoll(
                        FishingNonFishRollError::EnchantBookRequiresDedicatedResolver,
                    )) => Ok(
                        PendingManualFishingSchoolBaitSelectedOutcomeV1::DirectBookPending {
                            candidate: self.preview_school_bait_selected_direct_book_v1()?,
                        },
                    ),
                    Err(other) => Err(other),
                }
            }
        }
    }

    /// Composes the selected baseline branch after exactly one Sturdy Bait was
    /// consumed. Sturdy changes only physical capability and final line-break chance.
    pub async fn stage_sturdy_bait_selected_outcome_v1(
        &mut self,
    ) -> Result<PendingManualFishingSturdyBaitSelectedOutcomeV1, ManualFishingCastOwnerError> {
        let branch = self.preview_sturdy_bait_base_branch_v1()?;
        match branch {
            FishingCatchBranch::Fish => match self.preview_sturdy_bait_line_stage_v1(0)? {
                ManualFishingUnmodifiedLineStageV1::LineBroken { .. } => {
                    let durability = self.stage_sturdy_bait_rod_durability_v1().await?;
                    Ok(PendingManualFishingSturdyBaitSelectedOutcomeV1::LineBroken {
                        durability,
                    })
                }
                line_stage @ ManualFishingUnmodifiedLineStageV1::LineSurvivedOverCapCatchPending {
                    ..
                } => Ok(
                    PendingManualFishingSturdyBaitSelectedOutcomeV1::OverCapCatchPending {
                        line_stage,
                    },
                ),
                ManualFishingUnmodifiedLineStageV1::LandedWithoutLineRisk => {
                    let request = self.preview_sturdy_bait_fish_candidate_v1(0)?;
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
                    let durability = self.stage_sturdy_bait_rod_durability_v1().await?;
                    Ok(PendingManualFishingSturdyBaitSelectedOutcomeV1::LandedFish {
                        delivery,
                        progression,
                        durability,
                    })
                }
            },
            FishingCatchBranch::Junk | FishingCatchBranch::Treasure => {
                match self.stage_non_fish_candidate_v1(branch).await {
                    Ok(staged) => {
                        let durability = self.stage_sturdy_bait_rod_durability_v1().await?;
                        Ok(PendingManualFishingSturdyBaitSelectedOutcomeV1::NonFish {
                            staged,
                            durability,
                        })
                    }
                    Err(ManualFishingCastOwnerError::NonFishRoll(
                        FishingNonFishRollError::EnchantBookRequiresDedicatedResolver,
                    )) => Ok(PendingManualFishingSturdyBaitSelectedOutcomeV1::DirectBookPending {
                        candidate: self.preview_sturdy_bait_selected_direct_book_v1()?,
                    }),
                    Err(other) => Err(other),
                }
            }
        }
    }

    /// Composes the selected baseline branch for an exact SharpHook-only Rod loadout.
    ///
    /// SharpHook changes only the over-cap catch chance after line survival. The exact
    /// rational catch chance remains pending because no PPM conversion rule is frozen.
    pub async fn stage_sharp_hook_selected_outcome_v1(
        &mut self,
    ) -> Result<PendingManualFishingSharpHookSelectedOutcomeV1, ManualFishingCastOwnerError> {
        let branch = self.preview_sharp_hook_base_branch_v1()?;
        match branch {
            FishingCatchBranch::Fish => {
                let capability = self.preview_sharp_hook_fish_capability_v1(0)?;
                let line_stage =
                    resolve_manual_unmodified_line_stage_v1(self.preflight.rng, capability)?;
                match (capability, line_stage) {
                    (
                        ManualFishingUnmodifiedCapabilityPreviewV1::OverCapPending {
                            catch_chance,
                            ..
                        },
                        stage @ ManualFishingUnmodifiedLineStageV1::LineSurvivedOverCapCatchPending {
                            ..
                        },
                    ) => Ok(PendingManualFishingSharpHookSelectedOutcomeV1::OverCapCatchPending {
                        line_stage: stage,
                        catch_chance,
                    }),
                    (
                        ManualFishingUnmodifiedCapabilityPreviewV1::OverCapPending { .. },
                        ManualFishingUnmodifiedLineStageV1::LineBroken { .. },
                    ) => {
                        let durability = self.stage_sharp_hook_rod_durability_v1().await?;
                        Ok(PendingManualFishingSharpHookSelectedOutcomeV1::LineBroken {
                            durability,
                        })
                    }
                    (
                        ManualFishingUnmodifiedCapabilityPreviewV1::StarterPoolGuaranteedLanded
                        | ManualFishingUnmodifiedCapabilityPreviewV1::WithinRodCapability { .. },
                        ManualFishingUnmodifiedLineStageV1::LandedWithoutLineRisk,
                    ) => {
                        let request = self.preview_sharp_hook_fish_candidate_v1(0)?;
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
                        let durability = self.stage_sharp_hook_rod_durability_v1().await?;
                        Ok(PendingManualFishingSharpHookSelectedOutcomeV1::LandedFish {
                            delivery,
                            progression,
                            durability,
                        })
                    }
                    _ => Err(ManualFishingCastOwnerError::SharpHookCapabilityStageMismatch),
                }
            }
            FishingCatchBranch::Junk | FishingCatchBranch::Treasure => {
                match self.stage_non_fish_candidate_v1(branch).await {
                    Ok(staged) => {
                        let durability = self.stage_sharp_hook_rod_durability_v1().await?;
                        Ok(PendingManualFishingSharpHookSelectedOutcomeV1::NonFish {
                            staged,
                            durability,
                        })
                    }
                    Err(ManualFishingCastOwnerError::NonFishRoll(
                        FishingNonFishRollError::EnchantBookRequiresDedicatedResolver,
                    )) => Ok(
                        PendingManualFishingSharpHookSelectedOutcomeV1::DirectBookPending {
                            candidate: self.preview_sharp_hook_selected_direct_book_v1()?,
                        },
                    ),
                    Err(other) => Err(other),
                }
            }
        }
    }

    pub async fn stage_strengthen_selected_outcome_v1(
        &mut self,
    ) -> Result<PendingManualFishingStrengthenSelectedOutcomeV1, ManualFishingCastOwnerError> {
        let branch = self.preview_strengthen_base_branch_v1()?;
        match branch {
            FishingCatchBranch::Fish => match self.preview_strengthen_line_stage_v1(0)? {
                ManualFishingUnmodifiedLineStageV1::LineBroken { .. } => {
                    let durability = self.stage_strengthen_rod_durability_v1().await?;
                    Ok(PendingManualFishingStrengthenSelectedOutcomeV1::LineBroken {
                        durability,
                    })
                }
                line_stage @ ManualFishingUnmodifiedLineStageV1::LineSurvivedOverCapCatchPending { .. } => {
                    Ok(PendingManualFishingStrengthenSelectedOutcomeV1::OverCapCatchPending {
                        line_stage,
                    })
                }
                ManualFishingUnmodifiedLineStageV1::LandedWithoutLineRisk => {
                    let request = self.preview_strengthen_fish_candidate_v1(0)?;
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
                    let durability = self.stage_strengthen_rod_durability_v1().await?;
                    Ok(PendingManualFishingStrengthenSelectedOutcomeV1::LandedFish {
                        delivery,
                        progression,
                        durability,
                    })
                }
            },
            FishingCatchBranch::Junk | FishingCatchBranch::Treasure => {
                match self.stage_non_fish_candidate_v1(branch).await {
                    Ok(staged) => {
                        let durability = self.stage_strengthen_rod_durability_v1().await?;
                        Ok(PendingManualFishingStrengthenSelectedOutcomeV1::NonFish {
                            staged,
                            durability,
                        })
                    }
                    Err(ManualFishingCastOwnerError::NonFishRoll(
                        FishingNonFishRollError::EnchantBookRequiresDedicatedResolver,
                    )) => Ok(PendingManualFishingStrengthenSelectedOutcomeV1::DirectBookPending {
                        candidate: self.preview_strengthen_selected_direct_book_v1()?,
                    }),
                    Err(other) => Err(other),
                }
            }
        }
    }

    /// Composes the selected branch for the exact Unbreaking-X-only baseline path.
    ///
    /// Catch identity/capability remain the baseline kernels because Unbreaking X is
    /// branch-neutral. Landed Fish and non-Fish outcomes stage the same asset/XP
    /// children as the zero-enchant baseline, while ordinary Rod wear is resolved
    /// through the independent pinned Unbreaking domain. Line break still destroys
    /// the Rod with zero successful XP. Over-cap catch and direct Book delivery stay
    /// explicitly pending. The transaction remains non-committable.
    pub async fn stage_unbreaking_x_selected_outcome_v1(
        &mut self,
    ) -> Result<PendingManualFishingUnbreakingXSelectedOutcomeV1, ManualFishingCastOwnerError> {
        let branch = self.preview_unbreaking_x_base_branch_v1()?;
        match branch {
            FishingCatchBranch::Fish => {
                match self.preview_unbreaking_x_line_stage_v1(0)? {
                    ManualFishingUnmodifiedLineStageV1::LineBroken { .. } => {
                        let durability = self.stage_unbreaking_x_rod_durability_v1().await?;
                        Ok(PendingManualFishingUnbreakingXSelectedOutcomeV1::LineBroken {
                            durability,
                        })
                    }
                    line_stage @ ManualFishingUnmodifiedLineStageV1::LineSurvivedOverCapCatchPending { .. } => {
                        Ok(PendingManualFishingUnbreakingXSelectedOutcomeV1::OverCapCatchPending {
                            line_stage,
                        })
                    }
                    ManualFishingUnmodifiedLineStageV1::LandedWithoutLineRisk => {
                        let request = self.preview_unbreaking_x_fish_candidate_v1(0)?;
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
                        let durability = self.stage_unbreaking_x_rod_durability_v1().await?;
                        Ok(PendingManualFishingUnbreakingXSelectedOutcomeV1::LandedFish {
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
                        let durability = self.stage_unbreaking_x_rod_durability_v1().await?;
                        Ok(PendingManualFishingUnbreakingXSelectedOutcomeV1::NonFish {
                            staged,
                            durability,
                        })
                    }
                    Err(ManualFishingCastOwnerError::NonFishRoll(
                        FishingNonFishRollError::EnchantBookRequiresDedicatedResolver,
                    )) => Ok(PendingManualFishingUnbreakingXSelectedOutcomeV1::DirectBookPending {
                        candidate: self.preview_unbreaking_x_selected_direct_book_v1()?,
                    }),
                    Err(other) => Err(other),
                }
            }
        }
    }

    /// Atomically commits the fully resolved exact Strengthen-I-X-only path.
    ///
    /// Strengthen changes only the exact physical capability ratio. Landed
    /// Fish/Junk/non-book Treasure keep their existing asset/progression and
    /// ordinary-wear authorities; a line break commits only Rod destruction.
    /// Survived over-cap Fish and direct Book remain non-terminal.
    pub async fn commit_strengthen_v1(
        mut self,
    ) -> Result<ManualFishingStrengthenTerminalReceiptV1, ManualFishingCastOwnerError> {
        let strengthen_level = self.strengthen_only_level_v1()?;
        let staged = self.stage_strengthen_selected_outcome_v1().await?;
        let outcome = match staged {
            PendingManualFishingStrengthenSelectedOutcomeV1::LandedFish {
                delivery,
                progression,
                durability,
            } => {
                let durability = self.strengthen_terminal_durability_receipt_v1(&durability)?;
                ManualFishingStrengthenTerminalOutcomeV1::Fish {
                    delivery,
                    progression,
                    durability,
                }
            }
            PendingManualFishingStrengthenSelectedOutcomeV1::NonFish { staged, durability } => {
                let durability = self.strengthen_terminal_durability_receipt_v1(&durability)?;
                ManualFishingStrengthenTerminalOutcomeV1::NonFish {
                    delivery: staged.delivery,
                    progression: staged.progression,
                    durability,
                }
            }
            PendingManualFishingStrengthenSelectedOutcomeV1::LineBroken { durability } => {
                let durability = self.strengthen_terminal_durability_receipt_v1(&durability)?;
                ManualFishingStrengthenTerminalOutcomeV1::LineBroken { durability }
            }
            PendingManualFishingStrengthenSelectedOutcomeV1::OverCapCatchPending { .. } => {
                return Err(ManualFishingCastOwnerError::StrengthenTerminalOverCapPending);
            }
            PendingManualFishingStrengthenSelectedOutcomeV1::DirectBookPending { .. } => {
                return Err(ManualFishingCastOwnerError::StrengthenTerminalDirectBookPending);
            }
        };
        let receipt = ManualFishingStrengthenTerminalReceiptV1 {
            receipt_kind: ManualFishingTerminalReceiptKindV1::Strengthen,
            operation_id: self.preflight.operation_id,
            player_id: self.preflight.player_id,
            area: self.preflight.area_access.area,
            strengthen_level,
            outcome,
        };
        finalize_manual_fishing_strengthen_v1(&mut self.tx, &receipt).await?;
        self.tx
            .commit()
            .await
            .map_err(ManualFishingStrengthenTerminalError::from)?;
        Ok(receipt)
    }

    fn strengthen_terminal_durability_receipt_v1(
        &self,
        applied: &AppliedFishingRodDurabilityState,
    ) -> Result<FishingRodDurabilityMutationReceiptV1, ManualFishingCastOwnerError> {
        let receipt = self
            .staged_durability
            .as_ref()
            .ok_or(ManualFishingCastOwnerError::StrengthenTerminalDurabilityMissing)?;
        if &receipt.applied != applied {
            return Err(ManualFishingCastOwnerError::StrengthenTerminalDurabilityMismatch);
        }
        Ok(receipt.clone())
    }

    /// Atomically commits fully resolved exact SharpHook-I-X-only outcomes.
    ///
    /// SharpHook changes only the unresolved post-line-break over-cap catch chance.
    /// Therefore only landed within-capability Fish, Junk/non-book Treasure and
    /// line break may cross the terminal boundary. Survived over-cap Fish and
    /// direct Book stay non-terminal.
    pub async fn commit_sharp_hook_v1(
        mut self,
    ) -> Result<ManualFishingSharpHookTerminalReceiptV1, ManualFishingCastOwnerError> {
        let sharp_hook_level = self.sharp_hook_only_level_v1()?;
        let staged = self.stage_sharp_hook_selected_outcome_v1().await?;
        let outcome = match staged {
            PendingManualFishingSharpHookSelectedOutcomeV1::LandedFish {
                delivery,
                progression,
                durability,
            } => {
                let durability = self.sharp_hook_terminal_durability_receipt_v1(&durability)?;
                ManualFishingSharpHookTerminalOutcomeV1::Fish {
                    delivery,
                    progression,
                    durability,
                }
            }
            PendingManualFishingSharpHookSelectedOutcomeV1::NonFish { staged, durability } => {
                let durability = self.sharp_hook_terminal_durability_receipt_v1(&durability)?;
                ManualFishingSharpHookTerminalOutcomeV1::NonFish {
                    delivery: staged.delivery,
                    progression: staged.progression,
                    durability,
                }
            }
            PendingManualFishingSharpHookSelectedOutcomeV1::LineBroken { durability } => {
                let durability = self.sharp_hook_terminal_durability_receipt_v1(&durability)?;
                ManualFishingSharpHookTerminalOutcomeV1::LineBroken { durability }
            }
            PendingManualFishingSharpHookSelectedOutcomeV1::OverCapCatchPending { .. } => {
                return Err(ManualFishingCastOwnerError::SharpHookTerminalOverCapPending);
            }
            PendingManualFishingSharpHookSelectedOutcomeV1::DirectBookPending { .. } => {
                return Err(ManualFishingCastOwnerError::SharpHookTerminalDirectBookPending);
            }
        };
        let receipt = ManualFishingSharpHookTerminalReceiptV1 {
            receipt_kind: ManualFishingTerminalReceiptKindV1::SharpHook,
            operation_id: self.preflight.operation_id,
            player_id: self.preflight.player_id,
            area: self.preflight.area_access.area,
            sharp_hook_level,
            outcome,
        };
        finalize_manual_fishing_sharp_hook_v1(&mut self.tx, &receipt).await?;
        self.tx
            .commit()
            .await
            .map_err(ManualFishingSharpHookTerminalError::from)?;
        Ok(receipt)
    }

    fn sharp_hook_terminal_durability_receipt_v1(
        &self,
        applied: &AppliedFishingRodDurabilityState,
    ) -> Result<FishingRodDurabilityMutationReceiptV1, ManualFishingCastOwnerError> {
        let receipt = self
            .staged_durability
            .as_ref()
            .ok_or(ManualFishingCastOwnerError::SharpHookTerminalDurabilityMissing)?;
        if &receipt.applied != applied {
            return Err(ManualFishingCastOwnerError::SharpHookTerminalDurabilityMismatch);
        }
        Ok(receipt.clone())
    }

    /// Atomically commits the fully resolved exact Unbreaking-X-only path.
    ///
    /// Landed Fish/Junk/non-book Treasure include their successful children, the
    /// pinned 20% wear preview and the matching operation-keyed durability receipt.
    /// A line break commits Rod destruction without sampling Unbreaking or awarding
    /// successful XP. Survived over-cap Fish and direct Book remain non-terminal.
    pub async fn commit_unbreaking_x_v1(
        mut self,
    ) -> Result<ManualFishingUnbreakingXTerminalReceiptV1, ManualFishingCastOwnerError> {
        self.ensure_unbreaking_x_only_cast_v1()?;
        let staged = self.stage_unbreaking_x_selected_outcome_v1().await?;
        let outcome = match staged {
            PendingManualFishingUnbreakingXSelectedOutcomeV1::LandedFish {
                delivery,
                progression,
                durability,
            } => {
                let wear = self.preview_unbreaking_x_ordinary_wear_v1()?;
                let durability = self.unbreaking_x_terminal_durability_receipt_v1(&durability)?;
                ManualFishingUnbreakingXTerminalOutcomeV1::Fish {
                    delivery,
                    progression,
                    wear,
                    durability,
                }
            }
            PendingManualFishingUnbreakingXSelectedOutcomeV1::NonFish { staged, durability } => {
                let wear = self.preview_unbreaking_x_ordinary_wear_v1()?;
                let durability = self.unbreaking_x_terminal_durability_receipt_v1(&durability)?;
                ManualFishingUnbreakingXTerminalOutcomeV1::NonFish {
                    delivery: staged.delivery,
                    progression: staged.progression,
                    wear,
                    durability,
                }
            }
            PendingManualFishingUnbreakingXSelectedOutcomeV1::LineBroken { durability } => {
                let durability = self.unbreaking_x_terminal_durability_receipt_v1(&durability)?;
                ManualFishingUnbreakingXTerminalOutcomeV1::LineBroken { durability }
            }
            PendingManualFishingUnbreakingXSelectedOutcomeV1::OverCapCatchPending { .. } => {
                return Err(ManualFishingCastOwnerError::UnbreakingXTerminalOverCapPending);
            }
            PendingManualFishingUnbreakingXSelectedOutcomeV1::DirectBookPending { .. } => {
                return Err(ManualFishingCastOwnerError::UnbreakingXTerminalDirectBookPending);
            }
        };
        let receipt = ManualFishingUnbreakingXTerminalReceiptV1 {
            receipt_kind: ManualFishingTerminalReceiptKindV1::UnbreakingX,
            operation_id: self.preflight.operation_id,
            player_id: self.preflight.player_id,
            area: self.preflight.area_access.area,
            outcome,
        };
        finalize_manual_fishing_unbreaking_x_v1(&mut self.tx, &receipt).await?;
        self.tx
            .commit()
            .await
            .map_err(ManualFishingUnbreakingXTerminalError::from)?;
        Ok(receipt)
    }

    fn unbreaking_x_terminal_durability_receipt_v1(
        &self,
        applied: &AppliedFishingRodDurabilityState,
    ) -> Result<FishingRodDurabilityMutationReceiptV1, ManualFishingCastOwnerError> {
        let receipt = self
            .staged_durability
            .as_ref()
            .ok_or(ManualFishingCastOwnerError::UnbreakingXTerminalDurabilityMissing)?;
        if &receipt.applied != applied {
            return Err(ManualFishingCastOwnerError::UnbreakingXTerminalDurabilityMismatch);
        }
        Ok(receipt.clone())
    }

    /// Atomically commits the fully resolved zero-modifier ordinary non-Gold Rod path.
    ///
    /// Landed Fish/Junk/non-book Treasure carry their already-staged asset,
    /// successful progression and operation-keyed ordinary wear receipt. A line
    /// break commits only Rod destruction. Survived over-cap Fish and direct
    /// Enchant Book remain blocked before any terminal transition.
    pub async fn commit_unmodified_ordinary_v1(
        mut self,
    ) -> Result<ManualFishingUnmodifiedOrdinaryTerminalReceiptV1, ManualFishingCastOwnerError> {
        let EquippedFishingRodKind::Ordinary { tier } = self.preflight.rod.kind else {
            return Err(ManualFishingCastOwnerError::UnmodifiedOrdinaryTerminalRequiresOrdinaryRod);
        };
        if tier == EquipmentTier::Gold {
            return Err(ManualFishingCastOwnerError::UnmodifiedOrdinaryTerminalRequiresOrdinaryRod);
        }
        self.ensure_unmodified_cast_v1()?;

        let staged = self.stage_unmodified_selected_outcome_v1().await?;
        let outcome = match staged {
            PendingManualFishingUnmodifiedSelectedOutcomeV1::LandedFish {
                delivery,
                progression,
                durability,
            } => {
                let durability = self.terminal_durability_receipt_v1(&durability)?;
                ManualFishingUnmodifiedOrdinaryTerminalOutcomeV1::Fish {
                    delivery,
                    progression,
                    durability,
                }
            }
            PendingManualFishingUnmodifiedSelectedOutcomeV1::NonFish { staged, durability } => {
                let durability = self.terminal_durability_receipt_v1(&durability)?;
                ManualFishingUnmodifiedOrdinaryTerminalOutcomeV1::NonFish {
                    delivery: staged.delivery,
                    progression: staged.progression,
                    durability,
                }
            }
            PendingManualFishingUnmodifiedSelectedOutcomeV1::LineBroken { durability } => {
                let durability = self.terminal_durability_receipt_v1(&durability)?;
                ManualFishingUnmodifiedOrdinaryTerminalOutcomeV1::LineBroken { durability }
            }
            PendingManualFishingUnmodifiedSelectedOutcomeV1::OverCapCatchPending { .. } => {
                return Err(ManualFishingCastOwnerError::UnmodifiedOrdinaryTerminalOverCapPending);
            }
            PendingManualFishingUnmodifiedSelectedOutcomeV1::DirectBookPending { .. } => {
                return Err(
                    ManualFishingCastOwnerError::UnmodifiedOrdinaryTerminalDirectBookPending,
                );
            }
        };
        let receipt = ManualFishingUnmodifiedOrdinaryTerminalReceiptV1 {
            receipt_kind: ManualFishingTerminalReceiptKindV1::UnmodifiedOrdinary,
            operation_id: self.preflight.operation_id,
            player_id: self.preflight.player_id,
            area: self.preflight.area_access.area,
            outcome,
        };
        finalize_manual_fishing_unmodified_ordinary_v1(&mut self.tx, &receipt).await?;
        self.tx
            .commit()
            .await
            .map_err(ManualFishingUnmodifiedOrdinaryTerminalError::from)?;
        Ok(receipt)
    }

    fn terminal_durability_receipt_v1(
        &self,
        applied: &AppliedFishingRodDurabilityState,
    ) -> Result<FishingRodDurabilityMutationReceiptV1, ManualFishingCastOwnerError> {
        let receipt = self
            .staged_durability
            .as_ref()
            .ok_or(ManualFishingCastOwnerError::UnmodifiedOrdinaryTerminalDurabilityMissing)?;
        if &receipt.applied != applied {
            return Err(ManualFishingCastOwnerError::UnmodifiedOrdinaryTerminalDurabilityMismatch);
        }
        Ok(receipt.clone())
    }

    /// Atomically commits the complete no-modifier Starter Pool + Starter Basic
    /// path for Fish, Junk, or non-book Treasure.
    ///
    /// Direct Enchant Book remains blocked before terminal transition. No other
    /// area/Rod/modifier path is authorized by this narrow terminal owner.
    pub async fn commit_unmodified_starter_base_v1(
        mut self,
    ) -> Result<ManualFishingStarterBaseTerminalReceiptV1, ManualFishingCastOwnerError> {
        let staged = self.stage_unmodified_starter_base_outcome_v1().await?;
        let outcome = match staged {
            PendingManualFishingStarterBaseOutcomeV1::Fish {
                delivery,
                progression,
            } => ManualFishingStarterBaseTerminalOutcomeV1::Fish {
                delivery,
                progression,
            },
            PendingManualFishingStarterBaseOutcomeV1::NonFish { staged } => {
                ManualFishingStarterBaseTerminalOutcomeV1::NonFish {
                    delivery: staged.delivery,
                    progression: staged.progression,
                }
            }
            PendingManualFishingStarterBaseOutcomeV1::DirectBookPending { .. } => {
                return Err(ManualFishingCastOwnerError::StarterTerminalDirectBookPending);
            }
        };
        let receipt = ManualFishingStarterBaseTerminalReceiptV1 {
            operation_id: self.preflight.operation_id,
            player_id: self.preflight.player_id,
            area: self.preflight.area_access.area,
            outcome,
        };
        finalize_manual_fishing_starter_base_v1(&mut self.tx, &receipt).await?;
        self.tx
            .commit()
            .await
            .map_err(ManualFishingStarterBaseTerminalError::from)?;
        Ok(receipt)
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
/// Creates/resumes the canonical request-key operation and immediately enters
/// authoritative manual-Fishing preflight in the same caller-owned transaction.
///
/// This is the adapter-safe entrypoint for pre-terminal work: adapters provide
/// actor/request-key/area only and cannot supply an operation id, player id or RNG
/// root. If operation resolution or preflight fails, the owned transaction drops
/// and the newly inserted PENDING operation is rolled back with every partial
/// preflight mutation. The returned pending owner still has no commit path.
/// Resolves a request-key cast as either new/PENDING work or an exact typed
/// committed Starter-Basic replay. A COMMITTED replay never re-enters preflight,
/// bait, item, progression or RNG mutation; the read transaction is closed first.
pub async fn resolve_manual_fishing_cast_request_v1<'c>(
    mut tx: Transaction<'c, Postgres>,
    discord_user_id: u64,
    external_request_key: &str,
    area: FishingArea,
) -> Result<ManualFishingCastRequestResolutionV1<'c>, ManualFishingCastOwnerError> {
    match resolve_manual_fishing_operation_v1(&mut tx, discord_user_id, external_request_key, area)
        .await?
    {
        ManualFishingOperationResolutionV1::Pending(operation) => begin_manual_fishing_cast_v1(
            tx,
            operation.operation_id,
            operation.player_id,
            operation.area,
        )
        .await
        .map(Box::new)
        .map(ManualFishingCastRequestResolutionV1::Pending),
        ManualFishingOperationResolutionV1::CommittedStarterBase(receipt) => {
            tx.commit()
                .await
                .map_err(ManualFishingOperationError::from)?;
            Ok(ManualFishingCastRequestResolutionV1::CommittedStarterBase(
                receipt,
            ))
        }
        ManualFishingOperationResolutionV1::CommittedUnmodifiedOrdinary(receipt) => {
            tx.commit()
                .await
                .map_err(ManualFishingOperationError::from)?;
            Ok(ManualFishingCastRequestResolutionV1::CommittedUnmodifiedOrdinary(receipt))
        }
        ManualFishingOperationResolutionV1::CommittedStrengthen(receipt) => {
            tx.commit()
                .await
                .map_err(ManualFishingOperationError::from)?;
            Ok(ManualFishingCastRequestResolutionV1::CommittedStrengthen(
                receipt,
            ))
        }
        ManualFishingOperationResolutionV1::CommittedSharpHook(receipt) => {
            tx.commit()
                .await
                .map_err(ManualFishingOperationError::from)?;
            Ok(ManualFishingCastRequestResolutionV1::CommittedSharpHook(
                receipt,
            ))
        }
        ManualFishingOperationResolutionV1::CommittedUnbreakingX(receipt) => {
            tx.commit()
                .await
                .map_err(ManualFishingOperationError::from)?;
            Ok(ManualFishingCastRequestResolutionV1::CommittedUnbreakingX(
                receipt,
            ))
        }
    }
}

pub async fn begin_manual_fishing_cast_request_v1<'c>(
    mut tx: Transaction<'c, Postgres>,
    discord_user_id: u64,
    external_request_key: &str,
    area: FishingArea,
) -> Result<PendingManualFishingCastV1<'c>, ManualFishingCastOwnerError> {
    let operation = lock_or_create_manual_fishing_operation_v1(
        &mut tx,
        discord_user_id,
        external_request_key,
        area,
    )
    .await?;
    begin_manual_fishing_cast_v1(
        tx,
        operation.operation_id,
        operation.player_id,
        operation.area,
    )
    .await
}

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
