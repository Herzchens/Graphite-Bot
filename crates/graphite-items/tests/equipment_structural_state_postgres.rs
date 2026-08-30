use graphite_store::PgStore;
use sqlx::Row;
use uuid::Uuid;

const U64_MAX_DECIMAL: &str = "18446744073709551615";
const U64_OVERFLOW_DECIMAL: &str = "18446744073709551616";

#[tokio::test]
async fn structural_state_preserves_exact_u64_domain_and_creation_roll_immutability() {
    let Some(store) = test_store().await else {
        return;
    };

    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let equipment_key = format!("test.structural.rod.{nonce}");
    let totem_key = format!("test.structural.totem.{nonce}");
    seed_definition(&store, &equipment_key, "FISHING_ROD").await;
    seed_definition(&store, &totem_key, "TOTEM").await;

    let item_id = seed_item(&store, player_id, &equipment_key, &nonce, "equipment").await;
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
    .bind("1")
    .bind("3")
    .bind("0")
    .execute(store.pool())
    .await
    .unwrap();

    let row = sqlx::query(
        r#"
        SELECT creation_roll_numerator::TEXT AS creation_roll_numerator,
               creation_roll_denominator::TEXT AS creation_roll_denominator,
               upgrade_level::TEXT AS upgrade_level
          FROM item_instance_equipment_structural_state
         WHERE item_instance_id = $1
        "#,
    )
    .bind(item_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(
        row.try_get::<String, _>("creation_roll_numerator").unwrap(),
        "1"
    );
    assert_eq!(
        row.try_get::<String, _>("creation_roll_denominator")
            .unwrap(),
        "3"
    );
    assert_eq!(row.try_get::<String, _>("upgrade_level").unwrap(), "0");

    let max_denominator_item =
        seed_item(&store, player_id, &equipment_key, &nonce, "max-denominator").await;
    sqlx::query(
        r#"
        INSERT INTO item_instance_equipment_structural_state (
            item_instance_id,
            creation_roll_numerator,
            creation_roll_denominator,
            upgrade_level
        )
        VALUES ($1, 1, $2::NUMERIC, 0)
        "#,
    )
    .bind(max_denominator_item)
    .bind(U64_MAX_DECIMAL)
    .execute(store.pool())
    .await
    .unwrap();
    let persisted_max_denominator: String = sqlx::query_scalar(
        r#"
        SELECT creation_roll_denominator::TEXT
          FROM item_instance_equipment_structural_state
         WHERE item_instance_id = $1
        "#,
    )
    .bind(max_denominator_item)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(persisted_max_denominator, U64_MAX_DECIMAL);

    sqlx::query(
        r#"
        UPDATE item_instance_equipment_structural_state
           SET upgrade_level = $2::NUMERIC
         WHERE item_instance_id = $1
        "#,
    )
    .bind(item_id)
    .bind(U64_MAX_DECIMAL)
    .execute(store.pool())
    .await
    .unwrap();

    let persisted_max: String = sqlx::query_scalar(
        r#"
        SELECT upgrade_level::TEXT
          FROM item_instance_equipment_structural_state
         WHERE item_instance_id = $1
        "#,
    )
    .bind(item_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(persisted_max, U64_MAX_DECIMAL);

    let roll_update = sqlx::query(
        r#"
        UPDATE item_instance_equipment_structural_state
           SET creation_roll_numerator = 2
         WHERE item_instance_id = $1
        "#,
    )
    .bind(item_id)
    .execute(store.pool())
    .await;
    assert!(
        roll_update.is_err(),
        "Creation Roll must be immutable after insert"
    );

    let direct_delete = sqlx::query(
        "DELETE FROM item_instance_equipment_structural_state WHERE item_instance_id = $1",
    )
    .bind(item_id)
    .execute(store.pool())
    .await;
    assert!(
        direct_delete.is_err(),
        "structural state cannot be deleted while the ItemInstance still exists"
    );

    let invalid_repin = sqlx::query(
        r#"
        UPDATE item_instances
           SET definition_key = $2,
               definition_version = 1
         WHERE id = $1
        "#,
    )
    .bind(item_id)
    .bind(&totem_key)
    .execute(store.pool())
    .await;
    assert!(
        invalid_repin.is_err(),
        "an ItemInstance carrying structural equipment state cannot be repinned to a non-equipment definition"
    );

    sqlx::query("DELETE FROM item_instances WHERE id = $1")
        .bind(item_id)
        .execute(store.pool())
        .await
        .unwrap();
    let remaining: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM item_instance_equipment_structural_state WHERE item_instance_id = $1",
    )
    .bind(item_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(
        remaining, 0,
        "parent deletion must still cascade structural state cleanup"
    );
}

#[tokio::test]
async fn structural_state_rejects_invalid_ratio_range_and_non_equipment_rows() {
    let Some(store) = test_store().await else {
        return;
    };

    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let equipment_key = format!("test.structural.sword.{nonce}");
    let totem_key = format!("test.structural.non-equipment.{nonce}");
    seed_definition(&store, &equipment_key, "SWORD").await;
    seed_definition(&store, &totem_key, "TOTEM").await;

    let denominator_zero = seed_item(&store, player_id, &equipment_key, &nonce, "den-zero").await;
    assert_structural_insert_fails(&store, denominator_zero, "0", "0", "0").await;

    let ratio_over_one = seed_item(&store, player_id, &equipment_key, &nonce, "ratio-over").await;
    assert_structural_insert_fails(&store, ratio_over_one, "4", "3", "0").await;

    let non_reduced = seed_item(&store, player_id, &equipment_key, &nonce, "non-reduced").await;
    assert_structural_insert_fails(&store, non_reduced, "2", "4", "0").await;

    let non_reduced_zero = seed_item(
        &store,
        player_id,
        &equipment_key,
        &nonce,
        "non-reduced-zero",
    )
    .await;
    assert_structural_insert_fails(&store, non_reduced_zero, "0", "2", "0").await;

    let fractional_numerator =
        seed_item(&store, player_id, &equipment_key, &nonce, "fractional-num").await;
    assert_structural_insert_fails(&store, fractional_numerator, "1.5", "3", "0").await;

    let fractional_denominator =
        seed_item(&store, player_id, &equipment_key, &nonce, "fractional-den").await;
    assert_structural_insert_fails(&store, fractional_denominator, "1", "2.5", "0").await;

    let fractional_upgrade = seed_item(
        &store,
        player_id,
        &equipment_key,
        &nonce,
        "fractional-upgrade",
    )
    .await;
    assert_structural_insert_fails(&store, fractional_upgrade, "1", "2", "3.5").await;

    let numerator_overflow =
        seed_item(&store, player_id, &equipment_key, &nonce, "num-overflow").await;
    assert_structural_insert_fails(
        &store,
        numerator_overflow,
        U64_OVERFLOW_DECIMAL,
        U64_MAX_DECIMAL,
        "0",
    )
    .await;

    let denominator_overflow =
        seed_item(&store, player_id, &equipment_key, &nonce, "den-overflow").await;
    assert_structural_insert_fails(
        &store,
        denominator_overflow,
        U64_MAX_DECIMAL,
        U64_OVERFLOW_DECIMAL,
        "0",
    )
    .await;

    let upgrade_overflow = seed_item(
        &store,
        player_id,
        &equipment_key,
        &nonce,
        "upgrade-overflow",
    )
    .await;
    assert_structural_insert_fails(&store, upgrade_overflow, "1", "2", U64_OVERFLOW_DECIMAL).await;

    let negative_upgrade = seed_item(
        &store,
        player_id,
        &equipment_key,
        &nonce,
        "negative-upgrade",
    )
    .await;
    assert_structural_insert_fails(&store, negative_upgrade, "1", "2", "-1").await;

    let non_equipment = seed_item(&store, player_id, &totem_key, &nonce, "totem").await;
    assert_structural_insert_fails(&store, non_equipment, "1", "2", "0").await;
}

async fn assert_structural_insert_fails(
    store: &PgStore,
    item_id: Uuid,
    numerator: &str,
    denominator: &str,
    upgrade_level: &str,
) {
    let result = sqlx::query(
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
    .await;
    assert!(result.is_err());
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
        VALUES ($1, $2, $3, $4, 'EQUIPMENT_STRUCTURAL_STATE_TEST', 'PENDING', 1, $5, $6)
        "#,
    )
    .bind(operation_id)
    .bind(format!(
        "test:equipment-structural-state:{nonce}:{suffix}:{operation_id}"
    ))
    .bind(discord_user_id)
    .bind(player_id)
    .bind([23_u8; 32].as_slice())
    .bind([29_u8; 32].as_slice())
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
