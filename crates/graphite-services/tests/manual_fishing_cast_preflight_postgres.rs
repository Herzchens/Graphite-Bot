use graphite_core::{DomainRng, OperationId, RootSeed};
use graphite_progression::account_total_xp_for_level;
use graphite_services::{
    AppliedFishingRodDurabilityState, CanonicalEnchant, EquipmentTier, EquippedFishingRodKind,
    FishingArea, FishingAreaAccessOrigin, FishingCatchBranch, FishingDirectBookRollError,
    FishingResolvedNonFishOutcomeV1, MAX_FISH_PER_CAST, ManualFishingCastOwnerError,
    ManualFishingCastPreflightError, ManualFishingPpmDrawV1, ManualFishingRngDrawError,
    ManualFishingUnmodifiedCapabilityPreviewV1, ManualFishingUnmodifiedFishCandidateError,
    ManualFishingUnmodifiedLineStageV1, ManualFishingWeightedDrawV1, MultiTreasureCount,
    MulticatchCount, NATIVE_ACTIVE_BAIT_CATEGORY_SLOTS,
    PendingManualFishingBaitRackSelectedOutcomeV1,
    PendingManualFishingMultiTreasureSelectedOutcomeV1,
    PendingManualFishingMulticatchSelectedOutcomeV1,
    PendingManualFishingSharpHookSelectedOutcomeV1, PendingManualFishingStarterBaseOutcomeV1,
    PendingManualFishingStrengthenSelectedOutcomeV1,
    PendingManualFishingUnbreakingXSelectedOutcomeV1,
    PendingManualFishingUnmodifiedSelectedOutcomeV1, begin_manual_fishing_cast_v1,
    direct_fishing_book_pool_membership, direct_fishing_finished_book_level_weight,
    lock_manual_fishing_cast_preflight, manual_fishing_base_fish_cast_aexp,
    multi_treasure_count_policy, multicatch_count_policy, preview_over_cap_catch_chance,
    resolve_manual_fishing_direct_book_candidate_v1,
};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn pending_cast_owner_rolls_back_consumed_bait_and_retains_operation_identity() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    seed_starter_basic_rod(&store, player_id, nonce, "pending-owner").await;
    let operation_id = seed_operation(&store, player_id, nonce, "pending-owner").await;

    sqlx::query(
        "INSERT INTO item_stacks (player_id, definition_key, definition_version, location, quantity) VALUES ($1, 'fishing.bait.treasure', 1, 'ITEM_BAG', 2)",
    )
    .bind(player_id)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO player_active_fishing_baits (player_id, bait_kind, definition_key, definition_version, activated_by_operation_id) VALUES ($1, 'TREASURE', 'fishing.bait.treasure', 1, $2)",
    )
    .bind(player_id)
    .bind(operation_id)
    .execute(store.pool())
    .await
    .unwrap();

    let tx = store.pool().begin().await.unwrap();
    let mut pending =
        begin_manual_fishing_cast_v1(tx, operation_id, player_id, FishingArea::StarterPool)
            .await
            .unwrap();
    assert_eq!(pending.operation_id(), operation_id);
    assert_eq!(pending.player_id(), player_id);
    assert_eq!(pending.bait_settlement().plan.consumed_bait_categories, 1);
    assert!(matches!(
        pending.preview_unmodified_fish_candidate_v1(0),
        Err(ManualFishingCastOwnerError::ModifiedFishRequiresSharedComposer)
    ));
    assert!(matches!(
        pending.preview_unmodified_base_branch_v1(),
        Err(ManualFishingCastOwnerError::ModifiedFishRequiresSharedComposer)
    ));
    assert!(matches!(
        pending.stage_unmodified_rod_durability_v1().await,
        Err(ManualFishingCastOwnerError::ModifiedFishRequiresSharedComposer)
    ));
    let weights = pending.pre_cap_branch_weights();
    assert_eq!(
        (weights.fish, weights.junk, weights.treasure),
        (70_400, 6_120, 3_220)
    );
    let provisional = pending
        .preview_non_fish_candidate_v1(FishingCatchBranch::Junk)
        .unwrap();
    assert!(matches!(
        provisional,
        FishingResolvedNonFishOutcomeV1::Junk { .. }
    ));
    assert_eq!(
        provisional,
        pending
            .preview_non_fish_candidate_v1(FishingCatchBranch::Junk)
            .unwrap()
    );
    let first_stage = pending
        .stage_non_fish_candidate_v1(FishingCatchBranch::Junk)
        .await
        .unwrap();
    assert_eq!(first_stage.outcome, provisional);
    assert_eq!(first_stage.delivery.deliveries.len(), 1);
    assert_eq!(first_stage.progression.base_activity_xp, 2);
    assert_eq!(first_stage.progression.account_xp.granted_xp, 2);
    let replay = pending
        .stage_non_fish_candidate_v1(FishingCatchBranch::Junk)
        .await
        .unwrap();
    assert_eq!(first_stage, replay);

    pending.rollback().await.unwrap();

    let staged_events: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM progression_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(staged_events, 0);
    let quantity: i64 = sqlx::query_scalar(
        "SELECT quantity FROM item_stacks WHERE player_id = $1 AND definition_key = 'fishing.bait.treasure' AND definition_version = 1 AND location = 'ITEM_BAG'",
    )
    .bind(player_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(quantity, 2);
    assert_eq!(operation_state(&store, operation_id).await, "PENDING");

    let events: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM asset_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(events, 0);
}

#[tokio::test]
async fn unmodified_starter_cast_previews_stable_canonical_fish_and_rolls_back() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    seed_starter_basic_rod(&store, player_id, nonce, "unmodified-candidate").await;
    let operation_id = seed_operation(&store, player_id, nonce, "unmodified-candidate").await;

    let tx = store.pool().begin().await.unwrap();
    let mut pending =
        begin_manual_fishing_cast_v1(tx, operation_id, player_id, FishingArea::StarterPool)
            .await
            .unwrap();
    assert_eq!(
        pending.preview_unmodified_fish_capability_v1(0).unwrap(),
        ManualFishingUnmodifiedCapabilityPreviewV1::StarterPoolGuaranteedLanded
    );
    assert_eq!(
        pending.preview_unmodified_line_stage_v1(0).unwrap(),
        ManualFishingUnmodifiedLineStageV1::LandedWithoutLineRisk
    );
    let chosen_branch = pending.preview_unmodified_base_branch_v1().unwrap();
    assert_eq!(
        chosen_branch,
        pending.preview_unmodified_base_branch_v1().unwrap()
    );
    assert_eq!(
        chosen_branch,
        [
            FishingCatchBranch::Fish,
            FishingCatchBranch::Junk,
            FishingCatchBranch::Treasure,
        ][pending
            .preflight()
            .rng
            .sample_weighted_v1(ManualFishingWeightedDrawV1::CatchBranch, &[176, 17, 7])
            .unwrap()]
    );
    let preview0 =
        resolve_manual_fishing_direct_book_candidate_v1(pending.preflight().rng, 0).unwrap();
    assert_eq!(
        preview0,
        resolve_manual_fishing_direct_book_candidate_v1(pending.preflight().rng, 0).unwrap()
    );
    assert_eq!(preview0.treasure_slot, 0);
    assert_eq!(
        direct_fishing_book_pool_membership(preview0.enchant),
        Some(preview0.pool)
    );
    assert!(
        direct_fishing_finished_book_level_weight(preview0.enchant, preview0.level).unwrap() > 0
    );
    let preview1 =
        resolve_manual_fishing_direct_book_candidate_v1(pending.preflight().rng, 1).unwrap();
    assert_eq!(preview1.treasure_slot, 1);
    assert!(matches!(
        resolve_manual_fishing_direct_book_candidate_v1(pending.preflight().rng, 3),
        Err(FishingDirectBookRollError::Rng(
            ManualFishingRngDrawError::TreasureSlotOutOfRange(3)
        ))
    ));
    match pending.preview_unmodified_selected_direct_book_v1() {
        Ok(selected) => {
            assert_eq!(selected, preview0);
            assert_eq!(chosen_branch, FishingCatchBranch::Treasure);
        }
        Err(ManualFishingCastOwnerError::BookRequiresTreasureBranch) => {
            assert_ne!(chosen_branch, FishingCatchBranch::Treasure);
        }
        Err(ManualFishingCastOwnerError::BookRequiresSelectedEnchantBook) => {
            assert_eq!(chosen_branch, FishingCatchBranch::Treasure);
        }
        Err(other) => panic!("unexpected direct-book preview error: {other:?}"),
    }
    let first = pending.preview_unmodified_fish_candidate_v1(0).unwrap();
    let replay = pending.preview_unmodified_fish_candidate_v1(0).unwrap();
    assert_eq!(first, replay);
    assert_eq!(first.fish_slot, 0);
    assert_eq!(first.area, FishingArea::StarterPool);
    assert!(!first.numeric.quality_bait_active);
    assert_eq!(
        first.numeric.weight_quantile_ppm,
        pending
            .preflight()
            .rng
            .sample_ppm_v1(ManualFishingPpmDrawV1::FishWeight { fish_slot: 0 })
            .unwrap()
    );
    let second = pending.preview_unmodified_fish_candidate_v1(1).unwrap();
    assert_eq!(second.fish_slot, 1);
    assert!(matches!(
        pending.preview_unmodified_fish_candidate_v1(MAX_FISH_PER_CAST),
        Err(ManualFishingCastOwnerError::UnmodifiedFish(
            ManualFishingUnmodifiedFishCandidateError::Rng(
                ManualFishingRngDrawError::FishSlotOutOfRange(MAX_FISH_PER_CAST)
            )
        ))
    ));

    let starter_rod_receipt = pending.stage_unmodified_rod_durability_v1().await.unwrap();
    assert!(matches!(
        starter_rod_receipt,
        AppliedFishingRodDurabilityState::StarterBasicUnbreakable { .. }
    ));
    assert_eq!(
        starter_rod_receipt,
        pending.stage_unmodified_rod_durability_v1().await.unwrap()
    );

    let staged = pending
        .stage_unmodified_starter_pool_fish_v1()
        .await
        .unwrap();
    assert_eq!(staged.fish_slot, 0);
    assert_eq!(staged.operation_id, operation_id);
    assert_eq!(staged.player_id, player_id);
    let staged_again = pending
        .stage_unmodified_starter_pool_fish_v1()
        .await
        .unwrap();
    assert_eq!(staged, staged_again);
    let staged_item_id = staged.item_instance_id;

    pending.rollback().await.unwrap();
    let item_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM item_instances WHERE id = $1")
        .bind(staged_item_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(item_count, 0);
    let event_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM asset_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(event_count, 0);
    assert_eq!(operation_state(&store, operation_id).await, "PENDING");
}

