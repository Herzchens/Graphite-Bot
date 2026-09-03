use graphite_economy::{WalletSpendError, WalletSpendRequest, apply_wallet_spend};
use graphite_store::PgStore;
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
async fn wallet_spend_is_atomic_replay_safe_and_wallet_only() {
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
    insert_player(&store, player_id, persisted_discord_user_id, 500, 700, 11).await;

    let rollback_operation_id = Uuid::now_v7();
    insert_operation(
        &store,
        rollback_operation_id,
        Some(player_id),
        persisted_discord_user_id,
        "PENDING",
        &format!("test:wallet-spend:rollback:{nonce}"),
    )
    .await;
    let rollback_request = request(rollback_operation_id, player_id, 120, "ROLLBACK");
    let mut rollback_tx = store.pool().begin().await.unwrap();
    let rollback_receipt = apply_wallet_spend(&mut rollback_tx, &rollback_request)
        .await
        .unwrap();
    assert_eq!(rollback_receipt.wallet_before, 500);
    assert_eq!(rollback_receipt.wallet_after, 380);
    assert_eq!(
        balance_in_tx(&mut rollback_tx, player_id).await,
        (380, 700, 11)
    );
    assert_eq!(
        operation_state_in_tx(&mut rollback_tx, rollback_operation_id).await,
        "PENDING"
    );
    assert_ledger_in_tx(&mut rollback_tx, &rollback_receipt).await;
    rollback_tx.rollback().await.unwrap();
    assert_eq!(balance(&store, player_id).await, (500, 700, 11));
    assert_eq!(ledger_count(&store, rollback_operation_id).await, 0);

    let commit_operation_id = Uuid::now_v7();
    insert_operation(
        &store,
        commit_operation_id,
        Some(player_id),
        persisted_discord_user_id,
        "PENDING",
        &format!("test:wallet-spend:commit:{nonce}"),
    )
    .await;
    let commit_request = request(commit_operation_id, player_id, 200, "COMMIT");
    let mut commit_tx = store.pool().begin().await.unwrap();
    let committed_receipt = apply_wallet_spend(&mut commit_tx, &commit_request)
        .await
        .unwrap();
    assert_eq!(committed_receipt.wallet_before, 500);
    assert_eq!(committed_receipt.wallet_after, 300);
    assert_eq!(
        balance_in_tx(&mut commit_tx, player_id).await,
        (300, 700, 11)
    );
    sqlx::query(
        "UPDATE operations SET state = 'COMMITTED', result = $1, committed_at = now() WHERE id = $2",
    )
    .bind(json!({"test":"owner_committed"}))
    .bind(commit_operation_id)
    .execute(&mut *commit_tx)
    .await
    .unwrap();
    commit_tx.commit().await.unwrap();

    assert_eq!(balance(&store, player_id).await, (300, 700, 11));
    assert_eq!(ledger_count(&store, commit_operation_id).await, 1);

    let mut replay_tx = store.pool().begin().await.unwrap();
    let replay = apply_wallet_spend(&mut replay_tx, &commit_request)
        .await
        .unwrap();
    assert_eq!(replay, committed_receipt);
    replay_tx.commit().await.unwrap();
    assert_eq!(balance(&store, player_id).await, (300, 700, 11));

    let conflicting_amount = WalletSpendRequest {
        amount: 201,
        ..commit_request.clone()
    };
    let mut conflict_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        apply_wallet_spend(&mut conflict_tx, &conflicting_amount).await,
        Err(WalletSpendError::MutationConflict)
    ));
    conflict_tx.rollback().await.unwrap();
    assert_eq!(balance(&store, player_id).await, (300, 700, 11));

    let conflicting_provenance = WalletSpendRequest {
        provenance: json!({"origin":"different"}),
        ..commit_request.clone()
    };
    let mut provenance_conflict_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        apply_wallet_spend(&mut provenance_conflict_tx, &conflicting_provenance).await,
        Err(WalletSpendError::MutationConflict)
    ));
    provenance_conflict_tx.rollback().await.unwrap();

    let insufficient_operation_id = Uuid::now_v7();
    insert_operation(
        &store,
        insufficient_operation_id,
        Some(player_id),
        persisted_discord_user_id,
        "PENDING",
        &format!("test:wallet-spend:insufficient:{nonce}"),
    )
    .await;
    let insufficient_request = request(insufficient_operation_id, player_id, 301, "INSUFFICIENT");
    let mut insufficient_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        apply_wallet_spend(&mut insufficient_tx, &insufficient_request).await,
        Err(WalletSpendError::InsufficientWallet {
            available: 300,
            requested: 301,
        })
    ));
    insufficient_tx.rollback().await.unwrap();
    assert_eq!(balance(&store, player_id).await, (300, 700, 11));
    assert_eq!(ledger_count(&store, insufficient_operation_id).await, 0);

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
        persisted_discord_user_id,
        "PENDING",
        &format!("test:wallet-spend:frozen:{nonce}"),
    )
    .await;
    let frozen_request = request(frozen_operation_id, player_id, 1, "FROZEN");
    let mut frozen_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        apply_wallet_spend(&mut frozen_tx, &frozen_request).await,
        Err(WalletSpendError::AccountFrozen(ref status)) if status == "SOFT_FROZEN"
    ));
    frozen_tx.rollback().await.unwrap();
    sqlx::query("UPDATE players SET status = 'ACTIVE' WHERE id = $1")
        .bind(player_id)
        .execute(store.pool())
        .await
        .unwrap();

    let mismatch_operation_id = Uuid::now_v7();
    insert_operation(
        &store,
        mismatch_operation_id,
        Some(player_id),
        persisted_discord_user_id,
        "PENDING",
        &format!("test:wallet-spend:mismatch:{nonce}"),
    )
    .await;
    let mismatch_request = request(mismatch_operation_id, Uuid::now_v7(), 1, "MISMATCH");
    let mut mismatch_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        apply_wallet_spend(&mut mismatch_tx, &mismatch_request).await,
        Err(WalletSpendError::OperationPlayerMismatch)
    ));
    mismatch_tx.rollback().await.unwrap();

    let terminal_operation_id = Uuid::now_v7();
    insert_operation(
        &store,
        terminal_operation_id,
        Some(player_id),
        persisted_discord_user_id,
        "COMMITTED",
        &format!("test:wallet-spend:terminal:{nonce}"),
    )
    .await;
    let terminal_request = request(terminal_operation_id, player_id, 1, "TERMINAL");
    let mut terminal_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        apply_wallet_spend(&mut terminal_tx, &terminal_request).await,
        Err(WalletSpendError::OperationTerminal(ref state)) if state == "COMMITTED"
    ));
    terminal_tx.rollback().await.unwrap();

    let malformed_operation_id = Uuid::now_v7();
    insert_operation(
        &store,
        malformed_operation_id,
        Some(player_id),
        persisted_discord_user_id,
        "PENDING",
        &format!("test:wallet-spend:malformed:{nonce}"),
    )
    .await;
    let malformed_ledger_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO ledger_transactions (id, operation_id, kind, provenance) VALUES ($1, $2, 'WALLET_SPEND', '{}'::jsonb)",
    )
    .bind(malformed_ledger_id)
    .bind(malformed_operation_id)
    .execute(store.pool())
    .await
    .unwrap();
    let malformed_request = request(malformed_operation_id, player_id, 1, "MALFORMED");
    let mut malformed_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        apply_wallet_spend(&mut malformed_tx, &malformed_request).await,
        Err(WalletSpendError::InvalidStoredSpend(_))
    ));
    malformed_tx.rollback().await.unwrap();
}

