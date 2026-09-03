use chrono::{DateTime, Utc};
use graphite_economy::WalletSpendError;
use graphite_services::{
    OrdinarySoulBindUnbindPreflightError, SoulBindUnbindLifecycleError, SoulBindUnbindService,
};
use graphite_store::PgStore;
use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;

const FUNDED_WALLET: i64 = 10_000_000;
const FUNDED_BANK: i64 = 20_000_000;
const POOR_BANK: i64 = 100_000_000;

#[tokio::test]
async fn unbind_lifecycle_commits_replays_and_emits_one_auditable_result() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let discord_user_id = positive_snowflake(nonce);
    let player_id = seed_player(&store, discord_user_id, FUNDED_WALLET, FUNDED_BANK).await;
    let definition_key = format!("test.soulbind-unbind-lifecycle.{nonce}");
    seed_definition(&store, &definition_key).await;
    let item_id = seed_bound_item(
        &store,
        player_id,
        discord_user_id,
        &definition_key,
        &nonce,
        "success",
    )
    .await;
    let external_request_key = format!("test:soulbind-unbind-lifecycle:{nonce}:success");
    let service = SoulBindUnbindService::new(store.pool().clone());

    let receipt = service
        .unbind(
            u64::try_from(discord_user_id).unwrap(),
            item_id,
            &external_request_key,
        )
        .await
        .unwrap();

    assert_eq!(receipt.player_id, player_id);
    assert_eq!(receipt.item_instance_id, item_id);
    assert!(receipt.current_enhanced_appraisal > 0);
    assert!(receipt.money_fee > 0);
    assert_eq!(receipt.wallet_before, FUNDED_WALLET);
    assert_eq!(receipt.wallet_after, FUNDED_WALLET - receipt.money_fee);
    assert!(!receipt.refunds_binding_resources);
    assert_eq!(
        receipt
            .rebind_not_before
            .signed_duration_since(receipt.evaluated_at)
            .num_seconds(),
        7 * 24 * 60 * 60
    );
    assert_eq!(
        balance(&store, player_id).await,
        (receipt.wallet_after, FUNDED_BANK)
    );
    assert_eq!(
        soulbind_row(&store, item_id).await,
        Some((false, Some(receipt.rebind_not_before)))
    );

    let operation = operation_by_key(&store, &external_request_key)
        .await
        .unwrap();
    assert_eq!(operation.id, receipt.operation_id);
    assert_eq!(operation.player_id, Some(player_id));
    assert_eq!(operation.kind, "SOULBIND_UNBIND");
    assert_eq!(operation.state, "COMMITTED");
    assert_eq!(operation.policy_version, 1);
    assert_eq!(operation.request_hash.len(), 32);
    assert_eq!(operation.result, serde_json::to_value(&receipt).unwrap());

    assert_wallet_ledger(&store, &receipt).await;
    assert_asset_event(&store, &receipt).await;
    assert_outbox(&store, &receipt).await;

    let replay = service
        .unbind(
            u64::try_from(discord_user_id).unwrap(),
            item_id,
            &external_request_key,
        )
        .await
        .unwrap();
    assert_eq!(replay, receipt);
    assert_eq!(
        balance(&store, player_id).await,
        (receipt.wallet_after, FUNDED_BANK)
    );
    assert_eq!(ledger_count(&store, receipt.operation_id).await, 1);
    assert_eq!(asset_event_count(&store, receipt.operation_id).await, 1);
    assert_eq!(outbox_count(&store, receipt.operation_id).await, 1);
    assert_eq!(
        soulbind_row(&store, item_id).await,
        Some((false, Some(receipt.rebind_not_before)))
    );
}

