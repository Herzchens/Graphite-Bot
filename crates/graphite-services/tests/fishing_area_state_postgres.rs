use graphite_progression::account_total_xp_for_level;
use graphite_services::{
    FishingArea, FishingAreaAccessError, FishingAreaAccessOrigin,
    lock_or_grant_fishing_area_first_unlock,
};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn starter_pool_is_implicit_and_never_persists_an_unlock_row() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce), 0).await;
    let operation_id = seed_operation(&store, player_id, &nonce, "pool").await;

    let mut tx = store.pool().begin().await.unwrap();
    let access = lock_or_grant_fishing_area_first_unlock(
        &mut tx,
        operation_id,
        player_id,
        FishingArea::StarterPool,
    )
    .await
    .unwrap();
    assert_eq!(access.origin, FishingAreaAccessOrigin::StarterPoolDefault);
    assert_eq!(access.granted_by_operation_id, None);
    assert_eq!(access.unlocked_at, None);
    assert_eq!(access.first_unlock_preview, None);
    tx.commit().await.unwrap();

    assert_eq!(unlock_count(&store, player_id).await, 0);
}

#[tokio::test]
async fn river_unlock_uses_exact_authoritative_level_and_ordinary_rod_then_stays_permanent() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce), 0).await;
    let definition_key = format!("test.fishing-area.wood.{nonce}");
    seed_rod_definition(&store, &definition_key, "WOOD", true).await;
    let item_id =
        seed_equipped_rod(&store, player_id, &definition_key, false, &nonce, "wood").await;

    set_account_level(&store, player_id, 9).await;
    let below_operation = seed_operation(&store, player_id, &nonce, "river-below").await;
    let mut below_tx = store.pool().begin().await.unwrap();
    let error = lock_or_grant_fishing_area_first_unlock(
        &mut below_tx,
        below_operation,
        player_id,
        FishingArea::River,
    )
    .await
    .unwrap_err();
    match error {
        FishingAreaAccessError::FirstUnlockRequirementsNotMet { preview } => {
            assert!(!preview.account_level_met);
            assert!(preview.rod_requirement_met);
            assert!(!preview.eligible_for_first_unlock);
        }
        other => panic!("unexpected error: {other:?}"),
    }
    below_tx.rollback().await.unwrap();
    assert_eq!(unlock_count(&store, player_id).await, 0);

    set_account_level(&store, player_id, 10).await;
    let unlock_operation = seed_operation(&store, player_id, &nonce, "river-unlock").await;
    let mut unlock_tx = store.pool().begin().await.unwrap();
    let unlocked = lock_or_grant_fishing_area_first_unlock(
        &mut unlock_tx,
        unlock_operation,
        player_id,
        FishingArea::River,
    )
    .await
    .unwrap();
    assert_eq!(unlocked.origin, FishingAreaAccessOrigin::NewlyUnlocked);
    assert_eq!(unlocked.granted_by_operation_id, Some(unlock_operation));
    let preview = unlocked.first_unlock_preview.unwrap();
    assert_eq!(preview.policy.minimum_account_level, Some(10));
    assert_eq!(
        preview.policy.minimum_ordinary_rod_tier,
        Some(graphite_services::EquipmentTier::Wood)
    );
    assert!(preview.eligible_for_first_unlock);
    unlock_tx.commit().await.unwrap();
    assert_eq!(unlock_count(&store, player_id).await, 1);

    set_account_level(&store, player_id, 1).await;
    unequip_rod(&store, player_id, item_id).await;
    let replay_operation = seed_operation(&store, player_id, &nonce, "river-persisted").await;
    let mut replay_tx = store.pool().begin().await.unwrap();
    let persisted = lock_or_grant_fishing_area_first_unlock(
        &mut replay_tx,
        replay_operation,
        player_id,
        FishingArea::River,
    )
    .await
    .unwrap();
    assert_eq!(persisted.origin, FishingAreaAccessOrigin::Persisted);
    assert_eq!(persisted.granted_by_operation_id, Some(unlock_operation));
    assert_eq!(persisted.first_unlock_preview, None);
    replay_tx.commit().await.unwrap();
    assert_eq!(unlock_count(&store, player_id).await, 1);
}

