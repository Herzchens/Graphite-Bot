use graphite_services::{
    FishingArea, FishingRodDurabilityResolution, apply_resolved_equipped_fishing_rod_durability,
};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn durability_write_rolls_back_with_the_owning_cast_transaction() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let item_id = seed_ordinary_rod(&store, player_id, nonce, 5, 600).await;
    let operation_id = seed_operation(&store, player_id, nonce, "rollback").await;

    let mut tx = store.pool().begin().await.unwrap();
    apply_resolved_equipped_fishing_rod_durability(
        &mut tx,
        operation_id,
        player_id,
        FishingArea::River,
        Some(5),
        FishingRodDurabilityResolution::CompletedCastAttempt {
            ordinary_event_prevented_by_unbreaking: false,
        },
    )
    .await
    .unwrap();
    let inside: (i64, bool) =
        sqlx::query_as("SELECT current_durability, is_broken FROM item_instances WHERE id = $1")
            .bind(item_id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(inside, (4, false));

    tx.rollback().await.unwrap();
    assert_eq!(rod_durability(&store, item_id).await, (5, false));
}

#[tokio::test]
async fn authoritative_durability_resolution_retains_operation_player_item_and_slot_locks() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let item_id = seed_ordinary_rod(&store, player_id, nonce, 7, 600).await;
    let operation_id = seed_operation(&store, player_id, nonce, "locks").await;

    let mut owner_tx = store.pool().begin().await.unwrap();
    apply_resolved_equipped_fishing_rod_durability(
        &mut owner_tx,
        operation_id,
        player_id,
        FishingArea::Lake,
        Some(7),
        FishingRodDurabilityResolution::CompletedCastAttempt {
            ordinary_event_prevented_by_unbreaking: true,
        },
    )
    .await
    .unwrap();

    let mut operation_tx = store.pool().begin().await.unwrap();
    sqlx::query("SET LOCAL lock_timeout = '100ms'")
        .execute(&mut *operation_tx)
        .await
        .unwrap();
    let operation_error =
        sqlx::query("UPDATE operations SET error_code = error_code WHERE id = $1")
            .bind(operation_id)
            .execute(&mut *operation_tx)
            .await
            .unwrap_err();
    assert!(is_lock_timeout(&operation_error));
    operation_tx.rollback().await.unwrap();

    let mut player_tx = store.pool().begin().await.unwrap();
    sqlx::query("SET LOCAL lock_timeout = '100ms'")
        .execute(&mut *player_tx)
        .await
        .unwrap();
    let player_error = sqlx::query("UPDATE players SET status = status WHERE id = $1")
        .bind(player_id)
        .execute(&mut *player_tx)
        .await
        .unwrap_err();
    assert!(is_lock_timeout(&player_error));
    player_tx.rollback().await.unwrap();

    let mut item_tx = store.pool().begin().await.unwrap();
    sqlx::query("SET LOCAL lock_timeout = '100ms'")
        .execute(&mut *item_tx)
        .await
        .unwrap();
    let item_error = sqlx::query("UPDATE item_instances SET state = state WHERE id = $1")
        .bind(item_id)
        .execute(&mut *item_tx)
        .await
        .unwrap_err();
    assert!(is_lock_timeout(&item_error));
    item_tx.rollback().await.unwrap();

    let mut slot_tx = store.pool().begin().await.unwrap();
    sqlx::query("SET LOCAL lock_timeout = '100ms'")
        .execute(&mut *slot_tx)
        .await
        .unwrap();
    let slot_error = sqlx::query(
        "UPDATE equipment_slots SET item_instance_id = item_instance_id WHERE player_id = $1 AND slot = 'FISHING_ROD'",
    )
    .bind(player_id)
    .execute(&mut *slot_tx)
    .await
    .unwrap_err();
    assert!(is_lock_timeout(&slot_error));
    slot_tx.rollback().await.unwrap();

    owner_tx.rollback().await.unwrap();
    assert_eq!(rod_durability(&store, item_id).await, (7, false));
}

fn is_lock_timeout(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|database_error| database_error.code())
        .is_some_and(|code| code == "55P03")
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
        "INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'FISHING_ROD_DURABILITY_TX_TEST', 'PENDING', 1, $5, $6)",
    )
    .bind(operation_id)
    .bind(format!("test:fishing-rod-durability-tx:{nonce}:{suffix}:{operation_id}"))
    .bind(discord_user_id)
    .bind(player_id)
    .bind([61_u8; 32].as_slice())
    .bind([67_u8; 32].as_slice())
    .execute(store.pool())
    .await
    .unwrap();
    operation_id
}

async fn seed_ordinary_rod(
    store: &PgStore,
    player_id: Uuid,
    nonce: Uuid,
    current_durability: i64,
    max_durability: i64,
) -> Uuid {
    let definition_key = format!("test.fishing-rod-durability.tx.{nonce}");
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

    let creation_operation = seed_operation(store, player_id, nonce, "create").await;
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

async fn rod_durability(store: &PgStore, item_id: Uuid) -> (i64, bool) {
    sqlx::query_as("SELECT current_durability, is_broken FROM item_instances WHERE id = $1")
        .bind(item_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
}

fn positive_snowflake(nonce: Uuid) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    let value = (raw % 7_999_999_999_999_999_000_u64).saturating_add(1);
    i64::try_from(value).unwrap()
}
