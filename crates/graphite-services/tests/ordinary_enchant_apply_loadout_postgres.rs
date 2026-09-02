use graphite_services::{
    CanonicalEnchant, EnchantApplyAction, EquippedArmorEnchantLoadoutError,
    OrdinaryEnchantApplyStateWriterError,
    lock_validate_equipped_armor_enchant_loadout_for_owned_target,
    write_standard_finished_book_application_to_owned_ordinary_equipment,
};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn equipped_survival_core_apply_is_authoritative_and_rolls_back_with_the_caller() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let definition_key = format!("test.enchant-loadout.chest.{nonce}");
    seed_armor_definition(&store, &definition_key, "ARMOR_CHEST").await;
    let target_id = seed_item(&store, owner_id, &definition_key, &nonce, "target").await;
    seed_structural_state(&store, target_id).await;
    equip_item(&store, owner_id, target_id, "ARMOR_CHEST").await;

    let mut tx = store.pool().begin().await.unwrap();
    let action = write_standard_finished_book_application_to_owned_ordinary_equipment(
        &mut tx,
        owner_id,
        target_id,
        CanonicalEnchant::Guardian,
        1,
    )
    .await
    .unwrap();
    assert_eq!(action, EnchantApplyAction::InsertNew);

    let stored: (String, i16) = sqlx::query_as(
        "SELECT enchant_key, level FROM item_instance_embedded_enchants WHERE item_instance_id = $1",
    )
    .bind(target_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(stored, ("GUARDIAN".to_owned(), 1));
    tx.rollback().await.unwrap();

    let persisted: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM item_instance_embedded_enchants WHERE item_instance_id = $1",
    )
    .bind(target_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(persisted, 0);
}

