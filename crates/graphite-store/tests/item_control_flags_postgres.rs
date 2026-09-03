use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn item_control_flags_are_typed_independent_authoritative_state() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let item_id = seed_item(&store, &nonce, "typed").await;

    let initial: (bool, bool, bool) = sqlx::query_as(
        "SELECT is_favorite, is_protected, is_account_bound FROM item_instances WHERE id = $1",
    )
    .bind(item_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(initial, (false, false, false));

    sqlx::query(
        r#"UPDATE item_instances
              SET state = '{"favorite":true,"protected":true}'::jsonb
            WHERE id = $1"#,
    )
    .bind(item_id)
    .execute(store.pool())
    .await
    .unwrap();

    let after_json: (bool, bool) =
        sqlx::query_as("SELECT is_favorite, is_protected FROM item_instances WHERE id = $1")
            .bind(item_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(after_json, (false, false));

    sqlx::query("UPDATE item_instances SET is_favorite = TRUE WHERE id = $1")
        .bind(item_id)
        .execute(store.pool())
        .await
        .unwrap();
    let favorite_only: (bool, bool) =
        sqlx::query_as("SELECT is_favorite, is_protected FROM item_instances WHERE id = $1")
            .bind(item_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(favorite_only, (true, false));

    sqlx::query("UPDATE item_instances SET is_protected = TRUE WHERE id = $1")
        .bind(item_id)
        .execute(store.pool())
        .await
        .unwrap();
    let both: (bool, bool, bool) = sqlx::query_as(
        "SELECT is_favorite, is_protected, is_account_bound FROM item_instances WHERE id = $1",
    )
    .bind(item_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(both, (true, true, false));
}

#[tokio::test]
async fn item_control_flags_cannot_be_null() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let favorite_item = seed_item(&store, &nonce, "favorite-null").await;
    let protected_item = seed_item(&store, &nonce, "protected-null").await;

    let favorite_null = sqlx::query("UPDATE item_instances SET is_favorite = NULL WHERE id = $1")
        .bind(favorite_item)
        .execute(store.pool())
        .await;
    assert!(favorite_null.is_err());

    let protected_null = sqlx::query("UPDATE item_instances SET is_protected = NULL WHERE id = $1")
        .bind(protected_item)
        .execute(store.pool())
        .await;
    assert!(protected_null.is_err());

    let favorite_state: (bool, bool) =
        sqlx::query_as("SELECT is_favorite, is_protected FROM item_instances WHERE id = $1")
            .bind(favorite_item)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(favorite_state, (false, false));

    let protected_state: (bool, bool) =
        sqlx::query_as("SELECT is_favorite, is_protected FROM item_instances WHERE id = $1")
            .bind(protected_item)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(protected_state, (false, false));
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

async fn seed_item(store: &PgStore, nonce: &Uuid, suffix: &str) -> Uuid {
    let player_id = Uuid::now_v7();
    let discord_user_id = positive_snowflake(Uuid::now_v7());
    sqlx::query("INSERT INTO players (id, discord_user_id) VALUES ($1, $2)")
        .bind(player_id)
        .bind(discord_user_id)
        .execute(store.pool())
        .await
        .unwrap();

    let definition_key = format!("test.item-control-flags.{nonce}.{suffix}");
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
    sqlx::query("INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'ITEM_CONTROL_FLAGS_TEST', 'PENDING', 1, $5, $6)")
        .bind(operation_id)
        .bind(format!("test:item-control-flags:{nonce}:{suffix}:{operation_id}"))
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
