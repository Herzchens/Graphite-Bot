use graphite_services::{
    CanonicalEnchant, EnchantRemovalStateWriterError, RemovedEmbeddedEnchant,
    write_exact_enchant_removal_after_removability_check,
};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn exact_removal_deletes_only_the_selected_locked_enchant() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let item_id = seed_ordinary_sword(&store, owner_id, &nonce, "exact").await;
    seed_enchant(&store, item_id, "SHARPNESS", 4).await;
    seed_enchant(&store, item_id, "UNBREAKING", 2).await;

    let mut tx = store.pool().begin().await.unwrap();
    let removed = write_exact_enchant_removal_after_removability_check(
        &mut tx,
        owner_id,
        item_id,
        CanonicalEnchant::Sharpness,
        4,
    )
    .await
    .unwrap();
    assert_eq!(
        removed,
        RemovedEmbeddedEnchant {
            enchant: CanonicalEnchant::Sharpness,
            level: 4,
        }
    );
    tx.commit().await.unwrap();

    let rows: Vec<(String, i16)> = sqlx::query_as(
        "SELECT enchant_key, level FROM item_instance_embedded_enchants WHERE item_instance_id = $1 ORDER BY enchant_key",
    )
    .bind(item_id)
    .fetch_all(store.pool())
    .await
    .unwrap();
    assert_eq!(rows, vec![("UNBREAKING".to_owned(), 2)]);
}

