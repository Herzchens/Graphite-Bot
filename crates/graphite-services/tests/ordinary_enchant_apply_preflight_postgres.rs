use graphite_services::{
    CanonicalEnchant, EnchantApplyAction, EnchantApplyError, EnchantSlotFamily,
    OrdinaryEnchantApplyPreflightResolverError,
    lock_preview_standard_finished_book_application_for_owned_ordinary_equipment,
};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn authoritative_preflight_uses_persisted_enchants_and_slot_capacity() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let definition_key = format!("test.enchant-apply-preflight.armor.{nonce}");
    seed_definition(&store, &definition_key).await;
    let item_id = seed_item(&store, owner_id, &definition_key, &nonce, "target").await;
    seed_structural_state(&store, item_id).await;
    for (key, level) in [
        ("PROTECTION", 2_i16),
        ("UNBREAKING", 1_i16),
        ("MENDING", 1_i16),
        ("SOUL_GRIND", 1_i16),
    ] {
        seed_enchant(&store, item_id, key, level).await;
    }

    let mut tx = store.pool().begin().await.unwrap();
    let upgrade = lock_preview_standard_finished_book_application_for_owned_ordinary_equipment(
        &mut tx,
        owner_id,
        item_id,
        CanonicalEnchant::Protection,
        3,
    )
    .await
    .unwrap();
    assert_eq!(
        upgrade.action,
        EnchantApplyAction::UpgradeExisting { previous_level: 2 }
    );
    assert_eq!(upgrade.occupancy_before.normal_class, 4);
    assert_eq!(upgrade.occupancy_after.normal_class, 4);
    tx.rollback().await.unwrap();

    let mut equal_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_preview_standard_finished_book_application_for_owned_ordinary_equipment(
            &mut equal_tx,
            owner_id,
            item_id,
            CanonicalEnchant::Protection,
            2,
        )
        .await,
        Err(OrdinaryEnchantApplyPreflightResolverError::Apply(
            EnchantApplyError::LowerOrEqualReplacement {
                enchant: CanonicalEnchant::Protection,
                existing_level: 2,
                incoming_level: 2,
            }
        ))
    ));
    equal_tx.rollback().await.unwrap();

    let mut full_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_preview_standard_finished_book_application_for_owned_ordinary_equipment(
            &mut full_tx,
            owner_id,
            item_id,
            CanonicalEnchant::Guardian,
            1,
        )
        .await,
        Err(OrdinaryEnchantApplyPreflightResolverError::Apply(
            EnchantApplyError::NoFreeSlot {
                family: EnchantSlotFamily::NormalClass,
                occupied: 4,
                unlocked: 4,
            }
        ))
    ));
    full_tx.rollback().await.unwrap();

    sqlx::query(
        "UPDATE item_instance_equipment_structural_state SET normal_enchant_slot_capacity = 5 WHERE item_instance_id = $1",
    )
    .bind(item_id)
    .execute(store.pool())
    .await
    .unwrap();

    let mut unlocked_tx = store.pool().begin().await.unwrap();
    let unlocked = lock_preview_standard_finished_book_application_for_owned_ordinary_equipment(
        &mut unlocked_tx,
        owner_id,
        item_id,
        CanonicalEnchant::Guardian,
        1,
    )
    .await
    .unwrap();
    assert_eq!(unlocked.action, EnchantApplyAction::InsertNew);
    assert_eq!(unlocked.occupancy_after.normal_class, 5);
    assert!(unlocked.resulting_item_requires_equipped_armor_loadout_conflict_validation);
    unlocked_tx.rollback().await.unwrap();
}

#[tokio::test]
async fn authoritative_preflight_rejects_non_enchantable_item() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let definition_key = format!("test.enchant-apply-preflight.flags.{nonce}");
    seed_definition(&store, &definition_key).await;
    let item_id = seed_item(&store, owner_id, &definition_key, &nonce, "blocked").await;
    seed_structural_state(&store, item_id).await;
    sqlx::query("UPDATE item_instances SET is_enchantable = FALSE WHERE id = $1")
        .bind(item_id)
        .execute(store.pool())
        .await
        .unwrap();

    let mut tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_preview_standard_finished_book_application_for_owned_ordinary_equipment(
            &mut tx,
            owner_id,
            item_id,
            CanonicalEnchant::Protection,
            1,
        )
        .await,
        Err(OrdinaryEnchantApplyPreflightResolverError::ItemNotEnchantable)
    ));
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn authoritative_preflight_rejects_starter_item() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let definition_key = format!("test.enchant-apply-preflight.starter.{nonce}");
    seed_definition(&store, &definition_key).await;
    let item_id = seed_item(&store, owner_id, &definition_key, &nonce, "starter").await;
    seed_structural_state(&store, item_id).await;
    sqlx::query("UPDATE item_instances SET is_starter = TRUE WHERE id = $1")
        .bind(item_id)
        .execute(store.pool())
        .await
        .unwrap();

    let mut tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_preview_standard_finished_book_application_for_owned_ordinary_equipment(
            &mut tx,
            owner_id,
            item_id,
            CanonicalEnchant::Protection,
            1,
        )
        .await,
        Err(OrdinaryEnchantApplyPreflightResolverError::StarterEquipment)
    ));
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

async fn seed_definition(store: &PgStore, key: &str) {
    let data = r#"{"tier":"OBSIDIAN","slot":"ARMOR_CHEST"}"#;
    sqlx::query("INSERT INTO item_definitions (key, category, stackable, active, definition_version, rarity, stack_limit, data) VALUES ($1, 'ARMOR', FALSE, TRUE, 1, 'COMMON', NULL, $2::jsonb)")
        .bind(key)
        .bind(data)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO item_definition_versions (key, version, category, stackable, rarity, stack_limit, is_ordinary_equipment, data) VALUES ($1, 1, 'ARMOR', FALSE, 'COMMON', NULL, TRUE, $2::jsonb)")
        .bind(key)
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
    sqlx::query("INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'ENCHANT_APPLY_PREFLIGHT_TEST', 'PENDING', 1, $5, $6)")
        .bind(operation_id)
        .bind(format!(
            "test:enchant-apply-preflight:{nonce}:{suffix}:{operation_id}"
        ))
        .bind(discord_user_id)
        .bind(player_id)
        .bind([107_u8; 32].as_slice())
        .bind([109_u8; 32].as_slice())
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

fn positive_snowflake(nonce: Uuid) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    i64::try_from((raw % 8_000_000_000_000_000_000_u64).max(1)).unwrap()
}
