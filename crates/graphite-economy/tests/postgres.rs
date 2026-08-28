use graphite_economy::{BANK_MIN_WITHDRAWAL, BankError, BankService};
use graphite_store::PgStore;
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
async fn bank_mutations_are_idempotent_fifo_and_ledger_backed() {
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
    sqlx::query("INSERT INTO player_balances (player_id, wallet) VALUES ($1, 100000)")
        .bind(player_id)
        .execute(store.pool())
        .await
        .unwrap();

    let bank = BankService::new(store.clone());
    let deposit_key = format!("test:bank-deposit:{nonce}");
    let first_deposit = bank
        .deposit(discord_user_id, 20_000, &deposit_key)
        .await
        .unwrap();
    let deposit_retry = bank
        .deposit(discord_user_id, 20_000, &deposit_key)
        .await
        .unwrap();
    assert_eq!(first_deposit, deposit_retry);
    assert_eq!(first_deposit.wallet, 80_000);
    assert_eq!(first_deposit.bank, 20_000);

    let conflict = bank.deposit(discord_user_id, 20_001, &deposit_key).await;
    assert!(matches!(conflict, Err(BankError::IdempotencyConflict)));

    let withdraw_key = format!("test:bank-withdraw:{nonce}");
    let first_withdrawal = bank
        .withdraw(discord_user_id, 10_000, &withdraw_key)
        .await
        .unwrap();
    let withdrawal_retry = bank
        .withdraw(discord_user_id, 10_000, &withdraw_key)
        .await
        .unwrap();
    assert_eq!(first_withdrawal, withdrawal_retry);
    assert_eq!(first_withdrawal.gross_amount, 10_000);
    assert_eq!(first_withdrawal.fee_amount, 100);
    assert_eq!(first_withdrawal.net_amount, 9_900);
    assert_eq!(first_withdrawal.wallet, 89_900);
    assert_eq!(first_withdrawal.bank, 10_000);

    let snapshot = bank.snapshot(discord_user_id).await.unwrap();
    assert_eq!(snapshot.wallet, 89_900);
    assert_eq!(snapshot.bank, 10_000);
    assert_eq!(snapshot.active_lot_count, 1);

    let lot_principal: i64 = sqlx::query(
        "SELECT COALESCE(SUM(principal_remaining), 0)::BIGINT AS principal FROM bank_lots WHERE player_id = $1",
    )
    .bind(player_id)
    .fetch_one(store.pool())
    .await
    .unwrap()
    .try_get("principal")
    .unwrap();
    assert_eq!(lot_principal, 10_000);

    let withdrawal_audit_count: i64 =
        sqlx::query("SELECT COUNT(*) AS count FROM bank_withdrawals WHERE operation_id = $1")
            .bind(first_withdrawal.operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap()
            .try_get("count")
            .unwrap();
    assert_eq!(withdrawal_audit_count, 1);

    let below_minimum = bank
        .withdraw(
            discord_user_id,
            BANK_MIN_WITHDRAWAL - 1,
            &format!("test:bank-too-small:{nonce}"),
        )
        .await;
    assert!(matches!(
        below_minimum,
        Err(BankError::BelowMinimumWithdrawal)
    ));
}
