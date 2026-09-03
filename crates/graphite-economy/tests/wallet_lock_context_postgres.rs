use graphite_economy::{
    WalletSpendError, WalletSpendRequest, apply_wallet_spend, lock_new_wallet_spend_context,
};
use graphite_store::PgStore;
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn wallet_spend_context_preserves_lock_order_and_rejects_non_new_mutations() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL is not set; skipping PostgreSQL integration test");
        return;
    };

    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate().await.unwrap();

    let nonce = Uuid::now_v7();
    let discord_user_id = positive_snowflake(nonce);
    let player_id = Uuid::now_v7();
    insert_player(&store, player_id, discord_user_id, 500).await;

    let lock_operation_id = Uuid::now_v7();
    insert_operation(
        &store,
        lock_operation_id,
        Some(player_id),
        discord_user_id,
        "PENDING",
        &format!("test:wallet-lock-context:lock:{nonce}"),
    )
    .await;

    let mut owner_tx = store.pool().begin().await.unwrap();
    let context = lock_new_wallet_spend_context(&mut owner_tx, lock_operation_id, player_id)
        .await
        .unwrap();
    assert_eq!(context.operation_id, lock_operation_id);
    assert_eq!(context.player_id, player_id);
    assert_eq!(context.wallet, 500);

    let mut competing_tx = store.pool().begin().await.unwrap();
    let lock_error = sqlx::query(
        r#"
        SELECT p.id
          FROM players p
          JOIN player_balances b ON b.player_id = p.id
         WHERE p.id = $1
         FOR UPDATE OF p, b NOWAIT
        "#,
    )
    .bind(player_id)
    .fetch_one(&mut *competing_tx)
    .await
    .unwrap_err();
    assert_lock_not_available(lock_error);
    competing_tx.rollback().await.unwrap();
    owner_tx.rollback().await.unwrap();

    let mut after_release_tx = store.pool().begin().await.unwrap();
    sqlx::query(
        r#"
        SELECT p.id
          FROM players p
          JOIN player_balances b ON b.player_id = p.id
         WHERE p.id = $1
         FOR UPDATE OF p, b NOWAIT
        "#,
    )
    .bind(player_id)
    .fetch_one(&mut *after_release_tx)
    .await
    .unwrap();
    after_release_tx.rollback().await.unwrap();

    let existing_ledger_operation_id = Uuid::now_v7();
    insert_operation(
        &store,
        existing_ledger_operation_id,
        Some(player_id),
        discord_user_id,
        "PENDING",
        &format!("test:wallet-lock-context:ledger:{nonce}"),
    )
    .await;
    let mut spend_tx = store.pool().begin().await.unwrap();
    apply_wallet_spend(
        &mut spend_tx,
        &WalletSpendRequest {
            operation_id: existing_ledger_operation_id,
            player_id,
            amount: 1,
            source: "TEST_LOCK_CONTEXT".to_owned(),
            provenance: json!({"origin":"wallet_lock_context_test"}),
        },
    )
    .await
    .unwrap();
    spend_tx.commit().await.unwrap();

    let mut existing_ledger_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_new_wallet_spend_context(
            &mut existing_ledger_tx,
            existing_ledger_operation_id,
            player_id,
        )
        .await,
        Err(WalletSpendError::MutationConflict)
    ));
    existing_ledger_tx.rollback().await.unwrap();

    let terminal_operation_id = Uuid::now_v7();
    insert_operation(
        &store,
        terminal_operation_id,
        Some(player_id),
        discord_user_id,
        "COMMITTED",
        &format!("test:wallet-lock-context:terminal:{nonce}"),
    )
    .await;
    let mut terminal_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_new_wallet_spend_context(&mut terminal_tx, terminal_operation_id, player_id).await,
        Err(WalletSpendError::OperationTerminal(ref state)) if state == "COMMITTED"
    ));
    terminal_tx.rollback().await.unwrap();

    let mismatch_operation_id = Uuid::now_v7();
    insert_operation(
        &store,
        mismatch_operation_id,
        Some(player_id),
        discord_user_id,
        "PENDING",
        &format!("test:wallet-lock-context:mismatch:{nonce}"),
    )
    .await;
    let mut mismatch_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_new_wallet_spend_context(&mut mismatch_tx, mismatch_operation_id, Uuid::now_v7())
            .await,
        Err(WalletSpendError::OperationPlayerMismatch)
    ));
    mismatch_tx.rollback().await.unwrap();

    sqlx::query("UPDATE players SET status = 'SOFT_FROZEN' WHERE id = $1")
        .bind(player_id)
        .execute(store.pool())
        .await
        .unwrap();
    let frozen_operation_id = Uuid::now_v7();
    insert_operation(
        &store,
        frozen_operation_id,
        Some(player_id),
        discord_user_id,
        "PENDING",
        &format!("test:wallet-lock-context:frozen:{nonce}"),
    )
    .await;
    let mut frozen_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_new_wallet_spend_context(&mut frozen_tx, frozen_operation_id, player_id).await,
        Err(WalletSpendError::AccountFrozen(ref status)) if status == "SOFT_FROZEN"
    ));
    frozen_tx.rollback().await.unwrap();
}

async fn insert_player(store: &PgStore, player_id: Uuid, discord_user_id: i64, wallet: i64) {
    sqlx::query("INSERT INTO players (id, discord_user_id) VALUES ($1, $2)")
        .bind(player_id)
        .bind(discord_user_id)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO player_balances (player_id, wallet) VALUES ($1, $2)")
        .bind(player_id)
        .bind(wallet)
        .execute(store.pool())
        .await
        .unwrap();
}

async fn insert_operation(
    store: &PgStore,
    operation_id: Uuid,
    player_id: Option<Uuid>,
    actor_discord_user_id: i64,
    state: &str,
    external_request_key: &str,
) {
    sqlx::query(
        r#"
        INSERT INTO operations (
            id, external_request_key, actor_discord_user_id, player_id, kind, state,
            policy_version, request_hash, rng_root, result, committed_at
        )
        VALUES (
            $1, $2, $3, $4, 'TEST_WALLET_LOCK_CONTEXT', $5,
            1, $6, $7,
            CASE WHEN $5 = 'COMMITTED' THEN '{}'::jsonb ELSE NULL END,
            CASE WHEN $5 = 'COMMITTED' THEN now() ELSE NULL END
        )
        "#,
    )
    .bind(operation_id)
    .bind(external_request_key)
    .bind(actor_discord_user_id)
    .bind(player_id)
    .bind(state)
    .bind(vec![0xC3_u8; 32])
    .bind(vec![0x3C_u8; 32])
    .execute(store.pool())
    .await
    .unwrap();
}

fn assert_lock_not_available(error: sqlx::Error) {
    let sqlx::Error::Database(database) = error else {
        panic!("expected PostgreSQL lock-not-available error");
    };
    assert_eq!(database.code().as_deref(), Some("55P03"));
}

fn positive_snowflake(nonce: Uuid) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    i64::try_from((raw % 8_000_000_000_000_000_000_u64).max(1)).unwrap()
}
