use graphite_progression::account_total_xp_for_level;
use graphite_services::{
    CanonicalEnchant, EquipmentTier, EquippedFishingRodKind, FishingArea, FishingAreaAccessOrigin,
    ManualFishingCastPreflightError, NATIVE_ACTIVE_BAIT_CATEGORY_SLOTS,
    lock_manual_fishing_cast_preflight,
};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn starter_pool_preflight_accepts_starter_basic_and_uses_native_bait_capacity() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let item_id = seed_starter_basic_rod(&store, player_id, nonce, "starter").await;
    let operation_id = seed_operation(&store, player_id, nonce, "starter-cast").await;

    let mut tx = store.pool().begin().await.unwrap();
    let preflight = lock_manual_fishing_cast_preflight(
        &mut tx,
        operation_id,
        player_id,
        FishingArea::StarterPool,
    )
    .await
    .unwrap();

    assert_eq!(preflight.operation_id, operation_id);
    assert_eq!(preflight.player_id, player_id);
    assert_eq!(
        preflight.area_access.origin,
        FishingAreaAccessOrigin::StarterPoolDefault
    );
    assert_eq!(preflight.rod.item_instance_id, item_id);
    assert_eq!(preflight.rod.kind, EquippedFishingRodKind::StarterBasic);
    assert_eq!(
        preflight.bait_capacity.active_bait_category_slots,
        NATIVE_ACTIVE_BAIT_CATEGORY_SLOTS
    );
    assert_eq!(preflight.bait_capacity.bait_rack_level, None);
    tx.rollback().await.unwrap();

    assert_eq!(unlock_count(&store, player_id).await, 0);
    assert_eq!(operation_state(&store, operation_id).await, "PENDING");
}

#[tokio::test]
async fn first_river_cast_preflight_composes_permanent_unlock_and_locked_bait_rack_level() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    set_account_level(&store, player_id, 10).await;
    let item_id = seed_ordinary_rod(
        &store, player_id, nonce, "wood", "WOOD", 200, 600, false, 4, 3,
    )
    .await;
    seed_enchant(&store, item_id, "BAIT_RACK", 2).await;
    seed_enchant(&store, item_id, "MENDING", 1).await;
    let operation_id = seed_operation(&store, player_id, nonce, "river-cast").await;

    let mut tx = store.pool().begin().await.unwrap();
    let preflight =
        lock_manual_fishing_cast_preflight(&mut tx, operation_id, player_id, FishingArea::River)
            .await
            .unwrap();

    assert_eq!(
        preflight.area_access.origin,
        FishingAreaAccessOrigin::NewlyUnlocked
    );
    assert_eq!(
        preflight.area_access.granted_by_operation_id,
        Some(operation_id)
    );
    assert!(preflight.area_access.first_unlock_preview.is_some());
    assert_eq!(
        preflight.rod.kind,
        EquippedFishingRodKind::Ordinary {
            tier: EquipmentTier::Wood
        }
    );
    assert_eq!(preflight.bait_capacity.bait_rack_level, Some(2));
    assert_eq!(preflight.bait_capacity.active_bait_category_slots, 5);
    assert!(
        preflight
            .rod
            .embedded_enchants
            .iter()
            .any(|state| state.enchant == CanonicalEnchant::BaitRack && state.level == 2)
    );

    tx.rollback().await.unwrap();
    assert_eq!(unlock_count(&store, player_id).await, 0);
    assert_eq!(operation_state(&store, operation_id).await, "PENDING");
}