fn request(operation_id: Uuid, player_id: Uuid, amount: i64, label: &str) -> WalletSpendRequest {
    WalletSpendRequest {
        operation_id,
        player_id,
        amount,
        source: "TEST_SERVICE_COST".to_owned(),
        provenance: json!({
            "origin": "integration_test",
            "label": label,
        }),
    }
}

async fn insert_player(
    store: &PgStore,
    player_id: Uuid,
    discord_user_id: i64,
    wallet: i64,
    bank: i64,
    liability: i64,
) {
    sqlx::query("INSERT INTO players (id, discord_user_id) VALUES ($1, $2)")
        .bind(player_id)
        .bind(discord_user_id)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO player_balances (player_id, wallet, bank, liability) VALUES ($1, $2, $3, $4)",
    )
    .bind(player_id)
    .bind(wallet)
    .bind(bank)
    .bind(liability)
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
            $1, $2, $3, $4, 'TEST_WALLET_SPEND', $5,
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
    .bind(vec![0xA5_u8; 32])
    .bind(vec![0x5A_u8; 32])
    .execute(store.pool())
    .await
    .unwrap();
}

async fn balance(store: &PgStore, player_id: Uuid) -> (i64, i64, i64) {
    let row =
        sqlx::query("SELECT wallet, bank, liability FROM player_balances WHERE player_id = $1")
            .bind(player_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    (
        row.try_get("wallet").unwrap(),
        row.try_get("bank").unwrap(),
        row.try_get("liability").unwrap(),
    )
}

async fn balance_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    player_id: Uuid,
) -> (i64, i64, i64) {
    let row =
        sqlx::query("SELECT wallet, bank, liability FROM player_balances WHERE player_id = $1")
            .bind(player_id)
            .fetch_one(&mut **tx)
            .await
            .unwrap();
    (
        row.try_get("wallet").unwrap(),
        row.try_get("bank").unwrap(),
        row.try_get("liability").unwrap(),
    )
}

