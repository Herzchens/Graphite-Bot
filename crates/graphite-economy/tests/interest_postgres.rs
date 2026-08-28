use graphite_economy::{BankInterestService, BankService};
use graphite_store::PgStore;
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
async fn bank_interest_accrues_with_ledger_remainder_and_freeze_semantics() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL is not set; skipping PostgreSQL integration test");
        return;
    };

    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate().await.unwrap();

    let active_user = create_player_with_wallet(&store, 250_000, "ACTIVE").await;
    let bank = BankService::new(store.clone());
    bank.deposit(
        active_user,
        100_000,
        &format!("test:interest-deposit:{}", Uuid::now_v7()),
    )
    .await
    .unwrap();
    make_interest_due(&store, active_user).await;

    let interest = BankInterestService::new(store.clone());
    let first = interest.accrue_interest(active_user).await.unwrap();
    assert_eq!(first.days_processed, 1);
    assert_eq!(first.interest_credited, 4);

    let replay = interest.accrue_interest(active_user).await.unwrap();
    assert_eq!(replay.days_processed, 0);
    assert_eq!(replay.interest_credited, 0);

    let active_player_id = player_id(&store, active_user).await;
    let bank_balance: i64 = sqlx::query("SELECT bank FROM player_balances WHERE player_id = $1")
        .bind(active_player_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
        .try_get("bank")
        .unwrap();
    assert_eq!(bank_balance, 100_004);

    let lot_total: i64 = sqlx::query(
        "SELECT COALESCE(SUM(principal_remaining), 0)::BIGINT AS principal FROM bank_lots WHERE player_id = $1",
    )
    .bind(active_player_id)
    .fetch_one(store.pool())
    .await
    .unwrap()
    .try_get("principal")
    .unwrap();
    assert_eq!(lot_total, 100_004);

    let ledger_count: i64 = sqlx::query(
        "SELECT COUNT(*) AS count FROM ledger_transactions WHERE kind = 'BANK_INTEREST' AND operation_id IN (SELECT id FROM operations WHERE player_id = $1)",
    )
    .bind(active_player_id)
    .fetch_one(store.pool())
    .await
    .unwrap()
    .try_get("count")
    .unwrap();
    assert_eq!(ledger_count, 1);

    let outbox_count: i64 = sqlx::query(
        "SELECT COUNT(*) AS count FROM outbox_events WHERE topic = 'bank.interest_accrued' AND operation_id IN (SELECT id FROM operations WHERE player_id = $1)",
    )
    .bind(active_player_id)
    .fetch_one(store.pool())
    .await
    .unwrap()
    .try_get("count")
    .unwrap();
    assert_eq!(outbox_count, 1);

    let system_actor: Option<i64> = sqlx::query(
        "SELECT actor_discord_user_id FROM operations WHERE player_id = $1 AND kind = 'BANK_INTEREST'",
    )
    .bind(active_player_id)
    .fetch_one(store.pool())
    .await
    .unwrap()
    .try_get("actor_discord_user_id")
    .unwrap();
    assert_eq!(system_actor, None);

    let soft_user = create_player_with_wallet(&store, 200_000, "ACTIVE").await;
    bank.deposit(
        soft_user,
        100_000,
        &format!("test:soft-deposit:{}", Uuid::now_v7()),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE players SET status = 'SOFT_FROZEN' WHERE discord_user_id = $1")
        .bind(i64::try_from(soft_user).unwrap())
        .execute(store.pool())
        .await
        .unwrap();
    make_interest_due(&store, soft_user).await;
    let soft = interest.accrue_interest(soft_user).await.unwrap();
    assert_eq!(soft.interest_credited, 4);

    let hard_user = create_player_with_wallet(&store, 200_000, "ACTIVE").await;
    bank.deposit(
        hard_user,
        100_000,
        &format!("test:hard-deposit:{}", Uuid::now_v7()),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE players SET status = 'HARD_FROZEN' WHERE discord_user_id = $1")
        .bind(i64::try_from(hard_user).unwrap())
        .execute(store.pool())
        .await
        .unwrap();
    make_interest_due(&store, hard_user).await;
    let hard = interest.accrue_interest(hard_user).await.unwrap();
    assert_eq!(hard.days_processed, 1);
    assert_eq!(hard.interest_credited, 0);

    let hard_player_id = player_id(&store, hard_user).await;
    let hard_bank: i64 = sqlx::query("SELECT bank FROM player_balances WHERE player_id = $1")
        .bind(hard_player_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
        .try_get("bank")
        .unwrap();
    assert_eq!(hard_bank, 100_000);
}

async fn create_player_with_wallet(store: &PgStore, wallet: i64, status: &str) -> u64 {
    let nonce = Uuid::now_v7();
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    let discord_user_id = (raw % 8_000_000_000_000_000_000_u64).max(1);
    let player_id = Uuid::now_v7();
    sqlx::query("INSERT INTO players (id, discord_user_id, status) VALUES ($1, $2, $3)")
        .bind(player_id)
        .bind(i64::try_from(discord_user_id).unwrap())
        .bind(status)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO player_balances (player_id, wallet) VALUES ($1, $2)")
        .bind(player_id)
        .bind(wallet)
        .execute(store.pool())
        .await
        .unwrap();
    discord_user_id
}

async fn make_interest_due(store: &PgStore, discord_user_id: u64) {
    sqlx::query(
        "UPDATE bank_interest_state SET last_accrual_day = (now() AT TIME ZONE 'UTC')::date - 1 WHERE player_id = (SELECT id FROM players WHERE discord_user_id = $1)",
    )
    .bind(i64::try_from(discord_user_id).unwrap())
    .execute(store.pool())
    .await
    .unwrap();
}

async fn player_id(store: &PgStore, discord_user_id: u64) -> Uuid {
    sqlx::query("SELECT id FROM players WHERE discord_user_id = $1")
        .bind(i64::try_from(discord_user_id).unwrap())
        .fetch_one(store.pool())
        .await
        .unwrap()
        .try_get("id")
        .unwrap()
}
