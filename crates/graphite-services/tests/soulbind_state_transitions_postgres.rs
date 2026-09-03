use chrono::{DateTime, Duration, Utc};
use graphite_services::{
    EquipmentTier, OrdinaryEquipmentSoulBindStateError, PersistedSoulBindState,
    SOULBIND_REBIND_COOLDOWN_SECONDS, lock_owned_ordinary_equipment_soulbind_state,
    write_resolved_soulbind_bind_to_owned_ordinary_equipment,
    write_resolved_soulbind_unbind_to_owned_ordinary_equipment,
};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn bind_unbind_and_rebind_are_exact_and_transaction_composable() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let definition_key = format!("test.soulbind-transitions.netherite.{nonce}");
    seed_definition(&store, &definition_key, "NETHERITE").await;
    let item_id = seed_item(&store, owner_id, &definition_key, &nonce, "exact", true).await;
    seed_structural_state(&store, item_id).await;

    let mut snapshot_tx = store.pool().begin().await.unwrap();
    let snapshot =
        lock_owned_ordinary_equipment_soulbind_state(&mut snapshot_tx, owner_id, item_id)
            .await
            .unwrap();
    assert_eq!(snapshot.equipment.recraft.tier, EquipmentTier::Netherite);
    assert_eq!(snapshot.state, PersistedSoulBindState::NeverBound);
    snapshot_tx.rollback().await.unwrap();

    let mut rolled_back_bind = store.pool().begin().await.unwrap();
    let bound = write_resolved_soulbind_bind_to_owned_ordinary_equipment(
        &mut rolled_back_bind,
        owner_id,
        item_id,
    )
    .await
    .unwrap();
    assert_eq!(bound.previous_state, PersistedSoulBindState::NeverBound);
    assert_eq!(bound.new_state, PersistedSoulBindState::Bound);
    assert_eq!(
        soulbind_row_in_tx(&mut rolled_back_bind, item_id).await,
        Some((true, None))
    );
    rolled_back_bind.rollback().await.unwrap();
    assert_eq!(soulbind_row(&store, item_id).await, None);

    let mut bind_tx = store.pool().begin().await.unwrap();
    write_resolved_soulbind_bind_to_owned_ordinary_equipment(&mut bind_tx, owner_id, item_id)
        .await
        .unwrap();
    bind_tx.commit().await.unwrap();
    assert_eq!(soulbind_row(&store, item_id).await, Some((true, None)));

    let mut rolled_back_unbind = store.pool().begin().await.unwrap();
    let unbound = write_resolved_soulbind_unbind_to_owned_ordinary_equipment(
        &mut rolled_back_unbind,
        owner_id,
        item_id,
    )
    .await
    .unwrap();
    let PersistedSoulBindState::Unbound { rebind_not_before } = unbound.new_state else {
        panic!("unbind must produce an unbound cooldown state");
    };
    assert_eq!(
        rebind_not_before - unbound.evaluated_at,
        Duration::seconds(SOULBIND_REBIND_COOLDOWN_SECONDS)
    );
    rolled_back_unbind.rollback().await.unwrap();
    assert_eq!(soulbind_row(&store, item_id).await, Some((true, None)));

    let mut unbind_tx = store.pool().begin().await.unwrap();
    let transaction_started_at: DateTime<Utc> = sqlx::query_scalar("SELECT CURRENT_TIMESTAMP")
        .fetch_one(&mut *unbind_tx)
        .await
        .unwrap();
    sqlx::query("SELECT pg_sleep(0.02)")
        .execute(&mut *unbind_tx)
        .await
        .unwrap();
    let unbound = write_resolved_soulbind_unbind_to_owned_ordinary_equipment(
        &mut unbind_tx,
        owner_id,
        item_id,
    )
    .await
    .unwrap();
    assert!(
        unbound.evaluated_at > transaction_started_at,
        "cooldown must be anchored to the actual mutation statement, not transaction start"
    );
    let PersistedSoulBindState::Unbound { rebind_not_before } = unbound.new_state else {
        panic!("unbind must produce an unbound cooldown state");
    };
    assert_eq!(
        rebind_not_before - unbound.evaluated_at,
        Duration::seconds(SOULBIND_REBIND_COOLDOWN_SECONDS)
    );
    unbind_tx.commit().await.unwrap();
    assert_eq!(
        soulbind_row(&store, item_id).await,
        Some((false, Some(rebind_not_before)))
    );

    let mut blocked_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        write_resolved_soulbind_bind_to_owned_ordinary_equipment(
            &mut blocked_tx,
            owner_id,
            item_id,
        )
        .await,
        Err(OrdinaryEquipmentSoulBindStateError::RebindCooldownActive {
            rebind_not_before: blocked_until,
            ..
        }) if blocked_until == rebind_not_before
    ));
    blocked_tx.rollback().await.unwrap();

    sqlx::query(
        "UPDATE item_instance_soulbind_state SET rebind_not_before = clock_timestamp() - INTERVAL '1 second' WHERE item_instance_id = $1",
    )
    .bind(item_id)
    .execute(store.pool())
    .await
    .unwrap();

    let mut rebind_tx = store.pool().begin().await.unwrap();
    let rebound =
        write_resolved_soulbind_bind_to_owned_ordinary_equipment(&mut rebind_tx, owner_id, item_id)
            .await
            .unwrap();
    assert!(matches!(
        rebound.previous_state,
        PersistedSoulBindState::Unbound { .. }
    ));
    assert_eq!(rebound.new_state, PersistedSoulBindState::Bound);
    rebind_tx.commit().await.unwrap();
    assert_eq!(soulbind_row(&store, item_id).await, Some((true, None)));

    let account_bound: bool =
        sqlx::query_scalar("SELECT is_account_bound FROM item_instances WHERE id = $1")
            .bind(item_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert!(
        account_bound,
        "SoulBind transitions must not reuse or clear account binding"
    );
}

