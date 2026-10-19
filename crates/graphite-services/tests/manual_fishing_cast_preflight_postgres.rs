use graphite_core::{DomainRng, OperationId, RootSeed};
use graphite_progression::account_total_xp_for_level;
use graphite_services::{
    AppliedFishingRodDurabilityState, CanonicalEnchant, EquipmentTier, EquippedFishingRodKind,
    FishingArea, FishingAreaAccessOrigin, FishingCatchBranch, FishingDirectBookRollError,
    FishingResolvedNonFishOutcomeV1, MANUAL_FISHING_OPERATION_KIND,
    MANUAL_FISHING_OPERATION_POLICY_VERSION, MAX_FISH_PER_CAST, ManualFishingCastOwnerError,
    ManualFishingCastPreflightError, ManualFishingCastRequestResolutionV1,
    ManualFishingModifierTerminalOutcomeV1, ManualFishingPpmDrawV1, ManualFishingRngDrawError,
    ManualFishingSharpHookTerminalOutcomeV1, ManualFishingStarterBaseTerminalOutcomeV1,
    ManualFishingStrengthenTerminalOutcomeV1, ManualFishingSupportedTerminalReceiptV1,
    ManualFishingSupportedTerminalRouteV1, ManualFishingTreasureXTerminalOutcomeV1,
    ManualFishingUnbreakingXTerminalOutcomeV1, ManualFishingUnmodifiedCapabilityPreviewV1,
    ManualFishingUnmodifiedFishCandidateError, ManualFishingUnmodifiedLineStageV1,
    ManualFishingUnmodifiedOrdinaryTerminalOutcomeV1, ManualFishingWeightedDrawV1,
    MultiTreasureCount, MulticatchCount, NATIVE_ACTIVE_BAIT_CATEGORY_SLOTS,
    PendingManualFishingBaitRackSelectedOutcomeV1, PendingManualFishingLureXSelectedOutcomeV1,
    PendingManualFishingMultiTreasureSelectedOutcomeV1,
    PendingManualFishingMulticatchSelectedOutcomeV1,
    PendingManualFishingSchoolBaitSelectedOutcomeV1,
    PendingManualFishingSharpHookSelectedOutcomeV1, PendingManualFishingStarterBaseOutcomeV1,
    PendingManualFishingStrengthenSelectedOutcomeV1,
    PendingManualFishingSturdyBaitSelectedOutcomeV1,
    PendingManualFishingTreasureXSelectedOutcomeV1,
    PendingManualFishingUnbreakingXSelectedOutcomeV1,
    PendingManualFishingUnmodifiedSelectedOutcomeV1, begin_manual_fishing_cast_request_v1,
    begin_manual_fishing_cast_v1, commit_supported_manual_fishing_cast_request_v1,
    direct_fishing_book_pool_membership, direct_fishing_finished_book_level_weight,
    lock_manual_fishing_cast_preflight, manual_fishing_base_fish_cast_aexp,
    multi_treasure_count_policy, multicatch_count_policy, preview_over_cap_catch_chance,
    resolve_manual_fishing_cast_request_v1, resolve_manual_fishing_direct_book_candidate_v1,
    resolve_manual_fishing_treasure_x_branch_v1,
};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn supported_terminal_router_commits_and_replays_unmodified_ordinary_without_second_effect() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let actor = positive_snowflake(nonce);
    let player_id = seed_player(&store, actor).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "router-ordinary-book",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    let key = format!("test:manual-fishing-request:{nonce}:router-ordinary-book");

    let operation = {
        let mut tx = store.pool().begin().await.unwrap();
        let operation = graphite_services::lock_or_create_manual_fishing_operation_v1(
            &mut tx,
            u64::try_from(actor).unwrap(),
            &key,
            FishingArea::River,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        operation
    };
    let root = find_selected_direct_book_root(operation.operation_id);
    sqlx::query("UPDATE operations SET rng_root = $1 WHERE id = $2")
        .bind(root.as_slice())
        .bind(operation.operation_id)
        .execute(store.pool())
        .await
        .unwrap();

    let first = commit_supported_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap();
    assert_eq!(
        first.route(),
        ManualFishingSupportedTerminalRouteV1::UnmodifiedOrdinary
    );
    let ManualFishingSupportedTerminalReceiptV1::UnmodifiedOrdinary(receipt) = &first else {
        panic!("router must select the unmodified ordinary terminal owner");
    };
    let ManualFishingUnmodifiedOrdinaryTerminalOutcomeV1::Book {
        deliveries,
        progression,
        durability,
    } = &receipt.outcome
    else {
        panic!("forced direct Book seed must remain a Book through the router");
    };
    assert_eq!(deliveries.len(), 1);
    assert_eq!(progression.base_activity_xp, 5);
    let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = &durability.applied else {
        panic!("router must preserve ordinary Rod durability evidence");
    };
    assert_eq!(preview.resulting_durability, 199);
    assert_eq!(finished_book_count(&store, operation.operation_id).await, 1);

    let replay = commit_supported_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap();
    assert_eq!(replay, first);
    assert_eq!(finished_book_count(&store, operation.operation_id).await, 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap(),
        199
    );
    assert_terminal_once(&store, operation.operation_id).await;
}

#[tokio::test]
async fn supported_terminal_router_derives_strengthen_level_from_locked_rod() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let actor = positive_snowflake(nonce);
    let player_id = seed_player(&store, actor).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "router-strengthen",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    seed_enchant(&store, rod_id, "STRENGTHEN", 4).await;
    let key = format!("test:manual-fishing-request:{nonce}:router-strengthen");

    let pending = begin_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap();
    assert_eq!(
        pending.supported_terminal_route_v1(),
        Some(ManualFishingSupportedTerminalRouteV1::Strengthen { level: 4 })
    );
    pending.rollback().await.unwrap();
}

#[tokio::test]
async fn supported_terminal_router_keeps_lure_x_fail_closed_and_rolls_back_preflight() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let actor = positive_snowflake(nonce);
    let player_id = seed_player(&store, actor).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "router-lure-x",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    seed_enchant(&store, rod_id, "LURE", 10).await;
    let key = format!("test:manual-fishing-request:{nonce}:router-lure-x");

    let operation = {
        let mut tx = store.pool().begin().await.unwrap();
        let operation = graphite_services::lock_or_create_manual_fishing_operation_v1(
            &mut tx,
            u64::try_from(actor).unwrap(),
            &key,
            FishingArea::River,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        operation
    };

    let error = commit_supported_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        ManualFishingCastOwnerError::SupportedTerminalRouteUnavailable
    ));
    assert_eq!(
        operation_state(&store, operation.operation_id).await,
        "PENDING"
    );
    assert_eq!(unlock_count(&store, player_id).await, 0);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap(),
        200
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM asset_events WHERE operation_id = $1 AND mutation_key = 'fishing:cast:terminal'",
        )
        .bind(operation.operation_id)
        .fetch_one(store.pool())
        .await
        .unwrap(),
        0
    );
}

#[tokio::test]
async fn request_owned_starter_basic_direct_book_commits_and_replays_exact_item() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let actor = positive_snowflake(nonce);
    let player_id = seed_player(&store, actor).await;
    set_account_level(&store, player_id, 10).await;
    seed_starter_basic_rod(&store, player_id, nonce, "starter-terminal-book").await;
    let key = format!("test:manual-fishing-request:{nonce}:starter-book");

    let operation = {
        let mut tx = store.pool().begin().await.unwrap();
        let operation = graphite_services::lock_or_create_manual_fishing_operation_v1(
            &mut tx,
            u64::try_from(actor).unwrap(),
            &key,
            FishingArea::StarterPool,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        operation
    };
    let root = find_selected_direct_book_root(operation.operation_id);
    sqlx::query("UPDATE operations SET rng_root = $1 WHERE id = $2")
        .bind(root.as_slice())
        .bind(operation.operation_id)
        .execute(store.pool())
        .await
        .unwrap();

    let pending = begin_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::StarterPool,
    )
    .await
    .unwrap();
    let committed = pending.commit_unmodified_starter_base_v1().await.unwrap();
    let ManualFishingStarterBaseTerminalOutcomeV1::Book {
        deliveries,
        progression,
    } = &committed.outcome
    else {
        panic!("forced Starter Basic direct Book must commit as Book");
    };
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].candidate.treasure_slot, 0);
    assert_eq!(progression.base_activity_xp, 5);
    assert_eq!(finished_book_count(&store, operation.operation_id).await, 1);

    let replay = resolve_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::StarterPool,
    )
    .await
    .unwrap();
    let ManualFishingCastRequestResolutionV1::CommittedStarterBase(replayed) = replay else {
        panic!("committed Starter Basic Book must replay typed receipt");
    };
    assert_eq!(*replayed, committed);
    assert_eq!(finished_book_count(&store, operation.operation_id).await, 1);
    assert_terminal_once(&store, operation.operation_id).await;
}

#[tokio::test]
async fn request_owned_unmodified_ordinary_direct_book_commits_and_replays_once() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let actor = positive_snowflake(nonce);
    let player_id = seed_player(&store, actor).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "ordinary-terminal-book",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    let key = format!("test:manual-fishing-request:{nonce}:ordinary-book");

    let operation = {
        let mut tx = store.pool().begin().await.unwrap();
        let operation = graphite_services::lock_or_create_manual_fishing_operation_v1(
            &mut tx,
            u64::try_from(actor).unwrap(),
            &key,
            FishingArea::River,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        operation
    };
    let root = find_selected_direct_book_root(operation.operation_id);
    sqlx::query("UPDATE operations SET rng_root = $1 WHERE id = $2")
        .bind(root.as_slice())
        .bind(operation.operation_id)
        .execute(store.pool())
        .await
        .unwrap();

    let pending = begin_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap();
    let committed = pending.commit_unmodified_ordinary_v1().await.unwrap();
    let ManualFishingUnmodifiedOrdinaryTerminalOutcomeV1::Book {
        deliveries,
        progression,
        durability,
    } = &committed.outcome
    else {
        panic!("forced ordinary direct Book must commit as Book");
    };
    assert_eq!(deliveries.len(), 1);
    assert_eq!(progression.base_activity_xp, 5);
    let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = &durability.applied else {
        panic!("ordinary Book terminal must persist Rod wear");
    };
    assert_eq!(preview.resulting_durability, 199);
    assert_eq!(finished_book_count(&store, operation.operation_id).await, 1);

    let replay = resolve_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap();
    let ManualFishingCastRequestResolutionV1::CommittedUnmodifiedOrdinary(replayed) = replay else {
        panic!("committed ordinary Book must replay typed receipt");
    };
    assert_eq!(*replayed, committed);
    assert_eq!(finished_book_count(&store, operation.operation_id).await, 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap(),
        199
    );
    assert_terminal_once(&store, operation.operation_id).await;
}

