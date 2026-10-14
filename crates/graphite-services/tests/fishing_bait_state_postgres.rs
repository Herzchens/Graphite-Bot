use graphite_services::{
    FishingBait, FishingBaitCastConsumptionAction, FishingBaitStateError,
    replace_active_fishing_baits, settle_active_fishing_baits_for_cast,
};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn active_selection_is_capacity_checked_and_operation_idempotent() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    seed_starter_basic_rod(&store, player_id, nonce, "selection").await;

    let operation_id = seed_operation(&store, player_id, nonce, "select-three").await;
    let requested = [FishingBait::School, FishingBait::Quality, FishingBait::Rare];
    let mut tx = store.pool().begin().await.unwrap();
    let first = replace_active_fishing_baits(&mut tx, operation_id, player_id, &requested)
        .await
        .unwrap();
    let replay = replace_active_fishing_baits(&mut tx, operation_id, player_id, &requested)
        .await
        .unwrap();
    assert_eq!(first, replay);
    assert_eq!(first.previous.len(), 0);
    assert_eq!(first.active.len(), 3);
    sqlx::query("UPDATE operations SET state = 'COMMITTED' WHERE id = $1")
        .bind(operation_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let mut replay_tx = store.pool().begin().await.unwrap();
    assert_eq!(
        replace_active_fishing_baits(&mut replay_tx, operation_id, player_id, &requested)
            .await
            .unwrap(),
        first
    );
    replay_tx.rollback().await.unwrap();

    let over_capacity = seed_operation(&store, player_id, nonce, "select-four").await;
    let mut rejected = store.pool().begin().await.unwrap();
    assert!(matches!(
        replace_active_fishing_baits(
            &mut rejected,
            over_capacity,
            player_id,
            &[
                FishingBait::School,
                FishingBait::Quality,
                FishingBait::Rare,
                FishingBait::Treasure,
            ],
        )
        .await,
        Err(FishingBaitStateError::ActiveCategoryCapacityExceeded {
            selected: 4,
            capacity: 3,
        })
    ));
    rejected.rollback().await.unwrap();

    assert_eq!(
        active_bait_kinds(&store, player_id).await,
        vec!["QUALITY", "RARE", "SCHOOL"]
    );
}

#[tokio::test]
async fn cast_consumes_available_bait_auto_detaches_missing_and_replays_exactly_once() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    seed_starter_basic_rod(&store, player_id, nonce, "cast").await;
    replace_selection(
        &store,
        player_id,
        nonce,
        "activate",
        &[FishingBait::School, FishingBait::Treasure],
    )
    .await;
    seed_bait_stack(&store, player_id, "fishing.bait.school", 2).await;

    let cast_operation = seed_operation(&store, player_id, nonce, "cast-one").await;
    let mut tx = store.pool().begin().await.unwrap();
    let first = settle_active_fishing_baits_for_cast(&mut tx, cast_operation, player_id)
        .await
        .unwrap();
    assert_eq!(first.plan.consumed_bait_categories, 1);
    assert_eq!(first.plan.auto_detached_bait_categories, 1);
    assert_eq!(first.consumption_receipts.len(), 1);
    assert_eq!(first.auto_detached, vec![FishingBait::Treasure]);
    assert_eq!(
        first
            .plan
            .entries
            .iter()
            .find(|entry| entry.bait == FishingBait::School)
            .unwrap()
            .action,
        FishingBaitCastConsumptionAction::ConsumeForCast
    );
    assert_eq!(
        first
            .plan
            .entries
            .iter()
            .find(|entry| entry.bait == FishingBait::Treasure)
            .unwrap()
            .action,
        FishingBaitCastConsumptionAction::AutoDetachMissingUnit
    );

    let replay = settle_active_fishing_baits_for_cast(&mut tx, cast_operation, player_id)
        .await
        .unwrap();
    assert_eq!(replay, first);
    sqlx::query("UPDATE operations SET state = 'COMMITTED' WHERE id = $1")
        .bind(cast_operation)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert_eq!(
        bait_quantity(&store, player_id, "fishing.bait.school").await,
        1
    );
    assert_eq!(active_bait_kinds(&store, player_id).await, vec!["SCHOOL"]);

    let committed_replay = store.pool().begin().await.unwrap();
    let mut committed_replay = committed_replay;
    assert_eq!(
        settle_active_fishing_baits_for_cast(&mut committed_replay, cast_operation, player_id)
            .await
            .unwrap(),
        first
    );
    committed_replay.rollback().await.unwrap();
    assert_eq!(
        bait_quantity(&store, player_id, "fishing.bait.school").await,
        1
    );

    let last_unit_operation = seed_operation(&store, player_id, nonce, "cast-last-unit").await;
    let mut last_unit_tx = store.pool().begin().await.unwrap();
    let last_unit =
        settle_active_fishing_baits_for_cast(&mut last_unit_tx, last_unit_operation, player_id)
            .await
            .unwrap();
    assert_eq!(last_unit.plan.consumed_bait_categories, 1);
    assert!(last_unit.auto_detached.is_empty());
    sqlx::query("UPDATE operations SET state = 'COMMITTED' WHERE id = $1")
        .bind(last_unit_operation)
        .execute(&mut *last_unit_tx)
        .await
        .unwrap();
    last_unit_tx.commit().await.unwrap();
    assert_eq!(
        bait_quantity(&store, player_id, "fishing.bait.school").await,
        0
    );
    assert_eq!(active_bait_kinds(&store, player_id).await, vec!["SCHOOL"]);

    let detach_operation = seed_operation(&store, player_id, nonce, "cast-detach-empty").await;
    let mut detach_tx = store.pool().begin().await.unwrap();
    let detach = settle_active_fishing_baits_for_cast(&mut detach_tx, detach_operation, player_id)
        .await
        .unwrap();
    assert_eq!(detach.plan.consumed_bait_categories, 0);
    assert_eq!(detach.auto_detached, vec![FishingBait::School]);
    sqlx::query("UPDATE operations SET state = 'COMMITTED' WHERE id = $1")
        .bind(detach_operation)
        .execute(&mut *detach_tx)
        .await
        .unwrap();
    detach_tx.commit().await.unwrap();
    assert!(active_bait_kinds(&store, player_id).await.is_empty());
}

