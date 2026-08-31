use graphite_store::PgStore;
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
async fn embedded_enchant_state_preserves_identity_level_mutation_and_removal() {
    let Some(store) = test_store().await else {
        return;
    };

    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let equipment_key = format!("test.embedded-enchant.sword.{nonce}");
    seed_definition(&store, &equipment_key, "SWORD").await;
    let item_id = seed_item(&store, player_id, &equipment_key, &nonce, "equipment").await;

    insert_enchant(&store, item_id, "SHARPNESS", 5)
        .await
        .unwrap();
    insert_enchant(&store, item_id, "STABILIZE", 10)
        .await
        .unwrap();

    let rows = sqlx::query(
        r#"
        SELECT enchant_key, level
          FROM item_instance_embedded_enchants
         WHERE item_instance_id = $1
         ORDER BY enchant_key
        "#,
    )
    .bind(item_id)
    .fetch_all(store.pool())
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].try_get::<String, _>("enchant_key").unwrap(),
        "SHARPNESS"
    );
    assert_eq!(rows[0].try_get::<i16, _>("level").unwrap(), 5);
    assert_eq!(
        rows[1].try_get::<String, _>("enchant_key").unwrap(),
        "STABILIZE"
    );
    assert_eq!(rows[1].try_get::<i16, _>("level").unwrap(), 10);

    assert!(
        insert_enchant(&store, item_id, "SHARPNESS", 6)
            .await
            .is_err(),
        "one ItemInstance cannot carry two rows for the same enchant identity"
    );

    let rekey_result = sqlx::query(
        r#"
        UPDATE item_instance_embedded_enchants
           SET enchant_key = 'SPARKLING'
         WHERE item_instance_id = $1
           AND enchant_key = 'STABILIZE'
        "#,
    )
    .bind(item_id)
    .execute(store.pool())
    .await;
    assert!(
        rekey_result.is_err(),
        "embedded enchant identity changes must use explicit remove/apply transitions"
    );

    let move_target = seed_item(&store, player_id, &equipment_key, &nonce, "move-target").await;
    let move_result = sqlx::query(
        r#"
        UPDATE item_instance_embedded_enchants
           SET item_instance_id = $2
         WHERE item_instance_id = $1
           AND enchant_key = 'STABILIZE'
        "#,
    )
    .bind(item_id)
    .bind(move_target)
    .execute(store.pool())
    .await;
    assert!(
        move_result.is_err(),
        "embedded enchant rows cannot be transferred between ItemInstances"
    );

    sqlx::query(
        r#"
        UPDATE item_instance_embedded_enchants
           SET level = 9
         WHERE item_instance_id = $1
           AND enchant_key = 'STABILIZE'
        "#,
    )
    .bind(item_id)
    .execute(store.pool())
    .await
    .unwrap();
    let stabilize_level: i16 = sqlx::query_scalar(
        r#"
        SELECT level
          FROM item_instance_embedded_enchants
         WHERE item_instance_id = $1
           AND enchant_key = 'STABILIZE'
        "#,
    )
    .bind(item_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(
        stabilize_level, 9,
        "persistence must allow canonical level decay"
    );

    sqlx::query(
        r#"
        DELETE FROM item_instance_embedded_enchants
         WHERE item_instance_id = $1
           AND enchant_key = 'SHARPNESS'
        "#,
    )
    .bind(item_id)
    .execute(store.pool())
    .await
    .unwrap();
    let remaining: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM item_instance_embedded_enchants WHERE item_instance_id = $1",
    )
    .bind(item_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(remaining, 1, "enchant removal is a valid state transition");

    sqlx::query("DELETE FROM item_instances WHERE id = $1")
        .bind(item_id)
        .execute(store.pool())
        .await
        .unwrap();
    let after_parent_delete: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM item_instance_embedded_enchants WHERE item_instance_id = $1",
    )
    .bind(item_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(
        after_parent_delete, 0,
        "ItemInstance deletion must cascade enchant state"
    );
}

