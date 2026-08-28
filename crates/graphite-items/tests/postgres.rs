use graphite_items::{ItemError, ItemService};
use graphite_store::PgStore;
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
async fn storage_and_equipment_are_capacity_safe_idempotent_and_authoritative() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL is not set; skipping PostgreSQL integration test");
        return;
    };

    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate().await.unwrap();

    let nonce = Uuid::now_v7();
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    let discord_user_id = (raw % 8_000_000_000_000_000_000_u64).max(1);
    let player_id = Uuid::now_v7();
    sqlx::query("INSERT INTO players (id, discord_user_id) VALUES ($1, $2)")
        .bind(player_id)
        .bind(i64::try_from(discord_user_id).unwrap())
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO player_balances (player_id) VALUES ($1)")
        .bind(player_id)
        .execute(store.pool())
        .await
        .unwrap();

    let storage_profile_count: i64 =
        sqlx::query("SELECT COUNT(*) AS count FROM player_storage_profiles WHERE player_id = $1")
            .bind(player_id)
            .fetch_one(store.pool())
            .await
            .unwrap()
            .try_get("count")
            .unwrap();
    assert_eq!(storage_profile_count, 1);

    let stack_key = format!("test.stack.{nonce}");
    seed_definition(
        &store,
        &stack_key,
        "MATERIAL",
        true,
        Some(64),
        "COMMON",
        serde_json::json!({}),
    )
    .await;

    let items = ItemService::new(store.clone());
    let fill_key = format!("test:stack-fill:{nonce}");
    let fill = items
        .deliver_stack_to_item_bag(discord_user_id, &stack_key, 36 * 64, &fill_key)
        .await
        .unwrap();
    assert!(!fill.pending);
    let fill_retry = items
        .deliver_stack_to_item_bag(discord_user_id, &stack_key, 36 * 64, &fill_key)
        .await
        .unwrap();
    assert_eq!(fill, fill_retry);

    let bag = items.item_bag(discord_user_id).await.unwrap();
    assert_eq!(bag.capacity_slots, 36);
    assert_eq!(bag.used_slots, 36);
    assert_eq!(bag.stacks[0].quantity, 2304);

    let overflow = items
        .deliver_stack_to_item_bag(
            discord_user_id,
            &stack_key,
            1,
            &format!("test:stack-overflow:{nonce}"),
        )
        .await
        .unwrap();
    assert!(overflow.pending);
    let bag_after_overflow = items.item_bag(discord_user_id).await.unwrap();
    assert_eq!(bag_after_overflow.used_slots, 36);
    assert_eq!(bag_after_overflow.stacks[0].quantity, 2304);
    assert_eq!(bag_after_overflow.pending_deliveries, 1);

    let equip_definition = format!("test.pickaxe.{nonce}");
    seed_definition(
        &store,
        &equip_definition,
        "PICKAXE",
        false,
        None,
        "UNCOMMON",
        serde_json::json!({}),
    )
    .await;
    let creation_operation = seed_operation(&store, player_id, discord_user_id, &nonce).await;
    let item_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO item_instances (
            id, definition_key, definition_version, owner_player_id,
            created_by_operation_id, location
        )
        VALUES ($1, $2, 1, $3, $4, 'TOOL_LOCKER')
        "#,
    )
    .bind(item_id)
    .bind(&equip_definition)
    .bind(player_id)
    .bind(creation_operation)
    .execute(store.pool())
    .await
    .unwrap();

    let equip_key = format!("test:equip:{nonce}");
    let equipped = items
        .equip(discord_user_id, item_id, &equip_key)
        .await
        .unwrap();
    assert_eq!(equipped.slot.as_deref(), Some("PICKAXE"));
    assert_eq!(
        items
            .equip(discord_user_id, item_id, &equip_key)
            .await
            .unwrap(),
        equipped
    );
    let loadout = items.equipment(discord_user_id).await.unwrap();
    assert_eq!(loadout.len(), 1);
    assert_eq!(loadout[0].item.item_instance_id, item_id);

    let inspected = items.item(discord_user_id, item_id).await.unwrap();
    assert_eq!(inspected.location, "EQUIPPED");
    assert_eq!(inspected.definition_version, 1);

    let unequipped = items
        .unequip(discord_user_id, item_id, &format!("test:unequip:{nonce}"))
        .await
        .unwrap();
    assert_eq!(unequipped.slot.as_deref(), Some("PICKAXE"));
    assert!(items.equipment(discord_user_id).await.unwrap().is_empty());
    assert_eq!(items.locker(discord_user_id).await.unwrap().len(), 1);

    let catch_definition = format!("test.fish.{nonce}");
    seed_definition(
        &store,
        &catch_definition,
        "FISH",
        false,
        None,
        "RARE",
        serde_json::json!({}),
    )
    .await;
    let catch_operation = seed_operation(&store, player_id, discord_user_id, &Uuid::now_v7()).await;
    sqlx::query(
        r#"
        INSERT INTO item_instances (
            id, definition_key, definition_version, owner_player_id,
            created_by_operation_id, location, catch_weight_grams
        )
        VALUES ($1, $2, 1, $3, $4, 'CATCH_BAG', 1234)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(&catch_definition)
    .bind(player_id)
    .bind(catch_operation)
    .execute(store.pool())
    .await
    .unwrap();
    let catch_bag = items.catch_bag(discord_user_id).await.unwrap();
    assert_eq!(catch_bag.capacity_grams, 1_000_000);
    assert_eq!(catch_bag.used_grams, 1234);
    assert_eq!(catch_bag.catches.len(), 1);

    let conflict = items
        .deliver_stack_to_item_bag(discord_user_id, &stack_key, 2, &fill_key)
        .await;
    assert!(matches!(conflict, Err(ItemError::IdempotencyConflict)));
}

async fn seed_definition(
    store: &PgStore,
    key: &str,
    category: &str,
    stackable: bool,
    stack_limit: Option<i64>,
    rarity: &str,
    data: serde_json::Value,
) {
    sqlx::query(
        r#"
        INSERT INTO item_definitions (
            key, category, stackable, definition_version, rarity, stack_limit, data
        )
        VALUES ($1, $2, $3, 1, $4, $5, $6)
        "#,
    )
    .bind(key)
    .bind(category)
    .bind(stackable)
    .bind(rarity)
    .bind(stack_limit)
    .bind(&data)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO item_definition_versions (
            key, version, category, stackable, rarity, stack_limit, data
        )
        VALUES ($1, 1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(key)
    .bind(category)
    .bind(stackable)
    .bind(rarity)
    .bind(stack_limit)
    .bind(data)
    .execute(store.pool())
    .await
    .unwrap();
}

async fn seed_operation(
    store: &PgStore,
    player_id: Uuid,
    discord_user_id: u64,
    nonce: &Uuid,
) -> Uuid {
    let operation_id = Uuid::now_v7();
    let key = format!("test:asset-create:{nonce}:{operation_id}");
    sqlx::query(
        r#"
        INSERT INTO operations (
            id, external_request_key, actor_discord_user_id, player_id, kind, state,
            policy_version, request_hash, rng_root, result, committed_at
        )
        VALUES ($1, $2, $3, $4, 'TEST_ASSET_CREATE', 'COMMITTED', 1, $5, $6, '{}'::jsonb, now())
        "#,
    )
    .bind(operation_id)
    .bind(key)
    .bind(i64::try_from(discord_user_id).unwrap())
    .bind(player_id)
    .bind([7_u8; 32].as_slice())
    .bind([9_u8; 32].as_slice())
    .execute(store.pool())
    .await
    .unwrap();
    operation_id
}
