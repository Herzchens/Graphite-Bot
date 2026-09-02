use graphite_services::{
    CanonicalEnchant, EnchantApplyError, EquipmentSlot, OrdinarySlotOrbPreflightResolverError,
    OrdinarySlotOrbStateWriterError, SlotOrbFamily, SlotOrbUnlock,
    write_successful_slot_orb_unlock_to_owned_ordinary_equipment,
};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn successful_normal_unlock_is_transaction_composable_and_rolls_back_cleanly() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let definition_key = format!("test.slot-orb-writer.pickaxe.{nonce}");
    seed_definition(&store, &definition_key, "PICKAXE", r#"{"tier":"OBSIDIAN"}"#).await;
    let item_id = seed_item(&store, owner_id, &definition_key, &nonce, "rollback").await;
    seed_structural_state(&store, item_id, "5").await;

    let mut tx = store.pool().begin().await.unwrap();
    let unlock = write_successful_slot_orb_unlock_to_owned_ordinary_equipment(
        &mut tx,
        owner_id,
        item_id,
        SlotOrbUnlock::Normal5,
    )
    .await
    .unwrap();
    assert_eq!(unlock, SlotOrbUnlock::Normal5);
    assert_eq!(capacities_in_tx(&mut tx, item_id).await, (5, 3));
    tx.rollback().await.unwrap();

    assert_eq!(capacities(&store, item_id).await, (4, 3));
}

#[tokio::test]
async fn successful_special_unlock_commits_only_the_selected_family() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let definition_key = format!("test.slot-orb-writer.sword.{nonce}");
    seed_definition(&store, &definition_key, "SWORD", r#"{"tier":"OBSIDIAN"}"#).await;
    let item_id = seed_item(&store, owner_id, &definition_key, &nonce, "commit").await;
    seed_structural_state(&store, item_id, "7").await;

    let mut tx = store.pool().begin().await.unwrap();
    let unlock = write_successful_slot_orb_unlock_to_owned_ordinary_equipment(
        &mut tx,
        owner_id,
        item_id,
        SlotOrbUnlock::Special4,
    )
    .await
    .unwrap();
    assert_eq!(unlock, SlotOrbUnlock::Special4);
    assert_eq!(capacities_in_tx(&mut tx, item_id).await, (4, 4));
    tx.commit().await.unwrap();

    assert_eq!(capacities(&store, item_id).await, (4, 4));
}

#[tokio::test]
async fn writer_rejects_appraisal_valid_but_over_capacity_existing_enchant_state() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let definition_key = format!("test.slot-orb-writer.over-capacity.sword.{nonce}");
    seed_definition(&store, &definition_key, "SWORD", r#"{"tier":"OBSIDIAN"}"#).await;
    let item_id = seed_item(&store, owner_id, &definition_key, &nonce, "over-capacity").await;
    seed_structural_state(&store, item_id, "5").await;
    for key in ["LOOTING", "KNOCKBACK", "DEVOUR", "UNBREAKING", "MENDING"] {
        seed_enchant(&store, item_id, key, 1).await;
    }

    let mut tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        write_successful_slot_orb_unlock_to_owned_ordinary_equipment(
            &mut tx,
            owner_id,
            item_id,
            SlotOrbUnlock::Normal5,
        )
        .await,
        Err(OrdinarySlotOrbStateWriterError::Preflight(
            OrdinarySlotOrbPreflightResolverError::ExistingEnchantState(
                EnchantApplyError::ExistingOccupancyExceedsCapacity {
                    family: SlotOrbFamily::NormalClass,
                    occupied: 5,
                    unlocked: 4,
                }
            )
        ))
    ));
    assert_eq!(capacities_in_tx(&mut tx, item_id).await, (4, 3));
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn writer_rejects_existing_enchant_that_is_invalid_for_the_equipment_slot() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let definition_key = format!("test.slot-orb-writer.wrong-slot.armor.{nonce}");
    seed_definition(
        &store,
        &definition_key,
        "ARMOR",
        r#"{"tier":"OBSIDIAN","slot":"ARMOR_CHEST"}"#,
    )
    .await;
    let item_id = seed_item(&store, owner_id, &definition_key, &nonce, "wrong-slot").await;
    seed_structural_state(&store, item_id, "5").await;
    seed_enchant(&store, item_id, "EFFICIENCY", 1).await;

    let mut tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        write_successful_slot_orb_unlock_to_owned_ordinary_equipment(
            &mut tx,
            owner_id,
            item_id,
            SlotOrbUnlock::Normal5,
        )
        .await,
        Err(OrdinarySlotOrbStateWriterError::Preflight(
            OrdinarySlotOrbPreflightResolverError::ExistingEnchantState(
                EnchantApplyError::ExistingEnchantWrongEquipmentSlot {
                    enchant: CanonicalEnchant::Efficiency,
                    slot: EquipmentSlot::Chestplate,
                }
            )
        ))
    ));
    assert_eq!(capacities_in_tx(&mut tx, item_id).await, (4, 3));
    tx.rollback().await.unwrap();
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
    sqlx::query("INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'SLOT_ORB_STATE_WRITER_TEST', 'PENDING', 1, $5, $6)")
        .bind(operation_id)
        .bind(format!("test:slot-orb-state-writer:{nonce}:{suffix}:{operation_id}"))
        .bind(discord_user_id)
        .bind(player_id)
        .bind([149_u8; 32].as_slice())
        .bind([151_u8; 32].as_slice())
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

async fn seed_structural_state(store: &PgStore, item_id: Uuid, upgrade_level: &str) {
    sqlx::query("INSERT INTO item_instance_equipment_structural_state (item_instance_id, creation_roll_numerator, creation_roll_denominator, upgrade_level) VALUES ($1, 1, 2, $2::NUMERIC)")
        .bind(item_id)
        .bind(upgrade_level)
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

async fn capacities(store: &PgStore, item_id: Uuid) -> (i16, i16) {
    sqlx::query_as(
        "SELECT normal_enchant_slot_capacity, special_enchant_slot_capacity FROM item_instance_equipment_structural_state WHERE item_instance_id = $1",
    )
    .bind(item_id)
    .fetch_one(store.pool())
    .await
    .unwrap()
}

async fn capacities_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    item_id: Uuid,
) -> (i16, i16) {
    sqlx::query_as(
        "SELECT normal_enchant_slot_capacity, special_enchant_slot_capacity FROM item_instance_equipment_structural_state WHERE item_instance_id = $1",
    )
    .bind(item_id)
    .fetch_one(&mut **tx)
    .await
    .unwrap()
}

fn positive_snowflake(nonce: Uuid) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    i64::try_from((raw % 8_000_000_000_000_000_000_u64).max(1)).unwrap()
}
