use graphite_core::CanonicalEnchant;
use graphite_items::{ItemError, ItemService};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn conflicting_survival_core_target_is_rejected_without_mutating_equipment() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let discord_user_id = positive_snowflake(nonce);
    let player_id = seed_player(&store, discord_user_id).await;
    let legs_key = format!("test.equip-guard.legs.{nonce}");
    let chest_key = format!("test.equip-guard.chest.{nonce}");
    seed_armor_definition(&store, &legs_key, "ARMOR_LEGS").await;
    seed_armor_definition(&store, &chest_key, "ARMOR_CHEST").await;

    let legs_id = seed_item(
        &store,
        player_id,
        discord_user_id,
        &legs_key,
        &nonce,
        "legs",
    )
    .await;
    let chest_id = seed_item(
        &store,
        player_id,
        discord_user_id,
        &chest_key,
        &nonce,
        "chest",
    )
    .await;
    seed_enchant(&store, legs_id, "GUARDIAN", 1).await;
    seed_enchant(&store, chest_id, "NINE_LIFE", 3).await;

    let items = ItemService::new(store.clone());
    items
        .equip(
            u64::try_from(discord_user_id).unwrap(),
            legs_id,
            &format!("test:equip-guard:legs:{nonce}"),
        )
        .await
        .unwrap();

    let result = items
        .equip(
            u64::try_from(discord_user_id).unwrap(),
            chest_id,
            &format!("test:equip-guard:conflict:{nonce}"),
        )
        .await;
    assert!(matches!(
        result,
        Err(ItemError::EquippedArmorEnchantConflict {
            left: CanonicalEnchant::Guardian,
            right: CanonicalEnchant::NineLife,
            ..
        }) | Err(ItemError::EquippedArmorEnchantConflict {
            left: CanonicalEnchant::NineLife,
            right: CanonicalEnchant::Guardian,
            ..
        })
    ));

    assert_eq!(item_location(&store, legs_id).await, "EQUIPPED");
    assert_eq!(item_location(&store, chest_id).await, "TOOL_LOCKER");
    let chest_slot: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM equipment_slots WHERE player_id = $1 AND item_instance_id = $2",
    )
    .bind(player_id)
    .bind(chest_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(chest_slot, 0);
}

#[tokio::test]
async fn same_slot_replacement_excludes_the_displaced_armor_from_the_prospective_loadout() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let discord_user_id = positive_snowflake(nonce);
    let player_id = seed_player(&store, discord_user_id).await;
    let old_key = format!("test.equip-guard.old-chest.{nonce}");
    let new_key = format!("test.equip-guard.new-chest.{nonce}");
    seed_armor_definition(&store, &old_key, "ARMOR_CHEST").await;
    seed_armor_definition(&store, &new_key, "ARMOR_CHEST").await;

    let old_id = seed_item(&store, player_id, discord_user_id, &old_key, &nonce, "old").await;
    let new_id = seed_item(&store, player_id, discord_user_id, &new_key, &nonce, "new").await;
    seed_enchant(&store, old_id, "GUARDIAN", 1).await;
    seed_enchant(&store, new_id, "NINE_LIFE", 2).await;

    let items = ItemService::new(store.clone());
    items
        .equip(
            u64::try_from(discord_user_id).unwrap(),
            old_id,
            &format!("test:equip-guard:old:{nonce}"),
        )
        .await
        .unwrap();
    let receipt = items
        .equip(
            u64::try_from(discord_user_id).unwrap(),
            new_id,
            &format!("test:equip-guard:replace:{nonce}"),
        )
        .await
        .unwrap();

    assert_eq!(receipt.displaced_item_instance_id, Some(old_id));
    assert_eq!(item_location(&store, old_id).await, "TOOL_LOCKER");
    assert_eq!(item_location(&store, new_id).await, "EQUIPPED");
}

