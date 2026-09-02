use graphite_items::{ItemError, ItemService};
use graphite_services::{
    CanonicalEnchant, EnchantApplyAction,
    write_standard_finished_book_application_to_owned_ordinary_equipment,
};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn state_writer_inserts_canonical_key_and_rolls_back_with_caller_transaction() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let definition_key = format!("test.enchant-apply-writer.pickaxe.{nonce}");
    seed_definition(&store, &definition_key, "PICKAXE", r#"{"tier":"OBSIDIAN"}"#).await;
    let item_id = seed_item(&store, owner_id, &definition_key, &nonce, "rollback").await;
    seed_structural_state(&store, item_id).await;

    let mut tx = store.pool().begin().await.unwrap();
    let action = write_standard_finished_book_application_to_owned_ordinary_equipment(
        &mut tx,
        owner_id,
        item_id,
        CanonicalEnchant::Efficiency,
        3,
    )
    .await
    .unwrap();
    assert_eq!(action, EnchantApplyAction::InsertNew);

    let row: (String, i16) = sqlx::query_as(
        "SELECT enchant_key, level FROM item_instance_embedded_enchants WHERE item_instance_id = $1",
    )
    .bind(item_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(row, ("EFFICIENCY".to_owned(), 3));
    tx.rollback().await.unwrap();

    let persisted: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM item_instance_embedded_enchants WHERE item_instance_id = $1",
    )
    .bind(item_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(persisted, 0);
}

#[tokio::test]
async fn state_writer_upgrades_existing_identity_without_consuming_another_slot() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let definition_key = format!("test.enchant-apply-writer.upgrade.{nonce}");
    seed_definition(&store, &definition_key, "PICKAXE", r#"{"tier":"OBSIDIAN"}"#).await;
    let item_id = seed_item(&store, owner_id, &definition_key, &nonce, "upgrade").await;
    seed_structural_state(&store, item_id).await;
    seed_enchant(&store, item_id, "EFFICIENCY", 2).await;

    let mut tx = store.pool().begin().await.unwrap();
    let action = write_standard_finished_book_application_to_owned_ordinary_equipment(
        &mut tx,
        owner_id,
        item_id,
        CanonicalEnchant::Efficiency,
        4,
    )
    .await
    .unwrap();
    assert_eq!(
        action,
        EnchantApplyAction::UpgradeExisting { previous_level: 2 }
    );
    tx.commit().await.unwrap();

    let rows: Vec<(String, i16)> = sqlx::query_as(
        "SELECT enchant_key, level FROM item_instance_embedded_enchants WHERE item_instance_id = $1 ORDER BY enchant_key",
    )
    .bind(item_id)
    .fetch_all(store.pool())
    .await
    .unwrap();
    assert_eq!(rows, vec![("EFFICIENCY".to_owned(), 4)]);
}

#[tokio::test]
async fn unequipped_survival_core_state_can_be_written_but_equip_revalidates_membership() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let discord_user_id = positive_snowflake(nonce);
    let owner_id = seed_player(&store, discord_user_id).await;

    let legs_key = format!("test.enchant-apply-writer.dormant.legs.{nonce}");
    let chest_key = format!("test.enchant-apply-writer.dormant.chest.{nonce}");
    seed_definition(
        &store,
        &legs_key,
        "ARMOR",
        r#"{"tier":"OBSIDIAN","slot":"ARMOR_LEGS"}"#,
    )
    .await;
    seed_definition(
        &store,
        &chest_key,
        "ARMOR",
        r#"{"tier":"OBSIDIAN","slot":"ARMOR_CHEST"}"#,
    )
    .await;

    let legs_id = seed_item(&store, owner_id, &legs_key, &nonce, "active-nine-life").await;
    let chest_id = seed_item(&store, owner_id, &chest_key, &nonce, "dormant-guardian").await;
    seed_structural_state(&store, legs_id).await;
    seed_structural_state(&store, chest_id).await;
    seed_enchant(&store, legs_id, "NINE_LIFE", 3).await;

    let items = ItemService::new(store.clone());
    items
        .equip(
            u64::try_from(discord_user_id).unwrap(),
            legs_id,
            &format!("test:enchant-apply-state-writer:equip-legs:{nonce}"),
        )
        .await
        .unwrap();

    let mut tx = store.pool().begin().await.unwrap();
    let action = write_standard_finished_book_application_to_owned_ordinary_equipment(
        &mut tx,
        owner_id,
        chest_id,
        CanonicalEnchant::Guardian,
        1,
    )
    .await
    .unwrap();
    assert_eq!(action, EnchantApplyAction::InsertNew);
    tx.commit().await.unwrap();

    let stored: (String, i16) = sqlx::query_as(
        "SELECT enchant_key, level FROM item_instance_embedded_enchants WHERE item_instance_id = $1",
    )
    .bind(chest_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(stored, ("GUARDIAN".to_owned(), 1));

    let equip_result = items
        .equip(
            u64::try_from(discord_user_id).unwrap(),
            chest_id,
            &format!("test:enchant-apply-state-writer:equip-conflict:{nonce}"),
        )
        .await;
    assert!(matches!(
        equip_result,
        Err(ItemError::EquippedArmorEnchantConflict { .. })
    ));
    assert_eq!(item_location(&store, legs_id).await, "EQUIPPED");
    assert_eq!(item_location(&store, chest_id).await, "TOOL_LOCKER");
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

async fn seed_definition(store: &PgStore, key: &str, category: &str, data: &str) {
    sqlx::query("INSERT INTO item_definitions (key, category, stackable, active, definition_version, rarity, stack_limit, data) VALUES ($1, $2, FALSE, TRUE, 1, 'COMMON', NULL, $3::jsonb)")
        .bind(key)
        .bind(category)
        .bind(data)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO item_definition_versions (key, version, category, stackable, rarity, stack_limit, is_ordinary_equipment, data) VALUES ($1, 1, $2, FALSE, 'COMMON', NULL, TRUE, $3::jsonb)")
        .bind(key)
        .bind(category)
        .bind(data)
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
    sqlx::query("INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'ENCHANT_APPLY_STATE_WRITER_TEST', 'PENDING', 1, $5, $6)")
        .bind(operation_id)
        .bind(format!("test:enchant-apply-state-writer:{nonce}:{suffix}:{operation_id}"))
        .bind(discord_user_id)
        .bind(player_id)
        .bind([113_u8; 32].as_slice())
        .bind([127_u8; 32].as_slice())
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
    item_id
}

async fn seed_structural_state(store: &PgStore, item_id: Uuid) {
    sqlx::query("INSERT INTO item_instance_equipment_structural_state (item_instance_id, creation_roll_numerator, creation_roll_denominator, upgrade_level) VALUES ($1, 1, 2, 0)")
        .bind(item_id)
        .execute(store.pool())
        .await
        .unwrap();
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

async fn item_location(store: &PgStore, item_id: Uuid) -> String {
    sqlx::query_scalar("SELECT location FROM item_instances WHERE id = $1")
        .bind(item_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
}

fn positive_snowflake(nonce: Uuid) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    i64::try_from((raw % 8_000_000_000_000_000_000_u64).max(1)).unwrap()
}
