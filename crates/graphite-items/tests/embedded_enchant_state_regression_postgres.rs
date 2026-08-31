use graphite_store::PgStore;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

#[tokio::test]
async fn embedded_enchant_key_length_boundaries_are_enforced() {
    let Some(store) = test_store().await else {
        return;
    };

    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let equipment_key = format!("test.embedded-enchant.boundary.sword.{nonce}");
    seed_definition(&store, &equipment_key, "SWORD").await;

    let accepted_item = seed_item(&store, player_id, &equipment_key, &nonce, "accepted").await;
    let accepted_key = "A".repeat(64);
    sqlx::query(
        "INSERT INTO item_instance_embedded_enchants (item_instance_id, enchant_key, level) VALUES ($1, $2, 1)",
    )
    .bind(accepted_item)
    .bind(&accepted_key)
    .execute(store.pool())
    .await
    .unwrap();

    let rejected_item = seed_item(&store, player_id, &equipment_key, &nonce, "rejected").await;
    let rejected_key = "B".repeat(65);
    let result = sqlx::query(
        "INSERT INTO item_instance_embedded_enchants (item_instance_id, enchant_key, level) VALUES ($1, $2, 1)",
    )
    .bind(rejected_item)
    .bind(&rejected_key)
    .execute(store.pool())
    .await;
    assert!(
        result.is_err(),
        "the persisted raw enchant key bound must fail closed above 64 characters"
    );
}

#[tokio::test]
async fn deferred_parent_validation_rolls_back_the_whole_transaction() {
    let Some(store) = test_store().await else {
        return;
    };

    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let equipment_key = format!("test.embedded-enchant.rollback.rod.{nonce}");
    seed_definition(&store, &equipment_key, "FISHING_ROD").await;
    let item_id = seed_item(&store, player_id, &equipment_key, &nonce, "rollback").await;

    let mut tx = store.pool().begin().await.unwrap();
    insert_enchant_tx(&mut tx, item_id, "LURE", 3).await;
    sqlx::query("UPDATE item_instances SET is_enchantable = FALSE WHERE id = $1")
        .bind(item_id)
        .execute(&mut *tx)
        .await
        .unwrap();

    let commit = tx.commit().await;
    assert!(
        commit.is_err(),
        "the deferred invariant must reject an enchanted item that becomes non-enchantable before commit"
    );

    let enchant_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM item_instance_embedded_enchants WHERE item_instance_id = $1",
    )
    .bind(item_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(
        enchant_count, 0,
        "failed commit must not persist the child row"
    );

    let is_enchantable: bool =
        sqlx::query_scalar("SELECT is_enchantable FROM item_instances WHERE id = $1")
            .bind(item_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert!(
        is_enchantable,
        "failed deferred validation must roll back the parent mutation as well"
    );
}

async fn insert_enchant_tx(
    tx: &mut Transaction<'_, Postgres>,
    item_id: Uuid,
    enchant_key: &str,
    level: i16,
) {
    sqlx::query(
        "INSERT INTO item_instance_embedded_enchants (item_instance_id, enchant_key, level) VALUES ($1, $2, $3)",
    )
    .bind(item_id)
    .bind(enchant_key)
    .bind(level)
    .execute(&mut **tx)
    .await
    .unwrap();
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

async fn seed_definition(store: &PgStore, key: &str, category: &str) {
    sqlx::query(
        r#"
        INSERT INTO item_definitions (
            key, category, stackable, active, definition_version, rarity, stack_limit, data
        )
        VALUES ($1, $2, FALSE, TRUE, 1, 'COMMON', NULL, '{}'::jsonb)
        "#,
    )
    .bind(key)
    .bind(category)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO item_definition_versions (
            key, version, category, stackable, rarity, stack_limit,
            is_ordinary_equipment, data
        )
        VALUES ($1, 1, $2, FALSE, 'COMMON', NULL, FALSE, '{}'::jsonb)
        "#,
    )
    .bind(key)
    .bind(category)
    .execute(store.pool())
    .await
    .unwrap();
}

async fn seed_item(
    store: &PgStore,
    player_id: Uuid,
    definition_key: &str,
    nonce: &Uuid,
    suffix: &str,
) -> Uuid {
    let operation_id = Uuid::now_v7();
    let discord_user_id: i64 =
        sqlx::query_scalar("SELECT discord_user_id FROM players WHERE id = $1")
            .bind(player_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    sqlx::query(
        r#"
        INSERT INTO operations (
            id, external_request_key, actor_discord_user_id, player_id, kind, state,
            policy_version, request_hash, rng_root
        )
        VALUES ($1, $2, $3, $4, 'EMBEDDED_ENCHANT_STATE_REGRESSION_TEST', 'PENDING', 1, $5, $6)
        "#,
    )
    .bind(operation_id)
    .bind(format!(
        "test:embedded-enchant-state-regression:{nonce}:{suffix}:{operation_id}"
    ))
    .bind(discord_user_id)
    .bind(player_id)
    .bind([61_u8; 32].as_slice())
    .bind([67_u8; 32].as_slice())
    .execute(store.pool())
    .await
    .unwrap();

    let item_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO item_instances (
            id, definition_key, owner_player_id, created_by_operation_id,
            location, definition_version
        )
        VALUES ($1, $2, $3, $4, 'TOOL_LOCKER', 1)
        "#,
    )
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