#[tokio::test]
async fn request_owned_unbreaking_x_direct_book_commits_exact_wear_and_replays() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let actor = positive_snowflake(nonce);
    let player_id = seed_player(&store, actor).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "unbreaking-x-terminal-book",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    seed_enchant(&store, rod_id, "UNBREAKING", 10).await;
    let key = format!("test:manual-fishing-request:{nonce}:unbreaking-x-book");

    let operation = {
        let mut tx = store.pool().begin().await.unwrap();
        let operation = graphite_services::lock_or_create_manual_fishing_operation_v1(
            &mut tx,
            u64::try_from(actor).unwrap(),
            &key,
            FishingArea::River,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        operation
    };
    let root = find_selected_direct_book_root(operation.operation_id);
    sqlx::query("UPDATE operations SET rng_root = $1 WHERE id = $2")
        .bind(root.as_slice())
        .bind(operation.operation_id)
        .execute(store.pool())
        .await
        .unwrap();

    let pending = begin_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap();
    let committed = pending.commit_unbreaking_x_v1().await.unwrap();
    let ManualFishingUnbreakingXTerminalOutcomeV1::Book {
        deliveries,
        progression,
        wear,
        durability,
    } = &committed.outcome
    else {
        panic!("forced Unbreaking-X direct Book must commit as Book");
    };
    assert_eq!(deliveries.len(), 1);
    assert_eq!(progression.base_activity_xp, 5);
    assert_eq!(wear.chance_ppm, 200_000);
    assert_eq!(wear.prevented, wear.draw_ppm < wear.chance_ppm);
    let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = &durability.applied else {
        panic!("Unbreaking-X Book terminal must persist ordinary Rod consequence");
    };
    assert_eq!(
        preview.resulting_durability,
        if wear.prevented { 200 } else { 199 }
    );
    assert_eq!(finished_book_count(&store, operation.operation_id).await, 1);

    let replay = resolve_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap();
    let ManualFishingCastRequestResolutionV1::CommittedUnbreakingX(replayed) = replay else {
        panic!("committed Unbreaking-X Book must replay typed receipt");
    };
    assert_eq!(*replayed, committed);
    assert_eq!(finished_book_count(&store, operation.operation_id).await, 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap(),
        if wear.prevented { 200 } else { 199 }
    );
    assert_terminal_once(&store, operation.operation_id).await;
}

