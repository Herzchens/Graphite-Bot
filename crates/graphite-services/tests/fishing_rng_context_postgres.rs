use graphite_services::{ManualFishingRngContextError, lock_manual_fishing_rng_context};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn persisted_rng_context_replays_exactly_and_retains_the_operation_lock() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let operation_id = seed_operation(&store, player_id, nonce, "replay", [83; 32]).await;

    let mut owner = store.pool().begin().await.unwrap();
    let context = lock_manual_fishing_rng_context(&mut owner, operation_id, player_id)
        .await
        .unwrap();
    assert_eq!(context.operation_id(), operation_id);
    assert_operation_locked(&store, operation_id).await;
    owner.rollback().await.unwrap();

    let mut replay = store.pool().begin().await.unwrap();
    let replay_context = lock_manual_fishing_rng_context(&mut replay, operation_id, player_id)
        .await
        .unwrap();
    assert_eq!(replay_context, context);
    replay.rollback().await.unwrap();
}

#[tokio::test]
async fn rng_context_fails_closed_for_player_and_terminal_operation_mismatches() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let other_id = seed_player(&store, positive_snowflake_with_offset(nonce, 1)).await;
    let operation_id = seed_operation(&store, owner_id, nonce, "guards", [91; 32]).await;

    let mut wrong_player = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_manual_fishing_rng_context(&mut wrong_player, operation_id, other_id).await,
        Err(ManualFishingRngContextError::OperationPlayerMismatch)
    ));
    wrong_player.rollback().await.unwrap();

    sqlx::query("UPDATE operations SET state = 'COMMITTED' WHERE id = $1")
        .bind(operation_id)
        .execute(store.pool())
        .await
        .unwrap();

    let mut terminal = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_manual_fishing_rng_context(&mut terminal, operation_id, owner_id).await,
        Err(ManualFishingRngContextError::OperationTerminal(state)) if state == "COMMITTED"
    ));
    terminal.rollback().await.unwrap();
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

async fn seed_operation(
    store: &PgStore,
    player_id: Uuid,
    nonce: Uuid,
    suffix: &str,
    rng_root: [u8; 32],
) -> Uuid {
    let operation_id = Uuid::now_v7();
    let discord_user_id: i64 =
        sqlx::query_scalar("SELECT discord_user_id FROM players WHERE id = $1")
            .bind(player_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'MANUAL_FISHING_RNG_CONTEXT_TEST', 'PENDING', 1, $5, $6)",
    )
    .bind(operation_id)
    .bind(format!("test:manual-fishing-rng:{nonce}:{suffix}:{operation_id}"))
    .bind(discord_user_id)
    .bind(player_id)
    .bind([79_u8; 32].as_slice())
    .bind(rng_root.as_slice())
    .execute(store.pool())
    .await
    .unwrap();
    operation_id
}

async fn assert_operation_locked(store: &PgStore, operation_id: Uuid) {
    let mut contender = store.pool().begin().await.unwrap();
    let lock =
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM operations WHERE id = $1 FOR UPDATE NOWAIT")
            .bind(operation_id)
            .fetch_one(&mut *contender)
            .await;
    let error = lock.unwrap_err();
    let sqlx::Error::Database(database) = error else {
        panic!("expected PostgreSQL lock error, got {error:?}");
    };
    assert_eq!(database.code().as_deref(), Some("55P03"));
    contender.rollback().await.unwrap();
}

fn positive_snowflake(nonce: Uuid) -> i64 {
    positive_snowflake_with_offset(nonce, 0)
}

fn positive_snowflake_with_offset(nonce: Uuid, offset: u64) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    let value = (raw % 7_999_999_999_999_999_000_u64)
        .saturating_add(1)
        .saturating_add(offset);
    i64::try_from(value).unwrap()
}