#[tokio::test]
async fn embedded_enchant_state_rejects_invalid_levels_and_parent_shapes() {
    let Some(store) = test_store().await else {
        return;
    };

    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let equipment_key = format!("test.embedded-enchant.rod.{nonce}");
    let material_key = format!("test.embedded-enchant.material.{nonce}");
    seed_definition(&store, &equipment_key, "FISHING_ROD").await;
    seed_definition(&store, &material_key, "MATERIAL").await;

    let invalid_level_zero =
        seed_item(&store, player_id, &equipment_key, &nonce, "level-zero").await;
    assert!(
        insert_enchant(&store, invalid_level_zero, "LURE", 0)
            .await
            .is_err()
    );

    let invalid_level_eleven =
        seed_item(&store, player_id, &equipment_key, &nonce, "level-eleven").await;
    assert!(
        insert_enchant(&store, invalid_level_eleven, "LURE", 11)
            .await
            .is_err()
    );

    let empty_key = seed_item(&store, player_id, &equipment_key, &nonce, "empty-key").await;
    assert!(insert_enchant(&store, empty_key, "", 1).await.is_err());

    let padded_key = seed_item(&store, player_id, &equipment_key, &nonce, "padded-key").await;
    assert!(
        insert_enchant(&store, padded_key, " LURE ", 1)
            .await
            .is_err()
    );

    let material_item = seed_item(&store, player_id, &material_key, &nonce, "material").await;
    assert!(
        insert_enchant(&store, material_item, "LURE", 1)
            .await
            .is_err(),
        "non-equipment ItemInstances cannot carry embedded enchants"
    );

    let non_enchantable =
        seed_item(&store, player_id, &equipment_key, &nonce, "non-enchantable").await;
    sqlx::query("UPDATE item_instances SET is_enchantable = FALSE WHERE id = $1")
        .bind(non_enchantable)
        .execute(store.pool())
        .await
        .unwrap();
    assert!(
        insert_enchant(&store, non_enchantable, "LURE", 1)
            .await
            .is_err(),
        "non-enchantable ItemInstances cannot carry embedded enchants"
    );

    let starter = seed_item(&store, player_id, &equipment_key, &nonce, "starter").await;
    sqlx::query("UPDATE item_instances SET is_starter = TRUE WHERE id = $1")
        .bind(starter)
        .execute(store.pool())
        .await
        .unwrap();
    assert!(
        insert_enchant(&store, starter, "LURE", 1).await.is_err(),
        "starter equipment remains non-enchantable by lifecycle invariant"
    );
    sqlx::query("UPDATE item_instances SET is_starter = FALSE WHERE id = $1")
        .bind(starter)
        .execute(store.pool())
        .await
        .unwrap();

    let repin = seed_item(&store, player_id, &equipment_key, &nonce, "repin").await;
    insert_enchant(&store, repin, "LURE", 4).await.unwrap();
    let repin_result = sqlx::query(
        r#"
        UPDATE item_instances
           SET definition_key = $2,
               definition_version = 1
         WHERE id = $1
        "#,
    )
    .bind(repin)
    .bind(&material_key)
    .execute(store.pool())
    .await;
    assert!(
        repin_result.is_err(),
        "an enchanted ItemInstance cannot be repinned to a non-equipment definition"
    );

    let disable = seed_item(&store, player_id, &equipment_key, &nonce, "disable").await;
    insert_enchant(&store, disable, "LURE", 3).await.unwrap();
    let disable_result =
        sqlx::query("UPDATE item_instances SET is_enchantable = FALSE WHERE id = $1")
            .bind(disable)
            .execute(store.pool())
            .await;
    assert!(
        disable_result.is_err(),
        "an ItemInstance carrying enchants cannot become non-enchantable"
    );

    let make_starter = seed_item(&store, player_id, &equipment_key, &nonce, "make-starter").await;
    insert_enchant(&store, make_starter, "LURE", 2)
        .await
        .unwrap();
    let starter_result = sqlx::query("UPDATE item_instances SET is_starter = TRUE WHERE id = $1")
        .bind(make_starter)
        .execute(store.pool())
        .await;
    assert!(
        starter_result.is_err(),
        "an ItemInstance carrying enchants cannot become starter equipment"
    );
}

async fn insert_enchant(
    store: &PgStore,
    item_id: Uuid,
    enchant_key: &str,
    level: i16,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO item_instance_embedded_enchants (item_instance_id, enchant_key, level)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(item_id)
    .bind(enchant_key)
    .bind(level)
    .execute(store.pool())
    .await
    .map(|_| ())
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
        VALUES ($1, $2, $3, $4, 'EMBEDDED_ENCHANT_STATE_TEST', 'PENDING', 1, $5, $6)
        "#,
    )
    .bind(operation_id)
    .bind(format!(
        "test:embedded-enchant-state:{nonce}:{suffix}:{operation_id}"
    ))
    .bind(discord_user_id)
    .bind(player_id)
    .bind([47_u8; 32].as_slice())
    .bind([53_u8; 32].as_slice())
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
