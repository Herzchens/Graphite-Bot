use graphite_progression::{
    ACCOUNT_XP_CAP, AccountXpSettlementError, AccountXpSettlementRequest,
    account_total_xp_for_level, apply_account_xp_settlement, level_money_reward,
    lock_account_xp_settlement_context,
};
use graphite_store::PgStore;
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
async fn account_xp_settlement_replays_exactly_and_rolls_back_with_owner() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce, 0)).await;
    let operation_id = seed_operation(&store, player_id, nonce, "rollback").await;
    let request = request(operation_id, player_id, 2, "rollback");

    let mut owner = store.pool().begin().await.unwrap();
    let context = lock_account_xp_settlement_context(&mut owner, operation_id, player_id)
        .await
        .unwrap();
    assert_eq!(context.account_xp, 0);
    assert_eq!(context.wallet, 0);

    let first = apply_account_xp_settlement(&mut owner, &request)
        .await
        .unwrap();
    let replay = apply_account_xp_settlement(&mut owner, &request)
        .await
        .unwrap();
    assert_eq!(replay, first);
    assert_eq!(first.requested_xp, 2);
    assert_eq!(first.granted_xp, 2);
    assert_eq!(first.account_xp_before, 0);
    assert_eq!(first.account_xp_after, 2);
    assert_eq!(first.level_money_reward, 0);
    assert_eq!(first.wallet_before, 0);
    assert_eq!(first.wallet_after, 0);

    owner.rollback().await.unwrap();
    assert_eq!(account_xp(&store, player_id).await, 0);
    assert_eq!(wallet(&store, player_id).await, 0);
    assert_eq!(progression_event_count(&store, operation_id).await, 0);
    assert_eq!(ledger_count(&store, operation_id).await, 0);
}

#[tokio::test]
async fn account_xp_settlement_pays_level_reward_atomically_and_replays_after_commit() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce, 0)).await;
    let level_two_start = account_total_xp_for_level(2).unwrap();
    sqlx::query("UPDATE player_progression SET account_xp = $1 WHERE player_id = $2")
        .bind(level_two_start - 1)
        .bind(player_id)
        .execute(store.pool())
        .await
        .unwrap();
    let operation_id = seed_operation(&store, player_id, nonce, "level-reward").await;
    let request = request(operation_id, player_id, 2, "level-reward");

    let mut owner = store.pool().begin().await.unwrap();
    lock_account_xp_settlement_context(&mut owner, operation_id, player_id)
        .await
        .unwrap();
    let receipt = apply_account_xp_settlement(&mut owner, &request)
        .await
        .unwrap();
    let expected_reward = level_money_reward(2).unwrap();
    assert_eq!(receipt.level_before, 1);
    assert_eq!(receipt.level_after, 2);
    assert_eq!(receipt.level_money_reward, expected_reward);
    assert_eq!(receipt.wallet_before, 0);
    assert_eq!(receipt.wallet_after, expected_reward);

    sqlx::query(
        "UPDATE operations SET state = 'COMMITTED', committed_at = now() WHERE id = $1 AND state = 'PENDING'",
    )
    .bind(operation_id)
    .execute(&mut *owner)
    .await
    .unwrap();
    owner.commit().await.unwrap();

    assert_eq!(account_xp(&store, player_id).await, level_two_start + 1);
    assert_eq!(wallet(&store, player_id).await, expected_reward);
    assert_eq!(progression_event_count(&store, operation_id).await, 1);
    assert_eq!(ledger_count(&store, operation_id).await, 1);

    let ledger = sqlx::query("SELECT id, kind FROM ledger_transactions WHERE operation_id = $1")
        .bind(operation_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
    let ledger_id: Uuid = ledger.try_get("id").unwrap();
    let ledger_kind: String = ledger.try_get("kind").unwrap();
    assert_eq!(ledger_kind, "LEVEL_REWARD");
    let posting_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(amount), 0)::BIGINT FROM ledger_postings WHERE transaction_id = $1",
    )
    .bind(ledger_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(posting_sum, 0);

    let mut replay_tx = store.pool().begin().await.unwrap();
    let replay = apply_account_xp_settlement(&mut replay_tx, &request)
        .await
        .unwrap();
    assert_eq!(replay, receipt);

    let mut conflicting = request.clone();
    conflicting.amount = 3;
    assert!(matches!(
        apply_account_xp_settlement(&mut replay_tx, &conflicting).await,
        Err(AccountXpSettlementError::MutationConflict)
    ));
    replay_tx.rollback().await.unwrap();

    assert_eq!(account_xp(&store, player_id).await, level_two_start + 1);
    assert_eq!(wallet(&store, player_id).await, expected_reward);
    assert_eq!(progression_event_count(&store, operation_id).await, 1);
    assert_eq!(ledger_count(&store, operation_id).await, 1);
}