#[tokio::test]
async fn identical_survival_core_identity_remains_legal_across_equipped_armor() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let discord_user_id = positive_snowflake(nonce);
    let player_id = seed_player(&store, discord_user_id).await;
    let legs_key = format!("test.equip-guard.same.legs.{nonce}");
    let chest_key = format!("test.equip-guard.same.chest.{nonce}");
    seed_armor_definition(&store, &legs_key, "ARMOR_LEGS").await;
    seed_armor_definition(&store, &chest_key, "ARMOR_CHEST").await;

    let legs_id = seed_item(
        &store,
        player_id,
        discord_user_id,
        &legs_key,
        &nonce,
        "legs",
    )
    .await;
    let chest_id = seed_item(
        &store,
        player_id,
        discord_user_id,
        &chest_key,
        &nonce,
        "chest",
    )
    .await;
    seed_enchant(&store, legs_id, "GUARDIAN", 1).await;
    seed_enchant(&store, chest_id, "GUARDIAN", 4).await;

    let items = ItemService::new(store.clone());
    items
        .equip(
            u64::try_from(discord_user_id).unwrap(),
            legs_id,
            &format!("test:equip-guard:same-legs:{nonce}"),
        )
        .await
        .unwrap();
    items
        .equip(
            u64::try_from(discord_user_id).unwrap(),
            chest_id,
            &format!("test:equip-guard:same-chest:{nonce}"),
        )
        .await
        .unwrap();

    assert_eq!(item_location(&store, legs_id).await, "EQUIPPED");
    assert_eq!(item_location(&store, chest_id).await, "EQUIPPED");
}

#[tokio::test]
async fn unknown_persisted_enchant_identity_on_target_fails_closed_before_equip() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let discord_user_id = positive_snowflake(nonce);
    let player_id = seed_player(&store, discord_user_id).await;
    let chest_key = format!("test.equip-guard.unknown.chest.{nonce}");
    seed_armor_definition(&store, &chest_key, "ARMOR_CHEST").await;

    let chest_id = seed_item(
        &store,
        player_id,
        discord_user_id,
        &chest_key,
        &nonce,
        "chest",
    )
    .await;
    seed_enchant(&store, chest_id, "guardian", 1).await;

    let items = ItemService::new(store.clone());
    let result = items
        .equip(
            u64::try_from(discord_user_id).unwrap(),
            chest_id,
            &format!("test:equip-guard:unknown:{nonce}"),
        )
        .await;
    assert!(matches!(
        result,
        Err(ItemError::UnknownEmbeddedEnchantKey {
            item_instance_id,
            ref key,
        }) if item_instance_id == chest_id && key == "guardian"
    ));
    assert_eq!(item_location(&store, chest_id).await, "TOOL_LOCKER");
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
    discord_user_id: i64,
    definition_key: &str,
    nonce: &Uuid,
    suffix: &str,
) -> Uuid {
    let operation_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO operations (
            id, external_request_key, actor_discord_user_id, player_id, kind, state,
            policy_version, request_hash, rng_root, result, committed_at
        )
        VALUES ($1, $2, $3, $4, 'EQUIP_GUARD_TEST_ASSET', 'COMMITTED', 1, $5, $6, '{}'::jsonb, now())
        "#,
    )
    .bind(operation_id)
    .bind(format!("test:equip-guard:asset:{nonce}:{suffix}:{operation_id}"))
    .bind(discord_user_id)
    .bind(player_id)
    .bind([191_u8; 32].as_slice())
    .bind([193_u8; 32].as_slice())
    .execute(store.pool())
    .await
    .unwrap();

    let item_id = Uuid::now_v7();
    sqlx::query("INSERT INTO item_instances (id, definition_key, definition_version, owner_player_id, created_by_operation_id, location) VALUES ($1, $2, 1, $3, $4, 'TOOL_LOCKER')")
        .bind(item_id)
        .bind(definition_key)
        .bind(player_id)
        .bind(operation_id)
        .execute(store.pool())
        .await
        .unwrap();
    item_id
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
