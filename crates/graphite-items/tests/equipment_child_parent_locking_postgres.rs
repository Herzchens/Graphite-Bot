use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn deferred_equipment_child_validation_serializes_on_parent_item() {
    let Some(store) = test_store().await else {
        return;
    };

    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let equipment_key = format!("test.equipment-parent-lock.sword.{nonce}");
    let material_key = format!("test.equipment-parent-lock.material.{nonce}");
    seed_definition(&store, &equipment_key, "SWORD").await;
    seed_definition(&store, &material_key, "MATERIAL").await;

    let structural_item = seed_item(&store, player_id, &equipment_key, &nonce, "structural").await;
    let mut structural_tx = store.pool().begin().await.unwrap();
    sqlx::query(
        r#"
        INSERT INTO item_instance_equipment_structural_state (
            item_instance_id,
            creation_roll_numerator,
            creation_roll_denominator,
            upgrade_level
        )
        VALUES ($1, 1, 2, 0)
        "#,
    )
    .bind(structural_item)
    .execute(&mut *structural_tx)
    .await
    .unwrap();
    sqlx::query("SET CONSTRAINTS equipment_structural_state_write_consistency IMMEDIATE")
        .execute(&mut *structural_tx)
        .await
        .unwrap();

    let mut repin_tx = store.pool().begin().await.unwrap();
    sqlx::query("SET LOCAL lock_timeout = '100ms'")
        .execute(&mut *repin_tx)
        .await
        .unwrap();
    let repin = sqlx::query(
        r#"
        UPDATE item_instances
           SET definition_key = $2,
               definition_version = 1
         WHERE id = $1
        "#,
    )
    .bind(structural_item)
    .bind(&material_key)
    .execute(&mut *repin_tx)
    .await;
    assert!(
        repin.is_err(),
        "validated structural child state must hold the parent ItemInstance row lock"
    );
    repin_tx.rollback().await.unwrap();
    structural_tx.rollback().await.unwrap();

    let enchanted_item = seed_item(&store, player_id, &equipment_key, &nonce, "enchant").await;
    let mut enchant_tx = store.pool().begin().await.unwrap();
    sqlx::query(
        r#"
        INSERT INTO item_instance_embedded_enchants (item_instance_id, enchant_key, level)
        VALUES ($1, 'SHARPNESS', 5)
        "#,
    )
    .bind(enchanted_item)
    .execute(&mut *enchant_tx)
    .await
    .unwrap();
    sqlx::query("SET CONSTRAINTS embedded_enchant_write_consistency IMMEDIATE")
        .execute(&mut *enchant_tx)
        .await
        .unwrap();

    let mut disable_tx = store.pool().begin().await.unwrap();
    sqlx::query("SET LOCAL lock_timeout = '100ms'")
        .execute(&mut *disable_tx)
        .await
        .unwrap();
    let disable = sqlx::query("UPDATE item_instances SET is_enchantable = FALSE WHERE id = $1")
        .bind(enchanted_item)
        .execute(&mut *disable_tx)
        .await;
    assert!(
        disable.is_err(),
        "validated embedded enchant state must hold the parent ItemInstance row lock"
    );
    disable_tx.rollback().await.unwrap();
    enchant_tx.rollback().await.unwrap();
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
        VALUES ($1, $2, $3, $4, 'EQUIPMENT_PARENT_LOCK_TEST', 'PENDING', 1, $5, $6)
        "#,
    )
    .bind(operation_id)
    .bind(format!(
        "test:equipment-parent-lock:{nonce}:{suffix}:{operation_id}"
    ))
    .bind(discord_user_id)
    .bind(player_id)
    .bind([71_u8; 32].as_slice())
    .bind([73_u8; 32].as_slice())
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
