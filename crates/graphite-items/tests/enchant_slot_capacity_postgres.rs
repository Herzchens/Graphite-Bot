use graphite_store::PgStore;
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
async fn enchant_slot_capacity_is_typed_per_item_and_fails_closed_outside_frozen_bounds() {
    let Some(store) = test_store().await else {
        return;
    };

    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let equipment_key = format!("test.enchant-slot-capacity.sword.{nonce}");
    seed_definition(&store, &equipment_key).await;
    let item_id = seed_item(&store, player_id, &equipment_key, &nonce).await;

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
    .bind(item_id)
    .execute(store.pool())
    .await
    .unwrap();

    let row = sqlx::query(
        r#"
        SELECT normal_enchant_slot_capacity, special_enchant_slot_capacity
          FROM item_instance_equipment_structural_state
         WHERE item_instance_id = $1
        "#,
    )
    .bind(item_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(
        row.try_get::<i16, _>("normal_enchant_slot_capacity")
            .unwrap(),
        4
    );
    assert_eq!(
        row.try_get::<i16, _>("special_enchant_slot_capacity")
            .unwrap(),
        3
    );

    sqlx::query(
        r#"
        UPDATE item_instance_equipment_structural_state
           SET normal_enchant_slot_capacity = 6,
               special_enchant_slot_capacity = 6
         WHERE item_instance_id = $1
        "#,
    )
    .bind(item_id)
    .execute(store.pool())
    .await
    .unwrap();

    let row = sqlx::query(
        r#"
        SELECT normal_enchant_slot_capacity, special_enchant_slot_capacity
          FROM item_instance_equipment_structural_state
         WHERE item_instance_id = $1
        "#,
    )
    .bind(item_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(
        row.try_get::<i16, _>("normal_enchant_slot_capacity")
            .unwrap(),
        6
    );
    assert_eq!(
        row.try_get::<i16, _>("special_enchant_slot_capacity")
            .unwrap(),
        6
    );

    for invalid in [3_i16, 7_i16] {
        let result = sqlx::query(
            "UPDATE item_instance_equipment_structural_state SET normal_enchant_slot_capacity = $2 WHERE item_instance_id = $1",
        )
        .bind(item_id)
        .bind(invalid)
        .execute(store.pool())
        .await;
        assert!(
            result.is_err(),
            "Normal/class unlocked capacity must remain in the frozen 4..=6 range"
        );
    }

    for invalid in [2_i16, 7_i16] {
        let result = sqlx::query(
            "UPDATE item_instance_equipment_structural_state SET special_enchant_slot_capacity = $2 WHERE item_instance_id = $1",
        )
        .bind(item_id)
        .bind(invalid)
        .execute(store.pool())
        .await;
        assert!(
            result.is_err(),
            "Special/universal unlocked capacity must remain in the frozen 3..=6 range"
        );
    }

    let row = sqlx::query(
        r#"
        SELECT normal_enchant_slot_capacity, special_enchant_slot_capacity
          FROM item_instance_equipment_structural_state
         WHERE item_instance_id = $1
        "#,
    )
    .bind(item_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(
        row.try_get::<i16, _>("normal_enchant_slot_capacity")
            .unwrap(),
        6
    );
    assert_eq!(
        row.try_get::<i16, _>("special_enchant_slot_capacity")
            .unwrap(),
        6
    );
}

#[tokio::test]
async fn enchant_slot_capacity_columns_are_not_null_and_have_database_defaults() {
    let Some(store) = test_store().await else {
        return;
    };

    for column in [
        "normal_enchant_slot_capacity",
        "special_enchant_slot_capacity",
    ] {
        let row = sqlx::query(
            r#"
            SELECT is_nullable, column_default
              FROM information_schema.columns
             WHERE table_schema = 'public'
               AND table_name = 'item_instance_equipment_structural_state'
               AND column_name = $1
            "#,
        )
        .bind(column)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(row.try_get::<String, _>("is_nullable").unwrap(), "NO");
        assert!(
            row.try_get::<Option<String>, _>("column_default")
                .unwrap()
                .is_some(),
            "capacity columns must retain database defaults so historical rows and legacy explicit-column inserts receive native capacity"
        );
    }

    let constraint_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
          FROM pg_constraint c
          JOIN pg_class t ON t.oid = c.conrelid
          JOIN pg_namespace n ON n.oid = t.relnamespace
         WHERE n.nspname = 'public'
           AND t.relname = 'item_instance_equipment_structural_state'
           AND c.conname IN (
               'equipment_structural_state_normal_enchant_slot_capacity_supported',
               'equipment_structural_state_special_enchant_slot_capacity_supported'
           )
           AND c.contype = 'c'
        "#,
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(constraint_count, 2);
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
    sqlx::query(
        r#"
        INSERT INTO item_definitions (
            key, category, stackable, active, definition_version, rarity, stack_limit, data
        )
        VALUES ($1, 'SWORD', FALSE, TRUE, 1, 'COMMON', NULL, '{}'::jsonb)
        "#,
    )
    .bind(key)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO item_definition_versions (
            key, version, category, stackable, rarity, stack_limit,
            is_ordinary_equipment, data
        )
        VALUES ($1, 1, 'SWORD', FALSE, 'COMMON', NULL, TRUE, '{}'::jsonb)
        "#,
    )
    .bind(key)
    .execute(store.pool())
    .await
    .unwrap();
}

async fn seed_item(store: &PgStore, player_id: Uuid, definition_key: &str, nonce: &Uuid) -> Uuid {
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
        VALUES ($1, $2, $3, $4, 'ENCHANT_SLOT_CAPACITY_TEST', 'PENDING', 1, $5, $6)
        "#,
    )
    .bind(operation_id)
    .bind(format!("test:enchant-slot-capacity:{nonce}:{operation_id}"))
    .bind(discord_user_id)
    .bind(player_id)
    .bind([79_u8; 32].as_slice())
    .bind([83_u8; 32].as_slice())
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
