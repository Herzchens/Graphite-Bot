use graphite_services::{
    CanonicalEnchant, EnchantSlotFamily, EquipmentTier, EquippedFishingRodCastSnapshotError,
    EquippedFishingRodEnchantState, EquippedFishingRodKind, EquippedFishingRodStateError,
    lock_equipped_fishing_rod_cast_snapshot,
};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn ordinary_cast_snapshot_is_pinned_typed_capacity_checked_and_non_mutating() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let item_id = seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "ordinary",
        "NETHERITE",
        37,
        600,
        false,
        5,
        4,
        true,
    )
    .await;
    seed_enchant(&store, item_id, "BAIT_RACK", 3).await;
    seed_enchant(&store, item_id, "CARVING", 1).await;
    seed_enchant(&store, item_id, "MENDING", 1).await;
    let operation_id = seed_operation(&store, player_id, nonce, "ordinary-cast").await;

    let mut tx = store.pool().begin().await.unwrap();
    lock_owner_context(&mut tx, operation_id, player_id).await;
    let snapshot = lock_equipped_fishing_rod_cast_snapshot(&mut tx, player_id)
        .await
        .unwrap();

    assert_eq!(snapshot.player_id, player_id);
    assert_eq!(snapshot.item_instance_id, item_id);
    assert_eq!(snapshot.definition_version, 1);
    assert!(matches!(
        snapshot.kind,
        EquippedFishingRodKind::Ordinary {
            tier: EquipmentTier::Netherite
        }
    ));
    assert_eq!(snapshot.current_durability, Some(37));
    assert_eq!(snapshot.max_durability, Some(600));
    assert!(!snapshot.is_broken);
    assert_eq!(snapshot.normal_enchant_slot_capacity, 5);
    assert_eq!(snapshot.special_enchant_slot_capacity, 4);
    assert_eq!(
        snapshot.embedded_enchants,
        vec![
            EquippedFishingRodEnchantState {
                enchant: CanonicalEnchant::BaitRack,
                level: 3,
            },
            EquippedFishingRodEnchantState {
                enchant: CanonicalEnchant::Carving,
                level: 1,
            },
            EquippedFishingRodEnchantState {
                enchant: CanonicalEnchant::Mending,
                level: 1,
            },
        ]
    );
    tx.rollback().await.unwrap();

    let persisted: (Option<i64>, Option<i64>, bool) = sqlx::query_as(
        "SELECT current_durability, max_durability, is_broken FROM item_instances WHERE id = $1",
    )
    .bind(item_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(persisted, (Some(37), Some(600), false));
    assert_eq!(operation_state(&store, operation_id).await, "PENDING");
}

#[tokio::test]
async fn starter_and_consistently_broken_ordinary_rods_have_explicit_cast_snapshots() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();

    let starter_player = seed_player(&store, positive_snowflake(nonce)).await;
    let starter_item = seed_starter_basic_rod(&store, starter_player, nonce, "starter").await;
    let starter_operation = seed_operation(&store, starter_player, nonce, "starter-cast").await;
    let mut starter_tx = store.pool().begin().await.unwrap();
    lock_owner_context(&mut starter_tx, starter_operation, starter_player).await;
    let starter = lock_equipped_fishing_rod_cast_snapshot(&mut starter_tx, starter_player)
        .await
        .unwrap();
    assert_eq!(starter.item_instance_id, starter_item);
    assert_eq!(starter.kind, EquippedFishingRodKind::StarterBasic);
    assert_eq!(starter.current_durability, None);
    assert_eq!(starter.max_durability, None);
    assert!(!starter.is_broken);
    assert_eq!(starter.normal_enchant_slot_capacity, 0);
    assert_eq!(starter.special_enchant_slot_capacity, 0);
    assert!(starter.embedded_enchants.is_empty());
    starter_tx.rollback().await.unwrap();

    let broken_player = seed_player(&store, next_snowflake(nonce, 1)).await;
    let broken_item = seed_ordinary_rod(
        &store,
        broken_player,
        nonce,
        "broken",
        "OBSIDIAN",
        0,
        900,
        true,
        4,
        3,
        true,
    )
    .await;
    let broken_operation = seed_operation(&store, broken_player, nonce, "broken-cast").await;
    let mut broken_tx = store.pool().begin().await.unwrap();
    lock_owner_context(&mut broken_tx, broken_operation, broken_player).await;
    let broken = lock_equipped_fishing_rod_cast_snapshot(&mut broken_tx, broken_player)
        .await
        .unwrap();
    assert_eq!(broken.item_instance_id, broken_item);
    assert!(matches!(
        broken.kind,
        EquippedFishingRodKind::Ordinary {
            tier: EquipmentTier::Obsidian
        }
    ));
    assert_eq!(broken.current_durability, Some(0));
    assert_eq!(broken.max_durability, Some(900));
    assert!(broken.is_broken);
    broken_tx.rollback().await.unwrap();
}