#[tokio::test]
async fn elapsed_cooldown_is_rebind_eligible() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let definition_key = format!("test.soulbind-transitions.boundary.{nonce}");
    seed_definition(&store, &definition_key, "GRAPHITE").await;
    let item_id = seed_item(&store, owner_id, &definition_key, &nonce, "boundary", false).await;
    seed_structural_state(&store, item_id).await;

    let mut tx = store.pool().begin().await.unwrap();
    let elapsed_at: DateTime<Utc> =
        sqlx::query_scalar("SELECT clock_timestamp() - INTERVAL '1 second'")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO item_instance_soulbind_state (item_instance_id, is_soulbound, rebind_not_before) VALUES ($1, FALSE, $2)",
    )
    .bind(item_id)
    .bind(elapsed_at)
    .execute(&mut *tx)
    .await
    .unwrap();

    let rebound =
        write_resolved_soulbind_bind_to_owned_ordinary_equipment(&mut tx, owner_id, item_id)
            .await
            .unwrap();
    assert!(rebound.evaluated_at > elapsed_at);
    assert_eq!(rebound.new_state, PersistedSoulBindState::Bound);
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn state_guards_fail_closed_without_mutating_authoritative_rows() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let other_id = seed_player(&store, positive_snowflake(Uuid::now_v7())).await;

    let ineligible_key = format!("test.soulbind-transitions.obsidian.{nonce}");
    seed_definition(&store, &ineligible_key, "OBSIDIAN").await;
    let ineligible = seed_item(
        &store,
        owner_id,
        &ineligible_key,
        &nonce,
        "ineligible",
        false,
    )
    .await;
    seed_structural_state(&store, ineligible).await;
    let mut ineligible_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_owned_ordinary_equipment_soulbind_state(&mut ineligible_tx, owner_id, ineligible)
            .await,
        Err(OrdinaryEquipmentSoulBindStateError::Policy(_))
    ));
    ineligible_tx.rollback().await.unwrap();
    assert_eq!(soulbind_row(&store, ineligible).await, None);

    let eligible_key = format!("test.soulbind-transitions.guards.{nonce}");
    seed_definition(&store, &eligible_key, "NETHERITE").await;
    let item_id = seed_item(&store, owner_id, &eligible_key, &nonce, "guards", false).await;
    seed_structural_state(&store, item_id).await;

    let mut wrong_owner_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_owned_ordinary_equipment_soulbind_state(&mut wrong_owner_tx, other_id, item_id).await,
        Err(OrdinaryEquipmentSoulBindStateError::Enhanced(_))
    ));
    wrong_owner_tx.rollback().await.unwrap();
    assert_eq!(soulbind_row(&store, item_id).await, None);

    let mut unbind_never_bound_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        write_resolved_soulbind_unbind_to_owned_ordinary_equipment(
            &mut unbind_never_bound_tx,
            owner_id,
            item_id,
        )
        .await,
        Err(OrdinaryEquipmentSoulBindStateError::NotSoulBound)
    ));
    unbind_never_bound_tx.rollback().await.unwrap();
    assert_eq!(soulbind_row(&store, item_id).await, None);

    let mut bind_tx = store.pool().begin().await.unwrap();
    write_resolved_soulbind_bind_to_owned_ordinary_equipment(&mut bind_tx, owner_id, item_id)
        .await
        .unwrap();
    bind_tx.commit().await.unwrap();

    let mut duplicate_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        write_resolved_soulbind_bind_to_owned_ordinary_equipment(
            &mut duplicate_tx,
            owner_id,
            item_id,
        )
        .await,
        Err(OrdinaryEquipmentSoulBindStateError::AlreadySoulBound)
    ));
    duplicate_tx.rollback().await.unwrap();
    assert_eq!(soulbind_row(&store, item_id).await, Some((true, None)));
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

