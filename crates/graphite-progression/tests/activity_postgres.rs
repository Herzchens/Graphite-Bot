use graphite_progression::{
    ActivityXpError, ActivityXpMutationKind, ActivityXpMutationRequest, apply_activity_xp_mutation,
};
use graphite_store::PgStore;
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
async fn activity_xp_is_atomic_keyed_and_composite_operation_safe() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL is not set; skipping PostgreSQL integration test");
        return;
    };

    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate().await.unwrap();

    let nonce = Uuid::now_v7();
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    let discord_user_id = (raw % 8_000_000_000_000_000_000_u64).max(1);
    let persisted_discord_user_id = i64::try_from(discord_user_id).unwrap();
    let player_id = Uuid::now_v7();
    sqlx::query("INSERT INTO players (id, discord_user_id) VALUES ($1, $2)")
        .bind(player_id)
        .bind(persisted_discord_user_id)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO player_balances (player_id) VALUES ($1)")
        .bind(player_id)
        .execute(store.pool())
        .await
        .unwrap();

    let operation_id = Uuid::now_v7();
    insert_operation(
        &store,
        operation_id,
        player_id,
        persisted_discord_user_id,
        &format!("test:activity:composite:{nonce}"),
    )
    .await;

    let mut tx = store.pool().begin().await.unwrap();
    sqlx::query(
        r#"
        INSERT INTO progression_events (id, operation_id, player_id, event_kind, payload)
        VALUES ($1, $2, $3, 'ACCOUNT_XP_GRANTED', '{}'::jsonb)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(operation_id)
    .bind(player_id)
    .execute(&mut *tx)
    .await
    .unwrap();

    let grant_request = ActivityXpMutationRequest {
        operation_id,
        player_id,
        mutation_key: "activity:manual-reward".to_owned(),
        kind: ActivityXpMutationKind::Grant,
        amount: 1_000,
        source: "TEST_MANUAL_REWARD".to_owned(),
        provenance: json!({
            "origin": "integration_test",
            "base_points": 1_000,
            "modifiers_already_applied": true,
        }),
    };
    let grant = apply_activity_xp_mutation(&mut tx, &grant_request)
        .await
        .unwrap();
    assert_eq!(grant.before.points, 0);
    assert_eq!(grant.after.points, 1_000);

    let duplicate = apply_activity_xp_mutation(&mut tx, &grant_request)
        .await
        .unwrap();
    assert_eq!(duplicate, grant);

    let spend_request = ActivityXpMutationRequest {
        operation_id,
        player_id,
        mutation_key: "activity:service-cost".to_owned(),
        kind: ActivityXpMutationKind::Spend,
        amount: 250,
        source: "TEST_SERVICE_COST".to_owned(),
        provenance: json!({
            "origin": "integration_test",
            "service": "fixture",
        }),
    };
    let spend = apply_activity_xp_mutation(&mut tx, &spend_request)
        .await
        .unwrap();
    assert_eq!(spend.before.points, 1_000);
    assert_eq!(spend.after.points, 750);

    let conflicting_request = ActivityXpMutationRequest {
        amount: 251,
        ..spend_request.clone()
    };
    let conflict = apply_activity_xp_mutation(&mut tx, &conflicting_request).await;
    assert!(matches!(conflict, Err(ActivityXpError::MutationConflict)));

    sqlx::query(
        "UPDATE operations SET state = 'COMMITTED', result = '{}'::jsonb, committed_at = now() WHERE id = $1",
    )
    .bind(operation_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let points: i64 =
        sqlx::query("SELECT activity_xp_points FROM player_progression WHERE player_id = $1")
            .bind(player_id)
            .fetch_one(store.pool())
            .await
            .unwrap()
            .try_get("activity_xp_points")
            .unwrap();
    assert_eq!(points, 750);

    let event_count: i64 =
        sqlx::query("SELECT COUNT(*) AS count FROM progression_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap()
            .try_get("count")
            .unwrap();
    assert_eq!(event_count, 3);

    let mut replay_tx = store.pool().begin().await.unwrap();
    let committed_replay = apply_activity_xp_mutation(&mut replay_tx, &grant_request)
        .await
        .unwrap();
    assert_eq!(committed_replay, grant);
    replay_tx.commit().await.unwrap();

    let points_after_replay: i64 =
        sqlx::query("SELECT activity_xp_points FROM player_progression WHERE player_id = $1")
            .bind(player_id)
            .fetch_one(store.pool())
            .await
            .unwrap()
            .try_get("activity_xp_points")
            .unwrap();
    assert_eq!(points_after_replay, 750);

    let insufficient_operation_id = Uuid::now_v7();
    insert_operation(
        &store,
        insufficient_operation_id,
        player_id,
        persisted_discord_user_id,
        &format!("test:activity:insufficient:{nonce}"),
    )
    .await;
    let insufficient_request = ActivityXpMutationRequest {
        operation_id: insufficient_operation_id,
        player_id,
        mutation_key: "activity:overspend".to_owned(),
        kind: ActivityXpMutationKind::Spend,
        amount: 751,
        source: "TEST_OVERSPEND".to_owned(),
        provenance: json!({"origin":"integration_test"}),
    };
    let mut insufficient_tx = store.pool().begin().await.unwrap();
    let insufficient =
        apply_activity_xp_mutation(&mut insufficient_tx, &insufficient_request).await;
    assert!(matches!(
        insufficient,
        Err(ActivityXpError::InsufficientActivityXp {
            available: 750,
            requested: 751,
        })
    ));
    insufficient_tx.rollback().await.unwrap();

    let final_points: i64 =
        sqlx::query("SELECT activity_xp_points FROM player_progression WHERE player_id = $1")
            .bind(player_id)
            .fetch_one(store.pool())
            .await
            .unwrap()
            .try_get("activity_xp_points")
            .unwrap();
    assert_eq!(final_points, 750);
}

async fn insert_operation(
    store: &PgStore,
    operation_id: Uuid,
    player_id: Uuid,
    actor_discord_user_id: i64,
    external_request_key: &str,
) {
    sqlx::query(
        r#"
        INSERT INTO operations (
            id, external_request_key, actor_discord_user_id, player_id, kind, state,
            policy_version, request_hash, rng_root
        )
        VALUES ($1, $2, $3, $4, 'TEST_COMPOSITE', 'PENDING', 1, $5, $6)
        "#,
    )
    .bind(operation_id)
    .bind(external_request_key)
    .bind(actor_discord_user_id)
    .bind(player_id)
    .bind(vec![0xA5_u8; 32])
    .bind(vec![0x5A_u8; 32])
    .execute(store.pool())
    .await
    .unwrap();
}
