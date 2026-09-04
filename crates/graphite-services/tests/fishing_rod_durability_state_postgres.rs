use graphite_services::{
    AppliedFishingRodDurabilityState, FishingArea, FishingRodDurabilityConsequence,
    FishingRodDurabilityResolution, FishingRodDurabilityStateError,
    apply_resolved_equipped_fishing_rod_durability,
};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn ordinary_completed_cast_consumes_exactly_one_durability_and_keeps_operation_pending() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let item_id = seed_ordinary_rod(&store, player_id, nonce, "wear", 3, 600, true).await;
    let operation_id = seed_operation(&store, player_id, nonce, "wear-cast").await;

    let mut tx = store.pool().begin().await.unwrap();
    let applied = apply_resolved_equipped_fishing_rod_durability(
        &mut tx,
        operation_id,
        player_id,
        FishingArea::River,
        Some(3),
        FishingRodDurabilityResolution::CompletedCastAttempt {
            ordinary_event_prevented_by_unbreaking: false,
        },
    )
    .await
    .unwrap();
    match applied {
        AppliedFishingRodDurabilityState::Ordinary {
            item_instance_id,
            preview,
            ..
        } => {
            assert_eq!(item_instance_id, item_id);
            assert_eq!(preview.current_durability, 3);
            assert_eq!(preview.resulting_durability, 2);
            assert_eq!(
                preview.consequence,
                FishingRodDurabilityConsequence::OrdinaryWearApplied
            );
        }
        other => panic!("unexpected state result: {other:?}"),
    }
    tx.commit().await.unwrap();

    assert_eq!(
        rod_durability(&store, item_id).await,
        (Some(2), Some(600), false)
    );
    assert_eq!(operation_state(&store, operation_id).await, "PENDING");
}

#[tokio::test]
async fn authoritative_unbreaking_prevention_is_a_locked_noop() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let item_id = seed_ordinary_rod(&store, player_id, nonce, "unbreaking", 17, 600, true).await;
    let operation_id = seed_operation(&store, player_id, nonce, "unbreaking-cast").await;

    let mut tx = store.pool().begin().await.unwrap();
    let applied = apply_resolved_equipped_fishing_rod_durability(
        &mut tx,
        operation_id,
        player_id,
        FishingArea::Lake,
        Some(17),
        FishingRodDurabilityResolution::CompletedCastAttempt {
            ordinary_event_prevented_by_unbreaking: true,
        },
    )
    .await
    .unwrap();
    let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = applied else {
        panic!("ordinary Rod returned Starter state");
    };
    assert_eq!(preview.resulting_durability, 17);
    assert_eq!(
        preview.consequence,
        FishingRodDurabilityConsequence::OrdinaryWearPreventedByUnbreaking
    );
    tx.commit().await.unwrap();

    assert_eq!(
        rod_durability(&store, item_id).await,
        (Some(17), Some(600), false)
    );
}

#[tokio::test]
async fn line_break_and_last_normal_wear_both_persist_broken_state() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();

    let line_player = seed_player(&store, positive_snowflake(nonce)).await;
    let line_item = seed_ordinary_rod(&store, line_player, nonce, "line", 240, 600, true).await;
    let line_operation = seed_operation(&store, line_player, nonce, "line-cast").await;
    let mut line_tx = store.pool().begin().await.unwrap();
    let line = apply_resolved_equipped_fishing_rod_durability(
        &mut line_tx,
        line_operation,
        line_player,
        FishingArea::DeepSea,
        Some(240),
        FishingRodDurabilityResolution::LineBreak,
    )
    .await
    .unwrap();
    let AppliedFishingRodDurabilityState::Ordinary { preview, .. } = line else {
        panic!("ordinary Rod returned Starter state");
    };
    assert_eq!(preview.resulting_durability, 0);
    assert_eq!(
        preview.consequence,
        FishingRodDurabilityConsequence::LineBreakDestroyedRod
    );
    line_tx.commit().await.unwrap();
    assert_eq!(
        rod_durability(&store, line_item).await,
        (Some(0), Some(600), true)
    );

    let last_player = seed_player(&store, next_snowflake(nonce, 1)).await;
    let last_item = seed_ordinary_rod(&store, last_player, nonce, "last", 1, 600, true).await;
    let last_operation = seed_operation(&store, last_player, nonce, "last-cast").await;
    let mut last_tx = store.pool().begin().await.unwrap();
    apply_resolved_equipped_fishing_rod_durability(
        &mut last_tx,
        last_operation,
        last_player,
        FishingArea::Coast,
        Some(1),
        FishingRodDurabilityResolution::CompletedCastAttempt {
            ordinary_event_prevented_by_unbreaking: false,
        },
    )
    .await
    .unwrap();
    last_tx.commit().await.unwrap();
    assert_eq!(
        rod_durability(&store, last_item).await,
        (Some(0), Some(600), true)
    );
}

