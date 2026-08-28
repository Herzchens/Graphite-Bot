use graphite_progression::{
    ACCOUNT_LEVEL_REWARD_TOTAL, ACCOUNT_XP_CAP, ProgressionError, ProgressionService,
    account_total_xp_for_level, level_money_reward,
};
use graphite_store::PgStore;
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
async fn progression_is_capped_idempotent_ledgered_and_rebirth_safe() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL is not set; skipping PostgreSQL integration test");
        return;
    };

    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate().await.unwrap();

    let nonce = Uuid::now_v7();
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    let discord_user_id = (raw % 8_000_000_000_000_000_000_u64).max(1);
    let player_id = Uuid::now_v7();
    sqlx::query("INSERT INTO players (id, discord_user_id) VALUES ($1, $2)")
        .bind(player_id)
        .bind(i64::try_from(discord_user_id).unwrap())
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO player_balances (player_id) VALUES ($1)")
        .bind(player_id)
        .execute(store.pool())
        .await
        .unwrap();

    let progression = ProgressionService::new(store.clone());
    let initial = progression.snapshot(discord_user_id).await.unwrap();
    assert_eq!(initial.rebirth_count, 0);
    assert_eq!(initial.account.level, 1);
    assert_eq!(initial.account.total_xp, 0);
    assert_eq!(initial.activity.level, 0);
    assert_eq!(initial.activity.points, 0);

    let level_five_xp = account_total_xp_for_level(5).unwrap();
    let first_key = format!("test:account-xp:first:{nonce}");
    let first = progression
        .grant_account_xp(discord_user_id, level_five_xp, "TEST", &first_key)
        .await
        .unwrap();
    assert_eq!(first.granted_xp, level_five_xp);
    assert_eq!(first.level_before, 1);
    assert_eq!(first.level_after, 5);
    let expected_first_reward: i64 = (2_u16..=5)
        .map(|level| level_money_reward(level).unwrap())
        .sum();
    assert_eq!(first.level_money_reward, expected_first_reward);
    assert_eq!(first.wallet_after, expected_first_reward);

    let replay = progression
        .grant_account_xp(discord_user_id, level_five_xp, "TEST", &first_key)
        .await
        .unwrap();
    assert_eq!(replay, first);

    let conflict = progression
        .grant_account_xp(discord_user_id, level_five_xp + 1, "TEST", &first_key)
        .await;
    assert!(matches!(
        conflict,
        Err(ProgressionError::IdempotencyConflict)
    ));

    let cap_key = format!("test:account-xp:cap:{nonce}");
    let cap = progression
        .grant_account_xp(discord_user_id, ACCOUNT_XP_CAP, "TEST", &cap_key)
        .await
        .unwrap();
    assert_eq!(cap.granted_xp, ACCOUNT_XP_CAP - level_five_xp);
    assert_eq!(cap.level_after, 200);
    assert_eq!(cap.account_xp_after, ACCOUNT_XP_CAP);
    assert_eq!(cap.wallet_after, ACCOUNT_LEVEL_REWARD_TOTAL);

    sqlx::query("UPDATE player_progression SET activity_xp_points = 1600 WHERE player_id = $1")
        .bind(player_id)
        .execute(store.pool())
        .await
        .unwrap();
    let before_rebirth = progression.snapshot(discord_user_id).await.unwrap();
    assert_eq!(before_rebirth.activity.points, 1_600);
    assert_eq!(before_rebirth.activity.level, 31);

    let rebirth_key = format!("test:rebirth:{nonce}");
    let rebirth = progression
        .rebirth(discord_user_id, &rebirth_key)
        .await
        .unwrap();
    assert_eq!(rebirth.previous_rebirth_count, 0);
    assert_eq!(rebirth.rebirth_count, 1);
    assert_eq!(rebirth.activity_xp_points, 1_600);
    assert_eq!(rebirth.activity_level, 31);

    let rebirth_replay = progression
        .rebirth(discord_user_id, &rebirth_key)
        .await
        .unwrap();
    assert_eq!(rebirth_replay, rebirth);

    let after_rebirth = progression.snapshot(discord_user_id).await.unwrap();
    assert_eq!(after_rebirth.rebirth_count, 1);
    assert_eq!(after_rebirth.account.level, 1);
    assert_eq!(after_rebirth.account.total_xp, 0);
    assert_eq!(after_rebirth.activity.points, 1_600);
    assert_eq!(after_rebirth.activity.level, 31);

    let second_rebirth = progression
        .rebirth(discord_user_id, &format!("test:rebirth:second:{nonce}"))
        .await;
    assert!(matches!(
        second_rebirth,
        Err(ProgressionError::RebirthRequiresLevelCap)
    ));

    let wallet: i64 = sqlx::query("SELECT wallet FROM player_balances WHERE player_id = $1")
        .bind(player_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
        .try_get("wallet")
        .unwrap();
    assert_eq!(wallet, ACCOUNT_LEVEL_REWARD_TOTAL);

    let reward_transactions: i64 = sqlx::query(
        "SELECT COUNT(*) AS count FROM ledger_transactions WHERE kind = 'LEVEL_REWARD' AND operation_id IN ($1, $2)",
    )
    .bind(first.operation_id)
    .bind(cap.operation_id)
    .fetch_one(store.pool())
    .await
    .unwrap()
    .try_get("count")
    .unwrap();
    assert_eq!(reward_transactions, 2);

    let mutation =
        sqlx::query("UPDATE progression_events SET payload = '{}'::jsonb WHERE operation_id = $1")
            .bind(first.operation_id)
            .execute(store.pool())
            .await;
    assert!(
        mutation.is_err(),
        "progression event history must remain immutable"
    );
}