#[tokio::test]
async fn caller_rollback_restores_bait_stack_selection_and_child_audit() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    seed_starter_basic_rod(&store, player_id, nonce, "rollback").await;
    replace_selection(
        &store,
        player_id,
        nonce,
        "activate-quality",
        &[FishingBait::Quality],
    )
    .await;
    seed_bait_stack(&store, player_id, "fishing.bait.quality", 2).await;

    let operation_id = seed_operation(&store, player_id, nonce, "rollback-cast").await;
    let mut tx = store.pool().begin().await.unwrap();
    let receipt = settle_active_fishing_baits_for_cast(&mut tx, operation_id, player_id)
        .await
        .unwrap();
    assert_eq!(receipt.plan.consumed_bait_categories, 1);
    tx.rollback().await.unwrap();

    assert_eq!(
        bait_quantity(&store, player_id, "fishing.bait.quality").await,
        2
    );
    assert_eq!(active_bait_kinds(&store, player_id).await, vec!["QUALITY"]);
    assert_eq!(bait_event_count(&store, operation_id).await, 0);

    let mut retry = store.pool().begin().await.unwrap();
    let retry_receipt = settle_active_fishing_baits_for_cast(&mut retry, operation_id, player_id)
        .await
        .unwrap();
    assert_eq!(retry_receipt.plan.consumed_bait_categories, 1);
    sqlx::query("UPDATE operations SET state = 'COMMITTED' WHERE id = $1")
        .bind(operation_id)
        .execute(&mut *retry)
        .await
        .unwrap();
    retry.commit().await.unwrap();

    assert_eq!(
        bait_quantity(&store, player_id, "fishing.bait.quality").await,
        1
    );
    assert_eq!(bait_event_count(&store, operation_id).await, 2);
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
        "INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'FISHING_BAIT_STATE_TEST', 'PENDING', 1, $5, $6)",
    )
    .bind(operation_id)
    .bind(format!("test:fishing-bait-state:{nonce}:{suffix}:{operation_id}"))
    .bind(discord_user_id)
    .bind(player_id)
    .bind([101_u8; 32].as_slice())
    .bind([103_u8; 32].as_slice())
    .execute(store.pool())
    .await
    .unwrap();
    operation_id
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

async fn replace_selection(
    store: &PgStore,
    player_id: Uuid,
    nonce: Uuid,
    suffix: &str,
    requested: &[FishingBait],
) {
    let operation_id = seed_operation(store, player_id, nonce, suffix).await;
    let mut tx = store.pool().begin().await.unwrap();
    replace_active_fishing_baits(&mut tx, operation_id, player_id, requested)
        .await
        .unwrap();
    sqlx::query("UPDATE operations SET state = 'COMMITTED' WHERE id = $1")
        .bind(operation_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

async fn seed_bait_stack(store: &PgStore, player_id: Uuid, definition_key: &str, quantity: i64) {
    sqlx::query(
        "INSERT INTO item_stacks (player_id, definition_key, definition_version, location, quantity) VALUES ($1, $2, 1, 'ITEM_BAG', $3)",
    )
    .bind(player_id)
    .bind(definition_key)
    .bind(quantity)
    .execute(store.pool())
    .await
    .unwrap();
}

async fn active_bait_kinds(store: &PgStore, player_id: Uuid) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT bait_kind FROM player_active_fishing_baits WHERE player_id = $1 ORDER BY bait_kind",
    )
    .bind(player_id)
    .fetch_all(store.pool())
    .await
    .unwrap()
}

async fn bait_quantity(store: &PgStore, player_id: Uuid, definition_key: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT quantity FROM item_stacks WHERE player_id = $1 AND definition_key = $2 AND definition_version = 1 AND location = 'ITEM_BAG'",
    )
    .bind(player_id)
    .bind(definition_key)
    .fetch_optional(store.pool())
    .await
    .unwrap()
    .unwrap_or(0)
}

async fn bait_event_count(store: &PgStore, operation_id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM asset_events WHERE operation_id = $1")
        .bind(operation_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
}

fn positive_snowflake(nonce: Uuid) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    let value = (raw % 7_999_999_999_999_999_000_u64).saturating_add(1);
    i64::try_from(value).unwrap()
}