#[tokio::test]
async fn starter_basic_is_unbreakable_pool_only_and_pool_rejects_line_break_for_every_rod() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let starter_player = seed_player(&store, positive_snowflake(nonce)).await;
    let starter_item = seed_starter_basic_rod(&store, starter_player, nonce, "starter").await;

    let normal_operation = seed_operation(&store, starter_player, nonce, "starter-normal").await;
    let mut normal_tx = store.pool().begin().await.unwrap();
    let applied = apply_resolved_equipped_fishing_rod_durability(
        &mut normal_tx,
        normal_operation,
        starter_player,
        FishingArea::StarterPool,
        None,
        FishingRodDurabilityResolution::CompletedCastAttempt {
            ordinary_event_prevented_by_unbreaking: false,
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        applied,
        AppliedFishingRodDurabilityState::StarterBasicUnbreakable { item_instance_id, .. }
            if item_instance_id == starter_item
    ));
    normal_tx.commit().await.unwrap();
    assert_eq!(
        rod_durability(&store, starter_item).await,
        (None, None, false)
    );

    let outside_operation = seed_operation(&store, starter_player, nonce, "starter-outside").await;
    let mut outside_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        apply_resolved_equipped_fishing_rod_durability(
            &mut outside_tx,
            outside_operation,
            starter_player,
            FishingArea::River,
            None,
            FishingRodDurabilityResolution::CompletedCastAttempt {
                ordinary_event_prevented_by_unbreaking: false,
            },
        )
        .await,
        Err(FishingRodDurabilityStateError::StarterBasicOutsideStarterPool)
    ));
    outside_tx.rollback().await.unwrap();

    let ordinary_player = seed_player(&store, next_snowflake(nonce, 1)).await;
    let ordinary_item =
        seed_ordinary_rod(&store, ordinary_player, nonce, "pool-line", 20, 600, true).await;
    let line_operation = seed_operation(&store, ordinary_player, nonce, "pool-line").await;
    let mut line_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        apply_resolved_equipped_fishing_rod_durability(
            &mut line_tx,
            line_operation,
            ordinary_player,
            FishingArea::StarterPool,
            Some(20),
            FishingRodDurabilityResolution::LineBreak,
        )
        .await,
        Err(FishingRodDurabilityStateError::LineBreakDisabledInStarterPool)
    ));
    line_tx.rollback().await.unwrap();
    assert_eq!(
        rod_durability(&store, ordinary_item).await,
        (Some(20), Some(600), false)
    );
}

#[tokio::test]
async fn stale_expected_durability_and_already_broken_rods_fail_closed() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let item_id = seed_ordinary_rod(&store, player_id, nonce, "stale", 9, 600, true).await;
    let stale_operation = seed_operation(&store, player_id, nonce, "stale-cast").await;
    let mut stale_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        apply_resolved_equipped_fishing_rod_durability(
            &mut stale_tx,
            stale_operation,
            player_id,
            FishingArea::River,
            Some(10),
            FishingRodDurabilityResolution::CompletedCastAttempt {
                ordinary_event_prevented_by_unbreaking: false,
            },
        )
        .await,
        Err(FishingRodDurabilityStateError::DurabilityChanged {
            expected: 10,
            actual: 9
        })
    ));
    stale_tx.rollback().await.unwrap();
    assert_eq!(
        rod_durability(&store, item_id).await,
        (Some(9), Some(600), false)
    );

    sqlx::query("UPDATE item_instances SET current_durability = 0, is_broken = TRUE WHERE id = $1")
        .bind(item_id)
        .execute(store.pool())
        .await
        .unwrap();
    let broken_operation = seed_operation(&store, player_id, nonce, "broken-cast").await;
    let mut broken_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        apply_resolved_equipped_fishing_rod_durability(
            &mut broken_tx,
            broken_operation,
            player_id,
            FishingArea::River,
            Some(0),
            FishingRodDurabilityResolution::CompletedCastAttempt {
                ordinary_event_prevented_by_unbreaking: false,
            },
        )
        .await,
        Err(FishingRodDurabilityStateError::OrdinaryRodAlreadyBroken)
    ));
    broken_tx.rollback().await.unwrap();
}