#[tokio::test]
async fn request_owned_unbreaking_x_junk_commits_and_replays_exact_wear() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let actor = positive_snowflake(nonce);
    let player_id = seed_player(&store, actor).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "unbreaking-x-terminal-junk",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    seed_enchant(&store, rod_id, "UNBREAKING", 10).await;
    let key = format!("test:manual-fishing-request:{nonce}:unbreaking-x-junk");

    let operation = {
        let mut tx = store.pool().begin().await.unwrap();
        let operation = graphite_services::lock_or_create_manual_fishing_operation_v1(
            &mut tx,
            u64::try_from(actor).unwrap(),
            &key,
            FishingArea::River,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        operation
    };
    let root = find_baseline_branch_root(operation.operation_id, 1);
    sqlx::query("UPDATE operations SET rng_root = $1 WHERE id = $2")
        .bind(root.as_slice())
        .bind(operation.operation_id)
        .execute(store.pool())
        .await
        .unwrap();

    let tx = store.pool().begin().await.unwrap();
    let pending = begin_manual_fishing_cast_request_v1(
        tx,
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap();
    let expected_wear = pending.preview_unbreaking_x_ordinary_wear_v1().unwrap();
    assert_eq!(expected_wear.chance_ppm, 200_000);

    let committed = pending.commit_unbreaking_x_v1().await.unwrap();
    let ManualFishingUnbreakingXTerminalOutcomeV1::NonFish {
        progression,
        wear,
        durability,
        ..
    } = &committed.outcome
    else {
        panic!("forced baseline Junk branch must commit non-Fish Unbreaking X");
    };
    assert_eq!(*wear, expected_wear);
    assert_eq!(progression.account_xp.granted_xp, 2);
    assert_eq!(durability.operation_id, operation.operation_id);
    let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = &durability.applied else {
        panic!("Unbreaking X terminal receipt must carry ordinary Rod durability");
    };
    assert_eq!(
        preview.resulting_durability,
        if expected_wear.prevented { 200 } else { 199 }
    );

    let before: i64 =
        sqlx::query_scalar("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let replay = resolve_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap();
    let ManualFishingCastRequestResolutionV1::CommittedUnbreakingX(replayed) = replay else {
        panic!("committed Unbreaking X request must replay its typed receipt");
    };
    assert_eq!(*replayed, committed);
    let after: i64 =
        sqlx::query_scalar("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(after, before);
    assert_eq!(
        operation_state(&store, operation.operation_id).await,
        "COMMITTED"
    );
}

#[tokio::test]
async fn request_owned_strengthen_i_junk_commits_and_replays_without_second_wear() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let actor = positive_snowflake(nonce);
    let player_id = seed_player(&store, actor).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "strengthen-i-terminal-junk",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    seed_enchant(&store, rod_id, "STRENGTHEN", 1).await;
    let key = format!("test:manual-fishing-request:{nonce}:strengthen-i-junk");

    let operation = {
        let mut tx = store.pool().begin().await.unwrap();
        let operation = graphite_services::lock_or_create_manual_fishing_operation_v1(
            &mut tx,
            u64::try_from(actor).unwrap(),
            &key,
            FishingArea::River,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        operation
    };
    let root = find_baseline_branch_root(operation.operation_id, 1);
    sqlx::query("UPDATE operations SET rng_root = $1 WHERE id = $2")
        .bind(root.as_slice())
        .bind(operation.operation_id)
        .execute(store.pool())
        .await
        .unwrap();

    let tx = store.pool().begin().await.unwrap();
    let pending = begin_manual_fishing_cast_request_v1(
        tx,
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap();
    let committed = pending.commit_strengthen_v1().await.unwrap();
    assert_eq!(committed.strengthen_level, 1);
    let ManualFishingStrengthenTerminalOutcomeV1::NonFish {
        progression,
        durability,
        ..
    } = &committed.outcome
    else {
        panic!("forced baseline Junk branch must commit non-Fish Strengthen");
    };
    assert_eq!(progression.account_xp.granted_xp, 2);
    assert_eq!(durability.operation_id, operation.operation_id);
    let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = &durability.applied else {
        panic!("Strengthen terminal receipt must contain ordinary Rod durability");
    };
    assert_eq!(preview.resulting_durability, 199);

    let before: i64 =
        sqlx::query_scalar("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let replay = resolve_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap();
    let ManualFishingCastRequestResolutionV1::CommittedStrengthen(replayed) = replay else {
        panic!("committed Strengthen request must replay its typed receipt");
    };
    assert_eq!(*replayed, committed);
    let after: i64 =
        sqlx::query_scalar("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(after, before);
    assert_eq!(
        operation_state(&store, operation.operation_id).await,
        "COMMITTED"
    );

    let terminal_events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM asset_events WHERE operation_id = $1 AND mutation_key = 'fishing:cast:terminal'",
    )
    .bind(operation.operation_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    let outbox: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE operation_id = $1 AND topic = 'fishing.cast.completed'",
    )
    .bind(operation.operation_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!((terminal_events, outbox), (1, 1));
}

#[tokio::test]
async fn request_owned_sharp_hook_i_junk_commits_and_replays_without_second_wear() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let actor = positive_snowflake(nonce);
    let player_id = seed_player(&store, actor).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "sharp-hook-i-terminal-junk",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    seed_enchant(&store, rod_id, "SHARP_HOOK", 1).await;
    let key = format!("test:manual-fishing-request:{nonce}:sharp-hook-i-junk");

    let operation = {
        let mut tx = store.pool().begin().await.unwrap();
        let operation = graphite_services::lock_or_create_manual_fishing_operation_v1(
            &mut tx,
            u64::try_from(actor).unwrap(),
            &key,
            FishingArea::River,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        operation
    };
    let root = find_baseline_branch_root(operation.operation_id, 1);
    sqlx::query("UPDATE operations SET rng_root = $1 WHERE id = $2")
        .bind(root.as_slice())
        .bind(operation.operation_id)
        .execute(store.pool())
        .await
        .unwrap();

    let tx = store.pool().begin().await.unwrap();
    let pending = begin_manual_fishing_cast_request_v1(
        tx,
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap();
    let committed = pending.commit_sharp_hook_v1().await.unwrap();
    assert_eq!(committed.sharp_hook_level, 1);
    let ManualFishingSharpHookTerminalOutcomeV1::NonFish {
        progression,
        durability,
        ..
    } = &committed.outcome
    else {
        panic!("forced baseline Junk branch must commit non-Fish SharpHook");
    };
    assert_eq!(progression.account_xp.granted_xp, 2);
    assert_eq!(durability.operation_id, operation.operation_id);
    let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = &durability.applied else {
        panic!("SharpHook terminal receipt must contain ordinary Rod durability");
    };
    assert_eq!(preview.resulting_durability, 199);

    let before: i64 =
        sqlx::query_scalar("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let replay = resolve_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap();
    let ManualFishingCastRequestResolutionV1::CommittedSharpHook(replayed) = replay else {
        panic!("committed SharpHook request must replay its typed receipt");
    };
    assert_eq!(*replayed, committed);
    let after: i64 =
        sqlx::query_scalar("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(after, before);
    assert_eq!(
        operation_state(&store, operation.operation_id).await,
        "COMMITTED"
    );

    let terminal_events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM asset_events WHERE operation_id = $1 AND mutation_key = 'fishing:cast:terminal'",
    )
    .bind(operation.operation_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    let outbox: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE operation_id = $1 AND topic = 'fishing.cast.completed'",
    )
    .bind(operation.operation_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!((terminal_events, outbox), (1, 1));
}

#[tokio::test]
async fn request_owned_sharp_hook_direct_book_commits_and_replays_exact_book() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let actor = positive_snowflake(nonce);
    let player_id = seed_player(&store, actor).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "sharp-hook-terminal-book",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    seed_enchant(&store, rod_id, "SHARP_HOOK", 10).await;
    let key = format!("test:manual-fishing-request:{nonce}:sharp-hook-book");

    let operation = {
        let mut tx = store.pool().begin().await.unwrap();
        let operation = graphite_services::lock_or_create_manual_fishing_operation_v1(
            &mut tx,
            u64::try_from(actor).unwrap(),
            &key,
            FishingArea::River,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        operation
    };
    let root = find_selected_direct_book_root(operation.operation_id);
    sqlx::query("UPDATE operations SET rng_root = $1 WHERE id = $2")
        .bind(root.as_slice())
        .bind(operation.operation_id)
        .execute(store.pool())
        .await
        .unwrap();

    let pending = begin_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap();
    let committed = pending.commit_sharp_hook_v1().await.unwrap();
    let ManualFishingSharpHookTerminalOutcomeV1::Book {
        deliveries,
        progression,
        durability,
    } = &committed.outcome
    else {
        panic!("forced SharpHook direct Book must commit as Book");
    };
    assert_eq!(deliveries.len(), 1);
    assert_eq!(progression.base_activity_xp, 5);
    assert_eq!(progression.account_xp.granted_xp, 2);
    assert_eq!(deliveries[0].candidate.treasure_slot, 0);
    assert_eq!(deliveries[0].delivery.operation_id, operation.operation_id);
    assert!(!deliveries[0].delivery.pending);
    let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = &durability.applied else {
        panic!("SharpHook Book terminal must persist ordinary Rod wear");
    };
    assert_eq!(preview.resulting_durability, 199);
    assert_eq!(finished_book_count(&store, operation.operation_id).await, 1);

    let replay = resolve_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap();
    let ManualFishingCastRequestResolutionV1::CommittedSharpHook(replayed) = replay else {
        panic!("committed SharpHook Book must replay typed receipt");
    };
    assert_eq!(*replayed, committed);
    assert_eq!(finished_book_count(&store, operation.operation_id).await, 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap(),
        199
    );
    assert_terminal_once(&store, operation.operation_id).await;
}

#[tokio::test]
async fn request_owned_treasure_x_non_book_treasure_commits_and_replays_without_second_wear() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let actor = positive_snowflake(nonce);
    let player_id = seed_player(&store, actor).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "treasure-x-terminal-nonbook",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    seed_enchant(&store, rod_id, "FISHING_ROD_TREASURE", 10).await;
    let key = format!("test:manual-fishing-request:{nonce}:treasure-x-nonbook");

    let operation = {
        let mut tx = store.pool().begin().await.unwrap();
        let operation = graphite_services::lock_or_create_manual_fishing_operation_v1(
            &mut tx,
            u64::try_from(actor).unwrap(),
            &key,
            FishingArea::River,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        operation
    };
    let root = find_treasure_x_non_book_treasure_root(operation.operation_id);
    sqlx::query("UPDATE operations SET rng_root = $1 WHERE id = $2")
        .bind(root.as_slice())
        .bind(operation.operation_id)
        .execute(store.pool())
        .await
        .unwrap();

    let tx = store.pool().begin().await.unwrap();
    let pending = begin_manual_fishing_cast_request_v1(
        tx,
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap();
    let committed = pending.commit_treasure_x_v1().await.unwrap();
    let ManualFishingTreasureXTerminalOutcomeV1::NonFish {
        delivery,
        progression,
        durability,
    } = &committed.outcome
    else {
        panic!("forced Treasure-X non-book Treasure must commit as NonFish");
    };
    assert_eq!(delivery.deliveries.len(), 1);
    assert_eq!(progression.account_xp.granted_xp, 2);
    assert_eq!(progression.base_activity_xp, 5);
    assert_eq!(durability.operation_id, operation.operation_id);
    let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = &durability.applied else {
        panic!("Treasure X terminal receipt must contain ordinary Rod durability");
    };
    assert_eq!(preview.resulting_durability, 199);

    let before: i64 =
        sqlx::query_scalar("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let replay = resolve_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap();
    let ManualFishingCastRequestResolutionV1::CommittedTreasureX(replayed) = replay else {
        panic!("committed Treasure X request must replay its typed receipt");
    };
    assert_eq!(*replayed, committed);
    let after: i64 =
        sqlx::query_scalar("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(after, before);
    assert_eq!(
        operation_state(&store, operation.operation_id).await,
        "COMMITTED"
    );

    let terminal_events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM asset_events WHERE operation_id = $1 AND mutation_key = 'fishing:cast:terminal'",
    )
    .bind(operation.operation_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    let outbox: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE operation_id = $1 AND topic = 'fishing.cast.completed'",
    )
    .bind(operation.operation_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!((terminal_events, outbox), (1, 1));
}

#[tokio::test]
async fn request_owned_treasure_x_direct_book_commits_and_replays_exact_book() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let actor = positive_snowflake(nonce);
    let player_id = seed_player(&store, actor).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "treasure-x-terminal-book",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    seed_enchant(&store, rod_id, "FISHING_ROD_TREASURE", 10).await;
    let key = format!("test:manual-fishing-request:{nonce}:treasure-x-book");

    let operation = {
        let mut tx = store.pool().begin().await.unwrap();
        let operation = graphite_services::lock_or_create_manual_fishing_operation_v1(
            &mut tx,
            u64::try_from(actor).unwrap(),
            &key,
            FishingArea::River,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        operation
    };
    let root = find_treasure_x_direct_book_root(operation.operation_id);
    sqlx::query("UPDATE operations SET rng_root = $1 WHERE id = $2")
        .bind(root.as_slice())
        .bind(operation.operation_id)
        .execute(store.pool())
        .await
        .unwrap();

    let pending = begin_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap();
    let committed = pending.commit_treasure_x_v1().await.unwrap();
    let ManualFishingTreasureXTerminalOutcomeV1::Book {
        deliveries,
        progression,
        durability,
    } = &committed.outcome
    else {
        panic!("forced Treasure-X direct Book must commit as Book");
    };
    assert_eq!(deliveries.len(), 1);
    assert_eq!(progression.base_activity_xp, 5);
    assert_eq!(deliveries[0].candidate.treasure_slot, 0);
    let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = &durability.applied else {
        panic!("Treasure-X Book terminal must persist ordinary Rod wear");
    };
    assert_eq!(preview.resulting_durability, 199);
    assert_eq!(finished_book_count(&store, operation.operation_id).await, 1);

    let replay = resolve_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap();
    let ManualFishingCastRequestResolutionV1::CommittedTreasureX(replayed) = replay else {
        panic!("committed Treasure-X Book must replay typed receipt");
    };
    assert_eq!(*replayed, committed);
    assert_eq!(finished_book_count(&store, operation.operation_id).await, 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap(),
        199
    );
    assert_terminal_once(&store, operation.operation_id).await;
}

#[tokio::test]
async fn request_owned_bait_rack_iii_junk_commits_and_replays_without_second_wear() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let actor = positive_snowflake(nonce);
    let player_id = seed_player(&store, actor).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "bait-rack-iii-terminal-junk",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    seed_enchant(&store, rod_id, "BAIT_RACK", 3).await;
    let key = format!("test:manual-fishing-request:{nonce}:bait-rack-iii-junk");

    let operation = {
        let mut tx = store.pool().begin().await.unwrap();
        let operation = graphite_services::lock_or_create_manual_fishing_operation_v1(
            &mut tx,
            u64::try_from(actor).unwrap(),
            &key,
            FishingArea::River,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        operation
    };
    let root = find_baseline_branch_root(operation.operation_id, 1);
    sqlx::query("UPDATE operations SET rng_root = $1 WHERE id = $2")
        .bind(root.as_slice())
        .bind(operation.operation_id)
        .execute(store.pool())
        .await
        .unwrap();

    let pending = begin_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap();
    let committed = pending.commit_bait_rack_v1().await.unwrap();
    assert_eq!(committed.bait_rack_level, 3);
    let ManualFishingModifierTerminalOutcomeV1::NonFish {
        progression,
        durability,
        ..
    } = &committed.outcome
    else {
        panic!("forced Bait Rack Junk must commit as NonFish");
    };
    assert_eq!(progression.account_xp.granted_xp, 2);
    let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = &durability.applied else {
        panic!("Bait Rack terminal must persist ordinary Rod wear");
    };
    assert_eq!(preview.resulting_durability, 199);

    let replay = resolve_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap();
    let ManualFishingCastRequestResolutionV1::CommittedBaitRack(replayed) = replay else {
        panic!("committed Bait Rack request must replay its typed receipt");
    };
    assert_eq!(*replayed, committed);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap(),
        199
    );
    assert_terminal_once(&store, operation.operation_id).await;
}

#[tokio::test]
async fn request_owned_multicatch_x_multi_fish_commits_and_replays_without_second_wear() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let actor = positive_snowflake(nonce);
    let player_id = seed_player(&store, actor).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "multicatch-x-terminal",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    seed_enchant(&store, rod_id, "MULTICATCH", 10).await;
    let key = format!("test:manual-fishing-request:{nonce}:multicatch-x");

    let operation = {
        let mut tx = store.pool().begin().await.unwrap();
        let operation = graphite_services::lock_or_create_manual_fishing_operation_v1(
            &mut tx,
            u64::try_from(actor).unwrap(),
            &key,
            FishingArea::StarterPool,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        operation
    };
    let root = find_multicatch_fish_root(operation.operation_id, 10);
    sqlx::query("UPDATE operations SET rng_root = $1 WHERE id = $2")
        .bind(root.as_slice())
        .bind(operation.operation_id)
        .execute(store.pool())
        .await
        .unwrap();

    let pending = begin_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::StarterPool,
    )
    .await
    .unwrap();
    let committed = pending.commit_multicatch_v1().await.unwrap();
    assert_eq!(committed.multicatch_level, 10);
    let count = committed
        .multicatch_count
        .expect("Fish Multicatch terminal must pin its count");
    assert!(count.fish_count > 1);
    let ManualFishingModifierTerminalOutcomeV1::Fish {
        deliveries,
        progression,
        durability,
    } = &committed.outcome
    else {
        panic!("forced Starter Pool Multicatch Fish must commit as Fish");
    };
    assert_eq!(deliveries.len(), usize::from(count.fish_count));
    assert_eq!(progression.account_xp.granted_xp, 2);
    let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = &durability.applied else {
        panic!("Multicatch terminal must persist ordinary Rod wear");
    };
    assert_eq!(preview.resulting_durability, 199);

    let asset_before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM asset_events WHERE operation_id = $1")
            .bind(operation.operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let replay = resolve_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::StarterPool,
    )
    .await
    .unwrap();
    let ManualFishingCastRequestResolutionV1::CommittedMulticatch(replayed) = replay else {
        panic!("committed Multicatch request must replay its typed receipt");
    };
    assert_eq!(*replayed, committed);
    let asset_after: i64 =
        sqlx::query_scalar("SELECT count(*) FROM asset_events WHERE operation_id = $1")
            .bind(operation.operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(asset_after, asset_before);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap(),
        199
    );
    assert_terminal_once(&store, operation.operation_id).await;
}

#[tokio::test]
async fn request_owned_multi_treasure_x_repeated_non_book_commits_and_replays() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let actor = positive_snowflake(nonce);
    let player_id = seed_player(&store, actor).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "multi-treasure-x-terminal",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    seed_enchant(&store, rod_id, "MULTI_TREASURE", 10).await;
    let key = format!("test:manual-fishing-request:{nonce}:multi-treasure-x");

    let operation = {
        let mut tx = store.pool().begin().await.unwrap();
        let operation = graphite_services::lock_or_create_manual_fishing_operation_v1(
            &mut tx,
            u64::try_from(actor).unwrap(),
            &key,
            FishingArea::StarterPool,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        operation
    };
    let root = find_multi_treasure_root(operation.operation_id, 0, 10);
    sqlx::query("UPDATE operations SET rng_root = $1 WHERE id = $2")
        .bind(root.as_slice())
        .bind(operation.operation_id)
        .execute(store.pool())
        .await
        .unwrap();

    let pending = begin_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::StarterPool,
    )
    .await
    .unwrap();
    let committed = pending.commit_multi_treasure_v1().await.unwrap();
    assert_eq!(committed.multi_treasure_level, 10);
    let ManualFishingModifierTerminalOutcomeV1::NonFish {
        delivery,
        progression,
        durability,
    } = &committed.outcome
    else {
        panic!("forced repeated Multi Treasure must commit as NonFish");
    };
    assert!(delivery.deliveries.len() > 1);
    assert_eq!(progression.base_activity_xp, 10);
    assert_eq!(progression.account_xp.granted_xp, 2);
    let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = &durability.applied else {
        panic!("Multi Treasure terminal must persist ordinary Rod wear");
    };
    assert_eq!(preview.resulting_durability, 199);

    let asset_before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM asset_events WHERE operation_id = $1")
            .bind(operation.operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let replay = resolve_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::StarterPool,
    )
    .await
    .unwrap();
    let ManualFishingCastRequestResolutionV1::CommittedMultiTreasure(replayed) = replay else {
        panic!("committed Multi Treasure request must replay its typed receipt");
    };
    assert_eq!(*replayed, committed);
    let asset_after: i64 =
        sqlx::query_scalar("SELECT count(*) FROM asset_events WHERE operation_id = $1")
            .bind(operation.operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(asset_after, asset_before);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap(),
        199
    );
    assert_terminal_once(&store, operation.operation_id).await;
}

#[tokio::test]
async fn request_owned_multi_treasure_direct_books_commit_independent_items_and_replay() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let actor = positive_snowflake(nonce);
    let player_id = seed_player(&store, actor).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "multi-treasure-terminal-books",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    seed_enchant(&store, rod_id, "MULTI_TREASURE", 10).await;
    let key = format!("test:manual-fishing-request:{nonce}:multi-treasure-books");

    let operation = {
        let mut tx = store.pool().begin().await.unwrap();
        let operation = graphite_services::lock_or_create_manual_fishing_operation_v1(
            &mut tx,
            u64::try_from(actor).unwrap(),
            &key,
            FishingArea::StarterPool,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        operation
    };
    let root = find_multi_treasure_root(operation.operation_id, 2, 10);
    sqlx::query("UPDATE operations SET rng_root = $1 WHERE id = $2")
        .bind(root.as_slice())
        .bind(operation.operation_id)
        .execute(store.pool())
        .await
        .unwrap();

    let pending = begin_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::StarterPool,
    )
    .await
    .unwrap();
    let committed = pending.commit_multi_treasure_v1().await.unwrap();
    let ManualFishingModifierTerminalOutcomeV1::Book {
        deliveries,
        progression,
        durability,
    } = &committed.outcome
    else {
        panic!("forced Multi Treasure direct Books must commit as Book");
    };
    assert!(deliveries.len() > 1);
    assert_eq!(
        progression.base_activity_xp,
        i64::try_from(deliveries.len()).unwrap() * 5
    );
    for (slot, delivery) in deliveries.iter().enumerate() {
        assert_eq!(usize::from(delivery.candidate.treasure_slot), slot);
        assert_eq!(delivery.delivery.operation_id, operation.operation_id);
    }
    let unique_ids = deliveries
        .iter()
        .map(|delivery| delivery.delivery.item_instance_id)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(unique_ids.len(), deliveries.len());
    let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = &durability.applied else {
        panic!("Multi Treasure Book terminal must persist one ordinary Rod wear");
    };
    assert_eq!(preview.resulting_durability, 199);
    assert_eq!(
        finished_book_count(&store, operation.operation_id).await,
        i64::try_from(deliveries.len()).unwrap()
    );

    let replay = resolve_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::StarterPool,
    )
    .await
    .unwrap();
    let ManualFishingCastRequestResolutionV1::CommittedMultiTreasure(replayed) = replay else {
        panic!("committed Multi Treasure Books must replay typed receipt");
    };
    assert_eq!(*replayed, committed);
    assert_eq!(
        finished_book_count(&store, operation.operation_id).await,
        i64::try_from(deliveries.len()).unwrap()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap(),
        199
    );
    assert_terminal_once(&store, operation.operation_id).await;
}

#[tokio::test]
async fn request_owned_sturdy_bait_junk_commits_bait_and_replays_without_second_consumption() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let actor = positive_snowflake(nonce);
    let player_id = seed_player(&store, actor).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "sturdy-terminal-junk",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    let key = format!("test:manual-fishing-request:{nonce}:sturdy-junk");

    let operation = {
        let mut tx = store.pool().begin().await.unwrap();
        let operation = graphite_services::lock_or_create_manual_fishing_operation_v1(
            &mut tx,
            u64::try_from(actor).unwrap(),
            &key,
            FishingArea::River,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        operation
    };
    seed_active_fishing_bait(
        &store,
        player_id,
        operation.operation_id,
        "STURDY",
        "fishing.bait.sturdy",
        2,
    )
    .await;
    let root = find_baseline_branch_root(operation.operation_id, 1);
    sqlx::query("UPDATE operations SET rng_root = $1 WHERE id = $2")
        .bind(root.as_slice())
        .bind(operation.operation_id)
        .execute(store.pool())
        .await
        .unwrap();

    let pending = begin_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap();
    let committed = pending.commit_sturdy_bait_v1().await.unwrap();
    assert_eq!(committed.bait_settlement.plan.consumed_bait_categories, 1);
    assert_eq!(committed.bait_settlement.consumption_receipts.len(), 1);
    let ManualFishingModifierTerminalOutcomeV1::NonFish { durability, .. } = &committed.outcome
    else {
        panic!("forced Sturdy Junk must commit as NonFish");
    };
    let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = &durability.applied else {
        panic!("Sturdy terminal must persist ordinary Rod wear");
    };
    assert_eq!(preview.resulting_durability, 199);
    assert_eq!(
        fishing_bait_quantity(&store, player_id, "fishing.bait.sturdy").await,
        1
    );

    let replay = resolve_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap();
    let ManualFishingCastRequestResolutionV1::CommittedSturdyBait(replayed) = replay else {
        panic!("committed Sturdy Bait request must replay its typed receipt");
    };
    assert_eq!(*replayed, committed);
    assert_eq!(
        fishing_bait_quantity(&store, player_id, "fishing.bait.sturdy").await,
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap(),
        199
    );
    assert_eq!(
        bait_settlement_event_count(&store, operation.operation_id).await,
        1
    );
    assert_terminal_once(&store, operation.operation_id).await;
}

#[tokio::test]
async fn request_owned_sturdy_bait_direct_book_commits_bait_book_and_replays_once() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let actor = positive_snowflake(nonce);
    let player_id = seed_player(&store, actor).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "sturdy-terminal-book",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    let key = format!("test:manual-fishing-request:{nonce}:sturdy-book");

    let operation = {
        let mut tx = store.pool().begin().await.unwrap();
        let operation = graphite_services::lock_or_create_manual_fishing_operation_v1(
            &mut tx,
            u64::try_from(actor).unwrap(),
            &key,
            FishingArea::River,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        operation
    };
    seed_active_fishing_bait(
        &store,
        player_id,
        operation.operation_id,
        "STURDY",
        "fishing.bait.sturdy",
        2,
    )
    .await;
    let root = find_selected_direct_book_root(operation.operation_id);
    sqlx::query("UPDATE operations SET rng_root = $1 WHERE id = $2")
        .bind(root.as_slice())
        .bind(operation.operation_id)
        .execute(store.pool())
        .await
        .unwrap();

    let pending = begin_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap();
    let committed = pending.commit_sturdy_bait_v1().await.unwrap();
    let ManualFishingModifierTerminalOutcomeV1::Book {
        deliveries,
        progression,
        durability,
    } = &committed.outcome
    else {
        panic!("forced Sturdy direct Book must commit as Book");
    };
    assert_eq!(deliveries.len(), 1);
    assert_eq!(progression.base_activity_xp, 5);
    assert_eq!(
        fishing_bait_quantity(&store, player_id, "fishing.bait.sturdy").await,
        1
    );
    let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = &durability.applied else {
        panic!("Sturdy Book terminal must persist ordinary Rod wear");
    };
    assert_eq!(preview.resulting_durability, 199);
    assert_eq!(finished_book_count(&store, operation.operation_id).await, 1);
    assert_eq!(
        bait_settlement_event_count(&store, operation.operation_id).await,
        1
    );

    let replay = resolve_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap();
    let ManualFishingCastRequestResolutionV1::CommittedSturdyBait(replayed) = replay else {
        panic!("committed Sturdy Book must replay typed receipt");
    };
    assert_eq!(*replayed, committed);
    assert_eq!(
        fishing_bait_quantity(&store, player_id, "fishing.bait.sturdy").await,
        1
    );
    assert_eq!(finished_book_count(&store, operation.operation_id).await, 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap(),
        199
    );
    assert_terminal_once(&store, operation.operation_id).await;
}

#[tokio::test]
async fn request_owned_school_bait_triggered_multi_fish_commits_bait_and_replays() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let actor = positive_snowflake(nonce);
    let player_id = seed_player(&store, actor).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "school-terminal-triggered",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    let key = format!("test:manual-fishing-request:{nonce}:school-triggered");

    let operation = {
        let mut tx = store.pool().begin().await.unwrap();
        let operation = graphite_services::lock_or_create_manual_fishing_operation_v1(
            &mut tx,
            u64::try_from(actor).unwrap(),
            &key,
            FishingArea::StarterPool,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        operation
    };
    seed_active_fishing_bait(
        &store,
        player_id,
        operation.operation_id,
        "SCHOOL",
        "fishing.bait.school",
        2,
    )
    .await;
    let root = find_school_bait_triggered_fish_root(operation.operation_id);
    sqlx::query("UPDATE operations SET rng_root = $1 WHERE id = $2")
        .bind(root.as_slice())
        .bind(operation.operation_id)
        .execute(store.pool())
        .await
        .unwrap();

    let pending = begin_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::StarterPool,
    )
    .await
    .unwrap();
    let committed = pending.commit_school_bait_v1().await.unwrap();
    let quantity = committed
        .school_quantity
        .expect("School Fish terminal must pin quantity RNG");
    assert_eq!(quantity.chance_ppm, 80_000);
    assert_eq!(quantity.quantity.final_fish_count(), 2);
    let ManualFishingModifierTerminalOutcomeV1::Fish {
        deliveries,
        progression,
        durability,
    } = &committed.outcome
    else {
        panic!("forced triggered School Fish must commit as Fish");
    };
    assert_eq!(deliveries.len(), 2);
    assert_eq!(progression.account_xp.granted_xp, 2);
    let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = &durability.applied else {
        panic!("School terminal must persist ordinary Rod wear");
    };
    assert_eq!(preview.resulting_durability, 199);
    assert_eq!(
        fishing_bait_quantity(&store, player_id, "fishing.bait.school").await,
        1
    );

    let replay = resolve_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::StarterPool,
    )
    .await
    .unwrap();
    let ManualFishingCastRequestResolutionV1::CommittedSchoolBait(replayed) = replay else {
        panic!("committed School Bait request must replay its typed receipt");
    };
    assert_eq!(*replayed, committed);
    assert_eq!(
        fishing_bait_quantity(&store, player_id, "fishing.bait.school").await,
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap(),
        199
    );
    assert_eq!(
        bait_settlement_event_count(&store, operation.operation_id).await,
        1
    );
    assert_terminal_once(&store, operation.operation_id).await;
}

#[tokio::test]
async fn request_owned_unmodified_ordinary_junk_commits_and_replays_without_second_wear() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let actor = positive_snowflake(nonce);
    let player_id = seed_player(&store, actor).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "ordinary-terminal-junk",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    let key = format!("test:manual-fishing-request:{nonce}:ordinary-junk");

    let operation = {
        let mut tx = store.pool().begin().await.unwrap();
        let operation = graphite_services::lock_or_create_manual_fishing_operation_v1(
            &mut tx,
            u64::try_from(actor).unwrap(),
            &key,
            FishingArea::River,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        operation
    };
    let root = find_baseline_branch_root(operation.operation_id, 1);
    sqlx::query("UPDATE operations SET rng_root = $1 WHERE id = $2")
        .bind(root.as_slice())
        .bind(operation.operation_id)
        .execute(store.pool())
        .await
        .unwrap();

    let tx = store.pool().begin().await.unwrap();
    let pending = begin_manual_fishing_cast_request_v1(
        tx,
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap();
    let committed = pending.commit_unmodified_ordinary_v1().await.unwrap();
    assert_eq!(committed.operation_id, operation.operation_id);
    assert_eq!(committed.player_id, player_id);
    assert_eq!(committed.area, FishingArea::River);
    let ManualFishingUnmodifiedOrdinaryTerminalOutcomeV1::NonFish {
        progression,
        durability,
        ..
    } = &committed.outcome
    else {
        panic!("forced baseline Junk branch must commit a non-Fish ordinary result");
    };
    assert_eq!(progression.account_xp.granted_xp, 2);
    assert_eq!(durability.operation_id, operation.operation_id);
    let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = &durability.applied else {
        panic!("ordinary terminal receipt must contain ordinary Rod durability");
    };
    assert_eq!(preview.resulting_durability, 199);

    let durability_after_commit: i64 =
        sqlx::query_scalar("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(durability_after_commit, 199);
    let asset_before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM asset_events WHERE operation_id = $1")
            .bind(operation.operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let progression_before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM progression_events WHERE operation_id = $1")
            .bind(operation.operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();

    let replay = resolve_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap();
    let ManualFishingCastRequestResolutionV1::CommittedUnmodifiedOrdinary(replayed) = replay else {
        panic!("committed ordinary request must replay its typed terminal receipt");
    };
    assert_eq!(*replayed, committed);

    let durability_after_replay: i64 =
        sqlx::query_scalar("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let asset_after: i64 =
        sqlx::query_scalar("SELECT count(*) FROM asset_events WHERE operation_id = $1")
            .bind(operation.operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let progression_after: i64 =
        sqlx::query_scalar("SELECT count(*) FROM progression_events WHERE operation_id = $1")
            .bind(operation.operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(durability_after_replay, 199);
    assert_eq!(
        (asset_after, progression_after),
        (asset_before, progression_before)
    );
}

#[tokio::test]
async fn ordinary_rod_starter_pool_fish_can_commit_with_one_wear_event() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let actor = positive_snowflake(nonce);
    let player_id = seed_player(&store, actor).await;
    let rod_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "ordinary-terminal-fish",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    let key = format!("test:manual-fishing-request:{nonce}:ordinary-fish");

    let operation = {
        let mut tx = store.pool().begin().await.unwrap();
        let operation = graphite_services::lock_or_create_manual_fishing_operation_v1(
            &mut tx,
            u64::try_from(actor).unwrap(),
            &key,
            FishingArea::StarterPool,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        operation
    };
    let root = find_baseline_branch_root(operation.operation_id, 0);
    sqlx::query("UPDATE operations SET rng_root = $1 WHERE id = $2")
        .bind(root.as_slice())
        .bind(operation.operation_id)
        .execute(store.pool())
        .await
        .unwrap();

    let tx = store.pool().begin().await.unwrap();
    let pending = begin_manual_fishing_cast_request_v1(
        tx,
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::StarterPool,
    )
    .await
    .unwrap();
    let committed = pending.commit_unmodified_ordinary_v1().await.unwrap();
    let ManualFishingUnmodifiedOrdinaryTerminalOutcomeV1::Fish {
        delivery,
        progression,
        durability,
    } = &committed.outcome
    else {
        panic!("forced Starter Pool Fish branch must commit Fish");
    };
    assert_eq!(delivery.operation_id, operation.operation_id);
    assert_eq!(progression.account_xp.granted_xp, 2);
    let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = &durability.applied else {
        panic!("ordinary Rod terminal Fish must carry ordinary wear");
    };
    assert_eq!(preview.resulting_durability, 199);

    let current: i64 =
        sqlx::query_scalar("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(current, 199);
    assert_eq!(
        operation_state(&store, operation.operation_id).await,
        "COMMITTED"
    );
}

#[tokio::test]
async fn request_owned_committed_starter_cast_replays_typed_receipt_without_new_mutations() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let actor = positive_snowflake(nonce);
    let player_id = seed_player(&store, actor).await;
    seed_starter_basic_rod(&store, player_id, nonce, "request-committed-replay").await;
    let key = format!("test:manual-fishing-request:{nonce}:committed");

    let operation = {
        let mut tx = store.pool().begin().await.unwrap();
        let operation = graphite_services::lock_or_create_manual_fishing_operation_v1(
            &mut tx,
            u64::try_from(actor).unwrap(),
            &key,
            FishingArea::StarterPool,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        operation
    };
    let root = find_baseline_branch_root(operation.operation_id, 0);
    sqlx::query("UPDATE operations SET rng_root = $1 WHERE id = $2")
        .bind(root.as_slice())
        .bind(operation.operation_id)
        .execute(store.pool())
        .await
        .unwrap();

    let tx = store.pool().begin().await.unwrap();
    let pending = begin_manual_fishing_cast_request_v1(
        tx,
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::StarterPool,
    )
    .await
    .unwrap();
    let committed = pending.commit_unmodified_starter_base_v1().await.unwrap();

    let asset_before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM asset_events WHERE operation_id = $1")
            .bind(operation.operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let progression_before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM progression_events WHERE operation_id = $1")
            .bind(operation.operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();

    let replay = resolve_manual_fishing_cast_request_v1(
        store.pool().begin().await.unwrap(),
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::StarterPool,
    )
    .await
    .unwrap();
    let ManualFishingCastRequestResolutionV1::CommittedStarterBase(replayed) = replay else {
        panic!("committed request must replay the typed Starter Basic receipt");
    };
    assert_eq!(*replayed, committed);

    let asset_after: i64 =
        sqlx::query_scalar("SELECT count(*) FROM asset_events WHERE operation_id = $1")
            .bind(operation.operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let progression_after: i64 =
        sqlx::query_scalar("SELECT count(*) FROM progression_events WHERE operation_id = $1")
            .bind(operation.operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(
        (asset_after, progression_after),
        (asset_before, progression_before)
    );

    let outbox: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE operation_id = $1 AND topic = 'fishing.cast.completed'",
    )
    .bind(operation.operation_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(outbox, 1);
}

#[tokio::test]
async fn starter_basic_terminal_commit_persists_typed_result_and_outbox_atomically() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let actor = positive_snowflake(nonce);
    let player_id = seed_player(&store, actor).await;
    seed_starter_basic_rod(&store, player_id, nonce, "starter-terminal").await;
    let operation_id = seed_operation(&store, player_id, nonce, "starter-terminal").await;
    let root = find_baseline_branch_root(operation_id, 0);
    sqlx::query("UPDATE operations SET rng_root = $1 WHERE id = $2")
        .bind(root.as_slice())
        .bind(operation_id)
        .execute(store.pool())
        .await
        .unwrap();

    let tx = store.pool().begin().await.unwrap();
    let pending =
        begin_manual_fishing_cast_v1(tx, operation_id, player_id, FishingArea::StarterPool)
            .await
            .unwrap();
    let receipt = pending.commit_unmodified_starter_base_v1().await.unwrap();
    assert_eq!(receipt.operation_id, operation_id);
    assert_eq!(receipt.player_id, player_id);
    assert_eq!(receipt.area, FishingArea::StarterPool);
    let ManualFishingStarterBaseTerminalOutcomeV1::Fish {
        delivery,
        progression,
    } = &receipt.outcome
    else {
        panic!("forced baseline Fish branch must commit Fish");
    };
    assert_eq!(delivery.operation_id, operation_id);
    assert_eq!(progression.operation_id, operation_id);
    assert_eq!(progression.account_xp.granted_xp, 2);

    let row = sqlx::query("SELECT state, result, committed_at FROM operations WHERE id = $1")
        .bind(operation_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
    let state: String = sqlx::Row::try_get(&row, "state").unwrap();
    let result: serde_json::Value = sqlx::Row::try_get(&row, "result").unwrap();
    let committed_at: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::Row::try_get(&row, "committed_at").unwrap();
    assert_eq!(state, "COMMITTED");
    assert!(committed_at.is_some());
    assert_eq!(
        serde_json::from_value::<graphite_services::ManualFishingStarterBaseTerminalReceiptV1>(
            result,
        )
        .unwrap(),
        receipt
    );

    let terminal_events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM asset_events WHERE operation_id = $1 AND mutation_key = 'fishing:cast:terminal'",
    )
    .bind(operation_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    let outbox: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE operation_id = $1 AND topic = 'fishing.cast.completed'",
    )
    .bind(operation_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!((terminal_events, outbox), (1, 1));
}

fn find_baseline_branch_root(operation_id: Uuid, selected_index: usize) -> [u8; 32] {
    let operation = OperationId::from_uuid(operation_id);
    for nonce in 0_u64..20_000 {
        let mut bytes = [0_u8; 32];
        bytes[..8].copy_from_slice(&nonce.to_be_bytes());
        let root = RootSeed::from_bytes(bytes);
        let mut rng = DomainRng::derive(root, operation, "fishing.manual.v1.catch_branch");
        if rng.sample_weighted(&[176, 17, 7]).unwrap() == selected_index {
            return bytes;
        }
    }
    panic!("test fixture could not find requested baseline branch");
}

#[tokio::test]
async fn request_owned_entrypoint_creates_preflights_and_rolls_back_operation_together() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let actor = positive_snowflake(nonce);
    let player_id = seed_player(&store, actor).await;
    seed_starter_basic_rod(&store, player_id, nonce, "request-owned").await;
    let key = format!("test:manual-fishing-request:{nonce}");

    let tx = store.pool().begin().await.unwrap();
    let pending = begin_manual_fishing_cast_request_v1(
        tx,
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::StarterPool,
    )
    .await
    .unwrap();
    assert_eq!(pending.preflight().player_id, player_id);
    assert_eq!(
        pending.preflight().area_access.area,
        FishingArea::StarterPool
    );
    let operation_id = pending.preflight().operation_id;
    pending.rollback().await.unwrap();

    let operation_count: i64 = sqlx::query_scalar("SELECT count(*) FROM operations WHERE id = $1")
        .bind(operation_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(operation_count, 0);
}

#[tokio::test]
async fn request_owned_entrypoint_resumes_existing_pending_operation_without_rerolling_root() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let actor = positive_snowflake(nonce);
    let player_id = seed_player(&store, actor).await;
    seed_starter_basic_rod(&store, player_id, nonce, "request-resume").await;
    let key = format!("test:manual-fishing-request:{nonce}:resume");

    let operation = {
        let mut tx = store.pool().begin().await.unwrap();
        let operation = graphite_services::lock_or_create_manual_fishing_operation_v1(
            &mut tx,
            u64::try_from(actor).unwrap(),
            &key,
            FishingArea::StarterPool,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        operation
    };
    let original_root: Vec<u8> =
        sqlx::query_scalar("SELECT rng_root FROM operations WHERE id = $1")
            .bind(operation.operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();

    let tx = store.pool().begin().await.unwrap();
    let pending = begin_manual_fishing_cast_request_v1(
        tx,
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::StarterPool,
    )
    .await
    .unwrap();
    assert_eq!(pending.preflight().operation_id, operation.operation_id);
    let replay_root = pending.preflight().rng;
    assert_eq!(replay_root.operation_id(), operation.operation_id);
    pending.rollback().await.unwrap();

    let stored_root: Vec<u8> = sqlx::query_scalar("SELECT rng_root FROM operations WHERE id = $1")
        .bind(operation.operation_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(stored_root, original_root);
    assert_eq!(
        operation_state(&store, operation.operation_id).await,
        "PENDING"
    );
}

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
        let PendingManualFishingStarterBaseOutcomeV1::DirectBook { staged } = &first else {
            panic!("Starter Basic direct Book must stage a typed Book delivery");
        };
        assert_eq!(staged.deliveries.len(), 1);
        assert_eq!(staged.deliveries[0].candidate, candidate);
        assert_eq!(staged.progression.base_activity_xp, 5);
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
async fn selected_direct_book_staging_is_replay_stable_and_rolls_back_all_children() {
    let Some(store) = test_store().await else {
        return;
    };
    for (suffix, starter, area) in [
        ("starter-direct-book", true, FishingArea::StarterPool),
        ("ordinary-direct-book", false, FishingArea::River),
    ] {
        let nonce = Uuid::now_v7();
        let player_id = seed_player(&store, positive_snowflake(nonce)).await;
        set_account_level(&store, player_id, 10).await;
        let rod_id = if starter {
            seed_starter_basic_rod(&store, player_id, nonce, suffix).await
        } else {
            seed_ordinary_rod(
                &store, player_id, nonce, suffix, "WOOD", 200, 600, false, 4, 3,
            )
            .await
        };
        let operation_id = seed_operation(&store, player_id, nonce, suffix).await;
        let root = find_selected_direct_book_root(operation_id);
        sqlx::query("UPDATE operations SET rng_root = $1 WHERE id = $2")
            .bind(root.as_slice())
            .bind(operation_id)
            .execute(store.pool())
            .await
            .unwrap();

        let tx = store.pool().begin().await.unwrap();
        let mut pending = begin_manual_fishing_cast_v1(tx, operation_id, player_id, area)
            .await
            .unwrap();
        let candidate = pending
            .preview_unmodified_selected_direct_book_v1()
            .unwrap();

        if starter {
            let first = pending
                .stage_unmodified_starter_base_outcome_v1()
                .await
                .unwrap();
            let PendingManualFishingStarterBaseOutcomeV1::DirectBook { staged } = &first else {
                panic!("Starter Basic must stage the selected direct Book");
            };
            assert_eq!(staged.deliveries.len(), 1);
            assert_eq!(staged.deliveries[0].candidate, candidate);
            assert_eq!(staged.progression.base_activity_xp, 5);
            assert_eq!(
                first,
                pending
                    .stage_unmodified_starter_base_outcome_v1()
                    .await
                    .unwrap()
            );
        } else {
            let first = pending
                .stage_unmodified_selected_outcome_v1()
                .await
                .unwrap();
            let PendingManualFishingUnmodifiedSelectedOutcomeV1::DirectBook { staged, durability } =
                &first
            else {
                panic!("ordinary Rod must stage the selected direct Book");
            };
            assert_eq!(staged.deliveries.len(), 1);
            assert_eq!(staged.deliveries[0].candidate, candidate);
            assert_eq!(staged.progression.base_activity_xp, 5);
            assert!(matches!(
                durability,
                AppliedFishingRodDurabilityState::Ordinary { .. }
            ));
            assert_eq!(
                first,
                pending
                    .stage_unmodified_selected_outcome_v1()
                    .await
                    .unwrap()
            );
        }

        pending.rollback().await.unwrap();
        assert_eq!(finished_book_count(&store, operation_id).await, 0);
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM asset_events WHERE operation_id = $1"
            )
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM progression_events WHERE operation_id = $1"
            )
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap(),
            0
        );
        if !starter {
            assert_eq!(
                sqlx::query_scalar::<_, i64>(
                    "SELECT current_durability FROM item_instances WHERE id = $1"
                )
                .bind(rod_id)
                .fetch_one(store.pool())
                .await
                .unwrap(),
                200
            );
        }
        assert_eq!(operation_state(&store, operation_id).await, "PENDING");
    }
}

fn find_treasure_x_non_book_treasure_root(operation_id: Uuid) -> [u8; 32] {
    let operation = OperationId::from_uuid(operation_id);
    for nonce in 0_u64..20_000 {
        let mut bytes = [0_u8; 32];
        bytes[..8].copy_from_slice(&nonce.to_be_bytes());
        let root = RootSeed::from_bytes(bytes);
        let mut branch = DomainRng::derive(root, operation, "fishing.manual.v1.catch_branch");
        if branch.sample_weighted(&[880, 85, 63]).unwrap() != 2 {
            continue;
        }
        let mut treasure = DomainRng::derive(root, operation, "fishing.manual.v1.treasure_result");
        if treasure.sample_weighted(&[20, 15, 4, 4, 5, 2]).unwrap() != 2 {
            return bytes;
        }
    }
    panic!("test fixture could not find Treasure-X non-book Treasure seed");
}

fn find_treasure_x_direct_book_root(operation_id: Uuid) -> [u8; 32] {
    let operation = OperationId::from_uuid(operation_id);
    for nonce in 0_u64..20_000 {
        let mut bytes = [0_u8; 32];
        bytes[..8].copy_from_slice(&nonce.to_be_bytes());
        let root = RootSeed::from_bytes(bytes);
        let mut branch = DomainRng::derive(root, operation, "fishing.manual.v1.catch_branch");
        if branch.sample_weighted(&[880, 85, 63]).unwrap() != 2 {
            continue;
        }
        let mut treasure = DomainRng::derive(root, operation, "fishing.manual.v1.treasure_result");
        if treasure.sample_weighted(&[20, 15, 4, 4, 5, 2]).unwrap() == 2 {
            return bytes;
        }
    }
    panic!("test fixture could not find Treasure-X direct Book seed");
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

fn find_school_bait_triggered_fish_root(operation_id: Uuid) -> [u8; 32] {
    let operation = OperationId::from_uuid(operation_id);
    for nonce in 0_u64..200_000 {
        let mut bytes = [0_u8; 32];
        bytes[..8].copy_from_slice(&nonce.to_be_bytes());
        let root = RootSeed::from_bytes(bytes);
        let mut branch = DomainRng::derive(root, operation, "fishing.manual.v1.catch_branch");
        if branch.sample_weighted(&[176, 17, 7]).unwrap() != 0 {
            continue;
        }
        let mut school = DomainRng::derive(root, operation, "fishing.manual.v1.school_bait_proc");
        if school.sample_uniform_below(1_000_000).unwrap() < 80_000 {
            return bytes;
        }
    }
    panic!("test fixture could not find triggered School-Bait Fish seed");
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
        let PendingManualFishingUnmodifiedSelectedOutcomeV1::DirectBook { staged, durability } =
            &first
        else {
            panic!("ordinary direct Book must stage a typed Book delivery");
        };
        assert_eq!(staged.deliveries.len(), 1);
        assert_eq!(staged.deliveries[0].candidate, candidate);
        assert_eq!(staged.progression.base_activity_xp, 5);
        assert!(matches!(
            durability,
            AppliedFishingRodDurabilityState::Ordinary { .. }
        ));
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
async fn treasure_x_uses_modified_outer_branch_and_rolls_back_selected_outcome() {
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
        "treasure-x",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    seed_enchant(&store, rod_id, "FISHING_ROD_TREASURE", 10).await;
    let operation_id = seed_operation(&store, player_id, nonce, "treasure-x").await;
    let root = find_treasure_x_non_book_treasure_root(operation_id);
    sqlx::query("UPDATE operations SET rng_root = $1 WHERE id = $2")
        .bind(root.as_slice())
        .bind(operation_id)
        .execute(store.pool())
        .await
        .unwrap();

    let tx = store.pool().begin().await.unwrap();
    let mut pending = begin_manual_fishing_cast_v1(tx, operation_id, player_id, FishingArea::River)
        .await
        .unwrap();
    assert!(matches!(
        pending.preview_unmodified_base_branch_v1(),
        Err(ManualFishingCastOwnerError::ModifiedFishRequiresSharedComposer)
    ));
    assert_eq!(
        pending.preview_treasure_x_branch_v1().unwrap(),
        FishingCatchBranch::Treasure
    );
    assert_eq!(
        pending.preview_treasure_x_branch_v1().unwrap(),
        resolve_manual_fishing_treasure_x_branch_v1(pending.preflight().rng).unwrap()
    );

    let first = pending
        .stage_treasure_x_selected_outcome_v1()
        .await
        .unwrap();
    let replay = pending
        .stage_treasure_x_selected_outcome_v1()
        .await
        .unwrap();
    assert_eq!(first, replay);
    let PendingManualFishingTreasureXSelectedOutcomeV1::NonFish { staged, durability } = first
    else {
        panic!("forced Treasure-X non-book Treasure must settle as NonFish");
    };
    assert!(matches!(
        staged.outcome,
        FishingResolvedNonFishOutcomeV1::Treasure { .. }
    ));
    assert_eq!(staged.progression.account_xp.granted_xp, 2);
    assert!(matches!(
        durability,
        AppliedFishingRodDurabilityState::Ordinary { .. }
    ));

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
async fn treasure_x_rejects_lower_level_and_gold_combinations() {
    let Some(store) = test_store().await else {
        return;
    };
    for (suffix, tier, level) in [
        ("treasure-nine", "WOOD", 9_i16),
        ("gold-treasure-x", "GOLD", 10_i16),
    ] {
        let nonce = Uuid::now_v7();
        let player_id = seed_player(&store, positive_snowflake(nonce)).await;
        set_account_level(&store, player_id, 10).await;
        let rod_id = seed_ordinary_rod(
            &store, player_id, nonce, suffix, tier, 200, 600, false, 4, 3,
        )
        .await;
        seed_enchant(&store, rod_id, "FISHING_ROD_TREASURE", level).await;
        let operation_id = seed_operation(&store, player_id, nonce, suffix).await;
        let tx = store.pool().begin().await.unwrap();
        let pending = begin_manual_fishing_cast_v1(tx, operation_id, player_id, FishingArea::River)
            .await
            .unwrap();
        assert!(matches!(
            pending.preview_treasure_x_branch_v1(),
            Err(ManualFishingCastOwnerError::TreasureXRequiresExactLoadout)
        ));
        pending.rollback().await.unwrap();
    }
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
async fn multi_treasure_direct_book_staging_keeps_independent_typed_items_and_rolls_back() {
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
    let (count, candidates) = pending
        .preview_multi_treasure_selected_direct_books_v1()
        .unwrap();
    assert!(count > 1);
    assert_eq!(candidates.len(), usize::from(count));

    let first = pending
        .stage_multi_treasure_selected_outcome_v1()
        .await
        .unwrap();
    let PendingManualFishingMultiTreasureSelectedOutcomeV1::DirectBook { staged, durability } =
        &first
    else {
        panic!("Multi Treasure Book selection must stage typed Book deliveries");
    };
    assert_eq!(staged.deliveries.len(), candidates.len());
    for (slot, (delivery, candidate)) in staged.deliveries.iter().zip(candidates.iter()).enumerate()
    {
        assert_eq!(usize::from(delivery.candidate.treasure_slot), slot);
        assert_eq!(delivery.candidate, *candidate);
    }
    let ids = staged
        .deliveries
        .iter()
        .map(|delivery| delivery.delivery.item_instance_id)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), staged.deliveries.len());
    assert!(matches!(
        durability,
        AppliedFishingRodDurabilityState::Ordinary { .. }
    ));
    assert_eq!(
        first,
        pending
            .stage_multi_treasure_selected_outcome_v1()
            .await
            .unwrap()
    );

    pending.rollback().await.unwrap();
    assert_eq!(finished_book_count(&store, operation_id).await, 0);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap(),
        200
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM asset_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap(),
        0
    );
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
        let PendingManualFishingBaitRackSelectedOutcomeV1::DirectBook { staged, durability } =
            &first
        else {
            panic!("Bait Rack direct Book must stage a typed Book delivery");
        };
        assert_eq!(staged.deliveries.len(), 1);
        assert_eq!(staged.deliveries[0].candidate, candidate);
        assert_eq!(staged.progression.base_activity_xp, 5);
        assert!(matches!(
            durability,
            AppliedFishingRodDurabilityState::Ordinary { .. }
        ));
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
async fn school_bait_only_selected_outcome_uses_pinned_quantity_and_rolls_back() {
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
        "school-only",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    let operation_id = seed_operation(&store, player_id, nonce, "school-only").await;
    seed_active_fishing_bait(
        &store,
        player_id,
        operation_id,
        "SCHOOL",
        "fishing.bait.school",
        2,
    )
    .await;

    let tx = store.pool().begin().await.unwrap();
    let mut pending = begin_manual_fishing_cast_v1(tx, operation_id, player_id, FishingArea::River)
        .await
        .unwrap();
    assert_eq!(pending.bait_settlement().plan.consumed_bait_categories, 1);
    assert_eq!(pending.bait_settlement().plan.entries.len(), 1);
    let bait = pending.bait_settlement().plan.entries[0];
    assert_eq!(bait.units_consumed, 1);
    assert_eq!(bait.available_units_before, 2);
    assert_eq!(bait.available_units_after, 1);
    assert!(matches!(
        pending.preview_unmodified_base_branch_v1(),
        Err(ManualFishingCastOwnerError::ModifiedFishRequiresSharedComposer)
    ));
    let weights = pending.pre_cap_branch_weights();
    assert_eq!(
        (weights.fish, weights.junk, weights.treasure),
        (70_400, 6_800, 2_800)
    );

    let branch = pending.preview_school_bait_base_branch_v1().unwrap();
    assert_eq!(
        branch,
        pending.preview_school_bait_base_branch_v1().unwrap()
    );
    if branch == FishingCatchBranch::Fish {
        let quantity = pending.preview_school_bait_quantity_v1().unwrap();
        assert_eq!(quantity.chance_ppm, 80_000);
        assert_eq!(quantity, pending.preview_school_bait_quantity_v1().unwrap());
        let expected_count = if quantity.draw_ppm < quantity.chance_ppm {
            2
        } else {
            1
        };
        assert_eq!(quantity.quantity.final_fish_count(), expected_count);
        let candidates = pending.preview_school_bait_fish_candidates_v1().unwrap();
        assert_eq!(candidates.len(), usize::from(expected_count));
        for (slot, candidate) in candidates.iter().enumerate() {
            assert_eq!(usize::from(candidate.fish_slot), slot);
            assert_eq!(candidate.area, FishingArea::River);
        }
        assert_eq!(
            pending.preview_school_bait_fish_capability_v1().unwrap(),
            pending.preview_school_bait_fish_capability_v1().unwrap()
        );
    } else {
        assert!(matches!(
            pending.preview_school_bait_quantity_v1(),
            Err(ManualFishingCastOwnerError::SchoolBaitRequiresFishBranch)
        ));
    }

    if branch == FishingCatchBranch::Treasure
        && pending.preview_non_fish_candidate_v1(branch).is_err()
    {
        let candidate = pending
            .preview_school_bait_selected_direct_book_v1()
            .unwrap();
        let first = pending
            .stage_school_bait_selected_outcome_v1()
            .await
            .unwrap();
        let PendingManualFishingSchoolBaitSelectedOutcomeV1::DirectBook { staged, durability } =
            &first
        else {
            panic!("School direct Book must stage a typed Book delivery");
        };
        assert_eq!(staged.deliveries.len(), 1);
        assert_eq!(staged.deliveries[0].candidate, candidate);
        assert_eq!(staged.progression.base_activity_xp, 5);
        assert!(matches!(
            durability,
            AppliedFishingRodDurabilityState::Ordinary { .. }
        ));
        assert_eq!(
            first,
            pending
                .stage_school_bait_selected_outcome_v1()
                .await
                .unwrap()
        );
    } else {
        let first = pending
            .stage_school_bait_selected_outcome_v1()
            .await
            .unwrap();
        let replay = pending
            .stage_school_bait_selected_outcome_v1()
            .await
            .unwrap();
        assert_eq!(first, replay);
        match (branch, first) {
            (
                FishingCatchBranch::Fish,
                PendingManualFishingSchoolBaitSelectedOutcomeV1::LandedFish {
                    quantity,
                    deliveries,
                    progression,
                    durability,
                },
            ) => {
                assert_eq!(
                    deliveries.len(),
                    usize::from(quantity.quantity.final_fish_count())
                );
                assert_eq!(progression.account_xp.granted_xp, 2);
                assert!(matches!(
                    durability,
                    AppliedFishingRodDurabilityState::Ordinary { .. }
                ));
            }
            (
                FishingCatchBranch::Fish,
                PendingManualFishingSchoolBaitSelectedOutcomeV1::LineBroken {
                    quantity,
                    durability,
                },
            ) => {
                assert!((1..=2).contains(&quantity.quantity.final_fish_count()));
                let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = durability else {
                    panic!("line break must resolve on ordinary Rod");
                };
                assert_eq!(preview.resulting_durability, 0);
            }
            (
                FishingCatchBranch::Fish,
                PendingManualFishingSchoolBaitSelectedOutcomeV1::OverCapCatchPending {
                    quantity,
                    line_stage,
                },
            ) => {
                assert!((1..=2).contains(&quantity.quantity.final_fish_count()));
                assert!(matches!(
                    line_stage,
                    ManualFishingUnmodifiedLineStageV1::LineSurvivedOverCapCatchPending { .. }
                ));
            }
            (
                FishingCatchBranch::Junk | FishingCatchBranch::Treasure,
                PendingManualFishingSchoolBaitSelectedOutcomeV1::NonFish { staged, durability },
            ) => {
                assert_eq!(staged.progression.account_xp.granted_xp, 2);
                assert!(matches!(
                    durability,
                    AppliedFishingRodDurabilityState::Ordinary { .. }
                ));
            }
            (selected, staged) => {
                panic!("School selected branch/result mismatch: {selected:?} {staged:?}")
            }
        }
    }

    pending.rollback().await.unwrap();
    let bait_quantity: i64 = sqlx::query_scalar(
        "SELECT quantity FROM item_stacks WHERE player_id = $1 AND definition_key = 'fishing.bait.school' AND definition_version = 1 AND location = 'ITEM_BAG'",
    )
    .bind(player_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(bait_quantity, 2);
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
async fn school_bait_owner_rejects_gold_enchants_and_additional_consumed_bait() {
    let Some(store) = test_store().await else {
        return;
    };

    for (suffix, tier, extra_enchant, extra_bait) in [
        ("school-gold", "GOLD", false, false),
        ("school-enchant", "WOOD", true, false),
        ("school-plus-bait", "WOOD", false, true),
    ] {
        let nonce = Uuid::now_v7();
        let player_id = seed_player(&store, positive_snowflake(nonce)).await;
        set_account_level(&store, player_id, 10).await;
        let rod_id = seed_ordinary_rod(
            &store, player_id, nonce, suffix, tier, 200, 600, false, 4, 3,
        )
        .await;
        if extra_enchant {
            seed_enchant(&store, rod_id, "STRENGTHEN", 1).await;
        }
        let operation_id = seed_operation(&store, player_id, nonce, suffix).await;
        seed_active_fishing_bait(
            &store,
            player_id,
            operation_id,
            "SCHOOL",
            "fishing.bait.school",
            2,
        )
        .await;
        if extra_bait {
            seed_active_fishing_bait(
                &store,
                player_id,
                operation_id,
                "STURDY",
                "fishing.bait.sturdy",
                2,
            )
            .await;
        }

        let tx = store.pool().begin().await.unwrap();
        let pending = begin_manual_fishing_cast_v1(tx, operation_id, player_id, FishingArea::River)
            .await
            .unwrap();
        assert!(matches!(
            pending.preview_school_bait_base_branch_v1(),
            Err(ManualFishingCastOwnerError::SchoolBaitRequiresExactLoadout)
        ));
        pending.rollback().await.unwrap();
    }
}

#[tokio::test]
async fn sturdy_bait_only_selected_outcome_uses_exact_safety_effects_and_rolls_back() {
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
        "sturdy-only",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;
    let operation_id = seed_operation(&store, player_id, nonce, "sturdy-only").await;
    seed_active_fishing_bait(
        &store,
        player_id,
        operation_id,
        "STURDY",
        "fishing.bait.sturdy",
        2,
    )
    .await;

    let tx = store.pool().begin().await.unwrap();
    let mut pending = begin_manual_fishing_cast_v1(tx, operation_id, player_id, FishingArea::River)
        .await
        .unwrap();
    assert_eq!(pending.bait_settlement().plan.consumed_bait_categories, 1);
    assert_eq!(pending.bait_settlement().plan.entries.len(), 1);
    let bait = pending.bait_settlement().plan.entries[0];
    assert_eq!(bait.units_consumed, 1);
    assert_eq!(bait.available_units_before, 2);
    assert_eq!(bait.available_units_after, 1);
    assert!(matches!(
        pending.preview_unmodified_base_branch_v1(),
        Err(ManualFishingCastOwnerError::ModifiedFishRequiresSharedComposer)
    ));

    let weights = pending.pre_cap_branch_weights();
    assert_eq!(
        (weights.fish, weights.junk, weights.treasure),
        (70_400, 6_800, 2_800)
    );
    let branch = pending.preview_sturdy_bait_base_branch_v1().unwrap();
    assert_eq!(
        branch,
        pending.preview_sturdy_bait_base_branch_v1().unwrap()
    );
    let capability = pending.preview_sturdy_bait_fish_capability_v1(0).unwrap();
    assert_eq!(
        capability,
        pending.preview_sturdy_bait_fish_capability_v1(0).unwrap()
    );
    if let ManualFishingUnmodifiedCapabilityPreviewV1::OverCapPending { line_break, .. } =
        capability
    {
        assert!(line_break.sturdy_bait_active);
        assert!(line_break.final_probability_ppm <= line_break.base_probability_ppm);
    }

    if branch == FishingCatchBranch::Treasure
        && pending.preview_non_fish_candidate_v1(branch).is_err()
    {
        let candidate = pending
            .preview_sturdy_bait_selected_direct_book_v1()
            .unwrap();
        let first = pending
            .stage_sturdy_bait_selected_outcome_v1()
            .await
            .unwrap();
        let PendingManualFishingSturdyBaitSelectedOutcomeV1::DirectBook { staged, durability } =
            &first
        else {
            panic!("Sturdy direct Book must stage a typed Book delivery");
        };
        assert_eq!(staged.deliveries.len(), 1);
        assert_eq!(staged.deliveries[0].candidate, candidate);
        assert_eq!(staged.progression.base_activity_xp, 5);
        assert!(matches!(
            durability,
            AppliedFishingRodDurabilityState::Ordinary { .. }
        ));
        assert_eq!(
            first,
            pending
                .stage_sturdy_bait_selected_outcome_v1()
                .await
                .unwrap()
        );
    } else {
        let first = pending
            .stage_sturdy_bait_selected_outcome_v1()
            .await
            .unwrap();
        let replay = pending
            .stage_sturdy_bait_selected_outcome_v1()
            .await
            .unwrap();
        assert_eq!(first, replay);
        match (branch, first) {
            (
                FishingCatchBranch::Fish,
                PendingManualFishingSturdyBaitSelectedOutcomeV1::LandedFish {
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
                PendingManualFishingSturdyBaitSelectedOutcomeV1::LineBroken { durability },
            ) => {
                let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = durability else {
                    panic!("line break must resolve on ordinary Rod");
                };
                assert_eq!(preview.resulting_durability, 0);
            }
            (
                FishingCatchBranch::Fish,
                PendingManualFishingSturdyBaitSelectedOutcomeV1::OverCapCatchPending { line_stage },
            ) => {
                assert!(matches!(
                    line_stage,
                    ManualFishingUnmodifiedLineStageV1::LineSurvivedOverCapCatchPending { .. }
                ));
            }
            (
                FishingCatchBranch::Junk | FishingCatchBranch::Treasure,
                PendingManualFishingSturdyBaitSelectedOutcomeV1::NonFish { staged, durability },
            ) => {
                assert_eq!(staged.progression.account_xp.granted_xp, 2);
                assert!(matches!(
                    durability,
                    AppliedFishingRodDurabilityState::Ordinary { .. }
                ));
            }
            (selected, staged) => {
                panic!("Sturdy selected branch/result mismatch: {selected:?} {staged:?}")
            }
        }
    }

    pending.rollback().await.unwrap();
    let bait_quantity: i64 = sqlx::query_scalar(
        "SELECT quantity FROM item_stacks WHERE player_id = $1 AND definition_key = 'fishing.bait.sturdy' AND definition_version = 1 AND location = 'ITEM_BAG'",
    )
    .bind(player_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(bait_quantity, 2);
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
async fn sturdy_bait_owner_rejects_gold_enchants_and_additional_consumed_bait() {
    let Some(store) = test_store().await else {
        return;
    };

    for (suffix, tier, extra_enchant, extra_bait) in [
        ("sturdy-gold", "GOLD", false, false),
        ("sturdy-enchant", "WOOD", true, false),
        ("sturdy-plus-bait", "WOOD", false, true),
    ] {
        let nonce = Uuid::now_v7();
        let player_id = seed_player(&store, positive_snowflake(nonce)).await;
        set_account_level(&store, player_id, 10).await;
        let rod_id = seed_ordinary_rod(
            &store, player_id, nonce, suffix, tier, 200, 600, false, 4, 3,
        )
        .await;
        if extra_enchant {
            seed_enchant(&store, rod_id, "STRENGTHEN", 1).await;
        }
        let operation_id = seed_operation(&store, player_id, nonce, suffix).await;
        seed_active_fishing_bait(
            &store,
            player_id,
            operation_id,
            "STURDY",
            "fishing.bait.sturdy",
            2,
        )
        .await;
        if extra_bait {
            seed_active_fishing_bait(
                &store,
                player_id,
                operation_id,
                "TREASURE",
                "fishing.bait.treasure",
                2,
            )
            .await;
        }

        let tx = store.pool().begin().await.unwrap();
        let pending = begin_manual_fishing_cast_v1(tx, operation_id, player_id, FishingArea::River)
            .await
            .unwrap();
        assert!(matches!(
            pending.preview_sturdy_bait_base_branch_v1(),
            Err(ManualFishingCastOwnerError::SturdyBaitRequiresExactLoadout)
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
        let PendingManualFishingSharpHookSelectedOutcomeV1::DirectBook { staged, durability } =
            &first
        else {
            panic!("SharpHook direct Book must stage a typed Book delivery");
        };
        assert_eq!(staged.deliveries.len(), 1);
        assert_eq!(staged.deliveries[0].candidate, candidate);
        assert_eq!(staged.progression.base_activity_xp, 5);
        assert!(matches!(
            durability,
            AppliedFishingRodDurabilityState::Ordinary { .. }
        ));
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
        let PendingManualFishingStrengthenSelectedOutcomeV1::DirectBook { staged, durability } =
            &first
        else {
            panic!("Strengthen direct Book must stage a typed Book delivery");
        };
        assert_eq!(staged.deliveries.len(), 1);
        assert_eq!(staged.deliveries[0].candidate, candidate);
        assert_eq!(staged.progression.base_activity_xp, 5);
        assert!(matches!(
            durability,
            AppliedFishingRodDurabilityState::Ordinary { .. }
        ));
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
        let PendingManualFishingUnbreakingXSelectedOutcomeV1::DirectBook { staged, durability } =
            &first
        else {
            panic!("Unbreaking X direct Book must stage a typed Book delivery");
        };
        assert_eq!(staged.deliveries.len(), 1);
        assert_eq!(staged.deliveries[0].candidate, candidate);
        assert_eq!(staged.progression.base_activity_xp, 5);
        assert!(matches!(
            durability,
            AppliedFishingRodDurabilityState::Ordinary { .. }
        ));
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
async fn lure_x_uses_shared_speed_bucket_without_changing_baseline_catch_semantics() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    set_account_level(&store, player_id, 10).await;
    let rod_id = seed_ordinary_rod(
        &store, player_id, nonce, "lure-x", "WOOD", 200, 600, false, 4, 3,
    )
    .await;
    seed_enchant(&store, rod_id, "LURE", 10).await;
    let operation_id = seed_operation(&store, player_id, nonce, "lure-x").await;

    let tx = store.pool().begin().await.unwrap();
    let mut pending = begin_manual_fishing_cast_v1(tx, operation_id, player_id, FishingArea::River)
        .await
        .unwrap();
    assert!(matches!(
        pending.preview_unmodified_base_branch_v1(),
        Err(ManualFishingCastOwnerError::ModifiedFishRequiresSharedComposer)
    ));

    let cooldown = pending.preview_lure_x_cooldown_v1().unwrap();
    assert_eq!(cooldown.cooldown_millis, 8_696);
    assert_eq!(
        (
            cooldown.speed_policy.applied_bonus.numerator(),
            cooldown.speed_policy.applied_bonus.denominator(),
        ),
        (3, 20)
    );
    assert!(!cooldown.speed_policy.cap_applied);
    assert!(!cooldown.minimum_clamp_applied);

    let branch = pending.preview_lure_x_base_branch_v1().unwrap();
    assert_eq!(branch, pending.preview_lure_x_base_branch_v1().unwrap());
    let first = pending.stage_lure_x_selected_outcome_v1().await.unwrap();
    let replay = pending.stage_lure_x_selected_outcome_v1().await.unwrap();
    assert_eq!(first, replay);

    let staged_cooldown = match first {
        PendingManualFishingLureXSelectedOutcomeV1::LandedFish { cooldown, .. }
        | PendingManualFishingLureXSelectedOutcomeV1::NonFish { cooldown, .. }
        | PendingManualFishingLureXSelectedOutcomeV1::LineBroken { cooldown, .. }
        | PendingManualFishingLureXSelectedOutcomeV1::OverCapCatchPending { cooldown, .. }
        | PendingManualFishingLureXSelectedOutcomeV1::DirectBookPending { cooldown, .. } => {
            cooldown
        }
    };
    assert_eq!(staged_cooldown, cooldown);

    pending.rollback().await.unwrap();
    let current: i64 =
        sqlx::query_scalar("SELECT current_durability FROM item_instances WHERE id = $1")
            .bind(rod_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(current, 200);
    assert_eq!(operation_state(&store, operation_id).await, "PENDING");
}

#[tokio::test]
async fn lure_x_owner_rejects_lower_level_and_gold_combinations() {
    let Some(store) = test_store().await else {
        return;
    };
    for (suffix, tier, level) in [
        ("lure-nine", "WOOD", 9_i16),
        ("gold-lure-x", "GOLD", 10_i16),
    ] {
        let nonce = Uuid::now_v7();
        let player_id = seed_player(&store, positive_snowflake(nonce)).await;
        set_account_level(&store, player_id, 10).await;
        let rod_id = seed_ordinary_rod(
            &store, player_id, nonce, suffix, tier, 200, 600, false, 4, 3,
        )
        .await;
        seed_enchant(&store, rod_id, "LURE", level).await;
        let operation_id = seed_operation(&store, player_id, nonce, suffix).await;
        let tx = store.pool().begin().await.unwrap();
        let pending = begin_manual_fishing_cast_v1(tx, operation_id, player_id, FishingArea::River)
            .await
            .unwrap();
        assert!(matches!(
            pending.preview_lure_x_cooldown_v1(),
            Err(ManualFishingCastOwnerError::LureXRequiresExactLoadout)
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
        "INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, $5, 'PENDING', $6, $7, $8)",
    )
    .bind(operation_id)
    .bind(format!("test:manual-fishing-cast-preflight:{nonce}:{suffix}:{operation_id}"))
    .bind(discord_user_id)
    .bind(player_id)
    .bind(MANUAL_FISHING_OPERATION_KIND)
    .bind(MANUAL_FISHING_OPERATION_POLICY_VERSION)
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

async fn seed_active_fishing_bait(
    store: &PgStore,
    player_id: Uuid,
    operation_id: Uuid,
    bait_kind: &str,
    definition_key: &str,
    quantity: i64,
) {
    sqlx::query(
        "INSERT INTO item_stacks (player_id, definition_key, definition_version, location, quantity) VALUES ($1, $2, 1, 'ITEM_BAG', $3)",
    )
    .bind(player_id)
    .bind(definition_key)
    .bind(quantity)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO player_active_fishing_baits (player_id, bait_kind, definition_key, definition_version, activated_by_operation_id) VALUES ($1, $2, $3, 1, $4)",
    )
    .bind(player_id)
    .bind(bait_kind)
    .bind(definition_key)
    .bind(operation_id)
    .execute(store.pool())
    .await
    .unwrap();
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

async fn finished_book_count(store: &PgStore, operation_id: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM item_instances WHERE created_by_operation_id = $1 AND definition_key = 'enchant.book.finished'",
    )
    .bind(operation_id)
    .fetch_one(store.pool())
    .await
    .unwrap()
}

async fn fishing_bait_quantity(store: &PgStore, player_id: Uuid, definition_key: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT quantity FROM item_stacks WHERE player_id = $1 AND definition_key = $2 AND definition_version = 1 AND location = 'ITEM_BAG'",
    )
    .bind(player_id)
    .bind(definition_key)
    .fetch_one(store.pool())
    .await
    .unwrap()
}

async fn bait_settlement_event_count(store: &PgStore, operation_id: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM asset_events WHERE operation_id = $1 AND mutation_key = 'fishing:bait:cast'",
    )
    .bind(operation_id)
    .fetch_one(store.pool())
    .await
    .unwrap()
}

async fn assert_terminal_once(store: &PgStore, operation_id: Uuid) {
    assert_eq!(operation_state(store, operation_id).await, "COMMITTED");
    let terminal_events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM asset_events WHERE operation_id = $1 AND mutation_key = 'fishing:cast:terminal'",
    )
    .bind(operation_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    let outbox: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE operation_id = $1 AND topic = 'fishing.cast.completed'",
    )
    .bind(operation_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!((terminal_events, outbox), (1, 1));
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
    // UUIDv7's leading 64 bits are timestamp-heavy, so parallel tests created in
    // the same millisecond can share that prefix. Use the random tail instead.
    let raw = u64::from_be_bytes(nonce.as_bytes()[8..].try_into().unwrap());
    let value = (raw % 7_999_999_999_999_999_000_u64)
        .saturating_add(1)
        .saturating_add(offset);
    i64::try_from(value).unwrap()
}

#[test]
fn synthetic_snowflakes_distinguish_uuid_v7_random_tails() {
    let shared_prefix = 0x018f_1234_5678_7abc_u128 << 64;
    let first = Uuid::from_u128(shared_prefix | 0x8000_0000_0000_0001);
    let second = Uuid::from_u128(shared_prefix | 0x8000_0000_0000_0002);

    assert_ne!(positive_snowflake(first), positive_snowflake(second));
    assert_eq!(next_snowflake(first, 1), positive_snowflake(first) + 1);
}
