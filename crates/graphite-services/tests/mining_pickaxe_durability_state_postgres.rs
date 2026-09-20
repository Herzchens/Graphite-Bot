use graphite_services::{
    AppliedMiningPickaxeOrdinaryDurabilityState, MiningPickaxeDurabilityStateError,
    MiningPickaxeOrdinaryDurabilityConsequence,
    apply_resolved_equipped_pickaxe_ordinary_durability_event,
};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn ordinary_wear_persists_one_point_and_keeps_owner_operation_pending() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let item_id = seed_ordinary_pickaxe(&store, player_id, nonce, "wear", 9, 700, true).await;
    let operation_id = seed_operation(&store, player_id, nonce, "wear").await;

    let mut tx = store.pool().begin().await.unwrap();
    let applied = apply_resolved_equipped_pickaxe_ordinary_durability_event(
        &mut tx,
        operation_id,
        player_id,
        Some(9),
        false,
    )
    .await
    .unwrap();
    let AppliedMiningPickaxeOrdinaryDurabilityState::Ordinary { preview, .. } = applied else {
        panic!("ordinary Pickaxe returned Starter state");
    };
    assert_eq!(preview.resulting_durability, 8);
    assert_eq!(
        preview.consequence,
        MiningPickaxeOrdinaryDurabilityConsequence::WearApplied
    );
    tx.commit().await.unwrap();

    assert_eq!(
        pickaxe_durability(&store, item_id).await,
        (Some(8), Some(700), false)
    );
    assert_eq!(operation_state(&store, operation_id).await, "PENDING");
}

#[tokio::test]
async fn authoritative_unbreaking_prevention_is_a_locked_noop() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let item_id = seed_ordinary_pickaxe(&store, player_id, nonce, "unbreaking", 17, 700, true).await;
    let operation_id = seed_operation(&store, player_id, nonce, "unbreaking").await;

    let mut tx = store.pool().begin().await.unwrap();
    let applied = apply_resolved_equipped_pickaxe_ordinary_durability_event(
        &mut tx,
        operation_id,
        player_id,
        Some(17),
        true,
    )
    .await
    .unwrap();
    let AppliedMiningPickaxeOrdinaryDurabilityState::Ordinary { preview, .. } = applied else {
        panic!("ordinary Pickaxe returned Starter state");
    };
    assert_eq!(preview.resulting_durability, 17);
    assert_eq!(
        preview.consequence,
        MiningPickaxeOrdinaryDurabilityConsequence::WearPreventedByUnbreaking
    );
    tx.commit().await.unwrap();

    assert_eq!(
        pickaxe_durability(&store, item_id).await,
        (Some(17), Some(700), false)
    );
}

#[tokio::test]
async fn last_durability_point_sets_broken_atomically() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let item_id = seed_ordinary_pickaxe(&store, player_id, nonce, "last", 1, 700, true).await;
    let operation_id = seed_operation(&store, player_id, nonce, "last").await;

    let mut tx = store.pool().begin().await.unwrap();
    apply_resolved_equipped_pickaxe_ordinary_durability_event(
        &mut tx,
        operation_id,
        player_id,
        Some(1),
        false,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    assert_eq!(
        pickaxe_durability(&store, item_id).await,
        (Some(0), Some(700), true)
    );
}

#[tokio::test]
async fn starter_wood_pickaxe_is_an_explicit_unbreakable_noop() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let item_id = seed_starter_pickaxe(&store, player_id, nonce, "starter").await;
    let operation_id = seed_operation(&store, player_id, nonce, "starter").await;

    let mut tx = store.pool().begin().await.unwrap();
    let applied = apply_resolved_equipped_pickaxe_ordinary_durability_event(
        &mut tx,
        operation_id,
        player_id,
        None,
        false,
    )
    .await
    .unwrap();
    assert!(matches!(
        applied,
        AppliedMiningPickaxeOrdinaryDurabilityState::StarterWoodUnbreakable {
            item_instance_id,
            ..
        } if item_instance_id == item_id
    ));
    tx.commit().await.unwrap();

    assert_eq!(
        pickaxe_durability(&store, item_id).await,
        (None, None, false)
    );
}