#[tokio::test]
async fn gold_is_a_deep_sea_side_grade_but_never_satisfies_the_abyss_gate() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce), 1).await;
    set_account_level(&store, player_id, 100).await;
    let definition_key = format!("test.fishing-area.gold.{nonce}");
    seed_rod_definition(&store, &definition_key, "GOLD", true).await;
    let _item_id =
        seed_equipped_rod(&store, player_id, &definition_key, false, &nonce, "gold").await;

    let deep_operation = seed_operation(&store, player_id, &nonce, "deep-gold").await;
    let mut deep_tx = store.pool().begin().await.unwrap();
    let deep = lock_or_grant_fishing_area_first_unlock(
        &mut deep_tx,
        deep_operation,
        player_id,
        FishingArea::DeepSea,
    )
    .await
    .unwrap();
    assert_eq!(deep.origin, FishingAreaAccessOrigin::NewlyUnlocked);
    assert!(
        deep.first_unlock_preview
            .unwrap()
            .policy
            .gold_counts_as_side_grade
    );
    deep_tx.commit().await.unwrap();

    let abyss_operation = seed_operation(&store, player_id, &nonce, "abyss-gold").await;
    let mut abyss_tx = store.pool().begin().await.unwrap();
    let error = lock_or_grant_fishing_area_first_unlock(
        &mut abyss_tx,
        abyss_operation,
        player_id,
        FishingArea::Abyss,
    )
    .await
    .unwrap_err();
    match error {
        FishingAreaAccessError::FirstUnlockRequirementsNotMet { preview } => {
            assert!(preview.rebirth_met);
            assert!(!preview.rod_requirement_met);
            assert!(!preview.eligible_for_first_unlock);
        }
        other => panic!("unexpected error: {other:?}"),
    }
    abyss_tx.rollback().await.unwrap();
}

#[tokio::test]
async fn abyss_unlocks_at_rebirth_one_with_netherite_and_survives_later_rebirth_change() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce), 1).await;
    let definition_key = format!("test.fishing-area.netherite.{nonce}");
    seed_rod_definition(&store, &definition_key, "NETHERITE", true).await;
    let item_id = seed_equipped_rod(
        &store,
        player_id,
        &definition_key,
        false,
        &nonce,
        "netherite",
    )
    .await;

    let operation_id = seed_operation(&store, player_id, &nonce, "abyss-unlock").await;
    let mut tx = store.pool().begin().await.unwrap();
    let unlocked = lock_or_grant_fishing_area_first_unlock(
        &mut tx,
        operation_id,
        player_id,
        FishingArea::Abyss,
    )
    .await
    .unwrap();
    assert_eq!(unlocked.origin, FishingAreaAccessOrigin::NewlyUnlocked);
    let preview = unlocked.first_unlock_preview.unwrap();
    assert!(preview.rebirth_met);
    assert!(preview.rod_requirement_met);
    tx.commit().await.unwrap();

    sqlx::query("UPDATE players SET rebirth_count = 0 WHERE id = $1")
        .bind(player_id)
        .execute(store.pool())
        .await
        .unwrap();
    unequip_rod(&store, player_id, item_id).await;

    let later_operation = seed_operation(&store, player_id, &nonce, "abyss-persisted").await;
    let mut later_tx = store.pool().begin().await.unwrap();
    let persisted = lock_or_grant_fishing_area_first_unlock(
        &mut later_tx,
        later_operation,
        player_id,
        FishingArea::Abyss,
    )
    .await
    .unwrap();
    assert_eq!(persisted.origin, FishingAreaAccessOrigin::Persisted);
    assert_eq!(persisted.granted_by_operation_id, Some(operation_id));
    later_tx.commit().await.unwrap();
}