#[tokio::test]
async fn malformed_durability_wrong_slot_enchants_and_capacity_overflow_fail_closed() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();

    let malformed_player = seed_player(&store, positive_snowflake(nonce)).await;
    seed_ordinary_rod(
        &store,
        malformed_player,
        nonce,
        "malformed",
        "WOOD",
        0,
        600,
        false,
        4,
        3,
        true,
    )
    .await;
    let malformed_operation =
        seed_operation(&store, malformed_player, nonce, "malformed-cast").await;
    let mut malformed_tx = store.pool().begin().await.unwrap();
    lock_owner_context(&mut malformed_tx, malformed_operation, malformed_player).await;
    assert!(matches!(
        lock_equipped_fishing_rod_cast_snapshot(&mut malformed_tx, malformed_player).await,
        Err(EquippedFishingRodCastSnapshotError::InvalidOrdinaryRodDurabilityState)
    ));
    malformed_tx.rollback().await.unwrap();

    let wrong_slot_player = seed_player(&store, next_snowflake(nonce, 1)).await;
    let wrong_slot_item = seed_ordinary_rod(
        &store,
        wrong_slot_player,
        nonce,
        "wrong-slot",
        "STONE",
        10,
        600,
        false,
        4,
        3,
        true,
    )
    .await;
    seed_enchant(&store, wrong_slot_item, "SHARPNESS", 1).await;
    let wrong_slot_operation =
        seed_operation(&store, wrong_slot_player, nonce, "wrong-slot-cast").await;
    let mut wrong_slot_tx = store.pool().begin().await.unwrap();
    lock_owner_context(&mut wrong_slot_tx, wrong_slot_operation, wrong_slot_player).await;
    assert!(matches!(
        lock_equipped_fishing_rod_cast_snapshot(&mut wrong_slot_tx, wrong_slot_player).await,
        Err(
            EquippedFishingRodCastSnapshotError::EmbeddedEnchantWrongEquipmentSlot(
                CanonicalEnchant::Sharpness
            )
        )
    ));
    wrong_slot_tx.rollback().await.unwrap();

    let capacity_player = seed_player(&store, next_snowflake(nonce, 2)).await;
    let capacity_item = seed_ordinary_rod(
        &store,
        capacity_player,
        nonce,
        "capacity",
        "COPPER",
        10,
        600,
        false,
        4,
        3,
        true,
    )
    .await;
    for enchant in ["BAIT_RACK", "LURE", "LUCK", "MENDING", "STRENGTHEN"] {
        seed_enchant(&store, capacity_item, enchant, 1).await;
    }
    let capacity_operation = seed_operation(&store, capacity_player, nonce, "capacity-cast").await;
    let mut capacity_tx = store.pool().begin().await.unwrap();
    lock_owner_context(&mut capacity_tx, capacity_operation, capacity_player).await;
    assert!(matches!(
        lock_equipped_fishing_rod_cast_snapshot(&mut capacity_tx, capacity_player).await,
        Err(
            EquippedFishingRodCastSnapshotError::EmbeddedEnchantOccupancyExceedsCapacity {
                family: EnchantSlotFamily::NormalClass,
                occupied: 5,
                capacity: 4,
            }
        )
    ));
    capacity_tx.rollback().await.unwrap();
}

#[tokio::test]
async fn nonordinary_or_malformed_tier_rods_fail_at_the_shared_identity_boundary() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();

    let special_player = seed_player(&store, positive_snowflake(nonce)).await;
    seed_ordinary_rod(
        &store,
        special_player,
        nonce,
        "special",
        "WOOD",
        10,
        600,
        false,
        4,
        3,
        false,
    )
    .await;
    let special_operation = seed_operation(&store, special_player, nonce, "special-cast").await;
    let mut special_tx = store.pool().begin().await.unwrap();
    lock_owner_context(&mut special_tx, special_operation, special_player).await;
    assert!(matches!(
        lock_equipped_fishing_rod_cast_snapshot(&mut special_tx, special_player).await,
        Err(EquippedFishingRodCastSnapshotError::State(
            EquippedFishingRodStateError::NonOrdinaryFishingRod
        ))
    ));
    special_tx.rollback().await.unwrap();

    let tier_player = seed_player(&store, next_snowflake(nonce, 1)).await;
    seed_ordinary_rod(
        &store,
        tier_player,
        nonce,
        "bad-tier",
        "LEATHER",
        10,
        600,
        false,
        4,
        3,
        true,
    )
    .await;
    let tier_operation = seed_operation(&store, tier_player, nonce, "bad-tier-cast").await;
    let mut tier_tx = store.pool().begin().await.unwrap();
    lock_owner_context(&mut tier_tx, tier_operation, tier_player).await;
    assert!(matches!(
        lock_equipped_fishing_rod_cast_snapshot(&mut tier_tx, tier_player).await,
        Err(EquippedFishingRodCastSnapshotError::State(
            EquippedFishingRodStateError::InvalidOrdinaryRodTierMetadata
        ))
    ));
    tier_tx.rollback().await.unwrap();
}

