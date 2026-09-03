use graphite_progression::{
    ActivityXpError, ActivityXpMutationKind, ActivityXpMutationRequest, apply_activity_xp_mutation,
    lock_activity_xp_settlement_context,
};
use graphite_store::PgStore;
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
async fn activity_xp_settlement_context_prelocks_progression_and_composes_with_keyed_spend() {
    let Some(store) = test_store().await else {
        return;
    };

    let nonce = Uuid::now_v7();
    let discord_user_id = positive_snowflake(nonce);
    let player_id = seed_player(&store, discord_user_id).await;
    sqlx::query("UPDATE player_progression SET activity_xp_points = 50000 WHERE player_id = $1")
        .bind(player_id)
        .execute(store.pool())
        .await
        .unwrap();
    let operation_id = seed_pending_operation(
        &store,
        player_id,
        discord_user_id,
        &format!("test:activity-prelock:{nonce}"),
    )
    .await;

    let mut owner_tx = store.pool().begin().await.unwrap();
    let context = lock_activity_xp_settlement_context(&mut owner_tx, operation_id, player_id)
        .await
        .unwrap();
    assert_eq!(context.operation_id, operation_id);
    assert_eq!(context.player_id, player_id);
    assert_eq!(context.activity_xp_points, 50_000);

    let mut competing_progression_tx = store.pool().begin().await.unwrap();
    sqlx::query("SET LOCAL lock_timeout = '100ms'")
        .execute(&mut *competing_progression_tx)
        .await
        .unwrap();
    let blocked_progression = sqlx::query(
        "UPDATE player_progression SET activity_xp_points = activity_xp_points + 1 WHERE player_id = $1",
    )
    .bind(player_id)
    .execute(&mut *competing_progression_tx)
    .await;
    assert!(
        blocked_progression.is_err(),
        "settlement context must retain the progression row lock"
    );
    competing_progression_tx.rollback().await.unwrap();

    let mut competing_player_tx = store.pool().begin().await.unwrap();
    sqlx::query("SET LOCAL lock_timeout = '100ms'")
        .execute(&mut *competing_player_tx)
        .await
        .unwrap();
    let blocked_player = sqlx::query("UPDATE players SET status = 'SOFT_FROZEN' WHERE id = $1")
        .bind(player_id)
        .execute(&mut *competing_player_tx)
        .await;
    assert!(
        blocked_player.is_err(),
        "settlement context must retain the player row lock"
    );
    competing_player_tx.rollback().await.unwrap();

    let request = ActivityXpMutationRequest {
        operation_id,
        player_id,
        mutation_key: "soulbind:aexp".to_owned(),
        kind: ActivityXpMutationKind::Spend,
        amount: 25_000,
        source: "SOULBIND_BIND".to_owned(),
        provenance: json!({
            "origin": "integration_test",
            "service": "soulbind_bind",
        }),
    };
    let receipt = apply_activity_xp_mutation(&mut owner_tx, &request)
        .await
        .unwrap();
    assert_eq!(receipt.before.points, 50_000);
    assert_eq!(receipt.after.points, 25_000);

    commit_operation(&mut owner_tx, operation_id).await;
    owner_tx.commit().await.unwrap();

    assert_eq!(activity_xp_points(&store, player_id).await, 25_000);
    assert_eq!(activity_event_count(&store, operation_id).await, 1);
}

#[tokio::test]
async fn activity_xp_settlement_context_fails_closed_for_operation_and_account_guards() {
    let Some(store) = test_store().await else {
        return;
    };

    let nonce = Uuid::now_v7();
    let owner_discord = positive_snowflake(nonce);
    let owner = seed_player(&store, owner_discord).await;
    let other = seed_player(&store, next_snowflake(nonce, 1)).await;
    let operation_id = seed_pending_operation(
        &store,
        owner,
        owner_discord,
        &format!("test:activity-prelock-guards:{nonce}:owner"),
    )
    .await;

    let mut mismatch_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_activity_xp_settlement_context(&mut mismatch_tx, operation_id, other).await,
        Err(ActivityXpError::OperationPlayerMismatch)
    ));
    mismatch_tx.rollback().await.unwrap();

    sqlx::query(
        "UPDATE operations SET state = 'COMMITTED', result = '{}'::jsonb, committed_at = now() WHERE id = $1",
    )
    .bind(operation_id)
    .execute(store.pool())
    .await
    .unwrap();
    let mut terminal_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_activity_xp_settlement_context(&mut terminal_tx, operation_id, owner).await,
        Err(ActivityXpError::OperationTerminal(ref state)) if state == "COMMITTED"
    ));
    terminal_tx.rollback().await.unwrap();

    let frozen_operation = seed_pending_operation(
        &store,
        owner,
        owner_discord,
        &format!("test:activity-prelock-guards:{nonce}:frozen"),
    )
    .await;
    sqlx::query("UPDATE players SET status = 'SOFT_FROZEN' WHERE id = $1")
        .bind(owner)
        .execute(store.pool())
        .await
        .unwrap();
    let mut frozen_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_activity_xp_settlement_context(&mut frozen_tx, frozen_operation, owner).await,
        Err(ActivityXpError::AccountFrozen(ref status)) if status == "SOFT_FROZEN"
    ));
    frozen_tx.rollback().await.unwrap();
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

async fn seed_pending_operation(
    store: &PgStore,
    player_id: Uuid,
    discord_user_id: i64,
    external_request_key: &str,
) -> Uuid {
    let operation_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO operations (
            id, external_request_key, actor_discord_user_id, player_id, kind, state,
            policy_version, request_hash, rng_root
        )
        VALUES ($1, $2, $3, $4, 'ACTIVITY_PRELOCK_TEST', 'PENDING', 1, $5, $6)
        "#,
    )
    .bind(operation_id)
    .bind(external_request_key)
    .bind(discord_user_id)
    .bind(player_id)
    .bind([31_u8; 32].as_slice())
    .bind([37_u8; 32].as_slice())
    .execute(store.pool())
    .await
    .unwrap();
    operation_id
}

async fn commit_operation(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, operation_id: Uuid) {
    let result = sqlx::query(
        "UPDATE operations SET state = 'COMMITTED', result = '{}'::jsonb, committed_at = now() WHERE id = $1 AND state = 'PENDING'",
    )
    .bind(operation_id)
    .execute(&mut **tx)
    .await
    .unwrap();
    assert_eq!(result.rows_affected(), 1);
}

async fn activity_xp_points(store: &PgStore, player_id: Uuid) -> i64 {
    sqlx::query("SELECT activity_xp_points FROM player_progression WHERE player_id = $1")
        .bind(player_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
        .try_get("activity_xp_points")
        .unwrap()
}

async fn activity_event_count(store: &PgStore, operation_id: Uuid) -> i64 {
    sqlx::query("SELECT COUNT(*) AS count FROM progression_events WHERE operation_id = $1")
        .bind(operation_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
        .try_get("count")
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