#[tokio::test]
async fn pinned_baseline_starter_outcome_stages_only_the_selected_branch_and_rolls_back() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    seed_starter_basic_rod(&store, player_id, nonce, "selected-base").await;
    let operation_id = seed_operation(&store, player_id, nonce, "selected-base").await;
    let tx = store.pool().begin().await.unwrap();
    let mut pending =
        begin_manual_fishing_cast_v1(tx, operation_id, player_id, FishingArea::StarterPool)
            .await
            .unwrap();
    let selected = pending.preview_unmodified_base_branch_v1().unwrap();

    // A selected direct Book is a replayable pending identity, not a payout.
    if selected == FishingCatchBranch::Treasure
        && pending.preview_non_fish_candidate_v1(selected).is_err()
    {
        let candidate = pending
            .preview_unmodified_selected_direct_book_v1()
            .unwrap();
        let first = pending
            .stage_unmodified_starter_base_outcome_v1()
            .await
            .unwrap();
        assert_eq!(
            first,
            PendingManualFishingStarterBaseOutcomeV1::DirectBookPending { candidate }
        );
        assert_eq!(
            first,
            pending
                .stage_unmodified_starter_base_outcome_v1()
                .await
                .unwrap()
        );
    } else {
        let first = pending
            .stage_unmodified_starter_base_outcome_v1()
            .await
            .unwrap();
        let replay = pending
            .stage_unmodified_starter_base_outcome_v1()
            .await
            .unwrap();
        assert_eq!(first, replay);
        match (selected, first) {
            (
                FishingCatchBranch::Fish,
                PendingManualFishingStarterBaseOutcomeV1::Fish {
                    delivery,
                    progression,
                },
            ) => {
                assert_eq!(delivery.fish_slot, 0);
                assert_eq!(progression.account_xp.granted_xp, 2);
                assert!(progression.base_activity_xp > 0);
            }
            (
                FishingCatchBranch::Junk,
                PendingManualFishingStarterBaseOutcomeV1::NonFish { staged },
            ) => {
                assert!(matches!(
                    staged.outcome,
                    FishingResolvedNonFishOutcomeV1::Junk { .. }
                ));
                assert_eq!(staged.progression.base_activity_xp, 2);
            }
            (
                FishingCatchBranch::Treasure,
                PendingManualFishingStarterBaseOutcomeV1::NonFish { staged },
            ) => {
                assert!(matches!(
                    staged.outcome,
                    FishingResolvedNonFishOutcomeV1::Treasure { .. }
                ));
                assert!(staged.progression.base_activity_xp >= 5);
            }
            (branch, outcome) => panic!("selected branch/staging mismatch: {branch:?} {outcome:?}"),
        }
    }

    pending.rollback().await.unwrap();
    assert_eq!(operation_state(&store, operation_id).await, "PENDING");
    let asset_events: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM asset_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let xp_events: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM progression_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!((asset_events, xp_events), (0, 0));
}

#[tokio::test]
async fn selected_direct_book_is_reachable_and_replay_stable_in_starter_and_ordinary_casts() {
    let Some(store) = test_store().await else {
        return;
    };
    for (area, starter) in [
        (FishingArea::StarterPool, true),
        (FishingArea::River, false),
    ] {
        let nonce = Uuid::now_v7();
        let player_id = seed_player(&store, positive_snowflake(nonce)).await;
        let rod_id = if starter {
            seed_starter_basic_rod(&store, player_id, nonce, "book-positive").await
        } else {
            set_account_level(&store, player_id, 10).await;
            seed_ordinary_rod(
                &store,
                player_id,
                nonce,
                "book-positive",
                "WOOD",
                200,
                600,
                false,
                4,
                3,
            )
            .await
        };
        let operation_id = seed_operation(&store, player_id, nonce, "book-positive").await;
        let root = find_selected_direct_book_root(operation_id);
        sqlx::query("UPDATE operations SET rng_root = $1 WHERE id = $2")
            .bind(root.as_slice())
            .bind(operation_id)
            .execute(store.pool())
            .await
            .unwrap();

        let mut previous = None;
        for _ in 0..2 {
            let tx = store.pool().begin().await.unwrap();
            let mut pending = begin_manual_fishing_cast_v1(tx, operation_id, player_id, area)
                .await
                .unwrap();
            assert_eq!(
                pending.preview_unmodified_base_branch_v1().unwrap(),
                FishingCatchBranch::Treasure
            );
            assert!(matches!(
                pending.preview_non_fish_candidate_v1(FishingCatchBranch::Treasure),
                Err(ManualFishingCastOwnerError::NonFishRoll(
                    graphite_services::FishingNonFishRollError::EnchantBookRequiresDedicatedResolver
                ))
            ));
            let candidate = pending
                .preview_unmodified_selected_direct_book_v1()
                .unwrap();
            let actual = if starter {
                let outcome = pending
                    .stage_unmodified_starter_base_outcome_v1()
                    .await
                    .unwrap();
                let PendingManualFishingStarterBaseOutcomeV1::DirectBookPending {
                    candidate: selected,
                } = outcome
                else {
                    panic!("Starter Basic must keep selected direct Book pending");
                };
                selected
            } else {
                let outcome = pending
                    .stage_unmodified_selected_outcome_v1()
                    .await
                    .unwrap();
                let PendingManualFishingUnmodifiedSelectedOutcomeV1::DirectBookPending {
                    candidate: selected,
                } = outcome
                else {
                    panic!("ordinary Rod must keep selected direct Book pending");
                };
                selected
            };
            assert_eq!(actual, candidate);
            assert_eq!(candidate.treasure_slot, 0);
            if let Some(previous_candidate) = previous {
                assert_eq!(candidate, previous_candidate);
            }
            previous = Some(candidate);
            pending.rollback().await.unwrap();
        }
        let new_items: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM item_instances WHERE created_by_operation_id = $1",
        )
        .bind(operation_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        let asset_events: i64 =
            sqlx::query_scalar("SELECT count(*) FROM asset_events WHERE operation_id = $1")
                .bind(operation_id)
                .fetch_one(store.pool())
                .await
                .unwrap();
        let xp_events: i64 =
            sqlx::query_scalar("SELECT count(*) FROM progression_events WHERE operation_id = $1")
                .bind(operation_id)
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!((new_items, asset_events, xp_events), (0, 0, 0));
        if !starter {
            let durability: i64 =
                sqlx::query_scalar("SELECT current_durability FROM item_instances WHERE id = $1")
                    .bind(rod_id)
                    .fetch_one(store.pool())
                    .await
                    .unwrap();
            assert_eq!(durability, 200);
        }
        assert_eq!(operation_state(&store, operation_id).await, "PENDING");
    }
}

fn find_multi_treasure_root(
    operation_id: Uuid,
    treasure_result_index: usize,
    level: u8,
) -> [u8; 32] {
    let operation = OperationId::from_uuid(operation_id);
    let single_ppm = multi_treasure_count_policy(level, MultiTreasureCount::Single)
        .unwrap()
        .probability_ppm;
    for nonce in 0_u64..200_000 {
        let mut bytes = [0_u8; 32];
        bytes[..8].copy_from_slice(&nonce.to_be_bytes());
        let root = RootSeed::from_bytes(bytes);
        let mut branch = DomainRng::derive(root, operation, "fishing.manual.v1.catch_branch");
        if branch.sample_weighted(&[176, 17, 7]).unwrap() != 2 {
            continue;
        }
        let mut treasure = DomainRng::derive(root, operation, "fishing.manual.v1.treasure_result");
        if treasure.sample_weighted(&[20, 15, 4, 4, 5, 2]).unwrap() != treasure_result_index {
            continue;
        }
        let mut count =
            DomainRng::derive(root, operation, "fishing.manual.v1.multi_treasure_count");
        let draw = count.sample_uniform_below(1_000_000).unwrap();
        if draw >= u64::from(single_ppm) {
            return bytes;
        }
    }
    panic!("test fixture could not find a deterministic repeated Multi Treasure seed");
}

