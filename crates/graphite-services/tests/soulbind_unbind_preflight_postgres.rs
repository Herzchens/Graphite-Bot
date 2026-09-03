use chrono::{DateTime, Utc};
use graphite_services::{
    OrdinarySoulBindUnbindPreflightError, PersistedSoulBindState,
    lock_preview_soulbind_unbind_for_owned_ordinary_equipment, preview_soulbind_unbind,
};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn authoritative_unbind_preflight_is_exact_and_read_only() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let definition_key = format!("test.soulbind-unbind-preflight.{nonce}");
    seed_definition(&store, &definition_key).await;
    let item_id = seed_item(
        &store,
        player_id,
        &definition_key,
        &nonce,
        "eligible",
        false,
        false,
    )
    .await;
    seed_structural_state(&store, item_id).await;
    seed_bound_state(&store, item_id).await;

    let mut tx = store.pool().begin().await.unwrap();
    let preflight =
        lock_preview_soulbind_unbind_for_owned_ordinary_equipment(&mut tx, player_id, item_id)
            .await
            .unwrap();

    assert_eq!(preflight.snapshot.state, PersistedSoulBindState::Bound);
    assert!(!preflight.snapshot.is_favorite);
    assert!(!preflight.snapshot.is_protected);
    assert_eq!(
        preflight.preview,
        preview_soulbind_unbind(preflight.snapshot.equipment.enhanced_canonical_appraisal).unwrap()
    );
    assert!(!preflight.preview.refunds_binding_resources);
    assert!(preflight.preview.requires_unprotected);
    assert!(preflight.preview.requires_unfavorited);
    assert_eq!(
        soulbind_row_in_tx(&mut tx, item_id).await,
        Some((true, None))
    );

    tx.commit().await.unwrap();
    assert_eq!(soulbind_row(&store, item_id).await, Some((true, None)));
}

#[tokio::test]
async fn authoritative_unbind_preflight_rejects_state_and_control_flag_failures() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let definition_key = format!("test.soulbind-unbind-preflight-guards.{nonce}");
    seed_definition(&store, &definition_key).await;

    let favorite = seed_item(
        &store,
        player_id,
        &definition_key,
        &nonce,
        "favorite",
        true,
        false,
    )
    .await;
    let protected = seed_item(
        &store,
        player_id,
        &definition_key,
        &nonce,
        "protected",
        false,
        true,
    )
    .await;
    let both = seed_item(
        &store,
        player_id,
        &definition_key,
        &nonce,
        "both",
        true,
        true,
    )
    .await;
    let never_bound = seed_item(
        &store,
        player_id,
        &definition_key,
        &nonce,
        "never-bound",
        false,
        false,
    )
    .await;
    let unbound = seed_item(
        &store,
        player_id,
        &definition_key,
        &nonce,
        "unbound",
        false,
        false,
    )
    .await;

    for item_id in [favorite, protected, both, never_bound, unbound] {
        seed_structural_state(&store, item_id).await;
    }
    for item_id in [favorite, protected, both] {
        seed_bound_state(&store, item_id).await;
    }
    let rebind_not_before: DateTime<Utc> =
        sqlx::query_scalar("SELECT clock_timestamp() + INTERVAL '1 day'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO item_instance_soulbind_state (item_instance_id, is_soulbound, rebind_not_before) VALUES ($1, FALSE, $2)",
    )
    .bind(unbound)
    .bind(rebind_not_before)
    .execute(store.pool())
    .await
    .unwrap();

    let mut favorite_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_preview_soulbind_unbind_for_owned_ordinary_equipment(
            &mut favorite_tx,
            player_id,
            favorite,
        )
        .await,
        Err(OrdinarySoulBindUnbindPreflightError::ControlFlagsSet {
            is_favorite: true,
            is_protected: false,
        })
    ));
    favorite_tx.rollback().await.unwrap();

    let mut protected_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_preview_soulbind_unbind_for_owned_ordinary_equipment(
            &mut protected_tx,
            player_id,
            protected,
        )
        .await,
        Err(OrdinarySoulBindUnbindPreflightError::ControlFlagsSet {
            is_favorite: false,
            is_protected: true,
        })
    ));
    protected_tx.rollback().await.unwrap();

    let mut both_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_preview_soulbind_unbind_for_owned_ordinary_equipment(&mut both_tx, player_id, both)
            .await,
        Err(OrdinarySoulBindUnbindPreflightError::ControlFlagsSet {
            is_favorite: true,
            is_protected: true,
        })
    ));
    both_tx.rollback().await.unwrap();

    let mut never_bound_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_preview_soulbind_unbind_for_owned_ordinary_equipment(
            &mut never_bound_tx,
            player_id,
            never_bound,
        )
        .await,
        Err(OrdinarySoulBindUnbindPreflightError::NotSoulBound)
    ));
    never_bound_tx.rollback().await.unwrap();

    let mut unbound_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_preview_soulbind_unbind_for_owned_ordinary_equipment(
            &mut unbound_tx,
            player_id,
            unbound,
        )
        .await,
        Err(OrdinarySoulBindUnbindPreflightError::NotSoulBound)
    ));
    unbound_tx.rollback().await.unwrap();

    for item_id in [favorite, protected, both] {
        assert_eq!(soulbind_row(&store, item_id).await, Some((true, None)));
    }
    assert_eq!(soulbind_row(&store, never_bound).await, None);
    assert_eq!(
        soulbind_row(&store, unbound).await,
        Some((false, Some(rebind_not_before)))
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

async fn seed_definition(store: &PgStore, key: &str) {
    let data = r#"{"tier":"NETHERITE"}"#;
    sqlx::query("INSERT INTO item_definitions (key, category, stackable, active, definition_version, rarity, stack_limit, data) VALUES ($1, 'PICKAXE', FALSE, TRUE, 1, 'COMMON', NULL, $2::jsonb)")
        .bind(key)
        .bind(data)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO item_definition_versions (key, version, category, stackable, rarity, stack_limit, is_ordinary_equipment, data) VALUES ($1, 1, 'PICKAXE', FALSE, 'COMMON', NULL, TRUE, $2::jsonb)")
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
    is_favorite: bool,
    is_protected: bool,
) -> Uuid {
    let operation_id = Uuid::now_v7();
    let discord_user_id: i64 =
        sqlx::query_scalar("SELECT discord_user_id FROM players WHERE id = $1")
            .bind(player_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    sqlx::query("INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'SOULBIND_UNBIND_PREFLIGHT_TEST', 'PENDING', 1, $5, $6)")
        .bind(operation_id)
        .bind(format!("test:soulbind-unbind-preflight:{nonce}:{suffix}:{operation_id}"))
        .bind(discord_user_id)
        .bind(player_id)
        .bind([67_u8; 32].as_slice())
        .bind([71_u8; 32].as_slice())
        .execute(store.pool())
        .await
        .unwrap();

    let item_id = Uuid::now_v7();
    sqlx::query("INSERT INTO item_instances (id, definition_key, owner_player_id, created_by_operation_id, location, definition_version, is_favorite, is_protected) VALUES ($1, $2, $3, $4, 'TOOL_LOCKER', 1, $5, $6)")
        .bind(item_id)
        .bind(definition_key)
        .bind(player_id)
        .bind(operation_id)
        .bind(is_favorite)
        .bind(is_protected)
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