#[tokio::test]
async fn unbind_lifecycle_rejects_same_key_for_a_different_item_without_second_mutation() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let discord_user_id = positive_snowflake(nonce);
    let player_id = seed_player(&store, discord_user_id, FUNDED_WALLET, FUNDED_BANK).await;
    let definition_key = format!("test.soulbind-unbind-lifecycle-conflict.{nonce}");
    seed_definition(&store, &definition_key).await;
    let first_item = seed_bound_item(
        &store,
        player_id,
        discord_user_id,
        &definition_key,
        &nonce,
        "first",
    )
    .await;
    let second_item = seed_bound_item(
        &store,
        player_id,
        discord_user_id,
        &definition_key,
        &nonce,
        "second",
    )
    .await;
    let external_request_key = format!("test:soulbind-unbind-lifecycle:{nonce}:conflict");
    let service = SoulBindUnbindService::new(store.pool().clone());

    let first = service
        .unbind(
            u64::try_from(discord_user_id).unwrap(),
            first_item,
            &external_request_key,
        )
        .await
        .unwrap();
    let wallet_after_first = first.wallet_after;

    assert!(matches!(
        service
            .unbind(
                u64::try_from(discord_user_id).unwrap(),
                second_item,
                &external_request_key,
            )
            .await,
        Err(SoulBindUnbindLifecycleError::IdempotencyConflict)
    ));

    assert_eq!(
        balance(&store, player_id).await,
        (wallet_after_first, FUNDED_BANK)
    );
    assert_eq!(soulbind_row(&store, second_item).await, Some((true, None)));
    assert_eq!(ledger_count(&store, first.operation_id).await, 1);
    assert_eq!(asset_event_count(&store, first.operation_id).await, 1);
    assert_eq!(outbox_count(&store, first.operation_id).await, 1);
}

#[tokio::test]
async fn unbind_lifecycle_failure_rolls_back_new_operation_and_never_pulls_bank() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let discord_user_id = positive_snowflake(nonce);
    let player_id = seed_player(&store, discord_user_id, 0, POOR_BANK).await;
    let definition_key = format!("test.soulbind-unbind-lifecycle-poor.{nonce}");
    seed_definition(&store, &definition_key).await;
    let item_id = seed_bound_item(
        &store,
        player_id,
        discord_user_id,
        &definition_key,
        &nonce,
        "poor",
    )
    .await;
    let external_request_key = format!("test:soulbind-unbind-lifecycle:{nonce}:poor");
    let service = SoulBindUnbindService::new(store.pool().clone());

    assert!(matches!(
        service
            .unbind(
                u64::try_from(discord_user_id).unwrap(),
                item_id,
                &external_request_key,
            )
            .await,
        Err(SoulBindUnbindLifecycleError::Settlement(
            OrdinarySoulBindUnbindPreflightError::Wallet(
                WalletSpendError::InsufficientWallet {
                    available: 0,
                    requested,
                }
            )
        )) if requested > 0
    ));

    assert_eq!(balance(&store, player_id).await, (0, POOR_BANK));
    assert_eq!(soulbind_row(&store, item_id).await, Some((true, None)));
    assert!(
        operation_by_key(&store, &external_request_key)
            .await
            .is_none()
    );
}

