use graphite_items::{ItemError, lock_owned_item_ordinary_equipment_classification};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn ordinary_equipment_classification_resolver_is_version_pinned_owner_scoped_and_locks_the_item()
 {
    let Some(store) = test_store().await else {
        return;
    };

    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let other_id = seed_player(&store, positive_snowflake(Uuid::now_v7())).await;
    let definition_key = format!("test.classification.rod.{nonce}");

    sqlx::query(
        r#"
        INSERT INTO item_definitions (
            key, category, stackable, active, definition_version, rarity, stack_limit, data
        )
        VALUES ($1, 'FISHING_ROD', FALSE, TRUE, 2, 'COMMON', NULL, '{}'::jsonb)
        "#,
    )
    .bind(&definition_key)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO item_definition_versions (
            key, version, category, stackable, rarity, stack_limit,
            is_ordinary_equipment, data
        )
        VALUES
            ($1, 1, 'FISHING_ROD', FALSE, 'COMMON', NULL, TRUE, '{}'::jsonb),
            ($1, 2, 'FISHING_ROD', FALSE, 'COMMON', NULL, FALSE, '{}'::jsonb)
        "#,
    )
    .bind(&definition_key)
    .execute(store.pool())
    .await
    .unwrap();

    let old_item_id = seed_item(&store, owner_id, &definition_key, 1, &nonce, "old").await;
    let current_item_id = seed_item(&store, owner_id, &definition_key, 2, &nonce, "current").await;

    let mut tx = store.pool().begin().await.unwrap();
    let old = lock_owned_item_ordinary_equipment_classification(&mut tx, owner_id, old_item_id)
        .await
        .unwrap();
    assert_eq!(old.item_instance_id, old_item_id);
    assert_eq!(old.owner_player_id, owner_id);
    assert_eq!(old.definition_key, definition_key);
    assert_eq!(old.definition_version, 1);
    assert!(old.is_ordinary_equipment);

    let mut wrong_owner_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_owned_item_ordinary_equipment_classification(
            &mut wrong_owner_tx,
            other_id,
            old_item_id
        )
        .await,
        Err(ItemError::ItemNotFound)
    ));
    wrong_owner_tx.rollback().await.unwrap();

    let mut lock_probe = store.pool().begin().await.unwrap();
    let blocked = sqlx::query("SELECT id FROM item_instances WHERE id = $1 FOR UPDATE NOWAIT")
        .bind(old_item_id)
        .fetch_one(&mut *lock_probe)
        .await;
    assert!(
        blocked.is_err(),
        "resolver must retain a row lock on the ItemInstance for the caller transaction"
    );
    lock_probe.rollback().await.unwrap();
    tx.rollback().await.unwrap();

    let mut after_release = store.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM item_instances WHERE id = $1 FOR UPDATE NOWAIT")
        .bind(old_item_id)
        .fetch_one(&mut *after_release)
        .await
        .unwrap();
    after_release.rollback().await.unwrap();

    let mut current_tx = store.pool().begin().await.unwrap();
    let current = lock_owned_item_ordinary_equipment_classification(
        &mut current_tx,
        owner_id,
        current_item_id,
    )
    .await
    .unwrap();
    assert_eq!(current.definition_version, 2);
    assert!(
        !current.is_ordinary_equipment,
        "same-category current version must not be inferred ordinary when its explicit flag is false"
    );
    current_tx.rollback().await.unwrap();
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

async fn seed_item(
    store: &PgStore,
    player_id: Uuid,
    definition_key: &str,
    definition_version: i32,
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
        VALUES ($1, $2, $3, $4, 'ITEM_CLASSIFICATION_TEST', 'PENDING', 1, $5, $6)
        "#,
    )
    .bind(operation_id)
    .bind(format!(
        "test:item-classification:{nonce}:{suffix}:{operation_id}"
    ))
    .bind(discord_user_id)
    .bind(player_id)
    .bind([17_u8; 32].as_slice())
    .bind([19_u8; 32].as_slice())
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
        VALUES ($1, $2, $3, $4, 'TOOL_LOCKER', $5)
        "#,
    )
    .bind(item_id)
    .bind(definition_key)
    .bind(player_id)
    .bind(operation_id)
    .bind(definition_version)
    .execute(store.pool())
    .await
    .unwrap();
    item_id
}

fn positive_snowflake(nonce: Uuid) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    i64::try_from((raw % 8_000_000_000_000_000_000_u64).max(1)).unwrap()
}
