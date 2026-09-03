use chrono::{DateTime, Utc};
use graphite_economy::WalletSpendError;
use graphite_services::{
    OrdinarySoulBindUnbindPreflight, OrdinarySoulBindUnbindPreflightError, PersistedSoulBindState,
};
use graphite_store::PgStore;
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

const FUNDED_WALLET: i64 = 10_000_000;
const FUNDED_BANK: i64 = 20_000_000;
const POOR_BANK: i64 = 100_000_000;

#[tokio::test]
async fn soulbind_unbind_settlement_is_atomic_exact_and_wallet_only() {
    let Some(store) = test_store().await else {
        return;
    };

    let nonce = Uuid::now_v7();
    let funded_discord_user_id = positive_snowflake(nonce);
    let poor_discord_user_id = funded_discord_user_id
        .checked_add(1)
        .expect("test snowflake range leaves room for one distinct adjacent identity");
    let funded_player_id =
        seed_player(&store, funded_discord_user_id, FUNDED_WALLET, FUNDED_BANK).await;
    let poor_player_id = seed_player(&store, poor_discord_user_id, 0, POOR_BANK).await;
    let definition_key = format!("test.soulbind-unbind-settlement.{nonce}");
    seed_definition(&store, &definition_key).await;

    let rollback_item_id = seed_bound_item(
        &store,
        funded_player_id,
        funded_discord_user_id,
        &definition_key,
        &nonce,
        "rollback",
    )
    .await;
    let rollback_operation_id = seed_settlement_operation(
        &store,
        funded_player_id,
        funded_discord_user_id,
        &nonce,
        "rollback",
    )
    .await;

    let mut rollback_tx = store.pool().begin().await.unwrap();
    let (rollback_preflight, rollback_spend, rollback_transition) =
        OrdinarySoulBindUnbindPreflight::settle_for_owned_ordinary_equipment(
            &mut rollback_tx,
            rollback_operation_id,
            funded_player_id,
            rollback_item_id,
        )
        .await
        .unwrap();

    assert_eq!(rollback_spend.amount, rollback_preflight.preview.money_fee);
    assert_eq!(
        rollback_spend.wallet_before, FUNDED_WALLET,
        "settlement must charge the Wallet snapshot locked before item appraisal"
    );
    assert_eq!(
        rollback_spend.wallet_after,
        FUNDED_WALLET - rollback_preflight.preview.money_fee
    );
    assert!(!rollback_preflight.preview.refunds_binding_resources);
    assert!(rollback_preflight.preview.requires_unprotected);
    assert!(rollback_preflight.preview.requires_unfavorited);
    assert_eq!(
        balance_in_tx(&mut rollback_tx, funded_player_id).await,
        (
            FUNDED_WALLET - rollback_preflight.preview.money_fee,
            FUNDED_BANK,
        )
    );
    assert_unbound_transition(
        &rollback_transition,
        rollback_preflight.preview.rebind_cooldown_seconds,
    );
    assert_eq!(
        soulbind_row_in_tx(&mut rollback_tx, rollback_item_id).await,
        Some((false, transition_cooldown(&rollback_transition)))
    );
    assert_wallet_ledger_in_tx(
        &mut rollback_tx,
        rollback_operation_id,
        funded_player_id,
        rollback_preflight.preview.money_fee,
        rollback_item_id,
        rollback_preflight.preview.current_enhanced_appraisal,
    )
    .await;
    assert_eq!(
        operation_state_in_tx(&mut rollback_tx, rollback_operation_id).await,
        "PENDING"
    );

    rollback_tx.rollback().await.unwrap();
    assert_eq!(
        balance(&store, funded_player_id).await,
        (FUNDED_WALLET, FUNDED_BANK)
    );
    assert_eq!(
        soulbind_row(&store, rollback_item_id).await,
        Some((true, None))
    );
    assert_eq!(ledger_count(&store, rollback_operation_id).await, 0);

    let commit_item_id = seed_bound_item(
        &store,
        funded_player_id,
        funded_discord_user_id,
        &definition_key,
        &nonce,
        "commit",
    )
    .await;
    let commit_operation_id = seed_settlement_operation(
        &store,
        funded_player_id,
        funded_discord_user_id,
        &nonce,
        "commit",
    )
    .await;

    let mut commit_tx = store.pool().begin().await.unwrap();
    let (commit_preflight, commit_spend, commit_transition) =
        OrdinarySoulBindUnbindPreflight::settle_for_owned_ordinary_equipment(
            &mut commit_tx,
            commit_operation_id,
            funded_player_id,
            commit_item_id,
        )
        .await
        .unwrap();
    assert_eq!(commit_spend.amount, commit_preflight.preview.money_fee);
    assert_unbound_transition(
        &commit_transition,
        commit_preflight.preview.rebind_cooldown_seconds,
    );

    sqlx::query(
        "UPDATE operations SET state = 'COMMITTED', result = $1, committed_at = clock_timestamp() WHERE id = $2",
    )
    .bind(json!({
        "test": "soulbind_unbind_owner_committed",
        "item_instance_id": commit_item_id,
        "money_fee": commit_preflight.preview.money_fee,
    }))
    .bind(commit_operation_id)
    .execute(&mut *commit_tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO outbox_events (id, operation_id, topic, payload) VALUES ($1, $2, 'test.soulbind_unbound', $3)",
    )
    .bind(Uuid::now_v7())
    .bind(commit_operation_id)
    .bind(json!({"item_instance_id": commit_item_id}))
    .execute(&mut *commit_tx)
    .await
    .unwrap();
    commit_tx.commit().await.unwrap();

    assert_eq!(
        balance(&store, funded_player_id).await,
        (
            FUNDED_WALLET - commit_preflight.preview.money_fee,
            FUNDED_BANK,
        )
    );
    assert_eq!(
        soulbind_row(&store, commit_item_id).await,
        Some((false, transition_cooldown(&commit_transition)))
    );
    assert_eq!(ledger_count(&store, commit_operation_id).await, 1);
    assert_eq!(
        operation_state(&store, commit_operation_id).await,
        "COMMITTED"
    );

    let poor_item_id = seed_bound_item(
        &store,
        poor_player_id,
        poor_discord_user_id,
        &definition_key,
        &nonce,
        "insufficient",
    )
    .await;
    let poor_operation_id = seed_settlement_operation(
        &store,
        poor_player_id,
        poor_discord_user_id,
        &nonce,
        "insufficient",
    )
    .await;

    let mut poor_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        OrdinarySoulBindUnbindPreflight::settle_for_owned_ordinary_equipment(
            &mut poor_tx,
            poor_operation_id,
            poor_player_id,
            poor_item_id,
        )
        .await,
        Err(OrdinarySoulBindUnbindPreflightError::Wallet(
            WalletSpendError::InsufficientWallet {
                available: 0,
                requested,
            }
        )) if requested > 0
    ));
    assert_eq!(
        balance_in_tx(&mut poor_tx, poor_player_id).await,
        (0, POOR_BANK)
    );
    assert_eq!(
        soulbind_row_in_tx(&mut poor_tx, poor_item_id).await,
        Some((true, None))
    );
    assert_eq!(ledger_count_in_tx(&mut poor_tx, poor_operation_id).await, 0);
    poor_tx.rollback().await.unwrap();

    assert_eq!(balance(&store, poor_player_id).await, (0, POOR_BANK));
    assert_eq!(soulbind_row(&store, poor_item_id).await, Some((true, None)));
    assert_eq!(ledger_count(&store, poor_operation_id).await, 0);
}