fn find_multicatch_fish_root(operation_id: Uuid, level: u8) -> [u8; 32] {
    let operation = OperationId::from_uuid(operation_id);
    let single_ppm = multicatch_count_policy(level, MulticatchCount::Single)
        .unwrap()
        .probability_ppm;
    for nonce in 0_u64..20_000 {
        let mut bytes = [0_u8; 32];
        bytes[..8].copy_from_slice(&nonce.to_be_bytes());
        let root = RootSeed::from_bytes(bytes);
        let mut branch = DomainRng::derive(root, operation, "fishing.manual.v1.catch_branch");
        if branch.sample_weighted(&[176, 17, 7]).unwrap() != 0 {
            continue;
        }
        let mut count = DomainRng::derive(root, operation, "fishing.manual.v1.multicatch_count");
        let draw = count.sample_uniform_below(1_000_000).unwrap();
        if draw >= u64::from(single_ppm) {
            return bytes;
        }
    }
    panic!("test fixture could not find a deterministic multi-fish Multicatch seed");
}

fn find_selected_direct_book_root(operation_id: Uuid) -> [u8; 32] {
    let operation = OperationId::from_uuid(operation_id);
    // Deterministically exercise the real versioned branch and Treasure-result domains,
    // not a mocked Book branch. This search changes no gameplay probabilities.
    for nonce in 0_u64..20_000 {
        let mut bytes = [0_u8; 32];
        bytes[..8].copy_from_slice(&nonce.to_be_bytes());
        let root = RootSeed::from_bytes(bytes);
        let mut branch = DomainRng::derive(root, operation, "fishing.manual.v1.catch_branch");
        if branch.sample_weighted(&[176, 17, 7]).unwrap() != 2 {
            continue;
        }
        let mut treasure = DomainRng::derive(root, operation, "fishing.manual.v1.treasure_result");
        if treasure.sample_weighted(&[20, 15, 4, 4, 5, 2]).unwrap() == 2 {
            return bytes;
        }
    }
    panic!("test fixture could not find a deterministic direct Book seed");
}

#[tokio::test]
async fn unmodified_river_candidate_uses_exact_capability_kernel_without_settlement() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    set_account_level(&store, player_id, 10).await;
    seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "capability",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    let operation_id = seed_operation(&store, player_id, nonce, "river-capability").await;

    let tx = store.pool().begin().await.unwrap();
    let pending = begin_manual_fishing_cast_v1(tx, operation_id, player_id, FishingArea::River)
        .await
        .unwrap();
    let first = pending.preview_unmodified_fish_capability_v1(0).unwrap();
    let line_stage = pending.preview_unmodified_line_stage_v1(0).unwrap();
    assert_eq!(
        line_stage,
        pending.preview_unmodified_line_stage_v1(0).unwrap()
    );
    match (first, line_stage) {
        (
            ManualFishingUnmodifiedCapabilityPreviewV1::WithinRodCapability { .. },
            ManualFishingUnmodifiedLineStageV1::LandedWithoutLineRisk,
        ) => {}
        (
            ManualFishingUnmodifiedCapabilityPreviewV1::OverCapPending { line_break, .. },
            ManualFishingUnmodifiedLineStageV1::LineBroken {
                draw_ppm,
                chance_ppm,
            },
        ) => {
            assert_eq!(chance_ppm, line_break.final_probability_ppm);
            assert!(draw_ppm < chance_ppm);
        }
        (
            ManualFishingUnmodifiedCapabilityPreviewV1::OverCapPending { line_break, .. },
            ManualFishingUnmodifiedLineStageV1::LineSurvivedOverCapCatchPending {
                draw_ppm,
                chance_ppm,
            },
        ) => {
            assert_eq!(chance_ppm, line_break.final_probability_ppm);
            assert!(draw_ppm >= chance_ppm);
        }
        (preview, stage) => panic!("invalid physical line-stage pairing: {preview:?} {stage:?}"),
    }

    assert_eq!(
        pending.preview_unmodified_fish_capability_v1(0).unwrap(),
        first
    );
    match first {
        ManualFishingUnmodifiedCapabilityPreviewV1::WithinRodCapability { ratio } => {
            assert!(ratio.numerator() <= ratio.denominator());
        }
        ManualFishingUnmodifiedCapabilityPreviewV1::OverCapPending {
            ratio,
            line_break,
            catch_chance,
        } => {
            assert!(ratio.numerator() > ratio.denominator());
            assert!(line_break.final_probability_ppm <= 150_000);
            assert!(catch_chance.numerator() > 0);
            assert!(catch_chance.denominator() > 0);
        }
        ManualFishingUnmodifiedCapabilityPreviewV1::StarterPoolGuaranteedLanded => {
            panic!("non-pool fish cannot bypass capability");
        }
    }

    pending.rollback().await.unwrap();
    assert_eq!(unlock_count(&store, player_id).await, 0);
    assert_eq!(operation_state(&store, operation_id).await, "PENDING");
}

