use chrono::{DateTime, Utc};
use graphite_services::{
    OrdinarySoulBindContextError, PersistedSoulBindState,
    lock_owned_ordinary_soulbind_context_for_mutation,
};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn locked_context_serializes_player_and_absent_soulbind_state() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let (player_id, item_id) = seed_equipment(&store, nonce, "never-bound", 2).await;

    let mut tx = store.pool().begin().await.unwrap();
    let context = lock_owned_ordinary_soulbind_context_for_mutation(&mut tx, player_id, item_id)
        .await
        .unwrap();
    assert_eq!(context.player_id, player_id);
    assert_eq!(context.rebirth_count, 2);
    assert_eq!(context.equipment.recraft.item_instance_id, item_id);
    assert_eq!(context.state, PersistedSoulBindState::NeverBound);

    let mut concurrent = store.pool().begin().await.unwrap();
    sqlx::query("SET LOCAL lock_timeout = '100ms'")
        .execute(&mut *concurrent)
        .await
        .unwrap();
    let rebirth_write = sqlx::query("UPDATE players SET rebirth_count = 3 WHERE id = $1")
        .bind(player_id)
        .execute(&mut *concurrent)
        .await;
    assert!(rebirth_write.is_err());
    concurrent.rollback().await.unwrap();

    let mut concurrent = store.pool().begin().await.unwrap();
    sqlx::query("SET LOCAL lock_timeout = '100ms'")
        .execute(&mut *concurrent)
        .await
        .unwrap();
    let child_insert = sqlx::query(
        "INSERT INTO item_instance_soulbind_state (item_instance_id, is_soulbound, rebind_not_before) VALUES ($1, TRUE, NULL)",
    )
    .bind(item_id)
    .execute(&mut *concurrent)
    .await;
    assert!(child_insert.is_err());
    concurrent.rollback().await.unwrap();

    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn locked_context_decodes_bound_and_unbound_cooldown_states() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let (player_id, item_id) = seed_equipment(&store, nonce, "state-shapes", 4).await;

    sqlx::query(
        "INSERT INTO item_instance_soulbind_state (item_instance_id, is_soulbound, rebind_not_before) VALUES ($1, TRUE, NULL)",
    )
    .bind(item_id)
    .execute(store.pool())
    .await
    .unwrap();

    let mut tx = store.pool().begin().await.unwrap();
    let bound = lock_owned_ordinary_soulbind_context_for_mutation(&mut tx, player_id, item_id)
        .await
        .unwrap();
    assert_eq!(bound.rebirth_count, 4);
    assert_eq!(bound.state, PersistedSoulBindState::Bound);
    tx.rollback().await.unwrap();

    let rebind_not_before = fixed_utc("2030-01-08T00:00:00Z");
    sqlx::query(
        "UPDATE item_instance_soulbind_state SET is_soulbound = FALSE, rebind_not_before = $2 WHERE item_instance_id = $1",
    )
    .bind(item_id)
    .bind(rebind_not_before)
    .execute(store.pool())
    .await
    .unwrap();

    let mut tx = store.pool().begin().await.unwrap();
    let unbound = lock_owned_ordinary_soulbind_context_for_mutation(&mut tx, player_id, item_id)
        .await
        .unwrap();
    assert_eq!(
        unbound.state,
        PersistedSoulBindState::Unbound { rebind_not_before }
    );
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn frozen_account_fails_before_soulbind_mutation_context_is_returned() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let (player_id, item_id) = seed_equipment(&store, nonce, "frozen", 1).await;
    sqlx::query("UPDATE players SET status = 'SOFT_FROZEN' WHERE id = $1")
        .bind(player_id)
        .execute(store.pool())
        .await
        .unwrap();

    let mut tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_owned_ordinary_soulbind_context_for_mutation(&mut tx, player_id, item_id).await,
        Err(OrdinarySoulBindContextError::AccountNotMutable(status)) if status == "SOFT_FROZEN"
    ));
    tx.rollback().await.unwrap();
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

async fn seed_equipment(
    store: &PgStore,
    nonce: Uuid,
    suffix: &str,
    rebirth_count: i64,
) -> (Uuid, Uuid) {
    let player_id = Uuid::now_v7();
    let discord_user_id = positive_snowflake(Uuid::now_v7());
    sqlx::query("INSERT INTO players (id, discord_user_id, rebirth_count) VALUES ($1, $2, $3)")
        .bind(player_id)
        .bind(discord_user_id)
        .bind(rebirth_count)
        .execute(store.pool())
        .await
        .unwrap();

    let definition_key = format!("test.soulbind-context.{nonce}.{suffix}");
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
    sqlx::query("INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'SOULBIND_CONTEXT_TEST', 'PENDING', 1, $5, $6)")
        .bind(operation_id)
        .bind(format!("test:soulbind-context:{nonce}:{suffix}:{operation_id}"))
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
    sqlx::query("INSERT INTO item_instance_equipment_structural_state (item_instance_id, creation_roll_numerator, creation_roll_denominator, upgrade_level) VALUES ($1, 1, 2, 0)")
        .bind(item_id)
        .execute(store.pool())
        .await
        .unwrap();

    (player_id, item_id)
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