fn assert_unbound_transition(
    transition: &graphite_services::AppliedSoulBindStateTransition,
    expected_cooldown_seconds: i64,
) {
    assert_eq!(transition.previous_state, PersistedSoulBindState::Bound);
    let PersistedSoulBindState::Unbound { rebind_not_before } = &transition.new_state else {
        panic!("expected SoulBind unbind transition");
    };
    assert_eq!(
        rebind_not_before
            .signed_duration_since(transition.evaluated_at)
            .num_seconds(),
        expected_cooldown_seconds
    );
}

fn transition_cooldown(
    transition: &graphite_services::AppliedSoulBindStateTransition,
) -> Option<DateTime<Utc>> {
    match &transition.new_state {
        PersistedSoulBindState::Unbound { rebind_not_before } => Some(*rebind_not_before),
        _ => None,
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

async fn seed_player(store: &PgStore, discord_user_id: i64, wallet: i64, bank: i64) -> Uuid {
    let player_id = Uuid::now_v7();
    sqlx::query("INSERT INTO players (id, discord_user_id) VALUES ($1, $2)")
        .bind(player_id)
        .bind(discord_user_id)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO player_balances (player_id, wallet, bank) VALUES ($1, $2, $3)")
        .bind(player_id)
        .bind(wallet)
        .bind(bank)
        .execute(store.pool())
        .await
        .unwrap();
    player_id
}

async fn seed_definition(store: &PgStore, key: &str) {
    let data = r#"{"tier":"NETHERITE"}"#;
    sqlx::query("INSERT INTO item_definitions (key, category, stackable, active, definition_version, rarity, stack_limit, data) VALUES ($1, 'PICKAXE', FALSE, TRUE, 1, 'COMMON', NULL, $2::jsonb)")
        .bind(key)
        .bind(data)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO item_definition_versions (key, version, category, stackable, rarity, stack_limit, is_ordinary_equipment, data) VALUES ($1, 1, 'PICKAXE', FALSE, 'COMMON', NULL, TRUE, $2::jsonb)")
        .bind(key)
        .bind(data)
        .execute(store.pool())
        .await
        .unwrap();
}

async fn seed_bound_item(
    store: &PgStore,
    player_id: Uuid,
    discord_user_id: i64,
    definition_key: &str,
    nonce: &Uuid,
    suffix: &str,
) -> Uuid {
    let creation_operation_id = Uuid::now_v7();
    insert_operation(
        store,
        creation_operation_id,
        player_id,
        discord_user_id,
        "ITEM_CREATION_TEST",
        "PENDING",
        &format!("test:soulbind-unbind-settlement:create:{nonce}:{suffix}:{creation_operation_id}"),
    )
    .await;

    let item_id = Uuid::now_v7();
    sqlx::query("INSERT INTO item_instances (id, definition_key, owner_player_id, created_by_operation_id, location, definition_version, is_favorite, is_protected) VALUES ($1, $2, $3, $4, 'TOOL_LOCKER', 1, FALSE, FALSE)")
        .bind(item_id)
        .bind(definition_key)
        .bind(player_id)
        .bind(creation_operation_id)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO item_instance_equipment_structural_state (item_instance_id, creation_roll_numerator, creation_roll_denominator, upgrade_level, normal_enchant_slot_capacity, special_enchant_slot_capacity) VALUES ($1, 1, 1, 0, 4, 3)")
        .bind(item_id)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO item_instance_soulbind_state (item_instance_id, is_soulbound, rebind_not_before) VALUES ($1, TRUE, NULL)")
        .bind(item_id)
        .execute(store.pool())
        .await
        .unwrap();
    item_id
}

async fn seed_settlement_operation(
    store: &PgStore,
    player_id: Uuid,
    discord_user_id: i64,
    nonce: &Uuid,
    suffix: &str,
) -> Uuid {
    let operation_id = Uuid::now_v7();
    insert_operation(
        store,
        operation_id,
        player_id,
        discord_user_id,
        "SOULBIND_UNBIND_TEST",
        "PENDING",
        &format!("test:soulbind-unbind-settlement:{nonce}:{suffix}:{operation_id}"),
    )
    .await;
    operation_id
}

async fn insert_operation(
    store: &PgStore,
    operation_id: Uuid,
    player_id: Uuid,
    actor_discord_user_id: i64,
    kind: &str,
    state: &str,
    external_request_key: &str,
) {
    sqlx::query(
        "INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, $5, $6, 1, $7, $8)",
    )
    .bind(operation_id)
    .bind(external_request_key)
    .bind(actor_discord_user_id)
    .bind(player_id)
    .bind(kind)
    .bind(state)
    .bind(vec![0xD5_u8; 32])
    .bind(vec![0x5D_u8; 32])
    .execute(store.pool())
    .await
    .unwrap();
}

async fn balance(store: &PgStore, player_id: Uuid) -> (i64, i64) {
    let row = sqlx::query("SELECT wallet, bank FROM player_balances WHERE player_id = $1")
        .bind(player_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
    (row.try_get("wallet").unwrap(), row.try_get("bank").unwrap())
}

async fn balance_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    player_id: Uuid,
) -> (i64, i64) {
    let row = sqlx::query("SELECT wallet, bank FROM player_balances WHERE player_id = $1")
        .bind(player_id)
        .fetch_one(&mut **tx)
        .await
        .unwrap();
    (row.try_get("wallet").unwrap(), row.try_get("bank").unwrap())
}

async fn soulbind_row(store: &PgStore, item_id: Uuid) -> Option<(bool, Option<DateTime<Utc>>)> {
    sqlx::query_as("SELECT is_soulbound, rebind_not_before FROM item_instance_soulbind_state WHERE item_instance_id = $1")
        .bind(item_id)
        .fetch_optional(store.pool())
        .await
        .unwrap()
}

async fn soulbind_row_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    item_id: Uuid,
) -> Option<(bool, Option<DateTime<Utc>>)> {
    sqlx::query_as("SELECT is_soulbound, rebind_not_before FROM item_instance_soulbind_state WHERE item_instance_id = $1")
        .bind(item_id)
        .fetch_optional(&mut **tx)
        .await
        .unwrap()
}

async fn operation_state(store: &PgStore, operation_id: Uuid) -> String {
    sqlx::query("SELECT state FROM operations WHERE id = $1")
        .bind(operation_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
        .try_get("state")
        .unwrap()
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

async fn ledger_count_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    operation_id: Uuid,
) -> i64 {
    sqlx::query("SELECT COUNT(*) AS count FROM ledger_transactions WHERE operation_id = $1")
        .bind(operation_id)
        .fetch_one(&mut **tx)
        .await
        .unwrap()
        .try_get("count")
        .unwrap()
}

async fn assert_wallet_ledger_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
    amount: i64,
    item_id: Uuid,
    current_enhanced_appraisal: i64,
) {
    let ledger =
        sqlx::query("SELECT id, kind, provenance FROM ledger_transactions WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(&mut **tx)
            .await
            .unwrap();
    let transaction_id: Uuid = ledger.try_get("id").unwrap();
    assert_eq!(ledger.try_get::<String, _>("kind").unwrap(), "WALLET_SPEND");
    let provenance: serde_json::Value = ledger.try_get("provenance").unwrap();
    assert_eq!(provenance["source"], "SOULBIND_UNBIND");
    assert_eq!(
        provenance["request_provenance"]["item_instance_id"],
        item_id.to_string()
    );
    assert_eq!(
        provenance["request_provenance"]["current_enhanced_appraisal"],
        current_enhanced_appraisal
    );
    assert_eq!(provenance["request_provenance"]["money_fee"], amount);
    assert_eq!(
        provenance["request_provenance"]["refunds_binding_resources"],
        false
    );
    assert_eq!(
        provenance["request_provenance"]["requires_unprotected"],
        true
    );
    assert_eq!(
        provenance["request_provenance"]["requires_unfavorited"],
        true
    );

    let postings = sqlx::query(
        "SELECT sequence, player_id, account_kind, amount FROM ledger_postings WHERE transaction_id = $1 ORDER BY sequence ASC",
    )
    .bind(transaction_id)
    .fetch_all(&mut **tx)
    .await
    .unwrap();
    assert_eq!(postings.len(), 2);
    assert_eq!(postings[0].try_get::<i16, _>("sequence").unwrap(), 0);
    assert_eq!(
        postings[0].try_get::<Option<Uuid>, _>("player_id").unwrap(),
        Some(player_id)
    );
    assert_eq!(
        postings[0].try_get::<String, _>("account_kind").unwrap(),
        "WALLET"
    );
    assert_eq!(postings[0].try_get::<i64, _>("amount").unwrap(), -amount);
    assert_eq!(postings[1].try_get::<i16, _>("sequence").unwrap(), 1);
    assert_eq!(
        postings[1].try_get::<Option<Uuid>, _>("player_id").unwrap(),
        None
    );
    assert_eq!(
        postings[1].try_get::<String, _>("account_kind").unwrap(),
        "SYSTEM"
    );
    assert_eq!(postings[1].try_get::<i64, _>("amount").unwrap(), amount);
}

fn positive_snowflake(nonce: Uuid) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    i64::try_from((raw % 8_000_000_000_000_000_000_u64).max(1)).unwrap()
}