#[tokio::test]
async fn cast_snapshot_retains_item_slot_structural_and_enchant_locks_until_owner_finishes() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let item_id = seed_ordinary_rod(
        &store, player_id, nonce, "locks", "DIAMOND", 20, 600, false, 4, 3, true,
    )
    .await;
    seed_enchant(&store, item_id, "MENDING", 1).await;
    let operation_id = seed_operation(&store, player_id, nonce, "locks-cast").await;

    let mut owner = store.pool().begin().await.unwrap();
    lock_owner_context(&mut owner, operation_id, player_id).await;
    lock_equipped_fishing_rod_cast_snapshot(&mut owner, player_id)
        .await
        .unwrap();

    let mut item_contender = store.pool().begin().await.unwrap();
    let item_lock = sqlx::query("SELECT id FROM item_instances WHERE id = $1 FOR UPDATE NOWAIT")
        .bind(item_id)
        .fetch_one(&mut *item_contender)
        .await;
    assert_lock_not_available(item_lock.unwrap_err());
    item_contender.rollback().await.unwrap();

    let mut slot_contender = store.pool().begin().await.unwrap();
    let slot_lock = sqlx::query(
        "SELECT item_instance_id FROM equipment_slots WHERE player_id = $1 AND slot = 'FISHING_ROD' FOR UPDATE NOWAIT",
    )
    .bind(player_id)
    .fetch_one(&mut *slot_contender)
    .await;
    assert_lock_not_available(slot_lock.unwrap_err());
    slot_contender.rollback().await.unwrap();

    let mut structural_contender = store.pool().begin().await.unwrap();
    let structural_lock = sqlx::query(
        "SELECT item_instance_id FROM item_instance_equipment_structural_state WHERE item_instance_id = $1 FOR UPDATE NOWAIT",
    )
    .bind(item_id)
    .fetch_one(&mut *structural_contender)
    .await;
    assert_lock_not_available(structural_lock.unwrap_err());
    structural_contender.rollback().await.unwrap();

    let mut enchant_contender = store.pool().begin().await.unwrap();
    let enchant_lock = sqlx::query(
        "SELECT enchant_key FROM item_instance_embedded_enchants WHERE item_instance_id = $1 AND enchant_key = 'MENDING' FOR UPDATE NOWAIT",
    )
    .bind(item_id)
    .fetch_one(&mut *enchant_contender)
    .await;
    assert_lock_not_available(enchant_lock.unwrap_err());
    enchant_contender.rollback().await.unwrap();

    owner.rollback().await.unwrap();
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

async fn seed_operation(store: &PgStore, player_id: Uuid, nonce: Uuid, suffix: &str) -> Uuid {
    let operation_id = Uuid::now_v7();
    let discord_user_id: i64 =
        sqlx::query_scalar("SELECT discord_user_id FROM players WHERE id = $1")
            .bind(player_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'FISHING_ROD_CAST_SNAPSHOT_TEST', 'PENDING', 1, $5, $6)",
    )
    .bind(operation_id)
    .bind(format!("test:fishing-rod-cast-snapshot:{nonce}:{suffix}:{operation_id}"))
    .bind(discord_user_id)
    .bind(player_id)
    .bind([71_u8; 32].as_slice())
    .bind([73_u8; 32].as_slice())
    .execute(store.pool())
    .await
    .unwrap();
    operation_id
}