#[tokio::test]
async fn unmodified_rod_wear_is_staged_once_and_rollback_restores_ordinary_rod() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "wear-owner",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    let operation_id = seed_operation(&store, player_id, nonce, "wear-owner").await;
    let tx = store.pool().begin().await.unwrap();
    let mut pending = begin_manual_fishing_cast_v1(tx, operation_id, player_id, FishingArea::River)
        .await
        .unwrap();
    let selected = pending.preview_unmodified_base_branch_v1().unwrap();
    let broken_line = selected == FishingCatchBranch::Fish
        && matches!(
            pending.preview_unmodified_line_stage_v1(0).unwrap(),
            ManualFishingUnmodifiedLineStageV1::LineBroken { .. }
        );
    let first = pending.stage_unmodified_rod_durability_v1().await.unwrap();
    let replay = pending.stage_unmodified_rod_durability_v1().await.unwrap();
    assert_eq!(first, replay);
    let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = first else {
        panic!("an ordinary Wood Rod cannot be a Starter Basic Rod");
    };
    assert_eq!(preview.current_durability, 200);
    assert_eq!(preview.max_durability, 600);
    assert_eq!(
        preview.resulting_durability,
        if broken_line { 0 } else { 199 }
    );

    pending.rollback().await.unwrap();
    let current: i64 =
        sqlx::query_scalar("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let broken: bool = sqlx::query_scalar("SELECT is_broken FROM item_instances WHERE id = $1")
        .bind(rod_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!((current, broken), (200, false));
    assert_eq!(unlock_count(&store, player_id).await, 0);
    assert_eq!(operation_state(&store, operation_id).await, "PENDING");
}

#[tokio::test]
async fn selected_unmodified_river_attempt_never_issues_unresolved_fish_rewards() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "selected-river",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    let operation_id = seed_operation(&store, player_id, nonce, "selected-river").await;
    let tx = store.pool().begin().await.unwrap();
    let mut pending = begin_manual_fishing_cast_v1(tx, operation_id, player_id, FishingArea::River)
        .await
        .unwrap();
    let branch = pending.preview_unmodified_base_branch_v1().unwrap();

    if branch == FishingCatchBranch::Treasure
        && pending.preview_non_fish_candidate_v1(branch).is_err()
    {
        let candidate = pending
            .preview_unmodified_selected_direct_book_v1()
            .unwrap();
        let first = pending
            .stage_unmodified_selected_outcome_v1()
            .await
            .unwrap();
        assert_eq!(
            first,
            PendingManualFishingUnmodifiedSelectedOutcomeV1::DirectBookPending { candidate }
        );
        assert_eq!(
            first,
            pending
                .stage_unmodified_selected_outcome_v1()
                .await
                .unwrap()
        );
    } else {
        let first = pending
            .stage_unmodified_selected_outcome_v1()
            .await
            .unwrap();
        let replay = pending
            .stage_unmodified_selected_outcome_v1()
            .await
            .unwrap();
        assert_eq!(first, replay);
        match (branch, first) {
            (
                FishingCatchBranch::Fish,
                PendingManualFishingUnmodifiedSelectedOutcomeV1::LandedFish {
                    delivery,
                    progression,
                    durability,
                },
            ) => {
                assert_eq!(delivery.fish_slot, 0);
                assert_eq!(progression.account_xp.granted_xp, 2);
                assert!(matches!(
                    durability,
                    AppliedFishingRodDurabilityState::Ordinary { .. }
                ));
            }
            (
                FishingCatchBranch::Fish,
                PendingManualFishingUnmodifiedSelectedOutcomeV1::LineBroken { durability },
            ) => {
                let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = durability else {
                    panic!("line-break destruction must apply to the ordinary Rod");
                };
                assert_eq!(preview.resulting_durability, 0);
            }
            (
                FishingCatchBranch::Fish,
                PendingManualFishingUnmodifiedSelectedOutcomeV1::OverCapCatchPending { line_stage },
            ) => {
                assert!(matches!(
                    line_stage,
                    ManualFishingUnmodifiedLineStageV1::LineSurvivedOverCapCatchPending { .. }
                ));
            }
            (
                FishingCatchBranch::Junk | FishingCatchBranch::Treasure,
                PendingManualFishingUnmodifiedSelectedOutcomeV1::NonFish { staged, durability },
            ) => {
                assert!(matches!(
                    durability,
                    AppliedFishingRodDurabilityState::Ordinary { .. }
                ));
                assert_eq!(staged.progression.account_xp.granted_xp, 2);
                assert_eq!(
                    staged.delivery.deliveries.len(),
                    match staged.outcome {
                        FishingResolvedNonFishOutcomeV1::Junk { .. } => 1,
                        FishingResolvedNonFishOutcomeV1::Treasure { ref payloads, .. } =>
                            payloads.len(),
                    }
                );
            }
            (branch, receipt) => {
                panic!("selected branch and staged result disagree: {branch:?} {receipt:?}")
            }
        }
    }

    pending.rollback().await.unwrap();
    let rod_durability: i64 =
        sqlx::query_scalar("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(rod_durability, 200);
    let assets: i64 =
        sqlx::query_scalar("SELECT count(*) FROM asset_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let xp: i64 =
        sqlx::query_scalar("SELECT count(*) FROM progression_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!((assets, xp), (0, 0));
    assert_eq!(operation_state(&store, operation_id).await, "PENDING");
}

#[tokio::test]
async fn multi_treasure_non_book_repeats_payloads_and_one_rod_event() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "multi-treasure-nonbook",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    seed_enchant(&store, rod_id, "MULTI_TREASURE", 10).await;
    let operation_id = seed_operation(&store, player_id, nonce, "multi-treasure-nonbook").await;
    let root = find_multi_treasure_root(operation_id, 0, 10);
    sqlx::query("UPDATE operations SET rng_root = $1 WHERE id = $2")
        .bind(root.as_slice())
        .bind(operation_id)
        .execute(store.pool())
        .await
        .unwrap();

    let tx = store.pool().begin().await.unwrap();
    let mut pending =
        begin_manual_fishing_cast_v1(tx, operation_id, player_id, FishingArea::StarterPool)
            .await
            .unwrap();
    assert_eq!(
        pending.preview_multi_treasure_base_branch_v1().unwrap(),
        FishingCatchBranch::Treasure
    );
    let preview = pending
        .preview_non_fish_candidate_v1(FishingCatchBranch::Treasure)
        .unwrap();
    let FishingResolvedNonFishOutcomeV1::Treasure { payloads, .. } = &preview else {
        panic!("forced Treasure result must stay Treasure");
    };
    assert!(payloads.len() > 1);

    let first = pending
        .stage_multi_treasure_selected_outcome_v1()
        .await
        .unwrap();
    let replay = pending
        .stage_multi_treasure_selected_outcome_v1()
        .await
        .unwrap();
    assert_eq!(first, replay);
    let PendingManualFishingMultiTreasureSelectedOutcomeV1::NonFish { staged, durability } = first
    else {
        panic!("forced non-book Multi Treasure must settle as NonFish");
    };
    assert_eq!(staged.outcome, preview);
    assert_eq!(staged.delivery.deliveries.len(), payloads.len());
    assert_eq!(staged.progression.base_activity_xp, 10);
    assert_eq!(staged.progression.account_xp.granted_xp, 2);
    let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = durability else {
        panic!("Multi Treasure-only path uses ordinary Rod durability");
    };
    assert_eq!(preview.resulting_durability, 199);

    pending.rollback().await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap(),
        200
    );
    let assets: i64 =
        sqlx::query_scalar("SELECT count(*) FROM asset_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let xp: i64 =
        sqlx::query_scalar("SELECT count(*) FROM progression_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!((assets, xp), (0, 0));
    assert_eq!(operation_state(&store, operation_id).await, "PENDING");
}

#[tokio::test]
async fn multi_treasure_direct_books_keep_independent_pending_identities() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "multi-treasure-books",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    seed_enchant(&store, rod_id, "MULTI_TREASURE", 10).await;
    let operation_id = seed_operation(&store, player_id, nonce, "multi-treasure-books").await;
    let root = find_multi_treasure_root(operation_id, 2, 10);
    sqlx::query("UPDATE operations SET rng_root = $1 WHERE id = $2")
        .bind(root.as_slice())
        .bind(operation_id)
        .execute(store.pool())
        .await
        .unwrap();

    let tx = store.pool().begin().await.unwrap();
    let mut pending =
        begin_manual_fishing_cast_v1(tx, operation_id, player_id, FishingArea::StarterPool)
            .await
            .unwrap();
    assert_eq!(
        pending.preview_multi_treasure_base_branch_v1().unwrap(),
        FishingCatchBranch::Treasure
    );
    assert!(matches!(
        pending.preview_non_fish_candidate_v1(FishingCatchBranch::Treasure),
        Err(ManualFishingCastOwnerError::NonFishRoll(
            graphite_services::FishingNonFishRollError::EnchantBookRequiresDedicatedResolver
        ))
    ));

    let (count, candidates) = pending
        .preview_multi_treasure_selected_direct_books_v1()
        .unwrap();
    assert!(count > 1);
    assert_eq!(candidates.len(), usize::from(count));
    for (slot, candidate) in candidates.iter().enumerate() {
        assert_eq!(candidate.treasure_slot, u8::try_from(slot).unwrap());
        assert_eq!(
            *candidate,
            resolve_manual_fishing_direct_book_candidate_v1(
                pending.preflight().rng,
                u8::try_from(slot).unwrap(),
            )
            .unwrap()
        );
    }

    let first = pending
        .stage_multi_treasure_selected_outcome_v1()
        .await
        .unwrap();
    assert_eq!(
        first,
        PendingManualFishingMultiTreasureSelectedOutcomeV1::DirectBooksPending {
            count,
            candidates: candidates.clone(),
        }
    );
    assert_eq!(
        first,
        pending
            .stage_multi_treasure_selected_outcome_v1()
            .await
            .unwrap()
    );

    pending.rollback().await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap(),
        200
    );
    let assets: i64 =
        sqlx::query_scalar("SELECT count(*) FROM asset_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let xp: i64 =
        sqlx::query_scalar("SELECT count(*) FROM progression_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!((assets, xp), (0, 0));
    assert_eq!(operation_state(&store, operation_id).await, "PENDING");
}

#[tokio::test]
async fn multicatch_selected_fish_rolls_independent_slots_and_one_rod_event() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "multicatch-only",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    seed_enchant(&store, rod_id, "MULTICATCH", 10).await;
    let operation_id = seed_operation(&store, player_id, nonce, "multicatch-only").await;
    let root = find_multicatch_fish_root(operation_id, 10);
    sqlx::query("UPDATE operations SET rng_root = $1 WHERE id = $2")
        .bind(root.as_slice())
        .bind(operation_id)
        .execute(store.pool())
        .await
        .unwrap();

    let tx = store.pool().begin().await.unwrap();
    let mut pending =
        begin_manual_fishing_cast_v1(tx, operation_id, player_id, FishingArea::StarterPool)
            .await
            .unwrap();
    assert_eq!(
        pending.preview_multicatch_base_branch_v1().unwrap(),
        FishingCatchBranch::Fish
    );
    let count = pending.preview_multicatch_count_v1().unwrap();
    assert!(count.fish_count > 1);
    assert_eq!(count, pending.preview_multicatch_count_v1().unwrap());

    let candidates = pending.preview_multicatch_fish_candidates_v1().unwrap();
    assert_eq!(candidates.len(), usize::from(count.fish_count));
    for (index, candidate) in candidates.iter().enumerate() {
        assert_eq!(candidate.fish_slot, u8::try_from(index).unwrap());
    }
    assert_eq!(
        pending.preview_multicatch_fish_capability_v1().unwrap(),
        ManualFishingUnmodifiedCapabilityPreviewV1::StarterPoolGuaranteedLanded
    );

    let first = pending
        .stage_multicatch_selected_outcome_v1()
        .await
        .unwrap();
    let replay = pending
        .stage_multicatch_selected_outcome_v1()
        .await
        .unwrap();
    assert_eq!(first, replay);
    let PendingManualFishingMulticatchSelectedOutcomeV1::LandedFish {
        count: staged_count,
        deliveries,
        progression,
        durability,
    } = first
    else {
        panic!("forced Starter Pool Multicatch Fish must land");
    };
    assert_eq!(staged_count, count);
    assert_eq!(deliveries.len(), usize::from(count.fish_count));
    let mut ids = deliveries
        .iter()
        .map(|delivery| delivery.item_instance_id)
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), deliveries.len());
    let rarities = deliveries
        .iter()
        .map(|delivery| delivery.rarity)
        .collect::<Vec<_>>();
    assert_eq!(
        progression.base_activity_xp,
        manual_fishing_base_fish_cast_aexp(&rarities).unwrap()
    );
    assert_eq!(progression.account_xp.granted_xp, 2);
    let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = durability else {
        panic!("Multicatch-only path uses ordinary Rod durability");
    };
    assert_eq!(preview.resulting_durability, 199);

    pending.rollback().await.unwrap();
    let current: i64 =
        sqlx::query_scalar("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(current, 200);
    let created: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM item_instances WHERE created_by_operation_id = $1",
    )
    .bind(operation_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    let asset_events: i64 =
        sqlx::query_scalar("SELECT count(*) FROM asset_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let progression_events: i64 =
        sqlx::query_scalar("SELECT count(*) FROM progression_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!((created, asset_events, progression_events), (0, 0, 0));
    assert_eq!(operation_state(&store, operation_id).await, "PENDING");
}

#[tokio::test]
async fn multicatch_owner_rejects_gold_and_additional_enchant() {
    let Some(store) = test_store().await else {
        return;
    };
    for (suffix, tier, second_enchant) in [
        ("multicatch-gold", "GOLD", None),
        ("multicatch-plus", "WOOD", Some(("STRENGTHEN", 1_i16))),
    ] {
        let nonce = Uuid::now_v7();
        let player_id = seed_player(&store, positive_snowflake(nonce)).await;
        set_account_level(&store, player_id, 10).await;
        let rod_id = seed_ordinary_rod(
            &store, player_id, nonce, suffix, tier, 200, 600, false, 4, 3,
        )
        .await;
        seed_enchant(&store, rod_id, "MULTICATCH", 10).await;
        if let Some((key, level)) = second_enchant {
            seed_enchant(&store, rod_id, key, level).await;
        }
        let operation_id = seed_operation(&store, player_id, nonce, suffix).await;
        let tx = store.pool().begin().await.unwrap();
        let pending = begin_manual_fishing_cast_v1(tx, operation_id, player_id, FishingArea::River)
            .await
            .unwrap();
        assert!(matches!(
            pending.preview_multicatch_base_branch_v1(),
            Err(ManualFishingCastOwnerError::MulticatchRequiresExactLoadout)
        ));
        pending.rollback().await.unwrap();
    }
}

#[tokio::test]
async fn bait_rack_only_selected_outcome_keeps_baseline_rng_and_rolls_back() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "bait-rack-only",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    seed_enchant(&store, rod_id, "BAIT_RACK", 3).await;
    let operation_id = seed_operation(&store, player_id, nonce, "bait-rack-only").await;

    let tx = store.pool().begin().await.unwrap();
    let mut pending = begin_manual_fishing_cast_v1(tx, operation_id, player_id, FishingArea::River)
        .await
        .unwrap();
    assert_eq!(
        pending.preflight().bait_capacity.active_bait_category_slots,
        6
    );
    assert!(matches!(
        pending.preview_unmodified_base_branch_v1(),
        Err(ManualFishingCastOwnerError::ModifiedFishRequiresSharedComposer)
    ));

    let branch = pending.preview_bait_rack_base_branch_v1().unwrap();
    assert_eq!(branch, pending.preview_bait_rack_base_branch_v1().unwrap());

    if branch == FishingCatchBranch::Treasure
        && pending.preview_non_fish_candidate_v1(branch).is_err()
    {
        let candidate = pending.preview_bait_rack_selected_direct_book_v1().unwrap();
        let first = pending.stage_bait_rack_selected_outcome_v1().await.unwrap();
        assert_eq!(
            first,
            PendingManualFishingBaitRackSelectedOutcomeV1::DirectBookPending { candidate }
        );
        assert_eq!(
            first,
            pending.stage_bait_rack_selected_outcome_v1().await.unwrap()
        );
    } else {
        let first = pending.stage_bait_rack_selected_outcome_v1().await.unwrap();
        let replay = pending.stage_bait_rack_selected_outcome_v1().await.unwrap();
        assert_eq!(first, replay);
        match (branch, first) {
            (
                FishingCatchBranch::Fish,
                PendingManualFishingBaitRackSelectedOutcomeV1::LandedFish {
                    progression,
                    durability,
                    ..
                },
            ) => {
                assert_eq!(progression.account_xp.granted_xp, 2);
                assert!(matches!(
                    durability,
                    AppliedFishingRodDurabilityState::Ordinary { .. }
                ));
            }
            (
                FishingCatchBranch::Fish,
                PendingManualFishingBaitRackSelectedOutcomeV1::LineBroken { durability },
            ) => {
                let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = durability else {
                    panic!("line break must resolve on ordinary Rod");
                };
                assert_eq!(preview.resulting_durability, 0);
            }
            (
                FishingCatchBranch::Fish,
                PendingManualFishingBaitRackSelectedOutcomeV1::OverCapCatchPending { line_stage },
            ) => assert!(matches!(
                line_stage,
                ManualFishingUnmodifiedLineStageV1::LineSurvivedOverCapCatchPending { .. }
            )),
            (
                FishingCatchBranch::Junk | FishingCatchBranch::Treasure,
                PendingManualFishingBaitRackSelectedOutcomeV1::NonFish { staged, durability },
            ) => {
                assert_eq!(staged.progression.account_xp.granted_xp, 2);
                assert!(matches!(
                    durability,
                    AppliedFishingRodDurabilityState::Ordinary { .. }
                ));
            }
            (selected, staged) => {
                panic!("selected branch and staged result disagree: {selected:?} {staged:?}")
            }
        }
    }

    pending.rollback().await.unwrap();
    let current: i64 =
        sqlx::query_scalar("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(current, 200);
    let events: i64 =
        sqlx::query_scalar("SELECT count(*) FROM asset_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let xp_events: i64 =
        sqlx::query_scalar("SELECT count(*) FROM progression_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!((events, xp_events), (0, 0));
    assert_eq!(operation_state(&store, operation_id).await, "PENDING");
}

#[tokio::test]
async fn bait_rack_only_owner_rejects_gold_and_additional_enchant() {
    let Some(store) = test_store().await else {
        return;
    };
    for (suffix, tier, second_enchant) in [
        ("bait-rack-gold", "GOLD", None),
        ("bait-rack-plus", "WOOD", Some(("STRENGTHEN", 1_i16))),
    ] {
        let nonce = Uuid::now_v7();
        let player_id = seed_player(&store, positive_snowflake(nonce)).await;
        set_account_level(&store, player_id, 10).await;
        let rod_id = seed_ordinary_rod(
            &store, player_id, nonce, suffix, tier, 200, 600, false, 4, 3,
        )
        .await;
        seed_enchant(&store, rod_id, "BAIT_RACK", 3).await;
        if let Some((key, level)) = second_enchant {
            seed_enchant(&store, rod_id, key, level).await;
        }
        let operation_id = seed_operation(&store, player_id, nonce, suffix).await;
        let tx = store.pool().begin().await.unwrap();
        let pending = begin_manual_fishing_cast_v1(tx, operation_id, player_id, FishingArea::River)
            .await
            .unwrap();
        assert!(matches!(
            pending.preview_bait_rack_base_branch_v1(),
            Err(ManualFishingCastOwnerError::BaitRackRequiresExactLoadout)
        ));
        pending.rollback().await.unwrap();
    }
}

#[tokio::test]
async fn sharp_hook_only_selected_outcome_preserves_exact_pending_catch_chance() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "sharp-hook-only",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    seed_enchant(&store, rod_id, "SHARP_HOOK", 10).await;
    let operation_id = seed_operation(&store, player_id, nonce, "sharp-hook-only").await;

    let tx = store.pool().begin().await.unwrap();
    let mut pending = begin_manual_fishing_cast_v1(tx, operation_id, player_id, FishingArea::River)
        .await
        .unwrap();
    assert!(matches!(
        pending.preview_unmodified_base_branch_v1(),
        Err(ManualFishingCastOwnerError::ModifiedFishRequiresSharedComposer)
    ));

    let branch = pending.preview_sharp_hook_base_branch_v1().unwrap();
    assert_eq!(branch, pending.preview_sharp_hook_base_branch_v1().unwrap());
    let capability = pending.preview_sharp_hook_fish_capability_v1(0).unwrap();
    assert_eq!(
        capability,
        pending.preview_sharp_hook_fish_capability_v1(0).unwrap()
    );
    if let ManualFishingUnmodifiedCapabilityPreviewV1::OverCapPending {
        ratio,
        catch_chance,
        ..
    } = capability
    {
        assert_eq!(catch_chance.sharp_hook_level, Some(10));
        assert_eq!(
            catch_chance,
            preview_over_cap_catch_chance(ratio, Some(10)).unwrap()
        );
    }

    if branch == FishingCatchBranch::Treasure
        && pending.preview_non_fish_candidate_v1(branch).is_err()
    {
        let candidate = pending
            .preview_sharp_hook_selected_direct_book_v1()
            .unwrap();
        let first = pending
            .stage_sharp_hook_selected_outcome_v1()
            .await
            .unwrap();
        assert_eq!(
            first,
            PendingManualFishingSharpHookSelectedOutcomeV1::DirectBookPending { candidate }
        );
        assert_eq!(
            first,
            pending
                .stage_sharp_hook_selected_outcome_v1()
                .await
                .unwrap()
        );
    } else {
        let first = pending
            .stage_sharp_hook_selected_outcome_v1()
            .await
            .unwrap();
        let replay = pending
            .stage_sharp_hook_selected_outcome_v1()
            .await
            .unwrap();
        assert_eq!(first, replay);
        match (branch, first) {
            (
                FishingCatchBranch::Fish,
                PendingManualFishingSharpHookSelectedOutcomeV1::LandedFish {
                    progression,
                    durability,
                    ..
                },
            ) => {
                assert_eq!(progression.account_xp.granted_xp, 2);
                assert!(matches!(
                    durability,
                    AppliedFishingRodDurabilityState::Ordinary { .. }
                ));
            }
            (
                FishingCatchBranch::Fish,
                PendingManualFishingSharpHookSelectedOutcomeV1::LineBroken { durability },
            ) => {
                let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = durability else {
                    panic!("line break must resolve on ordinary Rod");
                };
                assert_eq!(preview.resulting_durability, 0);
            }
            (
                FishingCatchBranch::Fish,
                PendingManualFishingSharpHookSelectedOutcomeV1::OverCapCatchPending {
                    line_stage,
                    catch_chance,
                },
            ) => {
                assert!(matches!(
                    line_stage,
                    ManualFishingUnmodifiedLineStageV1::LineSurvivedOverCapCatchPending { .. }
                ));
                let ManualFishingUnmodifiedCapabilityPreviewV1::OverCapPending { ratio, .. } =
                    capability
                else {
                    panic!("pending SharpHook catch requires over-cap capability");
                };
                assert_eq!(
                    catch_chance,
                    preview_over_cap_catch_chance(ratio, Some(10)).unwrap()
                );
            }
            (
                FishingCatchBranch::Junk | FishingCatchBranch::Treasure,
                PendingManualFishingSharpHookSelectedOutcomeV1::NonFish { staged, durability },
            ) => {
                assert_eq!(staged.progression.account_xp.granted_xp, 2);
                assert!(matches!(
                    durability,
                    AppliedFishingRodDurabilityState::Ordinary { .. }
                ));
            }
            (selected, staged) => {
                panic!("SharpHook selected branch/result mismatch: {selected:?} {staged:?}")
            }
        }
    }

    pending.rollback().await.unwrap();
    let current: i64 =
        sqlx::query_scalar("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(current, 200);
    let assets: i64 =
        sqlx::query_scalar("SELECT count(*) FROM asset_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let xp: i64 =
        sqlx::query_scalar("SELECT count(*) FROM progression_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!((assets, xp), (0, 0));
    assert_eq!(operation_state(&store, operation_id).await, "PENDING");
}

#[tokio::test]
async fn sharp_hook_owner_accepts_i_through_x_but_rejects_gold_and_extra_modifiers() {
    let Some(store) = test_store().await else {
        return;
    };
    for (suffix, tier, level, extra, accepted) in [
        ("sharp-hook-i", "WOOD", 1_i16, false, true),
        ("sharp-hook-x", "WOOD", 10_i16, false, true),
        ("sharp-hook-gold", "GOLD", 10_i16, false, false),
        ("sharp-hook-extra", "WOOD", 10_i16, true, false),
    ] {
        let nonce = Uuid::now_v7();
        let player_id = seed_player(&store, positive_snowflake(nonce)).await;
        set_account_level(&store, player_id, 10).await;
        let rod_id = seed_ordinary_rod(
            &store, player_id, nonce, suffix, tier, 200, 600, false, 4, 3,
        )
        .await;
        seed_enchant(&store, rod_id, "SHARP_HOOK", level).await;
        if extra {
            seed_enchant(&store, rod_id, "MENDING", 1).await;
        }
        let operation_id = seed_operation(&store, player_id, nonce, suffix).await;
        let tx = store.pool().begin().await.unwrap();
        let pending = begin_manual_fishing_cast_v1(tx, operation_id, player_id, FishingArea::River)
            .await
            .unwrap();
        if accepted {
            assert!(pending.preview_sharp_hook_base_branch_v1().is_ok());
            assert!(pending.preview_sharp_hook_fish_capability_v1(0).is_ok());
        } else {
            assert!(matches!(
                pending.preview_sharp_hook_base_branch_v1(),
                Err(ManualFishingCastOwnerError::SharpHookRequiresExactLoadout)
            ));
        }
        pending.rollback().await.unwrap();
    }
}

#[tokio::test]
async fn strengthen_only_selected_outcome_uses_exact_level_and_rolls_back() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "strengthen-only",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    seed_enchant(&store, rod_id, "STRENGTHEN", 10).await;
    let operation_id = seed_operation(&store, player_id, nonce, "strengthen-only").await;

    let tx = store.pool().begin().await.unwrap();
    let mut pending = begin_manual_fishing_cast_v1(tx, operation_id, player_id, FishingArea::River)
        .await
        .unwrap();
    assert!(matches!(
        pending.preview_unmodified_base_branch_v1(),
        Err(ManualFishingCastOwnerError::ModifiedFishRequiresSharedComposer)
    ));

    let branch = pending.preview_strengthen_base_branch_v1().unwrap();
    assert_eq!(branch, pending.preview_strengthen_base_branch_v1().unwrap());
    let capability = pending.preview_strengthen_fish_capability_v1(0).unwrap();
    assert_eq!(
        capability,
        pending.preview_strengthen_fish_capability_v1(0).unwrap()
    );

    if branch == FishingCatchBranch::Treasure
        && pending.preview_non_fish_candidate_v1(branch).is_err()
    {
        let candidate = pending
            .preview_strengthen_selected_direct_book_v1()
            .unwrap();
        let first = pending
            .stage_strengthen_selected_outcome_v1()
            .await
            .unwrap();
        assert_eq!(
            first,
            PendingManualFishingStrengthenSelectedOutcomeV1::DirectBookPending { candidate }
        );
        assert_eq!(
            first,
            pending
                .stage_strengthen_selected_outcome_v1()
                .await
                .unwrap()
        );
    } else {
        let first = pending
            .stage_strengthen_selected_outcome_v1()
            .await
            .unwrap();
        let replay = pending
            .stage_strengthen_selected_outcome_v1()
            .await
            .unwrap();
        assert_eq!(first, replay);
        match (branch, first) {
            (
                FishingCatchBranch::Fish,
                PendingManualFishingStrengthenSelectedOutcomeV1::LandedFish {
                    progression,
                    durability,
                    ..
                },
            ) => {
                assert_eq!(progression.account_xp.granted_xp, 2);
                assert!(matches!(
                    durability,
                    AppliedFishingRodDurabilityState::Ordinary { .. }
                ));
            }
            (
                FishingCatchBranch::Fish,
                PendingManualFishingStrengthenSelectedOutcomeV1::LineBroken { durability },
            ) => {
                let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = durability else {
                    panic!("line break must resolve on ordinary Rod");
                };
                assert_eq!(preview.resulting_durability, 0);
            }
            (
                FishingCatchBranch::Fish,
                PendingManualFishingStrengthenSelectedOutcomeV1::OverCapCatchPending { line_stage },
            ) => {
                assert!(matches!(
                    line_stage,
                    ManualFishingUnmodifiedLineStageV1::LineSurvivedOverCapCatchPending { .. }
                ));
            }
            (
                FishingCatchBranch::Junk | FishingCatchBranch::Treasure,
                PendingManualFishingStrengthenSelectedOutcomeV1::NonFish { staged, durability },
            ) => {
                assert_eq!(staged.progression.account_xp.granted_xp, 2);
                assert!(matches!(
                    durability,
                    AppliedFishingRodDurabilityState::Ordinary { .. }
                ));
            }
            (selected, staged) => {
                panic!("Strengthen selected branch/result mismatch: {selected:?} {staged:?}")
            }
        }
    }

    pending.rollback().await.unwrap();
    let current: i64 =
        sqlx::query_scalar("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(current, 200);
    let assets: i64 =
        sqlx::query_scalar("SELECT count(*) FROM asset_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let xp: i64 =
        sqlx::query_scalar("SELECT count(*) FROM progression_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!((assets, xp), (0, 0));
    assert_eq!(operation_state(&store, operation_id).await, "PENDING");
}

#[tokio::test]
async fn strengthen_owner_accepts_i_through_x_but_rejects_gold_and_extra_modifiers() {
    let Some(store) = test_store().await else {
        return;
    };
    for (suffix, tier, level, extra, accepted) in [
        ("strengthen-i", "WOOD", 1_i16, false, true),
        ("strengthen-x", "WOOD", 10_i16, false, true),
        ("strengthen-gold", "GOLD", 10_i16, false, false),
        ("strengthen-extra", "WOOD", 10_i16, true, false),
    ] {
        let nonce = Uuid::now_v7();
        let player_id = seed_player(&store, positive_snowflake(nonce)).await;
        set_account_level(&store, player_id, 10).await;
        let rod_id = seed_ordinary_rod(
            &store, player_id, nonce, suffix, tier, 200, 600, false, 4, 3,
        )
        .await;
        seed_enchant(&store, rod_id, "STRENGTHEN", level).await;
        if extra {
            seed_enchant(&store, rod_id, "MENDING", 1).await;
        }
        let operation_id = seed_operation(&store, player_id, nonce, suffix).await;
        let tx = store.pool().begin().await.unwrap();
        let pending = begin_manual_fishing_cast_v1(tx, operation_id, player_id, FishingArea::River)
            .await
            .unwrap();
        if accepted {
            assert!(pending.preview_strengthen_base_branch_v1().is_ok());
            assert!(pending.preview_strengthen_fish_capability_v1(0).is_ok());
        } else {
            assert!(matches!(
                pending.preview_strengthen_base_branch_v1(),
                Err(ManualFishingCastOwnerError::StrengthenRequiresExactLoadout)
            ));
        }
        pending.rollback().await.unwrap();
    }
}

#[tokio::test]
async fn unbreaking_x_uses_independent_pinned_wear_rng_and_rolls_back() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "unbreaking-x",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    seed_enchant(&store, rod_id, "UNBREAKING", 10).await;
    let operation_id = seed_operation(&store, player_id, nonce, "unbreaking-x").await;

    let tx = store.pool().begin().await.unwrap();
    let mut pending = begin_manual_fishing_cast_v1(tx, operation_id, player_id, FishingArea::River)
        .await
        .unwrap();
    assert!(matches!(
        pending.preview_unmodified_base_branch_v1(),
        Err(ManualFishingCastOwnerError::ModifiedFishRequiresSharedComposer)
    ));

    let branch = pending.preview_unbreaking_x_base_branch_v1().unwrap();
    assert_eq!(
        branch,
        pending.preview_unbreaking_x_base_branch_v1().unwrap()
    );
    let wear = pending.preview_unbreaking_x_ordinary_wear_v1().unwrap();
    assert_eq!(wear.chance_ppm, 200_000);
    assert_eq!(wear.prevented, wear.draw_ppm < wear.chance_ppm);
    assert_eq!(
        wear,
        pending.preview_unbreaking_x_ordinary_wear_v1().unwrap()
    );

    let line_broken = branch == FishingCatchBranch::Fish
        && matches!(
            pending.preview_unbreaking_x_line_stage_v1(0).unwrap(),
            ManualFishingUnmodifiedLineStageV1::LineBroken { .. }
        );
    let first = pending
        .stage_unbreaking_x_rod_durability_v1()
        .await
        .unwrap();
    let replay = pending
        .stage_unbreaking_x_rod_durability_v1()
        .await
        .unwrap();
    assert_eq!(first, replay);
    let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = first else {
        panic!("Unbreaking X path requires an ordinary Rod");
    };
    assert_eq!(
        preview.resulting_durability,
        if line_broken {
            0
        } else if wear.prevented {
            200
        } else {
            199
        }
    );

    pending.rollback().await.unwrap();
    let current: i64 =
        sqlx::query_scalar("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(current, 200);
    let events: i64 =
        sqlx::query_scalar("SELECT count(*) FROM asset_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(events, 0);
    assert_eq!(operation_state(&store, operation_id).await, "PENDING");
}

#[tokio::test]
async fn unbreaking_x_selected_outcome_replays_and_rolls_back_all_children() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "unbreaking-x-selected",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    seed_enchant(&store, rod_id, "UNBREAKING", 10).await;
    let operation_id = seed_operation(&store, player_id, nonce, "unbreaking-x-selected").await;

    let tx = store.pool().begin().await.unwrap();
    let mut pending = begin_manual_fishing_cast_v1(tx, operation_id, player_id, FishingArea::River)
        .await
        .unwrap();
    let branch = pending.preview_unbreaking_x_base_branch_v1().unwrap();

    if branch == FishingCatchBranch::Treasure
        && pending.preview_non_fish_candidate_v1(branch).is_err()
    {
        let candidate = pending
            .preview_unbreaking_x_selected_direct_book_v1()
            .unwrap();
        let first = pending
            .stage_unbreaking_x_selected_outcome_v1()
            .await
            .unwrap();
        assert_eq!(
            first,
            PendingManualFishingUnbreakingXSelectedOutcomeV1::DirectBookPending { candidate }
        );
        assert_eq!(
            first,
            pending
                .stage_unbreaking_x_selected_outcome_v1()
                .await
                .unwrap()
        );
    } else {
        let first = pending
            .stage_unbreaking_x_selected_outcome_v1()
            .await
            .unwrap();
        let replay = pending
            .stage_unbreaking_x_selected_outcome_v1()
            .await
            .unwrap();
        assert_eq!(first, replay);
        match (branch, first) {
            (
                FishingCatchBranch::Fish,
                PendingManualFishingUnbreakingXSelectedOutcomeV1::LandedFish {
                    progression,
                    durability,
                    ..
                },
            ) => {
                assert_eq!(progression.account_xp.granted_xp, 2);
                assert!(matches!(
                    durability,
                    AppliedFishingRodDurabilityState::Ordinary { .. }
                ));
            }
            (
                FishingCatchBranch::Fish,
                PendingManualFishingUnbreakingXSelectedOutcomeV1::LineBroken { durability },
            ) => {
                let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = durability else {
                    panic!("line break must resolve on ordinary Rod");
                };
                assert_eq!(preview.resulting_durability, 0);
            }
            (
                FishingCatchBranch::Fish,
                PendingManualFishingUnbreakingXSelectedOutcomeV1::OverCapCatchPending {
                    line_stage,
                },
            ) => {
                assert!(matches!(
                    line_stage,
                    ManualFishingUnmodifiedLineStageV1::LineSurvivedOverCapCatchPending { .. }
                ));
            }
            (
                FishingCatchBranch::Junk | FishingCatchBranch::Treasure,
                PendingManualFishingUnbreakingXSelectedOutcomeV1::NonFish { staged, durability },
            ) => {
                assert_eq!(staged.progression.account_xp.granted_xp, 2);
                assert!(matches!(
                    durability,
                    AppliedFishingRodDurabilityState::Ordinary { .. }
                ));
            }
            (selected, staged) => {
                panic!("selected branch/result mismatch: {selected:?} {staged:?}")
            }
        }
    }

    pending.rollback().await.unwrap();
    let current: i64 =
        sqlx::query_scalar("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(current, 200);
    let assets: i64 =
        sqlx::query_scalar("SELECT count(*) FROM asset_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let xp: i64 =
        sqlx::query_scalar("SELECT count(*) FROM progression_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!((assets, xp), (0, 0));
    assert_eq!(operation_state(&store, operation_id).await, "PENDING");
}

#[tokio::test]
async fn unbreaking_x_owner_rejects_lower_level_and_other_branch_modifiers() {
    let Some(store) = test_store().await else {
        return;
    };

    for (suffix, tier, level) in [
        ("unbreaking-nine", "WOOD", 9_i16),
        ("gold-unbreaking-x", "GOLD", 10_i16),
    ] {
        let nonce = Uuid::now_v7();
        let player_id = seed_player(&store, positive_snowflake(nonce)).await;
        set_account_level(&store, player_id, 10).await;
        let rod_id = seed_ordinary_rod(
            &store, player_id, nonce, suffix, tier, 200, 600, false, 4, 3,
        )
        .await;
        seed_enchant(&store, rod_id, "UNBREAKING", level).await;
        let operation_id = seed_operation(&store, player_id, nonce, suffix).await;
        let tx = store.pool().begin().await.unwrap();
        let pending = begin_manual_fishing_cast_v1(tx, operation_id, player_id, FishingArea::River)
            .await
            .unwrap();
        assert!(matches!(
            pending.preview_unbreaking_x_base_branch_v1(),
            Err(ManualFishingCastOwnerError::UnbreakingXRequiresExactLoadout)
        ));
        pending.rollback().await.unwrap();
    }
}

#[tokio::test]
async fn starter_pool_preflight_accepts_starter_basic_and_uses_native_bait_capacity() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let item_id = seed_starter_basic_rod(&store, player_id, nonce, "starter").await;
    let operation_id = seed_operation(&store, player_id, nonce, "starter-cast").await;

    let mut tx = store.pool().begin().await.unwrap();
    let preflight = lock_manual_fishing_cast_preflight(
        &mut tx,
        operation_id,
        player_id,
        FishingArea::StarterPool,
    )
    .await
    .unwrap();

    assert_eq!(preflight.operation_id, operation_id);
    assert_eq!(preflight.player_id, player_id);
    assert_eq!(
        preflight.area_access.origin,
        FishingAreaAccessOrigin::StarterPoolDefault
    );
    assert_eq!(preflight.rod.item_instance_id, item_id);
    assert_eq!(preflight.rod.kind, EquippedFishingRodKind::StarterBasic);
    assert_eq!(
        preflight.bait_capacity.active_bait_category_slots,
        NATIVE_ACTIVE_BAIT_CATEGORY_SLOTS
    );
    assert_eq!(preflight.bait_capacity.bait_rack_level, None);
    tx.rollback().await.unwrap();

    assert_eq!(unlock_count(&store, player_id).await, 0);
    assert_eq!(operation_state(&store, operation_id).await, "PENDING");
}

#[tokio::test]
async fn first_river_cast_preflight_composes_permanent_unlock_and_locked_bait_rack_level() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    set_account_level(&store, player_id, 10).await;
    let item_id = seed_ordinary_rod(
        &store, player_id, nonce, "wood", "WOOD", 200, 600, false, 4, 3,
    )
    .await;
    seed_enchant(&store, item_id, "BAIT_RACK", 2).await;
    seed_enchant(&store, item_id, "MENDING", 1).await;
    let operation_id = seed_operation(&store, player_id, nonce, "river-cast").await;

    let mut tx = store.pool().begin().await.unwrap();
    let preflight =
        lock_manual_fishing_cast_preflight(&mut tx, operation_id, player_id, FishingArea::River)
            .await
            .unwrap();

    assert_eq!(
        preflight.area_access.origin,
        FishingAreaAccessOrigin::NewlyUnlocked
    );
    assert_eq!(
        preflight.area_access.granted_by_operation_id,
        Some(operation_id)
    );
    assert!(preflight.area_access.first_unlock_preview.is_some());
    assert_eq!(
        preflight.rod.kind,
        EquippedFishingRodKind::Ordinary {
            tier: EquipmentTier::Wood
        }
    );
    assert_eq!(preflight.bait_capacity.bait_rack_level, Some(2));
    assert_eq!(preflight.bait_capacity.active_bait_category_slots, 5);
    assert!(
        preflight
            .rod
            .embedded_enchants
            .iter()
            .any(|state| state.enchant == CanonicalEnchant::BaitRack && state.level == 2)
    );

    tx.rollback().await.unwrap();
    assert_eq!(unlock_count(&store, player_id).await, 0);
    assert_eq!(operation_state(&store, operation_id).await, "PENDING");
}

#[tokio::test]
async fn persisted_area_preflight_holds_balance_progression_and_cast_item_locks() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let item_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "persisted",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;

    let grant_operation = seed_operation(&store, player_id, nonce, "persisted-grant").await;
    sqlx::query(
        "INSERT INTO player_fishing_area_unlocks (player_id, area, granted_by_operation_id) VALUES ($1, 'RIVER', $2)",
    )
    .bind(player_id)
    .bind(grant_operation)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query("UPDATE operations SET state = 'COMMITTED' WHERE id = $1")
        .bind(grant_operation)
        .execute(store.pool())
        .await
        .unwrap();

    let operation_id = seed_operation(&store, player_id, nonce, "persisted-cast").await;
    let mut owner = store.pool().begin().await.unwrap();
    let preflight =
        lock_manual_fishing_cast_preflight(&mut owner, operation_id, player_id, FishingArea::River)
            .await
            .unwrap();
    assert_eq!(
        preflight.area_access.origin,
        FishingAreaAccessOrigin::Persisted
    );
    assert_eq!(preflight.rod.item_instance_id, item_id);

    assert_row_locked(
        &store,
        "SELECT player_id FROM player_balances WHERE player_id = $1 FOR UPDATE NOWAIT",
        player_id,
    )
    .await;
    assert_row_locked(
        &store,
        "SELECT player_id FROM player_progression WHERE player_id = $1 FOR UPDATE NOWAIT",
        player_id,
    )
    .await;
    assert_row_locked(
        &store,
        "SELECT id FROM item_instances WHERE id = $1 FOR UPDATE NOWAIT",
        item_id,
    )
    .await;

    owner.rollback().await.unwrap();
}

#[tokio::test]
async fn per_cast_starter_pool_only_and_broken_rod_guards_fail_closed() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();

    let starter_player = seed_player(&store, positive_snowflake(nonce)).await;
    seed_starter_basic_rod(&store, starter_player, nonce, "starter-outside-pool").await;
    let grant_operation = seed_operation(&store, starter_player, nonce, "river-grant").await;
    sqlx::query(
        "INSERT INTO player_fishing_area_unlocks (player_id, area, granted_by_operation_id) VALUES ($1, 'RIVER', $2)",
    )
    .bind(starter_player)
    .bind(grant_operation)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query("UPDATE operations SET state = 'COMMITTED' WHERE id = $1")
        .bind(grant_operation)
        .execute(store.pool())
        .await
        .unwrap();
    let starter_operation = seed_operation(&store, starter_player, nonce, "starter-river").await;
    let mut starter_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_manual_fishing_cast_preflight(
            &mut starter_tx,
            starter_operation,
            starter_player,
            FishingArea::River,
        )
        .await,
        Err(ManualFishingCastPreflightError::StarterBasicRodOutsidePool)
    ));
    starter_tx.rollback().await.unwrap();

    let broken_player = seed_player(&store, next_snowflake(nonce, 1)).await;
    seed_ordinary_rod(
        &store,
        broken_player,
        nonce,
        "broken",
        "WOOD",
        0,
        600,
        true,
        4,
        3,
    )
    .await;
    let broken_operation = seed_operation(&store, broken_player, nonce, "broken-pool").await;
    let mut broken_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_manual_fishing_cast_preflight(
            &mut broken_tx,
            broken_operation,
            broken_player,
            FishingArea::StarterPool,
        )
        .await,
        Err(ManualFishingCastPreflightError::BrokenFishingRod)
    ));
    broken_tx.rollback().await.unwrap();

    assert_eq!(operation_state(&store, starter_operation).await, "PENDING");
    assert_eq!(operation_state(&store, broken_operation).await, "PENDING");
}

async fn test_store() -> Option<PgStore> {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL is not set; skipping PostgreSQL integration test");
        return None;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate().await.unwrap();
    Some(store)
}

async fn seed_player(store: &PgStore, discord_user_id: i64) -> Uuid {
    let player_id = Uuid::now_v7();
    sqlx::query("INSERT INTO players (id, discord_user_id) VALUES ($1, $2)")
        .bind(player_id)
        .bind(discord_user_id)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO player_balances (player_id) VALUES ($1)")
        .bind(player_id)
        .execute(store.pool())
        .await
        .unwrap();
    player_id
}

async fn seed_operation(store: &PgStore, player_id: Uuid, nonce: Uuid, suffix: &str) -> Uuid {
    let operation_id = Uuid::now_v7();
    let discord_user_id: i64 =
        sqlx::query_scalar("SELECT discord_user_id FROM players WHERE id = $1")
            .bind(player_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'MANUAL_FISHING_CAST_PREFLIGHT_TEST', 'PENDING', 1, $5, $6)",
    )
    .bind(operation_id)
    .bind(format!("test:manual-fishing-cast-preflight:{nonce}:{suffix}:{operation_id}"))
    .bind(discord_user_id)
    .bind(player_id)
    .bind([79_u8; 32].as_slice())
    .bind([83_u8; 32].as_slice())
    .execute(store.pool())
    .await
    .unwrap();
    operation_id
}

#[allow(clippy::too_many_arguments)]
async fn seed_ordinary_rod(
    store: &PgStore,
    player_id: Uuid,
    nonce: Uuid,
    suffix: &str,
    tier: &str,
    current_durability: i64,
    max_durability: i64,
    is_broken: bool,
    normal_capacity: i16,
    special_capacity: i16,
) -> Uuid {
    let definition_key = format!("test.manual-fishing-cast-preflight.{suffix}.{nonce}");
    let data = serde_json::json!({"tier": tier});
    sqlx::query(
        "INSERT INTO item_definitions (key, category, stackable, active, definition_version, rarity, stack_limit, data) VALUES ($1, 'FISHING_ROD', FALSE, TRUE, 1, 'COMMON', NULL, $2)",
    )
    .bind(&definition_key)
    .bind(&data)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO item_definition_versions (key, version, category, stackable, rarity, stack_limit, is_ordinary_equipment, data) VALUES ($1, 1, 'FISHING_ROD', FALSE, 'COMMON', NULL, TRUE, $2)",
    )
    .bind(&definition_key)
    .bind(data)
    .execute(store.pool())
    .await
    .unwrap();

    let creation_operation =
        seed_operation(store, player_id, nonce, &format!("create-{suffix}")).await;
    let item_id = Uuid::now_v7();
    let mut tx = store.pool().begin().await.unwrap();
    sqlx::query(
        "INSERT INTO item_instances (id, definition_key, owner_player_id, created_by_operation_id, location, definition_version, current_durability, max_durability, is_broken) VALUES ($1, $2, $3, $4, 'EQUIPPED', 1, $5, $6, $7)",
    )
    .bind(item_id)
    .bind(&definition_key)
    .bind(player_id)
    .bind(creation_operation)
    .bind(current_durability)
    .bind(max_durability)
    .bind(is_broken)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO equipment_slots (player_id, slot, item_instance_id) VALUES ($1, 'FISHING_ROD', $2)",
    )
    .bind(player_id)
    .bind(item_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO item_instance_equipment_structural_state (item_instance_id, creation_roll_numerator, creation_roll_denominator, upgrade_level, normal_enchant_slot_capacity, special_enchant_slot_capacity) VALUES ($1, 1, 2, 0, $2, $3)",
    )
    .bind(item_id)
    .bind(normal_capacity)
    .bind(special_capacity)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    item_id
}

async fn seed_starter_basic_rod(
    store: &PgStore,
    player_id: Uuid,
    nonce: Uuid,
    suffix: &str,
) -> Uuid {
    let creation_operation =
        seed_operation(store, player_id, nonce, &format!("create-{suffix}")).await;
    let item_id = Uuid::now_v7();
    let mut tx = store.pool().begin().await.unwrap();
    sqlx::query(
        "INSERT INTO item_instances (id, definition_key, owner_player_id, created_by_operation_id, location, definition_version, is_starter, is_account_bound, is_tradeable, is_sellable, is_discardable, is_enchantable, is_upgradeable, is_unbreakable, is_repairable) VALUES ($1, 'equipment.rod.basic.starter', $2, $3, 'EQUIPPED', 1, TRUE, TRUE, FALSE, FALSE, FALSE, FALSE, FALSE, TRUE, FALSE)",
    )
    .bind(item_id)
    .bind(player_id)
    .bind(creation_operation)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO equipment_slots (player_id, slot, item_instance_id) VALUES ($1, 'FISHING_ROD', $2)",
    )
    .bind(player_id)
    .bind(item_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    item_id
}

async fn seed_enchant(store: &PgStore, item_id: Uuid, key: &str, level: i16) {
    sqlx::query(
        "INSERT INTO item_instance_embedded_enchants (item_instance_id, enchant_key, level) VALUES ($1, $2, $3)",
    )
    .bind(item_id)
    .bind(key)
    .bind(level)
    .execute(store.pool())
    .await
    .unwrap();
}

async fn set_account_level(store: &PgStore, player_id: Uuid, level: u16) {
    let account_xp = account_total_xp_for_level(level).unwrap();
    sqlx::query(
        "UPDATE player_progression SET account_xp = $1, updated_at = now() WHERE player_id = $2",
    )
    .bind(account_xp)
    .bind(player_id)
    .execute(store.pool())
    .await
    .unwrap();
}

async fn unlock_count(store: &PgStore, player_id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM player_fishing_area_unlocks WHERE player_id = $1")
        .bind(player_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
}

async fn operation_state(store: &PgStore, operation_id: Uuid) -> String {
    sqlx::query_scalar("SELECT state FROM operations WHERE id = $1")
        .bind(operation_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
}

async fn assert_row_locked(store: &PgStore, query: &'static str, id: Uuid) {
    let mut contender = store.pool().begin().await.unwrap();
    let lock = sqlx::query_scalar::<_, Uuid>(query)
        .bind(id)
        .fetch_one(&mut *contender)
        .await;
    assert_lock_not_available(lock.unwrap_err());
    contender.rollback().await.unwrap();
}

fn assert_lock_not_available(error: sqlx::Error) {
    let sqlx::Error::Database(database) = error else {
        panic!("expected PostgreSQL lock error, got {error:?}");
    };
    assert_eq!(database.code().as_deref(), Some("55P03"));
}

fn positive_snowflake(nonce: Uuid) -> i64 {
    next_snowflake(nonce, 0)
}

fn next_snowflake(nonce: Uuid, offset: u64) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    let value = (raw % 7_999_999_999_999_999_000_u64)
        .saturating_add(1)
        .saturating_add(offset);
    i64::try_from(value).unwrap()
}
