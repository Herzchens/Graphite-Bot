use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn soulbind_child_insert_and_update_serialize_through_parent_item_lock() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let item_id = seed_item(&store, nonce).await;

    let mut parent_tx = store.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM item_instances WHERE id = $1 FOR UPDATE")
        .bind(item_id)
        .fetch_one(&mut *parent_tx)
        .await
        .unwrap();

    let mut child_tx = store.pool().begin().await.unwrap();
    sqlx::query("SET LOCAL lock_timeout = '100ms'")
        .execute(&mut *child_tx)
        .await
        .unwrap();
    let blocked_insert = sqlx::query(
        "INSERT INTO item_instance_soulbind_state (item_instance_id, is_soulbound, rebind_not_before) VALUES ($1, TRUE, NULL)",
    )
    .bind(item_id)
    .execute(&mut *child_tx)
    .await;
    assert!(blocked_insert.is_err());
    child_tx.rollback().await.unwrap();
    parent_tx.rollback().await.unwrap();

    sqlx::query(
        "INSERT INTO item_instance_soulbind_state (item_instance_id, is_soulbound, rebind_not_before) VALUES ($1, TRUE, NULL)",
    )
    .bind(item_id)
    .execute(store.pool())
    .await
    .unwrap();

    let mut parent_tx = store.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM item_instances WHERE id = $1 FOR UPDATE")
        .bind(item_id)
        .fetch_one(&mut *parent_tx)
        .await
        .unwrap();

    let mut child_tx = store.pool().begin().await.unwrap();
    sqlx::query("SET LOCAL lock_timeout = '100ms'")
        .execute(&mut *child_tx)
        .await
        .unwrap();
    let blocked_update = sqlx::query(
        "UPDATE item_instance_soulbind_state SET is_soulbound = FALSE, rebind_not_before = '2030-01-01T00:00:00Z'::timestamptz WHERE item_instance_id = $1",
    )
    .bind(item_id)
    .execute(&mut *child_tx)
    .await;
    assert!(blocked_update.is_err());
    child_tx.rollback().await.unwrap();
    parent_tx.rollback().await.unwrap();

    let persisted: (bool, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as(
        "SELECT is_soulbound, rebind_not_before FROM item_instance_soulbind_state WHERE item_instance_id = $1",
    )
    .bind(item_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(persisted, (true, None));
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

async fn seed_item(store: &PgStore, nonce: Uuid) -> Uuid {
    let player_id = Uuid::now_v7();
    let discord_user_id = positive_snowflake(Uuid::now_v7());
    sqlx::query("INSERT INTO players (id, discord_user_id) VALUES ($1, $2)")
        .bind(player_id)
        .bind(discord_user_id)
        .execute(store.pool())
        .await
        .unwrap();

    let definition_key = format!("test.soulbind-parent-locking.{nonce}");
    let definition_data = r#"{"tier":"NETHERITE"}"#;
    sqlx::query("INSERT INTO item_definitions (key, category, stackable, active, definition_version, rarity, stack_limit, data) VALUES ($1, 'PICKAXE', FALSE, TRUE, 1, 'COMMON', NULL, $2::jsonb)")
        .bind(&definition_key)
        .bind(definition_data)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO item_definition_versions (key, version, category, stackable, rarity, stack_limit, is_ordinary_equipment, data) VALUES ($1, 1, 'PICKAXE', FALSE, 'COMMON', NULL, TRUE, $2::jsonb)")
        .bind(&definition_key)
        .bind(definition_data)
        .execute(store.pool())
        .await
        .unwrap();

    let operation_id = Uuid::now_v7();
    sqlx::query("INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'SOULBIND_PARENT_LOCK_TEST', 'PENDING', 1, $5, $6)")
        .bind(operation_id)
        .bind(format!("test:soulbind-parent-lock:{nonce}:{operation_id}"))
        .bind(discord_user_id)
        .bind(player_id)
        .bind([47_u8; 32].as_slice())
        .bind([53_u8; 32].as_slice())
        .execute(store.pool())
        .await
        .unwrap();

    let item_id = Uuid::now_v7();
    sqlx::query("INSERT INTO item_instances (id, definition_key, owner_player_id, created_by_operation_id, location, definition_version) VALUES ($1, $2, $3, $4, 'TOOL_LOCKER', 1)")
        .bind(item_id)
        .bind(definition_key)
        .bind(player_id)
        .bind(operation_id)
        .execute(store.pool())
        .await
        .unwrap();
    item_id
}

fn positive_snowflake(nonce: Uuid) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    i64::try_from((raw % 8_000_000_000_000_000_000_u64).max(1)).unwrap()
}