#[tokio::test]
async fn stale_and_malformed_or_broken_durability_fail_closed() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let item_id = seed_ordinary_pickaxe(&store, player_id, nonce, "stale", 9, 700, true).await;

    let stale_operation = seed_operation(&store, player_id, nonce, "stale").await;
    let mut stale_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        apply_resolved_equipped_pickaxe_ordinary_durability_event(
            &mut stale_tx,
            stale_operation,
            player_id,
            Some(10),
            false,
        )
        .await,
        Err(MiningPickaxeDurabilityStateError::DurabilityChanged {
            expected: 10,
            actual: 9
        })
    ));
    stale_tx.rollback().await.unwrap();

    sqlx::query("UPDATE item_instances SET current_durability = 0, is_broken = TRUE WHERE id = $1")
        .bind(item_id)
        .execute(store.pool())
        .await
        .unwrap();
    let broken_operation = seed_operation(&store, player_id, nonce, "broken").await;
    let mut broken_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        apply_resolved_equipped_pickaxe_ordinary_durability_event(
            &mut broken_tx,
            broken_operation,
            player_id,
            Some(0),
            false,
        )
        .await,
        Err(MiningPickaxeDurabilityStateError::OrdinaryPickaxeAlreadyBroken)
    ));
    broken_tx.rollback().await.unwrap();

    sqlx::query("UPDATE item_instances SET current_durability = 5, is_broken = TRUE WHERE id = $1")
        .bind(item_id)
        .execute(store.pool())
        .await
        .unwrap();
    let malformed_operation = seed_operation(&store, player_id, nonce, "malformed").await;
    let mut malformed_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        apply_resolved_equipped_pickaxe_ordinary_durability_event(
            &mut malformed_tx,
            malformed_operation,
            player_id,
            Some(5),
            false,
        )
        .await,
        Err(MiningPickaxeDurabilityStateError::InvalidOrdinaryPickaxeDurability)
    ));
    malformed_tx.rollback().await.unwrap();
}

#[tokio::test]
async fn nonordinary_pickaxe_and_operation_or_account_mismatches_fail_closed() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();

    let special_player = seed_player(&store, positive_snowflake(nonce)).await;
    seed_ordinary_pickaxe(&store, special_player, nonce, "special", 30, 700, false).await;
    let special_operation = seed_operation(&store, special_player, nonce, "special").await;
    let mut special_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        apply_resolved_equipped_pickaxe_ordinary_durability_event(
            &mut special_tx,
            special_operation,
            special_player,
            Some(30),
            false,
        )
        .await,
        Err(MiningPickaxeDurabilityStateError::NonOrdinaryPickaxe)
    ));
    special_tx.rollback().await.unwrap();

    let owner_player = seed_player(&store, next_snowflake(nonce, 1)).await;
    seed_ordinary_pickaxe(&store, owner_player, nonce, "owner", 30, 700, true).await;
    let other_player = seed_player(&store, next_snowflake(nonce, 2)).await;
    let mismatch_operation = seed_operation(&store, other_player, nonce, "mismatch").await;
    let mut mismatch_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        apply_resolved_equipped_pickaxe_ordinary_durability_event(
            &mut mismatch_tx,
            mismatch_operation,
            owner_player,
            Some(30),
            false,
        )
        .await,
        Err(MiningPickaxeDurabilityStateError::OperationPlayerMismatch)
    ));
    mismatch_tx.rollback().await.unwrap();

    let terminal_operation = seed_operation(&store, owner_player, nonce, "terminal").await;
    sqlx::query("UPDATE operations SET state = 'FAILED' WHERE id = $1")
        .bind(terminal_operation)
        .execute(store.pool())
        .await
        .unwrap();
    let mut terminal_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        apply_resolved_equipped_pickaxe_ordinary_durability_event(
            &mut terminal_tx,
            terminal_operation,
            owner_player,
            Some(30),
            false,
        )
        .await,
        Err(MiningPickaxeDurabilityStateError::OperationTerminal(ref state)) if state == "FAILED"
    ));
    terminal_tx.rollback().await.unwrap();

    sqlx::query("UPDATE players SET status = 'SOFT_FROZEN' WHERE id = $1")
        .bind(owner_player)
        .execute(store.pool())
        .await
        .unwrap();
    let frozen_operation = seed_operation(&store, owner_player, nonce, "frozen").await;
    let mut frozen_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        apply_resolved_equipped_pickaxe_ordinary_durability_event(
            &mut frozen_tx,
            frozen_operation,
            owner_player,
            Some(30),
            false,
        )
        .await,
        Err(MiningPickaxeDurabilityStateError::AccountFrozen(ref status))
            if status == "SOFT_FROZEN"
    ));
    frozen_tx.rollback().await.unwrap();
}

#[tokio::test]
async fn rollback_restores_durability_and_writer_retains_core_locks() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let item_id = seed_ordinary_pickaxe(&store, player_id, nonce, "rollback", 6, 700, true).await;
    let operation_id = seed_operation(&store, player_id, nonce, "rollback").await;

    let mut owner = store.pool().begin().await.unwrap();
    apply_resolved_equipped_pickaxe_ordinary_durability_event(
        &mut owner,
        operation_id,
        player_id,
        Some(6),
        false,
    )
    .await
    .unwrap();

    assert_eq!(
        sqlx::query_as::<_, (Option<i64>, bool)>(
            "SELECT current_durability, is_broken FROM item_instances WHERE id = $1"
        )
        .bind(item_id)
        .fetch_one(&mut *owner)
        .await
        .unwrap(),
        (Some(5), false)
    );

    assert_row_locked(
        &store,
        "SELECT id FROM operations WHERE id = $1 FOR UPDATE NOWAIT",
        operation_id,
    )
    .await;
    assert_row_locked(
        &store,
        "SELECT id FROM players WHERE id = $1 FOR UPDATE NOWAIT",
        player_id,
    )
    .await;
    assert_row_locked(
        &store,
        "SELECT id FROM item_instances WHERE id = $1 FOR UPDATE NOWAIT",
        item_id,
    )
    .await;
    assert_equipment_slot_locked(&store, player_id).await;

    owner.rollback().await.unwrap();
    assert_eq!(
        pickaxe_durability(&store, item_id).await,
        (Some(6), Some(700), false)
    );
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
        "INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'MINING_PICKAXE_DURABILITY_STATE_TEST', 'PENDING', 1, $5, $6)",
    )
    .bind(operation_id)
    .bind(format!("test:mining-pickaxe-durability-state:{nonce}:{suffix}:{operation_id}"))
    .bind(discord_user_id)
    .bind(player_id)
    .bind([61_u8; 32].as_slice())
    .bind([67_u8; 32].as_slice())
    .execute(store.pool())
    .await
    .unwrap();
    operation_id
}

