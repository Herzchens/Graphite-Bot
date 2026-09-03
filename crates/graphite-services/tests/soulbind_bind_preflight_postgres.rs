use chrono::{DateTime, Utc};
use graphite_services::{
    EquipmentTier, OrdinaryEquipmentSoulBindStateError, OrdinarySoulBindBindPreflightError,
    PersistedSoulBindState, SoulBindPolicyError,
    lock_preview_soulbind_bind_for_owned_ordinary_equipment, preview_soulbind_binding,
};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn authoritative_bind_preflight_uses_locked_rebirth_appraisal_and_is_read_only() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce), 1).await;
    let definition_key = format!("test.soulbind-bind-preflight.{nonce}");
    seed_definition(&store, &definition_key, "NETHERITE").await;
    let item_id = seed_item(&store, player_id, &definition_key, &nonce, "eligible").await;
    seed_structural_state(&store, item_id).await;

    let mut tx = store.pool().begin().await.unwrap();
    let preflight =
        lock_preview_soulbind_bind_for_owned_ordinary_equipment(&mut tx, player_id, item_id)
            .await
            .unwrap();

    assert_eq!(preflight.snapshot.state, PersistedSoulBindState::NeverBound);
    assert_eq!(
        preflight.snapshot.equipment.recraft.tier,
        EquipmentTier::Netherite
    );
    assert_eq!(preflight.preview.rebirth_count, 1);
    assert_eq!(
        preflight.preview,
        preview_soulbind_binding(
            EquipmentTier::Netherite,
            true,
            1,
            preflight.snapshot.equipment.enhanced_canonical_appraisal,
        )
        .unwrap()
    );
    assert_eq!(preflight.preview.package.soulbind_rune_quantity, 1);
    assert_eq!(preflight.preview.package.onyx_quantity, 20);
    assert_eq!(preflight.preview.package.platinum_ingot_quantity, 8);
    assert_eq!(preflight.preview.package.tier_component_quantity, 2);
    assert_eq!(preflight.preview.package.fixed_money_cost, 250_000);
    assert_eq!(preflight.preview.package.activity_xp_cost, 25_000);
    assert_eq!(soulbind_row_in_tx(&mut tx, item_id).await, None);

    tx.commit().await.unwrap();
    assert_eq!(soulbind_row(&store, item_id).await, None);
}

#[tokio::test]
async fn authoritative_bind_preflight_enforces_rebirth_account_and_persisted_state_guards() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let definition_key = format!("test.soulbind-bind-preflight-guards.{nonce}");
    seed_definition(&store, &definition_key, "GRAPHITE").await;

    let no_rebirth_player = seed_player(&store, positive_snowflake(nonce), 0).await;
    let no_rebirth_item = seed_item(
        &store,
        no_rebirth_player,
        &definition_key,
        &nonce,
        "no-rebirth",
    )
    .await;
    seed_structural_state(&store, no_rebirth_item).await;
    let mut no_rebirth_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_preview_soulbind_bind_for_owned_ordinary_equipment(
            &mut no_rebirth_tx,
            no_rebirth_player,
            no_rebirth_item,
        )
        .await,
        Err(OrdinarySoulBindBindPreflightError::Policy(
            SoulBindPolicyError::RebirthRequired {
                required: 1,
                current: 0,
            }
        ))
    ));
    no_rebirth_tx.rollback().await.unwrap();

    let frozen_player = seed_player(&store, next_snowflake(nonce, 1), 1).await;
    let frozen_item = seed_item(&store, frozen_player, &definition_key, &nonce, "frozen").await;
    seed_structural_state(&store, frozen_item).await;
    sqlx::query("UPDATE players SET status = 'SOFT_FROZEN' WHERE id = $1")
        .bind(frozen_player)
        .execute(store.pool())
        .await
        .unwrap();
    let mut frozen_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_preview_soulbind_bind_for_owned_ordinary_equipment(
            &mut frozen_tx,
            frozen_player,
            frozen_item,
        )
        .await,
        Err(OrdinarySoulBindBindPreflightError::AccountFrozen(ref status))
            if status == "SOFT_FROZEN"
    ));
    frozen_tx.rollback().await.unwrap();

    let player_id = seed_player(&store, next_snowflake(nonce, 2), 1).await;
    let bound = seed_item(&store, player_id, &definition_key, &nonce, "bound").await;
    let cooling = seed_item(&store, player_id, &definition_key, &nonce, "cooling").await;
    let elapsed = seed_item(&store, player_id, &definition_key, &nonce, "elapsed").await;
    for item_id in [bound, cooling, elapsed] {
        seed_structural_state(&store, item_id).await;
    }
    seed_bound_state(&store, bound).await;

    let cooling_until: DateTime<Utc> =
        sqlx::query_scalar("SELECT clock_timestamp() + INTERVAL '1 day'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    seed_unbound_state(&store, cooling, cooling_until).await;
    let elapsed_at: DateTime<Utc> =
        sqlx::query_scalar("SELECT clock_timestamp() - INTERVAL '1 second'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    seed_unbound_state(&store, elapsed, elapsed_at).await;

    let mut bound_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_preview_soulbind_bind_for_owned_ordinary_equipment(&mut bound_tx, player_id, bound)
            .await,
        Err(OrdinarySoulBindBindPreflightError::State(
            OrdinaryEquipmentSoulBindStateError::AlreadySoulBound
        ))
    ));
    bound_tx.rollback().await.unwrap();

    let mut cooling_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_preview_soulbind_bind_for_owned_ordinary_equipment(
            &mut cooling_tx,
            player_id,
            cooling,
        )
        .await,
        Err(OrdinarySoulBindBindPreflightError::State(
            OrdinaryEquipmentSoulBindStateError::RebindCooldownActive {
                rebind_not_before,
                ..
            }
        )) if rebind_not_before == cooling_until
    ));
    cooling_tx.rollback().await.unwrap();

    let mut elapsed_tx = store.pool().begin().await.unwrap();
    let elapsed_preflight = lock_preview_soulbind_bind_for_owned_ordinary_equipment(
        &mut elapsed_tx,
        player_id,
        elapsed,
    )
    .await
    .unwrap();
    assert_eq!(
        elapsed_preflight.snapshot.state,
        PersistedSoulBindState::Unbound {
            rebind_not_before: elapsed_at,
        }
    );
    assert!(elapsed_preflight.evaluated_at >= elapsed_at);
    assert_eq!(elapsed_preflight.preview.package.fixed_money_cost, 500_000);
    assert_eq!(elapsed_preflight.preview.package.activity_xp_cost, 50_000);
    elapsed_tx.commit().await.unwrap();

    assert_eq!(soulbind_row(&store, bound).await, Some((true, None)));
    assert_eq!(
        soulbind_row(&store, cooling).await,
        Some((false, Some(cooling_until)))
    );
    assert_eq!(
        soulbind_row(&store, elapsed).await,
        Some((false, Some(elapsed_at)))
    );
}

