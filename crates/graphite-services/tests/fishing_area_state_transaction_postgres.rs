use graphite_progression::account_total_xp_for_level;
use graphite_services::{
    FishingArea, FishingAreaAccessError, FishingAreaAccessOrigin,
    lock_or_grant_fishing_area_first_unlock,
};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn successful_first_unlock_rolls_back_with_its_owner_transaction() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let (player_id, _) = seed_eligible_river_player(&store, nonce).await;
    let operation_id = seed_operation(&store, player_id, nonce, "rollback").await;

    let mut tx = store.pool().begin().await.unwrap();
    let access = lock_or_grant_fishing_area_first_unlock(
        &mut tx,
        operation_id,
        player_id,
        FishingArea::River,
    )
    .await
    .unwrap();
    assert_eq!(access.origin, FishingAreaAccessOrigin::NewlyUnlocked);
    assert_eq!(access.granted_by_operation_id, Some(operation_id));
    assert_eq!(unlock_count(&store, player_id).await, 0);

    tx.rollback().await.unwrap();
    assert_eq!(unlock_count(&store, player_id).await, 0);
}

#[tokio::test]
async fn concurrent_first_unlocks_serialize_on_the_authoritative_player_lock() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let (player_id, _) = seed_eligible_river_player(&store, nonce).await;
    let first_operation = seed_operation(&store, player_id, nonce, "first").await;
    let second_operation = seed_operation(&store, player_id, nonce, "second").await;

    let mut first_tx = store.pool().begin().await.unwrap();
    let first = lock_or_grant_fishing_area_first_unlock(
        &mut first_tx,
        first_operation,
        player_id,
        FishingArea::River,
    )
    .await
    .unwrap();
    assert_eq!(first.origin, FishingAreaAccessOrigin::NewlyUnlocked);

    let mut competing_tx = store.pool().begin().await.unwrap();
    sqlx::query("SET LOCAL lock_timeout = '100ms'")
        .execute(&mut *competing_tx)
        .await
        .unwrap();
    assert!(matches!(
        lock_or_grant_fishing_area_first_unlock(
            &mut competing_tx,
            second_operation,
            player_id,
            FishingArea::River,
        )
        .await,
        Err(FishingAreaAccessError::Database(_))
    ));
    competing_tx.rollback().await.unwrap();

    first_tx.commit().await.unwrap();
    assert_eq!(unlock_count(&store, player_id).await, 1);

    let mut retry_tx = store.pool().begin().await.unwrap();
    let retry = lock_or_grant_fishing_area_first_unlock(
        &mut retry_tx,
        second_operation,
        player_id,
        FishingArea::River,
    )
    .await
    .unwrap();
    assert_eq!(retry.origin, FishingAreaAccessOrigin::Persisted);
    assert_eq!(retry.granted_by_operation_id, Some(first_operation));
    assert_eq!(retry.first_unlock_preview, None);
    retry_tx.commit().await.unwrap();
    assert_eq!(unlock_count(&store, player_id).await, 1);
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

async fn seed_eligible_river_player(store: &PgStore, nonce: Uuid) -> (Uuid, Uuid) {
    let player_id = Uuid::now_v7();
    sqlx::query("INSERT INTO players (id, discord_user_id) VALUES ($1, $2)")
        .bind(player_id)
        .bind(positive_snowflake(nonce))
        .execute(store.pool())
        .await
        .unwrap();

    let account_xp = account_total_xp_for_level(10).unwrap();
    sqlx::query(
        "UPDATE player_progression SET account_xp = $1, updated_at = now() WHERE player_id = $2",
    )
    .bind(account_xp)
    .bind(player_id)
    .execute(store.pool())
    .await
    .unwrap();

    let definition_key = format!("test.fishing-area.tx.wood.{nonce}");
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
        "INSERT INTO item_definition_versions (key, version, category, stackable, rarity, stack_limit, is_ordinary_equipment, data) VALUES ($1, 1, 'FISHING_ROD', FALSE, 'COMMON', NULL, TRUE, $2)",
    )
    .bind(&definition_key)
    .bind(data)
    .execute(store.pool())
    .await
    .unwrap();

    let creation_operation = seed_operation(store, player_id, nonce, "rod").await;
    let item_id = Uuid::now_v7();
    let mut tx = store.pool().begin().await.unwrap();
    sqlx::query(
        "INSERT INTO item_instances (id, definition_key, owner_player_id, created_by_operation_id, location, definition_version) VALUES ($1, $2, $3, $4, 'EQUIPPED', 1)",
    )
    .bind(item_id)
    .bind(&definition_key)
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

    (player_id, item_id)
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
        "INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'FISHING_AREA_UNLOCK_TEST', 'PENDING', 1, $5, $6)",
    )
    .bind(operation_id)
    .bind(format!("test:fishing-area-tx:{nonce}:{suffix}:{operation_id}"))
    .bind(discord_user_id)
    .bind(player_id)
    .bind([41_u8; 32].as_slice())
    .bind([43_u8; 32].as_slice())
    .execute(store.pool())
    .await
    .unwrap();
    operation_id
}

async fn unlock_count(store: &PgStore, player_id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM player_fishing_area_unlocks WHERE player_id = $1")
        .bind(player_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
}

fn positive_snowflake(nonce: Uuid) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    let value = (raw % 7_999_999_999_999_999_000_u64).saturating_add(1);
    i64::try_from(value).unwrap()
}
