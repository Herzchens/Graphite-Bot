use graphite_items::{
    ItemService, UniqueItemBagDeliveryMutationError, UniqueItemBagDeliveryMutationRequest,
    UniqueItemInstanceFlags, apply_unique_item_bag_delivery_mutation,
    claim_pending_unique_item_bag_deliveries,
};
use graphite_store::PgStore;
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn unique_item_bag_delivery_counts_unique_slots_preserves_overflow_and_replays_exactly() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let discord_user_id = positive_snowflake(nonce);
    let player_id = seed_player(&store, discord_user_id).await;
    let filler_key = format!("test.unique-item-bag.filler.{nonce}");
    seed_stack_definition(&store, &filler_key, 1).await;
    let unique_key = format!("test.unique-item-bag.asset.{nonce}");
    seed_unique_definition(&store, &unique_key).await;

    sqlx::query(
        "INSERT INTO item_stacks (player_id, definition_key, definition_version, location, quantity) VALUES ($1, $2, 1, 'ITEM_BAG', 35)",
    )
    .bind(player_id)
    .bind(&filler_key)
    .execute(store.pool())
    .await
    .unwrap();

    let operation_id = seed_operation(&store, player_id, nonce, "deliver").await;
    let first_request = request(operation_id, player_id, "unique:first", &unique_key, 1);
    let second_request = request(operation_id, player_id, "unique:second", &unique_key, 2);

    let mut tx = store.pool().begin().await.unwrap();
    let first = apply_unique_item_bag_delivery_mutation(&mut tx, &first_request)
        .await
        .unwrap();
    assert!(!first.pending);
    assert_eq!(first.pending_delivery_id, None);
    assert_eq!(
        apply_unique_item_bag_delivery_mutation(&mut tx, &first_request)
            .await
            .unwrap(),
        first
    );

    let second = apply_unique_item_bag_delivery_mutation(&mut tx, &second_request)
        .await
        .unwrap();
    assert!(second.pending);
    let second_pending = second.pending_delivery_id.expect("pending id");
    assert_ne!(first.item_instance_id, second.item_instance_id);
    assert_eq!(
        apply_unique_item_bag_delivery_mutation(&mut tx, &second_request)
            .await
            .unwrap(),
        second
    );
    commit_operation(&mut tx, operation_id).await;
    tx.commit().await.unwrap();

    let bag = ItemService::new(store.clone())
        .item_bag(u64::try_from(discord_user_id).unwrap())
        .await
        .unwrap();
    assert_eq!(bag.capacity_slots, 36);
    assert_eq!(bag.used_slots, 36);
    assert_eq!(bag.pending_deliveries, 1);
    assert_eq!(bag.unique_items.len(), 1);
    assert_eq!(bag.unique_items[0].item_instance_id, first.item_instance_id);

    let mut committed_replay = store.pool().begin().await.unwrap();
    assert_eq!(
        apply_unique_item_bag_delivery_mutation(&mut committed_replay, &second_request)
            .await
            .unwrap(),
        second
    );
    committed_replay.rollback().await.unwrap();

    let blocked_operation = seed_operation(&store, player_id, nonce, "blocked").await;
    let blocked_request = request(
        blocked_operation,
        player_id,
        "unique:blocked",
        &unique_key,
        3,
    );
    let mut blocked = store.pool().begin().await.unwrap();
    assert!(matches!(
        apply_unique_item_bag_delivery_mutation(&mut blocked, &blocked_request).await,
        Err(UniqueItemBagDeliveryMutationError::EarlierPendingUniqueDelivery)
    ));
    blocked.rollback().await.unwrap();

    sqlx::query(
        "UPDATE item_stacks SET quantity = 34 WHERE player_id = $1 AND definition_key = $2 AND definition_version = 1 AND location = 'ITEM_BAG'",
    )
    .bind(player_id)
    .bind(&filler_key)
    .execute(store.pool())
    .await
    .unwrap();

    let claim_operation = seed_operation(&store, player_id, nonce, "claim").await;
    let mut claim_tx = store.pool().begin().await.unwrap();
    let claim =
        claim_pending_unique_item_bag_deliveries(&mut claim_tx, claim_operation, player_id)
            .await
            .unwrap();
    assert_eq!(claim.claimed.len(), 1);
    assert_eq!(claim.claimed[0].pending_delivery_id, second_pending);
    assert_eq!(claim.claimed[0].item_instance_id, second.item_instance_id);
    assert_eq!(claim.blocked_pending_delivery_id, None);
    assert_eq!(claim.used_slots_after, 36);
    assert_eq!(
        claim_pending_unique_item_bag_deliveries(&mut claim_tx, claim_operation, player_id)
            .await
            .unwrap(),
        claim
    );
    commit_operation(&mut claim_tx, claim_operation).await;
    claim_tx.commit().await.unwrap();

    let pending_state: String =
        sqlx::query_scalar("SELECT state FROM pending_asset_deliveries WHERE id = $1")
            .bind(second_pending)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(pending_state, "CLAIMED");
    let second_location: String =
        sqlx::query_scalar("SELECT location FROM item_instances WHERE id = $1")
            .bind(second.item_instance_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(second_location, "ITEM_BAG");

    let bag = ItemService::new(store.clone())
        .item_bag(u64::try_from(discord_user_id).unwrap())
        .await
        .unwrap();
    assert_eq!(bag.used_slots, 36);
    assert_eq!(bag.pending_deliveries, 0);
    assert_eq!(bag.unique_items.len(), 2);

    let mut committed_claim_replay = store.pool().begin().await.unwrap();
    assert_eq!(
        claim_pending_unique_item_bag_deliveries(
            &mut committed_claim_replay,
            claim_operation,
            player_id,
        )
        .await
        .unwrap(),
        claim
    );
    committed_claim_replay.rollback().await.unwrap();
}