#[tokio::test]
async fn incoming_survival_core_conflict_with_sibling_armor_is_rejected_before_write() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;

    let chest_key = format!("test.enchant-loadout.conflict.chest.{nonce}");
    let legs_key = format!("test.enchant-loadout.conflict.legs.{nonce}");
    seed_armor_definition(&store, &chest_key, "ARMOR_CHEST").await;
    seed_armor_definition(&store, &legs_key, "ARMOR_LEGS").await;
    let target_id = seed_item(&store, owner_id, &chest_key, &nonce, "target").await;
    let sibling_id = seed_item(&store, owner_id, &legs_key, &nonce, "sibling").await;
    seed_structural_state(&store, target_id).await;
    seed_structural_state(&store, sibling_id).await;
    seed_enchant(&store, sibling_id, "NINE_LIFE", 3).await;
    equip_item(&store, owner_id, target_id, "ARMOR_CHEST").await;
    equip_item(&store, owner_id, sibling_id, "ARMOR_LEGS").await;

    let mut tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        write_standard_finished_book_application_to_owned_ordinary_equipment(
            &mut tx,
            owner_id,
            target_id,
            CanonicalEnchant::Guardian,
            1,
        )
        .await,
        Err(OrdinaryEnchantApplyStateWriterError::Loadout(
            EquippedArmorEnchantLoadoutError::IncomingLoadoutConflict {
                incoming: CanonicalEnchant::Guardian,
                existing_item_instance_id,
                existing: CanonicalEnchant::NineLife,
            }
        )) if existing_item_instance_id == sibling_id
    ));
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM item_instance_embedded_enchants WHERE item_instance_id = $1",
    )
    .bind(target_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(count, 0);
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn same_survival_core_identity_can_stack_across_equipped_armor_pieces() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;

    let helmet_key = format!("test.enchant-loadout.stack.helmet.{nonce}");
    let legs_key = format!("test.enchant-loadout.stack.legs.{nonce}");
    seed_armor_definition(&store, &helmet_key, "ARMOR_HELMET").await;
    seed_armor_definition(&store, &legs_key, "ARMOR_LEGS").await;
    let target_id = seed_item(&store, owner_id, &helmet_key, &nonce, "target").await;
    let sibling_id = seed_item(&store, owner_id, &legs_key, &nonce, "sibling").await;
    seed_structural_state(&store, target_id).await;
    seed_structural_state(&store, sibling_id).await;
    seed_enchant(&store, sibling_id, "NINE_LIFE", 2).await;
    equip_item(&store, owner_id, target_id, "ARMOR_HELMET").await;
    equip_item(&store, owner_id, sibling_id, "ARMOR_LEGS").await;

    let mut tx = store.pool().begin().await.unwrap();
    let action = write_standard_finished_book_application_to_owned_ordinary_equipment(
        &mut tx,
        owner_id,
        target_id,
        CanonicalEnchant::NineLife,
        3,
    )
    .await
    .unwrap();
    assert_eq!(action, EnchantApplyAction::InsertNew);

    let level: i16 = sqlx::query_scalar(
        "SELECT level FROM item_instance_embedded_enchants WHERE item_instance_id = $1 AND enchant_key = 'NINE_LIFE'",
    )
    .bind(target_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(level, 3);
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn preexisting_invalid_survival_core_loadout_blocks_further_enchant_mutation() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;

    let chest_key = format!("test.enchant-loadout.invalid.chest.{nonce}");
    let legs_key = format!("test.enchant-loadout.invalid.legs.{nonce}");
    let boots_key = format!("test.enchant-loadout.invalid.boots.{nonce}");
    seed_armor_definition(&store, &chest_key, "ARMOR_CHEST").await;
    seed_armor_definition(&store, &legs_key, "ARMOR_LEGS").await;
    seed_armor_definition(&store, &boots_key, "ARMOR_BOOTS").await;
    let guardian_id = seed_item(&store, owner_id, &chest_key, &nonce, "guardian").await;
    let nine_life_id = seed_item(&store, owner_id, &legs_key, &nonce, "nine-life").await;
    let target_id = seed_item(&store, owner_id, &boots_key, &nonce, "target").await;
    for item_id in [guardian_id, nine_life_id, target_id] {
        seed_structural_state(&store, item_id).await;
    }
    seed_enchant(&store, guardian_id, "GUARDIAN", 1).await;
    seed_enchant(&store, nine_life_id, "NINE_LIFE", 2).await;
    equip_item(&store, owner_id, guardian_id, "ARMOR_CHEST").await;
    equip_item(&store, owner_id, nine_life_id, "ARMOR_LEGS").await;
    equip_item(&store, owner_id, target_id, "ARMOR_BOOTS").await;

    let mut tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        write_standard_finished_book_application_to_owned_ordinary_equipment(
            &mut tx,
            owner_id,
            target_id,
            CanonicalEnchant::Protection,
            1,
        )
        .await,
        Err(OrdinaryEnchantApplyStateWriterError::Loadout(
            EquippedArmorEnchantLoadoutError::ExistingLoadoutConflict { .. }
        ))
    ));
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM item_instance_embedded_enchants WHERE item_instance_id = $1",
    )
    .bind(target_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(count, 0);
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn loadout_resolver_retains_player_and_sibling_item_locks_until_caller_releases_transaction()
{
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;

    let chest_key = format!("test.enchant-loadout.lock.chest.{nonce}");
    let legs_key = format!("test.enchant-loadout.lock.legs.{nonce}");
    seed_armor_definition(&store, &chest_key, "ARMOR_CHEST").await;
    seed_armor_definition(&store, &legs_key, "ARMOR_LEGS").await;
    let target_id = seed_item(&store, owner_id, &chest_key, &nonce, "target").await;
    let sibling_id = seed_item(&store, owner_id, &legs_key, &nonce, "sibling").await;
    seed_structural_state(&store, target_id).await;
    seed_structural_state(&store, sibling_id).await;
    equip_item(&store, owner_id, target_id, "ARMOR_CHEST").await;
    equip_item(&store, owner_id, sibling_id, "ARMOR_LEGS").await;

    let mut tx = store.pool().begin().await.unwrap();
    let snapshot =
        lock_validate_equipped_armor_enchant_loadout_for_owned_target(&mut tx, owner_id, target_id)
            .await
            .unwrap();
    assert_eq!(snapshot.items.len(), 2);

    let mut player_probe = store.pool().begin().await.unwrap();
    let blocked_player = sqlx::query("SELECT id FROM players WHERE id = $1 FOR UPDATE NOWAIT")
        .bind(owner_id)
        .fetch_one(&mut *player_probe)
        .await;
    assert!(
        blocked_player.is_err(),
        "loadout resolver must retain the player row lock for loadout-membership serialization"
    );
    player_probe.rollback().await.unwrap();

    let mut sibling_probe = store.pool().begin().await.unwrap();
    let blocked_sibling =
        sqlx::query("SELECT id FROM item_instances WHERE id = $1 FOR UPDATE NOWAIT")
            .bind(sibling_id)
            .fetch_one(&mut *sibling_probe)
            .await;
    assert!(
        blocked_sibling.is_err(),
        "loadout resolver must retain sibling ItemInstance locks for the caller transaction"
    );
    sibling_probe.rollback().await.unwrap();
    tx.rollback().await.unwrap();

    let mut after_release = store.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM players WHERE id = $1 FOR UPDATE NOWAIT")
        .bind(owner_id)
        .fetch_one(&mut *after_release)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM item_instances WHERE id = $1 FOR UPDATE NOWAIT")
        .bind(sibling_id)
        .fetch_one(&mut *after_release)
        .await
        .unwrap();
    after_release.rollback().await.unwrap();
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

async fn seed_armor_definition(store: &PgStore, key: &str, slot: &str) {
    let data = format!(r#"{{"tier":"OBSIDIAN","slot":"{slot}"}}"#);
    sqlx::query("INSERT INTO item_definitions (key, category, stackable, active, definition_version, rarity, stack_limit, data) VALUES ($1, 'ARMOR', FALSE, TRUE, 1, 'COMMON', NULL, $2::jsonb)")
        .bind(key)
        .bind(&data)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO item_definition_versions (key, version, category, stackable, rarity, stack_limit, is_ordinary_equipment, data) VALUES ($1, 1, 'ARMOR', FALSE, 'COMMON', NULL, TRUE, $2::jsonb)")
        .bind(key)
        .bind(&data)
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
    sqlx::query("INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'ENCHANT_APPLY_LOADOUT_TEST', 'PENDING', 1, $5, $6)")
        .bind(operation_id)
        .bind(format!("test:enchant-apply-loadout:{nonce}:{suffix}:{operation_id}"))
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

async fn equip_item(store: &PgStore, player_id: Uuid, item_id: Uuid, slot: &str) {
    let mut tx = store.pool().begin().await.unwrap();
    sqlx::query(
        "INSERT INTO equipment_slots (player_id, slot, item_instance_id) VALUES ($1, $2, $3)",
    )
    .bind(player_id)
    .bind(slot)
    .bind(item_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("UPDATE item_instances SET location = 'EQUIPPED' WHERE id = $1")
        .bind(item_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

fn positive_snowflake(nonce: Uuid) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    i64::try_from((raw % 8_000_000_000_000_000_000_u64).max(1)).unwrap()
}