#[tokio::test]
async fn bind_preflight_retains_player_lock_for_rebirth_snapshot() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce), 1).await;
    let definition_key = format!("test.soulbind-bind-preflight-lock.{nonce}");
    seed_definition(&store, &definition_key, "NETHERITE").await;
    let item_id = seed_item(&store, player_id, &definition_key, &nonce, "lock").await;
    seed_structural_state(&store, item_id).await;

    let mut owner_tx = store.pool().begin().await.unwrap();
    let preflight =
        lock_preview_soulbind_bind_for_owned_ordinary_equipment(&mut owner_tx, player_id, item_id)
            .await
            .unwrap();
    assert_eq!(preflight.preview.rebirth_count, 1);

    let mut competing_tx = store.pool().begin().await.unwrap();
    sqlx::query("SET LOCAL lock_timeout = '100ms'")
        .execute(&mut *competing_tx)
        .await
        .unwrap();
    let blocked = sqlx::query("UPDATE players SET rebirth_count = 2 WHERE id = $1")
        .bind(player_id)
        .execute(&mut *competing_tx)
        .await;
    assert!(
        blocked.is_err(),
        "preflight must retain the player row lock"
    );
    competing_tx.rollback().await.unwrap();

    owner_tx.rollback().await.unwrap();
    let rebirth_count: i64 = sqlx::query_scalar("SELECT rebirth_count FROM players WHERE id = $1")
        .bind(player_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(rebirth_count, 1);
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

async fn seed_definition(store: &PgStore, key: &str, tier: &str) {
    let data = serde_json::json!({"tier": tier});
    sqlx::query("INSERT INTO item_definitions (key, category, stackable, active, definition_version, rarity, stack_limit, data) VALUES ($1, 'PICKAXE', FALSE, TRUE, 1, 'COMMON', NULL, $2)")
        .bind(key)
        .bind(&data)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO item_definition_versions (key, version, category, stackable, rarity, stack_limit, is_ordinary_equipment, data) VALUES ($1, 1, 'PICKAXE', FALSE, 'COMMON', NULL, TRUE, $2)")
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
    sqlx::query("INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'SOULBIND_BIND_PREFLIGHT_TEST', 'PENDING', 1, $5, $6)")
        .bind(operation_id)
        .bind(format!("test:soulbind-bind-preflight:{nonce}:{suffix}:{operation_id}"))
        .bind(discord_user_id)
        .bind(player_id)
        .bind([73_u8; 32].as_slice())
        .bind([79_u8; 32].as_slice())
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
    sqlx::query("INSERT INTO item_instance_equipment_structural_state (item_instance_id, creation_roll_numerator, creation_roll_denominator, upgrade_level, normal_enchant_slot_capacity, special_enchant_slot_capacity) VALUES ($1, 1, 1, 0, 4, 3)")
        .bind(item_id)
        .execute(store.pool())
        .await
        .unwrap();
}

async fn seed_bound_state(store: &PgStore, item_id: Uuid) {
    sqlx::query(
        "INSERT INTO item_instance_soulbind_state (item_instance_id, is_soulbound, rebind_not_before) VALUES ($1, TRUE, NULL)",
    )
    .bind(item_id)
    .execute(store.pool())
    .await
    .unwrap();
}

async fn seed_unbound_state(store: &PgStore, item_id: Uuid, rebind_not_before: DateTime<Utc>) {
    sqlx::query(
        "INSERT INTO item_instance_soulbind_state (item_instance_id, is_soulbound, rebind_not_before) VALUES ($1, FALSE, $2)",
    )
    .bind(item_id)
    .bind(rebind_not_before)
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
    next_snowflake(nonce, 0)
}

fn next_snowflake(nonce: Uuid, offset: u64) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    let value = (raw % 7_999_999_999_999_999_000_u64)
        .saturating_add(1)
        .saturating_add(offset);
    i64::try_from(value).unwrap()
}