#[tokio::test]
async fn starter_basic_and_nonordinary_rods_fail_closed_for_non_pool_first_unlocks() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();

    let starter_player = seed_player(&store, positive_snowflake(nonce), 1).await;
    set_account_level(&store, starter_player, 200).await;
    let starter_item = seed_equipped_rod(
        &store,
        starter_player,
        "equipment.rod.basic.starter",
        true,
        &nonce,
        "starter",
    )
    .await;
    let starter_operation = seed_operation(&store, starter_player, &nonce, "starter-river").await;
    let mut starter_tx = store.pool().begin().await.unwrap();
    let error = lock_or_grant_fishing_area_first_unlock(
        &mut starter_tx,
        starter_operation,
        starter_player,
        FishingArea::River,
    )
    .await
    .unwrap_err();
    match error {
        FishingAreaAccessError::FirstUnlockRequirementsNotMet { preview } => {
            assert_eq!(preview.policy.minimum_account_level, Some(10));
            assert!(!preview.rod_requirement_met);
            assert!(!preview.eligible_for_first_unlock);
        }
        other => panic!("unexpected error: {other:?}"),
    }
    starter_tx.rollback().await.unwrap();
    unequip_rod(&store, starter_player, starter_item).await;

    let special_player = seed_player(&store, next_snowflake(nonce, 1), 1).await;
    set_account_level(&store, special_player, 200).await;
    let special_key = format!("test.fishing-area.special.{nonce}");
    seed_rod_definition(&store, &special_key, "GRAPHITE", false).await;
    let _special_item = seed_equipped_rod(
        &store,
        special_player,
        &special_key,
        false,
        &nonce,
        "special",
    )
    .await;
    let special_operation = seed_operation(&store, special_player, &nonce, "special-river").await;
    let mut special_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_or_grant_fishing_area_first_unlock(
            &mut special_tx,
            special_operation,
            special_player,
            FishingArea::River,
        )
        .await,
        Err(FishingAreaAccessError::NonOrdinaryFishingRod)
    ));
    special_tx.rollback().await.unwrap();
}

#[tokio::test]
async fn new_unlock_requires_active_account_pending_matching_operation_and_equipped_rod() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce), 0).await;
    set_account_level(&store, player_id, 10).await;

    let no_rod_operation = seed_operation(&store, player_id, &nonce, "no-rod").await;
    let mut no_rod_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_or_grant_fishing_area_first_unlock(
            &mut no_rod_tx,
            no_rod_operation,
            player_id,
            FishingArea::River,
        )
        .await,
        Err(FishingAreaAccessError::NoEquippedFishingRod)
    ));
    no_rod_tx.rollback().await.unwrap();

    let definition_key = format!("test.fishing-area.guard.{nonce}");
    seed_rod_definition(&store, &definition_key, "WOOD", true).await;
    let _item_id =
        seed_equipped_rod(&store, player_id, &definition_key, false, &nonce, "guard").await;

    sqlx::query("UPDATE players SET status = 'SOFT_FROZEN' WHERE id = $1")
        .bind(player_id)
        .execute(store.pool())
        .await
        .unwrap();
    let frozen_operation = seed_operation(&store, player_id, &nonce, "frozen").await;
    let mut frozen_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_or_grant_fishing_area_first_unlock(
            &mut frozen_tx,
            frozen_operation,
            player_id,
            FishingArea::River,
        )
        .await,
        Err(FishingAreaAccessError::AccountFrozen(ref status)) if status == "SOFT_FROZEN"
    ));
    frozen_tx.rollback().await.unwrap();
    sqlx::query("UPDATE players SET status = 'ACTIVE' WHERE id = $1")
        .bind(player_id)
        .execute(store.pool())
        .await
        .unwrap();

    let terminal_operation = seed_operation(&store, player_id, &nonce, "terminal").await;
    sqlx::query("UPDATE operations SET state = 'FAILED' WHERE id = $1")
        .bind(terminal_operation)
        .execute(store.pool())
        .await
        .unwrap();
    let mut terminal_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_or_grant_fishing_area_first_unlock(
            &mut terminal_tx,
            terminal_operation,
            player_id,
            FishingArea::River,
        )
        .await,
        Err(FishingAreaAccessError::OperationTerminal(ref state)) if state == "FAILED"
    ));
    terminal_tx.rollback().await.unwrap();

    let other_player = seed_player(&store, next_snowflake(nonce, 2), 0).await;
    let mismatch_operation = seed_operation(&store, other_player, &nonce, "mismatch").await;
    let mut mismatch_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_or_grant_fishing_area_first_unlock(
            &mut mismatch_tx,
            mismatch_operation,
            player_id,
            FishingArea::River,
        )
        .await,
        Err(FishingAreaAccessError::OperationPlayerMismatch)
    ));
    mismatch_tx.rollback().await.unwrap();
}