#[allow(clippy::too_many_arguments)]
async fn seed_ordinary_rod(
    store: &PgStore,
    player_id: Uuid,
    nonce: Uuid,
    suffix: &str,
    tier: &str,
    current_durability: i64,
    max_durability: i64,
    is_broken: bool,
    normal_capacity: i16,
    special_capacity: i16,
    ordinary: bool,
) -> Uuid {
    let definition_key = format!("test.fishing-rod-cast-snapshot.{suffix}.{nonce}");
    let data = serde_json::json!({"tier": tier});
    sqlx::query(
        "INSERT INTO item_definitions (key, category, stackable, active, definition_version, rarity, stack_limit, data) VALUES ($1, 'FISHING_ROD', FALSE, TRUE, 1, 'COMMON', NULL, $2)",
    )
    .bind(&definition_key)
    .bind(&data)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO item_definition_versions (key, version, category, stackable, rarity, stack_limit, is_ordinary_equipment, data) VALUES ($1, 1, 'FISHING_ROD', FALSE, 'COMMON', NULL, $2, $3)",
    )
    .bind(&definition_key)
    .bind(ordinary)
    .bind(data)
    .execute(store.pool())
    .await
    .unwrap();

    let creation_operation =
        seed_operation(store, player_id, nonce, &format!("create-{suffix}")).await;
    let item_id = Uuid::now_v7();
    let mut tx = store.pool().begin().await.unwrap();
    sqlx::query(
        "INSERT INTO item_instances (id, definition_key, owner_player_id, created_by_operation_id, location, definition_version, current_durability, max_durability, is_broken) VALUES ($1, $2, $3, $4, 'EQUIPPED', 1, $5, $6, $7)",
    )
    .bind(item_id)
    .bind(&definition_key)
    .bind(player_id)
    .bind(creation_operation)
    .bind(current_durability)
    .bind(max_durability)
    .bind(is_broken)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO equipment_slots (player_id, slot, item_instance_id) VALUES ($1, 'FISHING_ROD', $2)",
    )
    .bind(player_id)
    .bind(item_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO item_instance_equipment_structural_state (item_instance_id, creation_roll_numerator, creation_roll_denominator, upgrade_level, normal_enchant_slot_capacity, special_enchant_slot_capacity) VALUES ($1, 1, 2, 0, $2, $3)",
    )
    .bind(item_id)
    .bind(normal_capacity)
    .bind(special_capacity)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    item_id
}

async fn seed_starter_basic_rod(
    store: &PgStore,
    player_id: Uuid,
    nonce: Uuid,
    suffix: &str,
) -> Uuid {
    let creation_operation =
        seed_operation(store, player_id, nonce, &format!("create-{suffix}")).await;
    let item_id = Uuid::now_v7();
    let mut tx = store.pool().begin().await.unwrap();
    sqlx::query(
        "INSERT INTO item_instances (id, definition_key, owner_player_id, created_by_operation_id, location, definition_version, is_starter, is_account_bound, is_tradeable, is_sellable, is_discardable, is_enchantable, is_upgradeable, is_unbreakable, is_repairable) VALUES ($1, 'equipment.rod.basic.starter', $2, $3, 'EQUIPPED', 1, TRUE, TRUE, FALSE, FALSE, FALSE, FALSE, FALSE, TRUE, FALSE)",
    )
    .bind(item_id)
    .bind(player_id)
    .bind(creation_operation)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO equipment_slots (player_id, slot, item_instance_id) VALUES ($1, 'FISHING_ROD', $2)",
    )
    .bind(player_id)
    .bind(item_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    item_id
}

async fn seed_enchant(store: &PgStore, item_id: Uuid, key: &str, level: i16) {
    sqlx::query(
        "INSERT INTO item_instance_embedded_enchants (item_instance_id, enchant_key, level) VALUES ($1, $2, $3)",
    )
    .bind(item_id)
    .bind(key)
    .bind(level)
    .execute(store.pool())
    .await
    .unwrap();
}

async fn lock_owner_context(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
) {
    let _: String = sqlx::query_scalar("SELECT state FROM operations WHERE id = $1 FOR UPDATE")
        .bind(operation_id)
        .fetch_one(&mut **tx)
        .await
        .unwrap();
    let _: String = sqlx::query_scalar("SELECT status FROM players WHERE id = $1 FOR UPDATE")
        .bind(player_id)
        .fetch_one(&mut **tx)
        .await
        .unwrap();
}

async fn operation_state(store: &PgStore, operation_id: Uuid) -> String {
    sqlx::query_scalar("SELECT state FROM operations WHERE id = $1")
        .bind(operation_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
}

fn assert_lock_not_available(error: sqlx::Error) {
    let sqlx::Error::Database(database) = error else {
        panic!("expected PostgreSQL lock error, got {error:?}");
    };
    assert_eq!(database.code().as_deref(), Some("55P03"));
}

fn positive_snowflake(nonce: Uuid) -> i64 {
    next_snowflake(nonce, 0)
}

fn next_snowflake(nonce: Uuid, offset: u64) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    let value = (raw % 7_999_999_999_999_999_000_u64)
        .saturating_add(1)
        .saturating_add(offset);
    i64::try_from(value).unwrap()
}
