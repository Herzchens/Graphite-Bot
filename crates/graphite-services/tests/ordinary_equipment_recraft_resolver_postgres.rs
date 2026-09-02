use graphite_items::ItemError;
use graphite_services::{
    EquipmentSlot, EquipmentTier, OrdinaryEquipmentRecraftResolverError,
    lock_owned_ordinary_equipment_recraft_appraisal,
};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn ordinary_recraft_resolver_uses_pinned_definition_and_locked_structural_state() {
    let Some(store) = test_store().await else {
        return;
    };

    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let other_id = seed_player(&store, positive_snowflake(Uuid::now_v7())).await;

    let ordinary_key = format!("test.recraft-resolver.armor.{nonce}");
    seed_two_version_definition(&store, &ordinary_key).await;
    let ordinary_item = seed_item(&store, owner_id, &ordinary_key, 1, &nonce, "ordinary").await;
    seed_structural_state(&store, ordinary_item, "1", "2", "3").await;
    sqlx::query("UPDATE item_instances SET is_upgradeable = FALSE WHERE id = $1")
        .bind(ordinary_item)
        .execute(store.pool())
        .await
        .unwrap();

    let special_key = format!("test.recraft-resolver.special.{nonce}");
    seed_single_definition(
        &store,
        &special_key,
        "SWORD",
        false,
        r#"{"tier":"NETHERITE"}"#,
    )
    .await;
    let special_item = seed_item(&store, owner_id, &special_key, 1, &nonce, "special").await;
    seed_structural_state(&store, special_item, "1", "3", "2").await;

    let malformed_key = format!("test.recraft-resolver.malformed.{nonce}");
    seed_single_definition(
        &store,
        &malformed_key,
        "PICKAXE",
        true,
        r#"{"tier":"LEATHER"}"#,
    )
    .await;
    let malformed_item = seed_item(&store, owner_id, &malformed_key, 1, &nonce, "malformed").await;
    seed_structural_state(&store, malformed_item, "1", "4", "0").await;

    let gold_armor_key = format!("test.recraft-resolver.gold-armor.{nonce}");
    seed_single_definition(
        &store,
        &gold_armor_key,
        "ARMOR",
        true,
        r#"{"tier":"GOLD","slot":"ARMOR_CHEST"}"#,
    )
    .await;
    let gold_armor_item =
        seed_item(&store, owner_id, &gold_armor_key, 1, &nonce, "gold-armor").await;
    seed_structural_state(&store, gold_armor_item, "1", "2", "0").await;

    let mut tx = store.pool().begin().await.unwrap();
    let appraisal =
        lock_owned_ordinary_equipment_recraft_appraisal(&mut tx, owner_id, ordinary_item)
            .await
            .unwrap();
    assert_eq!(appraisal.item_instance_id, ordinary_item);
    assert_eq!(appraisal.owner_player_id, owner_id);
    assert_eq!(appraisal.definition_key, ordinary_key);
    assert_eq!(appraisal.definition_version, 1);
    assert!(!appraisal.is_starter);
    assert!(appraisal.is_enchantable);
    assert!(!appraisal.is_upgradeable);
    assert_eq!(appraisal.tier, EquipmentTier::Obsidian);
    assert_eq!(appraisal.slot, EquipmentSlot::Chestplate);
    assert_eq!(appraisal.base_appraisal.value, 1_181_300);
    assert_eq!(appraisal.creation_roll.numerator(), 1);
    assert_eq!(appraisal.creation_roll.denominator(), 2);
    assert_eq!(appraisal.upgrade_level, 3);
    assert_eq!(appraisal.recraft_appraisal, 1_287_479);

    let mut wrong_owner_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_owned_ordinary_equipment_recraft_appraisal(
            &mut wrong_owner_tx,
            other_id,
            ordinary_item,
        )
        .await,
        Err(OrdinaryEquipmentRecraftResolverError::Item(
            ItemError::ItemNotFound
        ))
    ));
    wrong_owner_tx.rollback().await.unwrap();

    let mut item_lock_probe = store.pool().begin().await.unwrap();
    let item_blocked = sqlx::query(
        r#"
        SELECT id
          FROM item_instances
         WHERE id = $1
         FOR UPDATE NOWAIT
        "#,
    )
    .bind(ordinary_item)
    .fetch_one(&mut *item_lock_probe)
    .await;
    assert!(
        item_blocked.is_err(),
        "recraft resolver must retain the ItemInstance lock while capability flags are authoritative"
    );
    item_lock_probe.rollback().await.unwrap();

    let mut structural_lock_probe = store.pool().begin().await.unwrap();
    let structural_blocked = sqlx::query(
        r#"
        SELECT item_instance_id
          FROM item_instance_equipment_structural_state
         WHERE item_instance_id = $1
         FOR UPDATE NOWAIT
        "#,
    )
    .bind(ordinary_item)
    .fetch_one(&mut *structural_lock_probe)
    .await;
    assert!(
        structural_blocked.is_err(),
        "recraft resolver must retain the structural-state lock for the caller transaction"
    );
    structural_lock_probe.rollback().await.unwrap();
    tx.rollback().await.unwrap();

    let mut special_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_owned_ordinary_equipment_recraft_appraisal(&mut special_tx, owner_id, special_item,)
            .await,
        Err(OrdinaryEquipmentRecraftResolverError::NotOrdinaryEquipment)
    ));
    special_tx.rollback().await.unwrap();

    let mut malformed_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_owned_ordinary_equipment_recraft_appraisal(
            &mut malformed_tx,
            owner_id,
            malformed_item,
        )
        .await,
        Err(OrdinaryEquipmentRecraftResolverError::InvalidTierMetadata)
    ));
    malformed_tx.rollback().await.unwrap();

    let mut gold_armor_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_owned_ordinary_equipment_recraft_appraisal(
            &mut gold_armor_tx,
            owner_id,
            gold_armor_item,
        )
        .await,
        Err(OrdinaryEquipmentRecraftResolverError::InvalidTierSlotCombination)
    ));
    gold_armor_tx.rollback().await.unwrap();

    sqlx::query("UPDATE item_instances SET is_upgradeable = TRUE WHERE id = $1")
        .bind(ordinary_item)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query(
        r#"
        UPDATE item_instance_equipment_structural_state
           SET upgrade_level = 4
         WHERE item_instance_id = $1
        "#,
    )
    .bind(ordinary_item)
    .execute(store.pool())
    .await
    .unwrap();

    let mut refreshed_tx = store.pool().begin().await.unwrap();
    let refreshed =
        lock_owned_ordinary_equipment_recraft_appraisal(&mut refreshed_tx, owner_id, ordinary_item)
            .await
            .unwrap();
    assert_eq!(refreshed.definition_version, 1);
    assert!(refreshed.is_upgradeable);
    assert_eq!(refreshed.tier, EquipmentTier::Obsidian);
    assert_eq!(refreshed.upgrade_level, 4);
    assert_eq!(refreshed.recraft_appraisal, 1_313_608);
    refreshed_tx.rollback().await.unwrap();
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

