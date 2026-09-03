use graphite_services::{PersistedSoulBindState, lock_owned_ordinary_equipment_soulbind_state};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn soulbind_snapshot_uses_typed_control_flags_and_retains_parent_lock() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let definition_key = format!("test.soulbind-control-flags.{nonce}");
    seed_definition(&store, &definition_key).await;
    let item_id = seed_item(&store, player_id, &definition_key, nonce).await;
    seed_structural_state(&store, item_id).await;

    sqlx::query(
        r#"
        UPDATE item_instances
           SET is_favorite = TRUE,
               is_protected = FALSE,
               state = '{"favorite":false,"protected":true}'::jsonb
         WHERE id = $1
        "#,
    )
    .bind(item_id)
    .execute(store.pool())
    .await
    .unwrap();

    let mut snapshot_tx = store.pool().begin().await.unwrap();
    let snapshot =
        lock_owned_ordinary_equipment_soulbind_state(&mut snapshot_tx, player_id, item_id)
            .await
            .unwrap();

    assert_eq!(snapshot.state, PersistedSoulBindState::NeverBound);
    assert!(snapshot.is_favorite);
    assert!(!snapshot.is_protected);

    let mut concurrent = store.pool().begin().await.unwrap();
    sqlx::query("SET LOCAL lock_timeout = '100ms'")
        .execute(&mut *concurrent)
        .await
        .unwrap();
    let blocked = sqlx::query("UPDATE item_instances SET is_protected = TRUE WHERE id = $1")
        .bind(item_id)
        .execute(&mut *concurrent)
        .await;
    assert!(
        blocked.is_err(),
        "the SoulBind snapshot must retain the authoritative parent ItemInstance lock"
    );
    concurrent.rollback().await.unwrap();

    snapshot_tx.rollback().await.unwrap();

    let persisted: (bool, bool) =
        sqlx::query_as("SELECT is_favorite, is_protected FROM item_instances WHERE id = $1")
            .bind(item_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(persisted, (true, false));
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

async fn seed_definition(store: &PgStore, key: &str) {
    let data = "{\"tier\":\"NETHERITE\"}";
    sqlx::query("INSERT INTO item_definitions (key, category, stackable, active, definition_version, rarity, stack_limit, data) VALUES ($1, 'PICKAXE', FALSE, TRUE, 1, 'COMMON', NULL, $2::jsonb)")
        .bind(key)
        .bind(data)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO item_definition_versions (key, version, category, stackable, rarity, stack_limit, is_ordinary_equipment, data) VALUES ($1, 1, 'PICKAXE', FALSE, 'COMMON', NULL, TRUE, $2::jsonb)")
        .bind(key)
        .bind(data)
        .execute(store.pool())
        .await
        .unwrap();
}

async fn seed_item(store: &PgStore, player_id: Uuid, definition_key: &str, nonce: Uuid) -> Uuid {
    let operation_id = Uuid::now_v7();
    let discord_user_id: i64 =
        sqlx::query_scalar("SELECT discord_user_id FROM players WHERE id = $1")
            .bind(player_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    sqlx::query("INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'SOULBIND_CONTROL_FLAGS_TEST', 'PENDING', 1, $5, $6)")
        .bind(operation_id)
        .bind(format!("test:soulbind-control-flags:{nonce}:{operation_id}"))
        .bind(discord_user_id)
        .bind(player_id)
        .bind([59_u8; 32].as_slice())
        .bind([61_u8; 32].as_slice())
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

async fn seed_structural_state(store: &PgStore, item_id: Uuid) {
    sqlx::query("INSERT INTO item_instance_equipment_structural_state (item_instance_id, creation_roll_numerator, creation_roll_denominator, upgrade_level, normal_enchant_slot_capacity, special_enchant_slot_capacity) VALUES ($1, 1, 1, 0, 4, 3)")
        .bind(item_id)
        .execute(store.pool())
        .await
        .unwrap();
}

fn positive_snowflake(nonce: Uuid) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    i64::try_from((raw % 8_000_000_000_000_000_000_u64).max(1)).unwrap()
}