#[tokio::test]
async fn persisted_area_domain_rejects_default_or_unknown_rows() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce), 0).await;
    let operation_id = seed_operation(&store, player_id, &nonce, "invalid-area").await;

    for invalid in ["STARTER_POOL", "UNKNOWN"] {
        let result = sqlx::query(
            "INSERT INTO player_fishing_area_unlocks (player_id, area, granted_by_operation_id) VALUES ($1, $2, $3)",
        )
        .bind(player_id)
        .bind(invalid)
        .bind(operation_id)
        .execute(store.pool())
        .await;
        assert!(
            result.is_err(),
            "invalid persisted area {invalid} must fail"
        );
    }
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

async fn seed_player(store: &PgStore, discord_user_id: i64, rebirth_count: i64) -> Uuid {
    let player_id = Uuid::now_v7();
    sqlx::query("INSERT INTO players (id, discord_user_id, rebirth_count) VALUES ($1, $2, $3)")
        .bind(player_id)
        .bind(discord_user_id)
        .bind(rebirth_count)
        .execute(store.pool())
        .await
        .unwrap();
    player_id
}

async fn seed_operation(store: &PgStore, player_id: Uuid, nonce: &Uuid, suffix: &str) -> Uuid {
    let operation_id = Uuid::now_v7();
    let discord_user_id: i64 =
        sqlx::query_scalar("SELECT discord_user_id FROM players WHERE id = $1")
            .bind(player_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'FISHING_AREA_UNLOCK_TEST', 'PENDING', 1, $5, $6)",
    )
    .bind(operation_id)
    .bind(format!("test:fishing-area:{nonce}:{suffix}:{operation_id}"))
    .bind(discord_user_id)
    .bind(player_id)
    .bind([31_u8; 32].as_slice())
    .bind([37_u8; 32].as_slice())
    .execute(store.pool())
    .await
    .unwrap();
    operation_id
}

async fn seed_rod_definition(store: &PgStore, key: &str, tier: &str, ordinary: bool) {
    let data = serde_json::json!({"tier": tier});
    sqlx::query(
        "INSERT INTO item_definitions (key, category, stackable, active, definition_version, rarity, stack_limit, data) VALUES ($1, 'FISHING_ROD', FALSE, TRUE, 1, 'COMMON', NULL, $2)",
    )
    .bind(key)
    .bind(&data)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO item_definition_versions (key, version, category, stackable, rarity, stack_limit, is_ordinary_equipment, data) VALUES ($1, 1, 'FISHING_ROD', FALSE, 'COMMON', NULL, $2, $3)",
    )
    .bind(key)
    .bind(ordinary)
    .bind(data)
    .execute(store.pool())
    .await
    .unwrap();
}

async fn seed_equipped_rod(
    store: &PgStore,
    player_id: Uuid,
    definition_key: &str,
    is_starter: bool,
    nonce: &Uuid,
    suffix: &str,
) -> Uuid {
    let operation_id = seed_operation(store, player_id, nonce, &format!("item-{suffix}")).await;
    let item_id = Uuid::now_v7();
    let mut tx = store.pool().begin().await.unwrap();
    sqlx::query(
        "INSERT INTO item_instances (id, definition_key, owner_player_id, created_by_operation_id, location, definition_version, is_starter, is_account_bound, is_tradeable, is_sellable, is_discardable, is_enchantable, is_upgradeable, is_unbreakable, is_repairable) VALUES ($1, $2, $3, $4, 'EQUIPPED', 1, $5, $5, NOT $5, NOT $5, NOT $5, NOT $5, NOT $5, $5, NOT $5)",
    )
    .bind(item_id)
    .bind(definition_key)
    .bind(player_id)
    .bind(operation_id)
    .bind(is_starter)
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

async fn unequip_rod(store: &PgStore, player_id: Uuid, item_id: Uuid) {
    let mut tx = store.pool().begin().await.unwrap();
    sqlx::query("DELETE FROM equipment_slots WHERE player_id = $1 AND slot = 'FISHING_ROD' AND item_instance_id = $2")
        .bind(player_id)
        .bind(item_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE item_instances SET location = 'TOOL_LOCKER' WHERE id = $1 AND owner_player_id = $2",
    )
    .bind(item_id)
    .bind(player_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
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
