use chrono::{DateTime, Utc};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn soulbind_state_persists_only_bound_or_cooldown_shapes() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let bound_item = seed_equipment_item(&store, &nonce, "bound").await;
    let cooldown_item = seed_equipment_item(&store, &nonce, "cooldown").await;
    let expired_item = seed_equipment_item(&store, &nonce, "expired").await;
    let invalid_bound_item = seed_equipment_item(&store, &nonce, "invalid-bound").await;
    let invalid_unbound_item = seed_equipment_item(&store, &nonce, "invalid-unbound").await;

    sqlx::query(
        "INSERT INTO item_instance_soulbind_state (item_instance_id, is_soulbound, rebind_not_before) VALUES ($1, TRUE, NULL)",
    )
    .bind(bound_item)
    .execute(store.pool())
    .await
    .unwrap();

    let future = fixed_utc("2030-01-01T00:00:00Z");
    sqlx::query(
        "INSERT INTO item_instance_soulbind_state (item_instance_id, is_soulbound, rebind_not_before) VALUES ($1, FALSE, $2)",
    )
    .bind(cooldown_item)
    .bind(future)
    .execute(store.pool())
    .await
    .unwrap();

    let past = fixed_utc("2020-01-01T00:00:00Z");
    sqlx::query(
        "INSERT INTO item_instance_soulbind_state (item_instance_id, is_soulbound, rebind_not_before) VALUES ($1, FALSE, $2)",
    )
    .bind(expired_item)
    .bind(past)
    .execute(store.pool())
    .await
    .unwrap();

    let invalid_bound = sqlx::query(
        "INSERT INTO item_instance_soulbind_state (item_instance_id, is_soulbound, rebind_not_before) VALUES ($1, TRUE, $2)",
    )
    .bind(invalid_bound_item)
    .bind(future)
    .execute(store.pool())
    .await;
    assert!(invalid_bound.is_err());

    let invalid_unbound = sqlx::query(
        "INSERT INTO item_instance_soulbind_state (item_instance_id, is_soulbound, rebind_not_before) VALUES ($1, FALSE, NULL)",
    )
    .bind(invalid_unbound_item)
    .execute(store.pool())
    .await;
    assert!(invalid_unbound.is_err());

    let bound: (bool, Option<DateTime<Utc>>) = sqlx::query_as(
        "SELECT is_soulbound, rebind_not_before FROM item_instance_soulbind_state WHERE item_instance_id = $1",
    )
    .bind(bound_item)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(bound, (true, None));

    let cooldown: (bool, Option<DateTime<Utc>>) = sqlx::query_as(
        "SELECT is_soulbound, rebind_not_before FROM item_instance_soulbind_state WHERE item_instance_id = $1",
    )
    .bind(cooldown_item)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(cooldown, (false, Some(future)));

    let expired: (bool, Option<DateTime<Utc>>) = sqlx::query_as(
        "SELECT is_soulbound, rebind_not_before FROM item_instance_soulbind_state WHERE item_instance_id = $1",
    )
    .bind(expired_item)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(expired, (false, Some(past)));
}

#[tokio::test]
async fn soulbind_state_identity_is_immutable_and_parent_delete_cascades() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let first_item = seed_equipment_item(&store, &nonce, "identity-a").await;
    let second_item = seed_equipment_item(&store, &nonce, "identity-b").await;

    sqlx::query(
        "INSERT INTO item_instance_soulbind_state (item_instance_id, is_soulbound, rebind_not_before) VALUES ($1, TRUE, NULL)",
    )
    .bind(first_item)
    .execute(store.pool())
    .await
    .unwrap();

    let moved = sqlx::query(
        "UPDATE item_instance_soulbind_state SET item_instance_id = $2 WHERE item_instance_id = $1",
    )
    .bind(first_item)
    .bind(second_item)
    .execute(store.pool())
    .await;
    assert!(moved.is_err());

    let still_first: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM item_instance_soulbind_state WHERE item_instance_id = $1",
    )
    .bind(first_item)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(still_first, 1);

    sqlx::query("DELETE FROM item_instances WHERE id = $1")
        .bind(first_item)
        .execute(store.pool())
        .await
        .unwrap();

    let after_delete: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM item_instance_soulbind_state WHERE item_instance_id = $1",
    )
    .bind(first_item)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(after_delete, 0);
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

async fn seed_equipment_item(store: &PgStore, nonce: &Uuid, suffix: &str) -> Uuid {
    let player_id = Uuid::now_v7();
    let discord_user_id = positive_snowflake(Uuid::now_v7());
    sqlx::query("INSERT INTO players (id, discord_user_id) VALUES ($1, $2)")
        .bind(player_id)
        .bind(discord_user_id)
        .execute(store.pool())
        .await
        .unwrap();

    let definition_key = format!("test.soulbind-state.{nonce}.{suffix}");
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
    sqlx::query("INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'SOULBIND_STATE_TEST', 'PENDING', 1, $5, $6)")
        .bind(operation_id)
        .bind(format!("test:soulbind-state:{nonce}:{suffix}:{operation_id}"))
        .bind(discord_user_id)
        .bind(player_id)
        .bind([41_u8; 32].as_slice())
        .bind([43_u8; 32].as_slice())
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

fn fixed_utc(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&Utc)
}

fn positive_snowflake(nonce: Uuid) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    i64::try_from((raw % 8_000_000_000_000_000_000_u64).max(1)).unwrap()
}