#[tokio::test]
async fn account_xp_settlement_at_cap_records_zero_effect_without_money() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce, 0)).await;
    sqlx::query("UPDATE player_progression SET account_xp = $1 WHERE player_id = $2")
        .bind(ACCOUNT_XP_CAP)
        .bind(player_id)
        .execute(store.pool())
        .await
        .unwrap();
    let operation_id = seed_operation(&store, player_id, nonce, "cap").await;
    let request = request(operation_id, player_id, 2, "cap");

    let mut owner = store.pool().begin().await.unwrap();
    let context = lock_account_xp_settlement_context(&mut owner, operation_id, player_id)
        .await
        .unwrap();
    assert_eq!(context.account_xp, ACCOUNT_XP_CAP);

    let receipt = apply_account_xp_settlement(&mut owner, &request)
        .await
        .unwrap();
    assert_eq!(receipt.requested_xp, 2);
    assert_eq!(receipt.granted_xp, 0);
    assert_eq!(receipt.account_xp_before, ACCOUNT_XP_CAP);
    assert_eq!(receipt.account_xp_after, ACCOUNT_XP_CAP);
    assert_eq!(receipt.level_before, 200);
    assert_eq!(receipt.level_after, 200);
    assert_eq!(receipt.level_money_reward, 0);
    assert_eq!(receipt.wallet_before, 0);
    assert_eq!(receipt.wallet_after, 0);

    sqlx::query(
        "UPDATE operations SET state = 'COMMITTED', committed_at = now() WHERE id = $1 AND state = 'PENDING'",
    )
    .bind(operation_id)
    .execute(&mut *owner)
    .await
    .unwrap();
    owner.commit().await.unwrap();

    assert_eq!(account_xp(&store, player_id).await, ACCOUNT_XP_CAP);
    assert_eq!(wallet(&store, player_id).await, 0);
    assert_eq!(progression_event_count(&store, operation_id).await, 1);
    assert_eq!(ledger_count(&store, operation_id).await, 0);
}

#[tokio::test]
async fn account_xp_settlement_rejects_an_existing_monetary_ledger() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce, 0)).await;
    let operation_id = seed_operation(&store, player_id, nonce, "ledger-conflict").await;
    seed_balanced_ledger(&store, operation_id).await;
    let request = request(operation_id, player_id, 2, "ledger-conflict");

    let mut tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_account_xp_settlement_context(&mut tx, operation_id, player_id).await,
        Err(AccountXpSettlementError::MonetaryLedgerConflict(ref kind)) if kind == "TEST_LEDGER"
    ));
    tx.rollback().await.unwrap();

    let mut tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        apply_account_xp_settlement(&mut tx, &request).await,
        Err(AccountXpSettlementError::MonetaryLedgerConflict(ref kind)) if kind == "TEST_LEDGER"
    ));
    tx.rollback().await.unwrap();

    assert_eq!(account_xp(&store, player_id).await, 0);
    assert_eq!(progression_event_count(&store, operation_id).await, 0);
    assert_eq!(ledger_count(&store, operation_id).await, 1);
}

#[tokio::test]
async fn account_xp_prelock_retains_player_balance_and_progression_locks() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce, 0)).await;
    let operation_id = seed_operation(&store, player_id, nonce, "lock-retention").await;

    let mut owner = store.pool().begin().await.unwrap();
    lock_account_xp_settlement_context(&mut owner, operation_id, player_id)
        .await
        .unwrap();

    assert_nowait_row_locked(
        &store,
        "SELECT id FROM players WHERE id = $1 FOR UPDATE NOWAIT",
        player_id,
    )
    .await;
    assert_nowait_row_locked(
        &store,
        "SELECT player_id FROM player_balances WHERE player_id = $1 FOR UPDATE NOWAIT",
        player_id,
    )
    .await;
    assert_nowait_row_locked(
        &store,
        "SELECT player_id FROM player_progression WHERE player_id = $1 FOR UPDATE NOWAIT",
        player_id,
    )
    .await;

    owner.rollback().await.unwrap();
}