async fn seed_ordinary_pickaxe(
    store: &PgStore,
    player_id: Uuid,
    nonce: Uuid,
    suffix: &str,
    current_durability: i64,
    max_durability: i64,
    ordinary: bool,
) -> Uuid {
    let definition_key = format!("test.mining-pickaxe-durability-state.{suffix}.{nonce}");
    let data = serde_json::json!({"tier": "WOOD"});
    sqlx::query(
        "INSERT INTO item_definitions (key, category, stackable, active, definition_version, rarity, stack_limit, data) VALUES ($1, 'PICKAXE', FALSE, TRUE, 1, 'COMMON', NULL, $2)",
    )
    .bind(&definition_key)
    .bind(&data)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO item_definition_versions (key, version, category, stackable, rarity, stack_limit, is_ordinary_equipment, data) VALUES ($1, 1, 'PICKAXE', FALSE, 'COMMON', NULL, $2, $3)",
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
        "INSERT INTO item_instances (id, definition_key, owner_player_id, created_by_operation_id, location, definition_version, current_durability, max_durability) VALUES ($1, $2, $3, $4, 'EQUIPPED', 1, $5, $6)",
    )
    .bind(item_id)
    .bind(&definition_key)
    .bind(player_id)
    .bind(creation_operation)
    .bind(current_durability)
    .bind(max_durability)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO equipment_slots (player_id, slot, item_instance_id) VALUES ($1, 'PICKAXE', $2)",
    )
    .bind(player_id)
    .bind(item_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    item_id
}

async fn seed_starter_pickaxe(
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
        "INSERT INTO item_instances (id, definition_key, owner_player_id, created_by_operation_id, location, definition_version, is_starter, is_account_bound, is_tradeable, is_sellable, is_discardable, is_enchantable, is_upgradeable, is_unbreakable, is_repairable) VALUES ($1, 'equipment.pickaxe.wood.starter', $2, $3, 'EQUIPPED', 1, TRUE, TRUE, FALSE, FALSE, FALSE, FALSE, FALSE, TRUE, FALSE)",
    )
    .bind(item_id)
    .bind(player_id)
    .bind(creation_operation)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO equipment_slots (player_id, slot, item_instance_id) VALUES ($1, 'PICKAXE', $2)",
    )
    .bind(player_id)
    .bind(item_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    item_id
}

async fn pickaxe_durability(
    store: &PgStore,
    item_id: Uuid,
) -> (Option<i64>, Option<i64>, bool) {
    sqlx::query_as(
        "SELECT current_durability, max_durability, is_broken FROM item_instances WHERE id = $1",
    )
    .bind(item_id)
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

async fn assert_row_locked(store: &PgStore, query: &'static str, id: Uuid) {
    let mut contender = store.pool().begin().await.unwrap();
    let result = sqlx::query_scalar::<_, Uuid>(query)
        .bind(id)
        .fetch_one(&mut *contender)
        .await;
    let error = result.unwrap_err();
    let sqlx::Error::Database(database) = error else {
        panic!("expected PostgreSQL lock error, got {error:?}");
    };
    assert_eq!(database.code().as_deref(), Some("55P03"));
    contender.rollback().await.unwrap();
}

async fn assert_equipment_slot_locked(store: &PgStore, player_id: Uuid) {
    let mut contender = store.pool().begin().await.unwrap();
    let result = sqlx::query_scalar::<_, Uuid>(
        "SELECT item_instance_id FROM equipment_slots WHERE player_id = $1 AND slot = 'PICKAXE' FOR UPDATE NOWAIT",
    )
    .bind(player_id)
    .fetch_one(&mut *contender)
    .await;
    let error = result.unwrap_err();
    let sqlx::Error::Database(database) = error else {
        panic!("expected PostgreSQL lock error, got {error:?}");
    };
    assert_eq!(database.code().as_deref(), Some("55P03"));
    contender.rollback().await.unwrap();
}

fn positive_snowflake(nonce: Uuid) -> i64 {
    next_snowflake(nonce, 0)
}

fn next_snowflake(nonce: Uuid, offset: u64) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[8..].try_into().unwrap());
    let base = raw & 0x3fff_ffff_ffff_ffff;
    let value = base.saturating_add(1).saturating_add(offset);
    i64::try_from(value).unwrap()
}