#[tokio::test]
async fn caller_rollback_restores_the_removed_enchant_row() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let item_id = seed_ordinary_sword(&store, owner_id, &nonce, "rollback").await;
    seed_enchant(&store, item_id, "SHARPNESS", 6).await;

    let mut tx = store.pool().begin().await.unwrap();
    write_exact_enchant_removal_after_removability_check(
        &mut tx,
        owner_id,
        item_id,
        CanonicalEnchant::Sharpness,
        6,
    )
    .await
    .unwrap();
    let count_inside: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM item_instance_embedded_enchants WHERE item_instance_id = $1 AND enchant_key = 'SHARPNESS'",
    )
    .bind(item_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(count_inside, 0);
    tx.rollback().await.unwrap();

    let restored_level: i16 = sqlx::query_scalar(
        "SELECT level FROM item_instance_embedded_enchants WHERE item_instance_id = $1 AND enchant_key = 'SHARPNESS'",
    )
    .bind(item_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(restored_level, 6);
}

#[tokio::test]
async fn stale_expected_level_fails_before_removal() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let item_id = seed_ordinary_sword(&store, owner_id, &nonce, "stale").await;
    seed_enchant(&store, item_id, "SHARPNESS", 5).await;

    let mut tx = store.pool().begin().await.unwrap();
    let result = write_exact_enchant_removal_after_removability_check(
        &mut tx,
        owner_id,
        item_id,
        CanonicalEnchant::Sharpness,
        4,
    )
    .await;
    assert!(matches!(
        result,
        Err(
            EnchantRemovalStateWriterError::SelectedEnchantLevelChanged {
                enchant: CanonicalEnchant::Sharpness,
                expected_level: 4,
                actual_level: 5,
            }
        )
    ));
    let persisted_level: i16 = sqlx::query_scalar(
        "SELECT level FROM item_instance_embedded_enchants WHERE item_instance_id = $1 AND enchant_key = 'SHARPNESS'",
    )
    .bind(item_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(persisted_level, 5);
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn absent_selected_identity_fails_without_touching_sibling_state() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let item_id = seed_ordinary_sword(&store, owner_id, &nonce, "missing").await;
    seed_enchant(&store, item_id, "UNBREAKING", 3).await;

    let mut tx = store.pool().begin().await.unwrap();
    let result = write_exact_enchant_removal_after_removability_check(
        &mut tx,
        owner_id,
        item_id,
        CanonicalEnchant::Sharpness,
        3,
    )
    .await;
    assert!(matches!(
        result,
        Err(EnchantRemovalStateWriterError::SelectedEnchantNotFound(
            CanonicalEnchant::Sharpness
        ))
    ));
    let rows: Vec<(String, i16)> = sqlx::query_as(
        "SELECT enchant_key, level FROM item_instance_embedded_enchants WHERE item_instance_id = $1 ORDER BY enchant_key",
    )
    .bind(item_id)
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    assert_eq!(rows, vec![("UNBREAKING".to_owned(), 3)]);
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn malformed_sibling_identity_fails_closed_before_selected_removal() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let item_id = seed_ordinary_sword(&store, owner_id, &nonce, "malformed").await;
    seed_enchant(&store, item_id, "SHARPNESS", 5).await;
    seed_enchant(&store, item_id, "unbreaking", 1).await;

    let mut tx = store.pool().begin().await.unwrap();
    let result = write_exact_enchant_removal_after_removability_check(
        &mut tx,
        owner_id,
        item_id,
        CanonicalEnchant::Sharpness,
        5,
    )
    .await;
    assert!(matches!(
        result,
        Err(EnchantRemovalStateWriterError::Enhanced(_))
    ));
    let selected_level: i16 = sqlx::query_scalar(
        "SELECT level FROM item_instance_embedded_enchants WHERE item_instance_id = $1 AND enchant_key = 'SHARPNESS'",
    )
    .bind(item_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(selected_level, 5);
    tx.rollback().await.unwrap();
}

async fn test_store() -> Option<PgStore> {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
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

async fn seed_ordinary_sword(store: &PgStore, player_id: Uuid, nonce: &Uuid, suffix: &str) -> Uuid {
    let definition_key = format!("test.enchant-remove-writer.sword.{nonce}.{suffix}");
    let data = r#"{"tier":"OBSIDIAN"}"#;
    sqlx::query("INSERT INTO item_definitions (key, category, stackable, active, definition_version, rarity, stack_limit, data) VALUES ($1, 'SWORD', FALSE, TRUE, 1, 'COMMON', NULL, $2::jsonb)")
        .bind(&definition_key)
        .bind(data)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO item_definition_versions (key, version, category, stackable, rarity, stack_limit, is_ordinary_equipment, data) VALUES ($1, 1, 'SWORD', FALSE, 'COMMON', NULL, TRUE, $2::jsonb)")
        .bind(&definition_key)
        .bind(data)
        .execute(store.pool())
        .await
        .unwrap();

    let discord_user_id: i64 =
        sqlx::query_scalar("SELECT discord_user_id FROM players WHERE id = $1")
            .bind(player_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let operation_id = Uuid::now_v7();
    sqlx::query("INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'ENCHANT_REMOVE_STATE_WRITER_TEST', 'PENDING', 1, $5, $6)")
        .bind(operation_id)
        .bind(format!("test:enchant-remove-state-writer:{nonce}:{suffix}:{operation_id}"))
        .bind(discord_user_id)
        .bind(player_id)
        .bind([139_u8; 32].as_slice())
        .bind([149_u8; 32].as_slice())
        .execute(store.pool())
        .await
        .unwrap();

    let item_id = Uuid::now_v7();
    sqlx::query("INSERT INTO item_instances (id, definition_key, owner_player_id, created_by_operation_id, location, definition_version) VALUES ($1, $2, $3, $4, 'TOOL_LOCKER', 1)")
        .bind(item_id)
        .bind(&definition_key)
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
    item_id
}

async fn seed_enchant(store: &PgStore, item_id: Uuid, key: &str, level: i16) {
    sqlx::query("INSERT INTO item_instance_embedded_enchants (item_instance_id, enchant_key, level) VALUES ($1, $2, $3)")
        .bind(item_id)
        .bind(key)
        .bind(level)
        .execute(store.pool())
        .await
        .unwrap();
}

fn positive_snowflake(nonce: Uuid) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    i64::try_from((raw % 8_000_000_000_000_000_000_u64).max(1)).unwrap()
}