#[tokio::test]
async fn account_xp_prelock_rejects_player_actor_status_and_operation_mismatches() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce, 0)).await;
    let other_player_id = seed_player(&store, positive_snowflake(nonce, 2)).await;

    let player_mismatch_operation = seed_operation(&store, player_id, nonce, "player").await;
    let mut tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_account_xp_settlement_context(&mut tx, player_mismatch_operation, other_player_id)
            .await,
        Err(AccountXpSettlementError::OperationPlayerMismatch)
    ));
    tx.rollback().await.unwrap();

    let wrong_actor_operation = seed_operation_with_actor(
        &store,
        player_id,
        positive_snowflake(nonce, 1),
        nonce,
        "actor",
    )
    .await;
    let mut tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_account_xp_settlement_context(&mut tx, wrong_actor_operation, player_id).await,
        Err(AccountXpSettlementError::OperationActorMismatch)
    ));
    tx.rollback().await.unwrap();

    sqlx::query("UPDATE players SET status = 'SOFT_FROZEN' WHERE id = $1")
        .bind(player_id)
        .execute(store.pool())
        .await
        .unwrap();
    let frozen_operation = seed_operation(&store, player_id, nonce, "frozen").await;
    let mut tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_account_xp_settlement_context(&mut tx, frozen_operation, player_id).await,
        Err(AccountXpSettlementError::AccountFrozen(ref status)) if status == "SOFT_FROZEN"
    ));
    tx.rollback().await.unwrap();

    sqlx::query("UPDATE players SET status = 'ACTIVE' WHERE id = $1")
        .bind(player_id)
        .execute(store.pool())
        .await
        .unwrap();
    let terminal_operation = seed_operation(&store, player_id, nonce, "terminal").await;
    sqlx::query("UPDATE operations SET state = 'CANCELLED' WHERE id = $1")
        .bind(terminal_operation)
        .execute(store.pool())
        .await
        .unwrap();
    let mut tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_account_xp_settlement_context(&mut tx, terminal_operation, player_id).await,
        Err(AccountXpSettlementError::OperationTerminal(ref state)) if state == "CANCELLED"
    ));
    tx.rollback().await.unwrap();
}

fn request(
    operation_id: Uuid,
    player_id: Uuid,
    amount: i64,
    label: &str,
) -> AccountXpSettlementRequest {
    AccountXpSettlementRequest {
        operation_id,
        player_id,
        amount,
        source: "MANUAL_FISHING".to_owned(),
        provenance: json!({"test_case": label}),
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
    let actor: i64 = sqlx::query_scalar("SELECT discord_user_id FROM players WHERE id = $1")
        .bind(player_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
    seed_operation_with_actor(store, player_id, actor, nonce, suffix).await
}

async fn seed_operation_with_actor(
    store: &PgStore,
    player_id: Uuid,
    actor: i64,
    nonce: Uuid,
    suffix: &str,
) -> Uuid {
    let operation_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'ACCOUNT_XP_SETTLEMENT_TEST', 'PENDING', 1, $5, $6)",
    )
    .bind(operation_id)
    .bind(format!("test:account-xp-settlement:{nonce}:{suffix}:{operation_id}"))
    .bind(actor)
    .bind(player_id)
    .bind([41_u8; 32].as_slice())
    .bind([43_u8; 32].as_slice())
    .execute(store.pool())
    .await
    .unwrap();
    operation_id
}

async fn seed_balanced_ledger(store: &PgStore, operation_id: Uuid) {
    let transaction_id = Uuid::now_v7();
    let mut tx = store.pool().begin().await.unwrap();
    sqlx::query(
        "INSERT INTO ledger_transactions (id, operation_id, kind, provenance) VALUES ($1, $2, 'TEST_LEDGER', '{}'::jsonb)",
    )
    .bind(transaction_id)
    .bind(operation_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO ledger_postings (transaction_id, sequence, account_kind, amount) VALUES ($1, 0, 'SYSTEM', 1), ($1, 1, 'SYSTEM', -1)",
    )
    .bind(transaction_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

async fn assert_nowait_row_locked(store: &PgStore, query: &'static str, player_id: Uuid) {
    let mut contender = store.pool().begin().await.unwrap();
    let error = sqlx::query_scalar::<_, Uuid>(query)
        .bind(player_id)
        .fetch_one(&mut *contender)
        .await
        .unwrap_err();
    let database = error
        .as_database_error()
        .expect("expected database lock error");
    assert_eq!(database.code().as_deref(), Some("55P03"));
    contender.rollback().await.unwrap();
}

fn positive_snowflake(nonce: Uuid, offset: u64) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    let value =
        ((raw % 7_000_000_000_000_000_000_u64) + offset + 1) % 8_000_000_000_000_000_000_u64;
    i64::try_from(value.max(1)).unwrap()
}

async fn account_xp(store: &PgStore, player_id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT account_xp FROM player_progression WHERE player_id = $1")
        .bind(player_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
}

async fn wallet(store: &PgStore, player_id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT wallet FROM player_balances WHERE player_id = $1")
        .bind(player_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
}

async fn progression_event_count(store: &PgStore, operation_id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM progression_events WHERE operation_id = $1")
        .bind(operation_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
}

async fn ledger_count(store: &PgStore, operation_id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM ledger_transactions WHERE operation_id = $1")
        .bind(operation_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
}
