use graphite_services::{
    FishingRarity, MANUAL_FISHING_ACCOUNT_XP_REWARD, ManualFishingSuccessfulProgressionOutcomeV1,
    settle_manual_fishing_successful_progression_v1,
};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn successful_fish_progression_settles_once_and_replays_after_commit() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let operation_id = seed_operation(&store, player_id, nonce, "success").await;
    let outcome = ManualFishingSuccessfulProgressionOutcomeV1::Fish {
        landed_rarities: vec![FishingRarity::Common, FishingRarity::Mythic],
    };

    let mut tx = store.pool().begin().await.unwrap();
    let first =
        settle_manual_fishing_successful_progression_v1(&mut tx, operation_id, player_id, &outcome)
            .await
            .unwrap();
    assert_eq!(first.base_activity_xp, 9);
    assert_eq!(
        first.account_xp.requested_xp,
        MANUAL_FISHING_ACCOUNT_XP_REWARD
    );
    assert_eq!(first.account_xp.granted_xp, 2);
    assert_eq!(first.activity_xp.amount, 9);

    let replay =
        settle_manual_fishing_successful_progression_v1(&mut tx, operation_id, player_id, &outcome)
            .await
            .unwrap();
    assert_eq!(replay, first);

    sqlx::query("UPDATE operations SET state = 'COMMITTED', committed_at = now() WHERE id = $1")
        .bind(operation_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert_eq!(account_xp(&store, player_id).await, 2);
    assert_eq!(activity_xp(&store, player_id).await, 9);

    let mut committed_replay = store.pool().begin().await.unwrap();
    assert_eq!(
        settle_manual_fishing_successful_progression_v1(
            &mut committed_replay,
            operation_id,
            player_id,
            &outcome,
        )
        .await
        .unwrap(),
        first
    );
    committed_replay.rollback().await.unwrap();
    assert_eq!(account_xp(&store, player_id).await, 2);
    assert_eq!(activity_xp(&store, player_id).await, 9);
}

#[tokio::test]
async fn rollback_removes_both_progression_children_before_retry() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let operation_id = seed_operation(&store, player_id, nonce, "rollback").await;
    let outcome = ManualFishingSuccessfulProgressionOutcomeV1::Treasure { landed_count: 3 };

    let mut tx = store.pool().begin().await.unwrap();
    let first =
        settle_manual_fishing_successful_progression_v1(&mut tx, operation_id, player_id, &outcome)
            .await
            .unwrap();
    assert_eq!(first.base_activity_xp, 10);
    tx.rollback().await.unwrap();

    assert_eq!(account_xp(&store, player_id).await, 0);
    assert_eq!(activity_xp(&store, player_id).await, 0);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM progression_events WHERE operation_id = $1"
        )
        .bind(operation_id)
        .fetch_one(store.pool())
        .await
        .unwrap(),
        0
    );

    let mut retry = store.pool().begin().await.unwrap();
    let second = settle_manual_fishing_successful_progression_v1(
        &mut retry,
        operation_id,
        player_id,
        &outcome,
    )
    .await
    .unwrap();
    assert_eq!(second.base_activity_xp, 10);
    retry.commit().await.unwrap();

    assert_eq!(account_xp(&store, player_id).await, 2);
    assert_eq!(activity_xp(&store, player_id).await, 10);
}

#[tokio::test]
async fn junk_and_treasure_use_exact_frozen_reward_amounts() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();

    for (offset, outcome, expected_aexp) in [
        (
            0_u64,
            ManualFishingSuccessfulProgressionOutcomeV1::Junk,
            2_i64,
        ),
        (
            1_u64,
            ManualFishingSuccessfulProgressionOutcomeV1::Treasure { landed_count: 1 },
            5_i64,
        ),
        (
            2_u64,
            ManualFishingSuccessfulProgressionOutcomeV1::Treasure { landed_count: 2 },
            10_i64,
        ),
    ] {
        let player_id = seed_player(&store, next_snowflake(nonce, offset)).await;
        let operation_id =
            seed_operation(&store, player_id, nonce, &format!("table-{offset}")).await;
        let mut tx = store.pool().begin().await.unwrap();
        let receipt = settle_manual_fishing_successful_progression_v1(
            &mut tx,
            operation_id,
            player_id,
            &outcome,
        )
        .await
        .unwrap();
        assert_eq!(receipt.base_activity_xp, expected_aexp);
        tx.rollback().await.unwrap();
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

async fn seed_player(store: &PgStore, discord_user_id: i64) -> Uuid {
    let player_id = Uuid::now_v7();
    sqlx::query("INSERT INTO players (id, discord_user_id) VALUES ($1, $2)")
        .bind(player_id)
        .bind(discord_user_id)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO player_balances (player_id) VALUES ($1)")
        .bind(player_id)
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
        "INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'MANUAL_FISHING_PROGRESSION_TEST', 'PENDING', 1, $5, $6)",
    )
    .bind(operation_id)
    .bind(format!("test:fishing-progression:{nonce}:{suffix}:{operation_id}"))
    .bind(discord_user_id)
    .bind(player_id)
    .bind([131_u8; 32].as_slice())
    .bind([137_u8; 32].as_slice())
    .execute(store.pool())
    .await
    .unwrap();
    operation_id
}

async fn account_xp(store: &PgStore, player_id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT account_xp FROM player_progression WHERE player_id = $1")
        .bind(player_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
}

async fn activity_xp(store: &PgStore, player_id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT activity_xp_points FROM player_progression WHERE player_id = $1")
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
    let value = (raw.wrapping_add(offset) % 7_999_999_999_999_999_000_u64).saturating_add(1);
    i64::try_from(value).unwrap()
}