async fn seed_two_version_definition(store: &PgStore, key: &str) {
    sqlx::query(
        r#"
        INSERT INTO item_definitions (
            key, category, stackable, active, definition_version, rarity, stack_limit, data
        )
        VALUES ($1, 'ARMOR', FALSE, TRUE, 2, 'COMMON', NULL,
                '{"tier":"GRAPHITE","slot":"ARMOR_CHEST"}'::jsonb)
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
        VALUES
            ($1, 1, 'ARMOR', FALSE, 'COMMON', NULL, TRUE,
             '{"tier":"OBSIDIAN","slot":"ARMOR_CHEST"}'::jsonb),
            ($1, 2, 'ARMOR', FALSE, 'COMMON', NULL, TRUE,
             '{"tier":"GRAPHITE","slot":"ARMOR_CHEST"}'::jsonb)
        "#,
    )
    .bind(key)
    .execute(store.pool())
    .await
    .unwrap();
}

async fn seed_single_definition(
    store: &PgStore,
    key: &str,
    category: &str,
    is_ordinary_equipment: bool,
    data_json: &str,
) {
    sqlx::query(
        r#"
        INSERT INTO item_definitions (
            key, category, stackable, active, definition_version, rarity, stack_limit, data
        )
        VALUES ($1, $2, FALSE, TRUE, 1, 'COMMON', NULL, $3::jsonb)
        "#,
    )
    .bind(key)
    .bind(category)
    .bind(data_json)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO item_definition_versions (
            key, version, category, stackable, rarity, stack_limit,
            is_ordinary_equipment, data
        )
        VALUES ($1, 1, $2, FALSE, 'COMMON', NULL, $3, $4::jsonb)
        "#,
    )
    .bind(key)
    .bind(category)
    .bind(is_ordinary_equipment)
    .bind(data_json)
    .execute(store.pool())
    .await
    .unwrap();
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
        VALUES ($1, $2, $3, $4, 'ORDINARY_RECRAFT_RESOLVER_TEST', 'PENDING', 1, $5, $6)
        "#,
    )
    .bind(operation_id)
    .bind(format!(
        "test:ordinary-recraft-resolver:{nonce}:{suffix}:{operation_id}"
    ))
    .bind(discord_user_id)
    .bind(player_id)
    .bind([41_u8; 32].as_slice())
    .bind([43_u8; 32].as_slice())
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