#[derive(Debug)]
struct OperationRow {
    id: Uuid,
    player_id: Option<Uuid>,
    kind: String,
    state: String,
    policy_version: i32,
    request_hash: Vec<u8>,
    result: Value,
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
        &format!("test:soulbind-unbind-lifecycle:create:{nonce}:{suffix}:{creation_operation_id}"),
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

async fn insert_operation(
    store: &PgStore,
    operation_id: Uuid,
    player_id: Uuid,
    actor_discord_user_id: i64,
    kind: &str,
    external_request_key: &str,
) {
    sqlx::query(
        "INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, $5, 'PENDING', 1, $6, $7)",
    )
    .bind(operation_id)
    .bind(external_request_key)
    .bind(actor_discord_user_id)
    .bind(player_id)
    .bind(kind)
    .bind(vec![0xA7_u8; 32])
    .bind(vec![0x7A_u8; 32])
    .execute(store.pool())
    .await
    .unwrap();
}

async fn operation_by_key(store: &PgStore, external_request_key: &str) -> Option<OperationRow> {
    sqlx::query(
        "SELECT id, player_id, kind, state, policy_version, request_hash, result FROM operations WHERE external_request_key = $1",
    )
    .bind(external_request_key)
    .fetch_optional(store.pool())
    .await
    .unwrap()
    .map(|row| OperationRow {
        id: row.try_get("id").unwrap(),
        player_id: row.try_get("player_id").unwrap(),
        kind: row.try_get("kind").unwrap(),
        state: row.try_get("state").unwrap(),
        policy_version: row.try_get("policy_version").unwrap(),
        request_hash: row.try_get("request_hash").unwrap(),
        result: row.try_get("result").unwrap(),
    })
}

async fn balance(store: &PgStore, player_id: Uuid) -> (i64, i64) {
    let row = sqlx::query("SELECT wallet, bank FROM player_balances WHERE player_id = $1")
        .bind(player_id)
        .fetch_one(store.pool())
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

async fn assert_wallet_ledger(store: &PgStore, receipt: &graphite_services::SoulBindUnbindReceipt) {
    let transaction =
        sqlx::query("SELECT id, kind, provenance FROM ledger_transactions WHERE operation_id = $1")
            .bind(receipt.operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let transaction_id: Uuid = transaction.try_get("id").unwrap();
    let kind: String = transaction.try_get("kind").unwrap();
    let provenance: Value = transaction.try_get("provenance").unwrap();
    assert_eq!(kind, "WALLET_SPEND");
    assert_eq!(provenance["source"], "SOULBIND_UNBIND");
    assert_eq!(
        provenance["request_provenance"]["item_instance_id"],
        receipt.item_instance_id.to_string()
    );
    assert_eq!(
        provenance["request_provenance"]["money_fee"],
        receipt.money_fee
    );

    let rows = sqlx::query(
        "SELECT sequence, player_id, account_kind, amount FROM ledger_postings WHERE transaction_id = $1 ORDER BY sequence",
    )
    .bind(transaction_id)
    .fetch_all(store.pool())
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].try_get::<i16, _>("sequence").unwrap(), 0);
    assert_eq!(
        rows[0].try_get::<Option<Uuid>, _>("player_id").unwrap(),
        Some(receipt.player_id)
    );
    assert_eq!(
        rows[0].try_get::<String, _>("account_kind").unwrap(),
        "WALLET"
    );
    assert_eq!(
        rows[0].try_get::<i64, _>("amount").unwrap(),
        -receipt.money_fee
    );
    assert_eq!(rows[1].try_get::<i16, _>("sequence").unwrap(), 1);
    assert_eq!(
        rows[1].try_get::<Option<Uuid>, _>("player_id").unwrap(),
        None
    );
    assert_eq!(
        rows[1].try_get::<String, _>("account_kind").unwrap(),
        "SYSTEM"
    );
    assert_eq!(
        rows[1].try_get::<i64, _>("amount").unwrap(),
        receipt.money_fee
    );
}

async fn assert_asset_event(store: &PgStore, receipt: &graphite_services::SoulBindUnbindReceipt) {
    let row = sqlx::query(
        "SELECT mutation_key, player_id, event_kind, payload FROM asset_events WHERE operation_id = $1",
    )
    .bind(receipt.operation_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(
        row.try_get::<String, _>("mutation_key").unwrap(),
        "soulbind:unbind"
    );
    assert_eq!(
        row.try_get::<Uuid, _>("player_id").unwrap(),
        receipt.player_id
    );
    assert_eq!(
        row.try_get::<String, _>("event_kind").unwrap(),
        "SOULBIND_UNBOUND"
    );
    let payload: Value = row.try_get("payload").unwrap();
    assert_eq!(
        payload["item_instance_id"],
        receipt.item_instance_id.to_string()
    );
    assert_eq!(payload["money_fee"], receipt.money_fee);
    assert_eq!(
        payload["rebind_not_before"],
        serde_json::to_value(receipt.rebind_not_before).unwrap()
    );
}

async fn assert_outbox(store: &PgStore, receipt: &graphite_services::SoulBindUnbindReceipt) {
    let row =
        sqlx::query("SELECT topic, payload, state FROM outbox_events WHERE operation_id = $1")
            .bind(receipt.operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(
        row.try_get::<String, _>("topic").unwrap(),
        "soulbind.unbound"
    );
    assert_eq!(row.try_get::<String, _>("state").unwrap(), "PENDING");
    let payload: Value = row.try_get("payload").unwrap();
    assert_eq!(payload["player_id"], receipt.player_id.to_string());
    assert_eq!(
        payload["item_instance_id"],
        receipt.item_instance_id.to_string()
    );
    assert_eq!(payload["money_fee"], receipt.money_fee);
    assert_eq!(
        payload["rebind_not_before"],
        serde_json::to_value(receipt.rebind_not_before).unwrap()
    );
}

async fn ledger_count(store: &PgStore, operation_id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM ledger_transactions WHERE operation_id = $1")
        .bind(operation_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
}

async fn asset_event_count(store: &PgStore, operation_id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM asset_events WHERE operation_id = $1")
        .bind(operation_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
}

async fn outbox_count(store: &PgStore, operation_id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM outbox_events WHERE operation_id = $1")
        .bind(operation_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
}

fn positive_snowflake(nonce: Uuid) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    let value = (raw % 7_999_999_999_999_999_000_u64).saturating_add(1);
    i64::try_from(value).unwrap()
}