#[tokio::test]
async fn special_rods_and_operation_or_account_authority_mismatches_fail_closed() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();

    let special_player = seed_player(&store, positive_snowflake(nonce)).await;
    let _special_item =
        seed_ordinary_rod(&store, special_player, nonce, "special", 30, 600, false).await;
    let special_operation = seed_operation(&store, special_player, nonce, "special-cast").await;
    let mut special_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        apply_resolved_equipped_fishing_rod_durability(
            &mut special_tx,
            special_operation,
            special_player,
            FishingArea::River,
            Some(30),
            FishingRodDurabilityResolution::CompletedCastAttempt {
                ordinary_event_prevented_by_unbreaking: false,
            },
        )
        .await,
        Err(FishingRodDurabilityStateError::NonOrdinaryFishingRod)
    ));
    special_tx.rollback().await.unwrap();

    let owner_player = seed_player(&store, next_snowflake(nonce, 1)).await;
    let _owner_item = seed_ordinary_rod(&store, owner_player, nonce, "owner", 30, 600, true).await;
    let other_player = seed_player(&store, next_snowflake(nonce, 2)).await;
    let mismatch_operation = seed_operation(&store, other_player, nonce, "mismatch").await;
    let mut mismatch_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        apply_resolved_equipped_fishing_rod_durability(
            &mut mismatch_tx,
            mismatch_operation,
            owner_player,
            FishingArea::River,
            Some(30),
            FishingRodDurabilityResolution::CompletedCastAttempt {
                ordinary_event_prevented_by_unbreaking: false,
            },
        )
        .await,
        Err(FishingRodDurabilityStateError::OperationPlayerMismatch)
    ));
    mismatch_tx.rollback().await.unwrap();

    let terminal_operation = seed_operation(&store, owner_player, nonce, "terminal").await;
    sqlx::query("UPDATE operations SET state = 'FAILED' WHERE id = $1")
        .bind(terminal_operation)
        .execute(store.pool())
        .await
        .unwrap();
    let mut terminal_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        apply_resolved_equipped_fishing_rod_durability(
            &mut terminal_tx,
            terminal_operation,
            owner_player,
            FishingArea::River,
            Some(30),
            FishingRodDurabilityResolution::CompletedCastAttempt {
                ordinary_event_prevented_by_unbreaking: false,
            },
        )
        .await,
        Err(FishingRodDurabilityStateError::OperationTerminal(ref state)) if state == "FAILED"
    ));
    terminal_tx.rollback().await.unwrap();

    sqlx::query("UPDATE players SET status = 'SOFT_FROZEN' WHERE id = $1")
        .bind(owner_player)
        .execute(store.pool())
        .await
        .unwrap();
    let frozen_operation = seed_operation(&store, owner_player, nonce, "frozen").await;
    let mut frozen_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        apply_resolved_equipped_fishing_rod_durability(
            &mut frozen_tx,
            frozen_operation,
            owner_player,
            FishingArea::River,
            Some(30),
            FishingRodDurabilityResolution::CompletedCastAttempt {
                ordinary_event_prevented_by_unbreaking: false,
            },
        )
        .await,
        Err(FishingRodDurabilityStateError::AccountFrozen(ref status)) if status == "SOFT_FROZEN"
    ));
    frozen_tx.rollback().await.unwrap();
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
        "INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'FISHING_ROD_DURABILITY_TEST', 'PENDING', 1, $5, $6)",
    )
    .bind(operation_id)
    .bind(format!("test:fishing-rod-durability:{nonce}:{suffix}:{operation_id}"))
    .bind(discord_user_id)
    .bind(player_id)
    .bind([53_u8; 32].as_slice())
    .bind([59_u8; 32].as_slice())
    .execute(store.pool())
    .await
    .unwrap();
    operation_id
}

async fn seed_ordinary_rod(
    store: &PgStore,
    player_id: Uuid,
    nonce: Uuid,
    suffix: &str,
    current_durability: i64,
    max_durability: i64,
    ordinary: bool,
) -> Uuid {
    let definition_key = format!("test.fishing-rod-durability.{suffix}.{nonce}");
    let data = serde_json::json!({"tier": "WOOD"});
    sqlx::query(
        "INSERT INTO item_definitions (key, category, stackable, active, definition_version, rarity, stack_limit, data) VALUES ($1, 'FISHING_ROD', FALSE, TRUE, 1, 'COMMON', NULL, $2)",
    )
    .bind(&definition_key)
    .bind(&data)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO item_definition_versions (key, version, category, stackable, rarity, stack_limit, is_ordinary_equipment, data) VALUES ($1, 1, 'FISHING_ROD', FALSE, 'COMMON', NULL, $2, $3)",
    )
    .bind(&definition_key)
    .bind(ordinary)
    .bind(data)
    .execute(store.pool())
    .await
    .unwrap();

    let creation_operation =
        seed_operation(store, player_id, nonce, &format!("create-{suffix}")).await;
    let item_id = Uuid::now_v7();
    let mut tx = store.pool().begin().await.unwrap();
    sqlx::query(
        "INSERT INTO item_instances (id, definition_key, owner_player_id, created_by_operation_id, location, definition_version, current_durability, max_durability) VALUES ($1, $2, $3, $4, 'EQUIPPED', 1, $5, $6)",
    )
    .bind(item_id)
    .bind(&definition_key)
    .bind(player_id)
    .bind(creation_operation)
    .bind(current_durability)
    .bind(max_durability)
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

async fn rod_durability(store: &PgStore, item_id: Uuid) -> (Option<i64>, Option<i64>, bool) {
    sqlx::query_as(
        "SELECT current_durability, max_durability, is_broken FROM item_instances WHERE id = $1",
    )
    .bind(item_id)
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