#[tokio::test]
async fn persisted_area_preflight_prelocks_progression_before_cast_item_state() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    seed_ordinary_rod(
        &store,
        player_id,
        nonce,
        "persisted",
        "WOOD",
        200,
        600,
        false,
        4,
        3,
    )
    .await;

    let grant_operation = seed_operation(&store, player_id, nonce, "persisted-grant").await;
    sqlx::query(
        "INSERT INTO player_fishing_area_unlocks (player_id, area, granted_by_operation_id) VALUES ($1, 'RIVER', $2)",
    )
    .bind(player_id)
    .bind(grant_operation)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query("UPDATE operations SET state = 'COMMITTED' WHERE id = $1")
        .bind(grant_operation)
        .execute(store.pool())
        .await
        .unwrap();

    let operation_id = seed_operation(&store, player_id, nonce, "persisted-cast").await;
    let mut owner = store.pool().begin().await.unwrap();
    let preflight =
        lock_manual_fishing_cast_preflight(&mut owner, operation_id, player_id, FishingArea::River)
            .await
            .unwrap();
    assert_eq!(
        preflight.area_access.origin,
        FishingAreaAccessOrigin::Persisted
    );

    let mut contender = store.pool().begin().await.unwrap();
    let progression_lock = sqlx::query_scalar::<_, Uuid>(
        "SELECT player_id FROM player_progression WHERE player_id = $1 FOR UPDATE NOWAIT",
    )
    .bind(player_id)
    .fetch_one(&mut *contender)
    .await;
    assert_lock_not_available(progression_lock.unwrap_err());
    contender.rollback().await.unwrap();
    owner.rollback().await.unwrap();
}

#[tokio::test]
async fn per_cast_starter_pool_only_and_broken_rod_guards_fail_closed() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();

    let starter_player = seed_player(&store, positive_snowflake(nonce)).await;
    seed_starter_basic_rod(&store, starter_player, nonce, "starter-outside-pool").await;
    let grant_operation = seed_operation(&store, starter_player, nonce, "river-grant").await;
    sqlx::query(
        "INSERT INTO player_fishing_area_unlocks (player_id, area, granted_by_operation_id) VALUES ($1, 'RIVER', $2)",
    )
    .bind(starter_player)
    .bind(grant_operation)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query("UPDATE operations SET state = 'COMMITTED' WHERE id = $1")
        .bind(grant_operation)
        .execute(store.pool())
        .await
        .unwrap();
    let starter_operation = seed_operation(&store, starter_player, nonce, "starter-river").await;
    let mut starter_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_manual_fishing_cast_preflight(
            &mut starter_tx,
            starter_operation,
            starter_player,
            FishingArea::River,
        )
        .await,
        Err(ManualFishingCastPreflightError::StarterBasicRodOutsidePool)
    ));
    starter_tx.rollback().await.unwrap();

    let broken_player = seed_player(&store, next_snowflake(nonce, 1)).await;
    seed_ordinary_rod(
        &store,
        broken_player,
        nonce,
        "broken",
        "WOOD",
        0,
        600,
        true,
        4,
        3,
    )
    .await;
    let broken_operation = seed_operation(&store, broken_player, nonce, "broken-pool").await;
    let mut broken_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_manual_fishing_cast_preflight(
            &mut broken_tx,
            broken_operation,
            broken_player,
            FishingArea::StarterPool,
        )
        .await,
        Err(ManualFishingCastPreflightError::BrokenFishingRod)
    ));
    broken_tx.rollback().await.unwrap();

    assert_eq!(operation_state(&store, starter_operation).await, "PENDING");
    assert_eq!(operation_state(&store, broken_operation).await, "PENDING");
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
        "INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'MANUAL_FISHING_CAST_PREFLIGHT_TEST', 'PENDING', 1, $5, $6)",
    )
    .bind(operation_id)
    .bind(format!("test:manual-fishing-cast-preflight:{nonce}:{suffix}:{operation_id}"))
    .bind(discord_user_id)
    .bind(player_id)
    .bind([79_u8; 32].as_slice())
    .bind([83_u8; 32].as_slice())
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
) -> Uuid {
    let definition_key = format!("test.manual-fishing-cast-preflight.{suffix}.{nonce}");
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
        "INSERT INTO item_definition_versions (key, version, category, stackable, rarity, stack_limit, is_ordinary_equipment, data) VALUES ($1, 1, 'FISHING_ROD', FALSE, 'COMMON', NULL, TRUE, $2)",
    )
    .bind(&definition_key)
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

async fn set_account_level(store: &PgStore, player_id: Uuid, level: u16) {
    let account_xp = account_total_xp_for_level(level).unwrap();
    sqlx::query(
        "UPDATE player_progression SET account_xp = $1, updated_at = now() WHERE player_id = $2",
    )
    .bind(account_xp)
    .bind(player_id)
    .execute(store.pool())
    .await
    .unwrap();
}

async fn unlock_count(store: &PgStore, player_id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM player_fishing_area_unlocks WHERE player_id = $1")
        .bind(player_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
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