#[tokio::test]
async fn unique_item_bag_delivery_rolls_back_item_pending_and_audit_together() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let discord_user_id = positive_snowflake(nonce);
    let player_id = seed_player(&store, discord_user_id).await;
    let unique_key = format!("test.unique-item-bag.rollback.{nonce}");
    seed_unique_definition(&store, &unique_key).await;
    let operation_id = seed_operation(&store, player_id, nonce, "rollback").await;
    let request = request(operation_id, player_id, "unique:rollback", &unique_key, 1);

    let mut tx = store.pool().begin().await.unwrap();
    let receipt = apply_unique_item_bag_delivery_mutation(&mut tx, &request)
        .await
        .unwrap();
    tx.rollback().await.unwrap();

    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM item_instances WHERE id = $1"
        )
        .bind(receipt.item_instance_id)
        .fetch_one(store.pool())
        .await
        .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM asset_events WHERE operation_id = $1"
        )
        .bind(operation_id)
        .fetch_one(store.pool())
        .await
        .unwrap(),
        0
    );
}

fn request(
    operation_id: Uuid,
    player_id: Uuid,
    mutation_key: &str,
    definition_key: &str,
    marker: i64,
) -> UniqueItemBagDeliveryMutationRequest {
    UniqueItemBagDeliveryMutationRequest {
        operation_id,
        player_id,
        mutation_key: mutation_key.to_owned(),
        definition_key: definition_key.to_owned(),
        definition_version: 1,
        flags: UniqueItemInstanceFlags {
            is_account_bound: false,
            is_tradeable: false,
            is_sellable: false,
            is_discardable: false,
            is_enchantable: false,
            is_upgradeable: false,
            is_unbreakable: false,
            is_repairable: false,
        },
        state: json!({"test_marker": marker}),
        source: "UNIQUE_ITEM_BAG_TEST".to_owned(),
        provenance: json!({"test": true, "marker": marker}),
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
    player_id
}

async fn seed_stack_definition(store: &PgStore, key: &str, stack_limit: i64) {
    sqlx::query(
        "INSERT INTO item_definitions (key, category, stackable, active, definition_version, rarity, stack_limit, data) VALUES ($1, 'MATERIAL', TRUE, TRUE, 1, 'COMMON', $2, '{}'::jsonb)",
    )
    .bind(key)
    .bind(stack_limit)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO item_definition_versions (key, version, category, stackable, rarity, stack_limit, data) VALUES ($1, 1, 'MATERIAL', TRUE, 'COMMON', $2, '{}'::jsonb)",
    )
    .bind(key)
    .bind(stack_limit)
    .execute(store.pool())
    .await
    .unwrap();
}

async fn seed_unique_definition(store: &PgStore, key: &str) {
    sqlx::query(
        "INSERT INTO item_definitions (key, category, stackable, active, definition_version, rarity, data) VALUES ($1, 'TEST_UNIQUE', FALSE, TRUE, 1, 'COMMON', '{}'::jsonb)",
    )
    .bind(key)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO item_definition_versions (key, version, category, stackable, rarity, data) VALUES ($1, 1, 'TEST_UNIQUE', FALSE, 'COMMON', '{}'::jsonb)",
    )
    .bind(key)
    .execute(store.pool())
    .await
    .unwrap();
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
        "INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'UNIQUE_ITEM_BAG_TEST', 'PENDING', 1, $5, $6)",
    )
    .bind(operation_id)
    .bind(format!("test:unique-item-bag:{nonce}:{suffix}:{operation_id}"))
    .bind(discord_user_id)
    .bind(player_id)
    .bind([41_u8; 32].as_slice())
    .bind([43_u8; 32].as_slice())
    .execute(store.pool())
    .await
    .unwrap();
    operation_id
}

async fn commit_operation(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    operation_id: Uuid,
) {
    let result = sqlx::query(
        "UPDATE operations SET state = 'COMMITTED', result = '{}'::jsonb, committed_at = now() WHERE id = $1 AND state = 'PENDING'",
    )
    .bind(operation_id)
    .execute(&mut **tx)
    .await
    .unwrap();
    assert_eq!(result.rows_affected(), 1);
}

fn positive_snowflake(nonce: Uuid) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[8..].try_into().unwrap());
    let value = (raw % 7_999_999_999_999_999_000_u64).saturating_add(1);
    i64::try_from(value).unwrap()
}
