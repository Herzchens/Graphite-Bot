use graphite_items::{ItemError, lock_owned_item_equipment_structural_state};
use graphite_store::PgStore;
use uuid::Uuid;

const U64_MAX_DECIMAL: &str = "18446744073709551615";

#[tokio::test]
async fn structural_state_resolver_is_owner_scoped_exact_and_locks_mutable_state() {
    let Some(store) = test_store().await else {
        return;
    };

    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let other_id = seed_player(&store, positive_snowflake(Uuid::now_v7())).await;
    let ordinary_key = format!("test.structural-resolver.ordinary.{nonce}");
    let special_key = format!("test.structural-resolver.special.{nonce}");
    seed_definition(&store, &ordinary_key, "FISHING_ROD", true).await;
    seed_definition(&store, &special_key, "SWORD", false).await;

    let ordinary_item = seed_item(&store, owner_id, &ordinary_key, &nonce, "ordinary").await;
    seed_structural_state(&store, ordinary_item, "1", U64_MAX_DECIMAL, U64_MAX_DECIMAL).await;

    let special_item = seed_item(&store, owner_id, &special_key, &nonce, "special").await;
    seed_structural_state(&store, special_item, "1.0", "2.000", "3.00").await;

    let missing_state_item = seed_item(&store, owner_id, &ordinary_key, &nonce, "missing").await;

    let mut tx = store.pool().begin().await.unwrap();
    let snapshot = lock_owned_item_equipment_structural_state(&mut tx, owner_id, ordinary_item)
        .await
        .unwrap();
    assert_eq!(snapshot.item.item_instance_id, ordinary_item);
    assert_eq!(snapshot.item.owner_player_id, owner_id);
    assert_eq!(snapshot.item.definition_key, ordinary_key);
    assert_eq!(snapshot.item.definition_version, 1);
    assert!(snapshot.item.is_ordinary_equipment);
    assert_eq!(snapshot.creation_roll_numerator, 1);
    assert_eq!(snapshot.creation_roll_denominator, u64::MAX);
    assert_eq!(snapshot.upgrade_level, u64::MAX);

    let mut wrong_owner_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_owned_item_equipment_structural_state(&mut wrong_owner_tx, other_id, ordinary_item,)
            .await,
        Err(ItemError::ItemNotFound)
    ));
    wrong_owner_tx.rollback().await.unwrap();

    let mut missing_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_owned_item_equipment_structural_state(&mut missing_tx, owner_id, missing_state_item,)
            .await,
        Err(ItemError::EquipmentStructuralStateMissing)
    ));
    missing_tx.rollback().await.unwrap();

    let mut lock_probe = store.pool().begin().await.unwrap();
    let blocked = sqlx::query(
        r#"
        SELECT item_instance_id
          FROM item_instance_equipment_structural_state
         WHERE item_instance_id = $1
         FOR UPDATE NOWAIT
        "#,
    )
    .bind(ordinary_item)
    .fetch_one(&mut *lock_probe)
    .await;
    assert!(
        blocked.is_err(),
        "resolver must retain the structural-state row lock for the caller transaction"
    );
    lock_probe.rollback().await.unwrap();
    tx.rollback().await.unwrap();

    let mut after_release = store.pool().begin().await.unwrap();
    sqlx::query(
        r#"
        SELECT item_instance_id
          FROM item_instance_equipment_structural_state
         WHERE item_instance_id = $1
         FOR UPDATE NOWAIT
        "#,
    )
    .bind(ordinary_item)
    .fetch_one(&mut *after_release)
    .await
    .unwrap();
    after_release.rollback().await.unwrap();

    sqlx::query(
        r#"
        UPDATE item_instance_equipment_structural_state
           SET upgrade_level = 7
         WHERE item_instance_id = $1
        "#,
    )
    .bind(ordinary_item)
    .execute(store.pool())
    .await
    .unwrap();

    let mut refreshed_tx = store.pool().begin().await.unwrap();
    let refreshed =
        lock_owned_item_equipment_structural_state(&mut refreshed_tx, owner_id, ordinary_item)
            .await
            .unwrap();
    assert_eq!(refreshed.upgrade_level, 7);
    refreshed_tx.rollback().await.unwrap();

    let mut special_tx = store.pool().begin().await.unwrap();
    let special =
        lock_owned_item_equipment_structural_state(&mut special_tx, owner_id, special_item)
            .await
            .unwrap();
    assert!(!special.item.is_ordinary_equipment);
    assert_eq!(special.creation_roll_numerator, 1);
    assert_eq!(special.creation_roll_denominator, 2);
    assert_eq!(special.upgrade_level, 3);
    special_tx.rollback().await.unwrap();
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

async fn seed_definition(store: &PgStore, key: &str, category: &str, is_ordinary: bool) {
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
        VALUES ($1, 1, $2, FALSE, 'COMMON', NULL, $3, '{}'::jsonb)
        "#,
    )
    .bind(key)
    .bind(category)
    .bind(is_ordinary)
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
        VALUES ($1, $2, $3, $4, 'EQUIPMENT_STRUCTURAL_RESOLVER_TEST', 'PENDING', 1, $5, $6)
        "#,
    )
    .bind(operation_id)
    .bind(format!(
        "test:equipment-structural-resolver:{nonce}:{suffix}:{operation_id}"
    ))
    .bind(discord_user_id)
    .bind(player_id)
    .bind([31_u8; 32].as_slice())
    .bind([37_u8; 32].as_slice())
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

async fn seed_structural_state(
    store: &PgStore,
    item_id: Uuid,
    numerator: &str,
    denominator: &str,
    upgrade_level: &str,
) {
    sqlx::query(
        r#"
        INSERT INTO item_instance_equipment_structural_state (
            item_instance_id,
            creation_roll_numerator,
            creation_roll_denominator,
            upgrade_level
        )
        VALUES ($1, $2::NUMERIC, $3::NUMERIC, $4::NUMERIC)
        "#,
    )
    .bind(item_id)
    .bind(numerator)
    .bind(denominator)
    .bind(upgrade_level)
    .execute(store.pool())
    .await
    .unwrap();
}

fn positive_snowflake(nonce: Uuid) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    i64::try_from((raw % 8_000_000_000_000_000_000_u64).max(1)).unwrap()
}