async fn seed_definition(store: &PgStore, key: &str, tier: &str) {
    let data = format!(r#"{{"tier":"{tier}"}}"#);
    sqlx::query("INSERT INTO item_definitions (key, category, stackable, active, definition_version, rarity, stack_limit, data) VALUES ($1, 'PICKAXE', FALSE, TRUE, 1, 'COMMON', NULL, $2::jsonb)")
        .bind(key)
        .bind(&data)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO item_definition_versions (key, version, category, stackable, rarity, stack_limit, is_ordinary_equipment, data) VALUES ($1, 1, 'PICKAXE', FALSE, 'COMMON', NULL, TRUE, $2::jsonb)")
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
    is_account_bound: bool,
) -> Uuid {
    let operation_id = Uuid::now_v7();
    let discord_user_id: i64 =
        sqlx::query_scalar("SELECT discord_user_id FROM players WHERE id = $1")
            .bind(player_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    sqlx::query("INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'SOULBIND_STATE_TRANSITION_TEST', 'PENDING', 1, $5, $6)")
        .bind(operation_id)
        .bind(format!("test:soulbind-state-transition:{nonce}:{suffix}:{operation_id}"))
        .bind(discord_user_id)
        .bind(player_id)
        .bind([173_u8; 32].as_slice())
        .bind([179_u8; 32].as_slice())
        .execute(store.pool())
        .await
        .unwrap();

    let item_id = Uuid::now_v7();
    sqlx::query("INSERT INTO item_instances (id, definition_key, owner_player_id, created_by_operation_id, location, definition_version, is_account_bound) VALUES ($1, $2, $3, $4, 'TOOL_LOCKER', 1, $5)")
        .bind(item_id)
        .bind(definition_key)
        .bind(player_id)
        .bind(operation_id)
        .bind(is_account_bound)
        .execute(store.pool())
        .await
        .unwrap();
    item_id
}

async fn seed_structural_state(store: &PgStore, item_id: Uuid) {
    sqlx::query("INSERT INTO item_instance_equipment_structural_state (item_instance_id, creation_roll_numerator, creation_roll_denominator, upgrade_level, normal_enchant_slot_capacity, special_enchant_slot_capacity) VALUES ($1, 1, 1, 0, 4, 3)")
        .bind(item_id)
        .execute(store.pool())
        .await
        .unwrap();
}

async fn soulbind_row(store: &PgStore, item_id: Uuid) -> Option<(bool, Option<DateTime<Utc>>)> {
    sqlx::query_as(
        "SELECT is_soulbound, rebind_not_before FROM item_instance_soulbind_state WHERE item_instance_id = $1",
    )
    .bind(item_id)
    .fetch_optional(store.pool())
    .await
    .unwrap()
}

async fn soulbind_row_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    item_id: Uuid,
) -> Option<(bool, Option<DateTime<Utc>>)> {
    sqlx::query_as(
        "SELECT is_soulbound, rebind_not_before FROM item_instance_soulbind_state WHERE item_instance_id = $1",
    )
    .bind(item_id)
    .fetch_optional(&mut **tx)
    .await
    .unwrap()
}

fn positive_snowflake(nonce: Uuid) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    i64::try_from((raw % 8_000_000_000_000_000_000_u64).max(1)).unwrap()
}