async fn operation_state_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    operation_id: Uuid,
) -> String {
    sqlx::query("SELECT state FROM operations WHERE id = $1")
        .bind(operation_id)
        .fetch_one(&mut **tx)
        .await
        .unwrap()
        .try_get("state")
        .unwrap()
}

async fn ledger_count(store: &PgStore, operation_id: Uuid) -> i64 {
    sqlx::query("SELECT COUNT(*) AS count FROM ledger_transactions WHERE operation_id = $1")
        .bind(operation_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
        .try_get("count")
        .unwrap()
}

async fn assert_ledger_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    receipt: &graphite_economy::WalletSpendReceipt,
) {
    let ledger = sqlx::query(
        "SELECT kind, provenance FROM ledger_transactions WHERE id = $1 AND operation_id = $2",
    )
    .bind(receipt.ledger_transaction_id)
    .bind(receipt.operation_id)
    .fetch_one(&mut **tx)
    .await
    .unwrap();
    let kind: String = ledger.try_get("kind").unwrap();
    assert_eq!(kind, "WALLET_SPEND");
    let provenance: serde_json::Value = ledger.try_get("provenance").unwrap();
    assert_eq!(provenance["wallet_spend_policy_version"], 1);
    assert_eq!(provenance["source"], "TEST_SERVICE_COST");

    let postings = sqlx::query(
        r#"
        SELECT sequence, player_id, account_kind, amount
          FROM ledger_postings
         WHERE transaction_id = $1
         ORDER BY sequence ASC
        "#,
    )
    .bind(receipt.ledger_transaction_id)
    .fetch_all(&mut **tx)
    .await
    .unwrap();
    assert_eq!(postings.len(), 2);
    assert_eq!(postings[0].try_get::<i16, _>("sequence").unwrap(), 0);
    assert_eq!(
        postings[0].try_get::<Option<Uuid>, _>("player_id").unwrap(),
        Some(receipt.player_id)
    );
    assert_eq!(
        postings[0].try_get::<String, _>("account_kind").unwrap(),
        "WALLET"
    );
    assert_eq!(
        postings[0].try_get::<i64, _>("amount").unwrap(),
        -receipt.amount
    );
    assert_eq!(postings[1].try_get::<i16, _>("sequence").unwrap(), 1);
    assert_eq!(
        postings[1].try_get::<Option<Uuid>, _>("player_id").unwrap(),
        None
    );
    assert_eq!(
        postings[1].try_get::<String, _>("account_kind").unwrap(),
        "SYSTEM"
    );
    assert_eq!(
        postings[1].try_get::<i64, _>("amount").unwrap(),
        receipt.amount
    );
}
