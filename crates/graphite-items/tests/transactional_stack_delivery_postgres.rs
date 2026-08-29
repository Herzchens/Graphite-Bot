use graphite_items::{
    StackDeliveryMutationError, StackDeliveryMutationRequest, apply_stack_delivery_mutation,
};
use graphite_store::PgStore;
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
async fn transactional_stack_delivery_is_version_pinned_keyed_and_replay_safe() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL is not set; skipping PostgreSQL integration test");
        return;
    };

    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate().await.unwrap();

    let nonce = Uuid::now_v7();
    let discord_user_id = positive_snowflake(nonce);
    let player_id = Uuid::now_v7();
    sqlx::query("INSERT INTO players (id, discord_user_id) VALUES ($1, $2)")
        .bind(player_id)
        .bind(discord_user_id)
        .execute(store.pool())
        .await
        .unwrap();

    let filler_key = format!("test.stack.tx.filler.{nonce}");
    seed_single_version_definition(&store, &filler_key, 1).await;
    sqlx::query(
        r#"
        INSERT INTO item_stacks (
            player_id, definition_key, definition_version, location, quantity
        )
        VALUES ($1, $2, 1, 'ITEM_BAG', 35)
        "#,
    )
    .bind(player_id)
    .bind(&filler_key)
    .execute(store.pool())
    .await
    .unwrap();

    let historical_key = format!("test.stack.tx.historical.{nonce}");
    seed_two_version_definition(&store, &historical_key, 64, 1).await;
    let fuel_key = format!("test.stack.tx.fuel.{nonce}");
    seed_single_version_definition(&store, &fuel_key, 64).await;
    let raw_key = format!("test.stack.tx.raw.{nonce}");
    seed_single_version_definition(&store, &raw_key, 64).await;

    let operation_id = seed_pending_operation(&store, player_id, discord_user_id, &nonce).await;

    let output_request = request(
        operation_id,
        player_id,
        "smelt:output",
        &historical_key,
        1,
        64,
        "SMELTING_OUTPUT",
    );
    let fuel_request = request(
        operation_id,
        player_id,
        "smelt:fuel-return",
        &fuel_key,
        1,
        1,
        "SMELTING_FUEL_RETURN",
    );
    let raw_request = request(
        operation_id,
        player_id,
        "smelt:raw-return",
        &raw_key,
        1,
        1,
        "SMELTING_INPUT_RETURN",
    );

    let mut tx = store.pool().begin().await.unwrap();
    let output = apply_stack_delivery_mutation(&mut tx, &output_request)
        .await
        .unwrap();
    assert!(!output.pending);
    assert_eq!(output.definition_version, 1);
    assert_eq!(output.pending_delivery_id, None);

    let output_retry = apply_stack_delivery_mutation(&mut tx, &output_request)
        .await
        .unwrap();
    assert_eq!(output_retry, output);

    let fuel = apply_stack_delivery_mutation(&mut tx, &fuel_request)
        .await
        .unwrap();
    assert!(fuel.pending);
    assert!(fuel.pending_delivery_id.is_some());

    let raw = apply_stack_delivery_mutation(&mut tx, &raw_request)
        .await
        .unwrap();
    assert!(raw.pending);
    assert!(raw.pending_delivery_id.is_some());
    assert_ne!(raw.pending_delivery_id, fuel.pending_delivery_id);

    let raw_retry = apply_stack_delivery_mutation(&mut tx, &raw_request)
        .await
        .unwrap();
    assert_eq!(raw_retry, raw);
    tx.commit().await.unwrap();

    let delivered_quantity: i64 = sqlx::query(
        r#"
        SELECT quantity
          FROM item_stacks
         WHERE player_id = $1
           AND definition_key = $2
           AND definition_version = 1
           AND location = 'ITEM_BAG'
        "#,
    )
    .bind(player_id)
    .bind(&historical_key)
    .fetch_one(store.pool())
    .await
    .unwrap()
    .try_get("quantity")
    .unwrap();
    assert_eq!(delivered_quantity, 64);

    let current_version_quantity: i64 = sqlx::query(
        r#"
        SELECT COUNT(*) AS count
          FROM item_stacks
         WHERE player_id = $1
           AND definition_key = $2
           AND definition_version = 2
        "#,
    )
    .bind(player_id)
    .bind(&historical_key)
    .fetch_one(store.pool())
    .await
    .unwrap()
    .try_get("count")
    .unwrap();
    assert_eq!(current_version_quantity, 0);

    let pending_count: i64 = sqlx::query(
        "SELECT COUNT(*) AS count FROM pending_asset_deliveries WHERE operation_id = $1",
    )
    .bind(operation_id)
    .fetch_one(store.pool())
    .await
    .unwrap()
    .try_get("count")
    .unwrap();
    assert_eq!(pending_count, 2);

    let event_count: i64 =
        sqlx::query("SELECT COUNT(*) AS count FROM asset_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap()
            .try_get("count")
            .unwrap();
    assert_eq!(event_count, 3);

    let mut tx = store.pool().begin().await.unwrap();
    assert_eq!(
        apply_stack_delivery_mutation(&mut tx, &output_request)
            .await
            .unwrap(),
        output
    );
    sqlx::query(
        "UPDATE operations SET state = 'COMMITTED', committed_at = now() WHERE id = $1 AND state = 'PENDING'",
    )
    .bind(operation_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let mut tx = store.pool().begin().await.unwrap();
    assert_eq!(
        apply_stack_delivery_mutation(&mut tx, &fuel_request)
            .await
            .unwrap(),
        fuel
    );

    let mut conflicting_output = output_request.clone();
    conflicting_output.quantity = 63;
    assert!(matches!(
        apply_stack_delivery_mutation(&mut tx, &conflicting_output).await,
        Err(StackDeliveryMutationError::MutationConflict)
    ));

    let new_request = request(
        operation_id,
        player_id,
        "smelt:new-after-commit",
        &raw_key,
        1,
        1,
        "SMELTING_INPUT_RETURN",
    );
    assert!(matches!(
        apply_stack_delivery_mutation(&mut tx, &new_request).await,
        Err(StackDeliveryMutationError::OperationTerminal(state)) if state == "COMMITTED"
    ));
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn transactional_stack_delivery_rolls_back_all_asset_effects_with_owning_transaction() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL is not set; skipping PostgreSQL integration test");
        return;
    };

    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate().await.unwrap();

    let nonce = Uuid::now_v7();
    let discord_user_id = positive_snowflake(nonce);
    let player_id = Uuid::now_v7();
    sqlx::query("INSERT INTO players (id, discord_user_id) VALUES ($1, $2)")
        .bind(player_id)
        .bind(discord_user_id)
        .execute(store.pool())
        .await
        .unwrap();

    let definition_key = format!("test.stack.tx.rollback.{nonce}");
    seed_single_version_definition(&store, &definition_key, 64).await;
    let operation_id = seed_pending_operation(&store, player_id, discord_user_id, &nonce).await;
    let delivery = request(
        operation_id,
        player_id,
        "rollback:delivery",
        &definition_key,
        1,
        5,
        "ROLLBACK_TEST",
    );

    let mut tx = store.pool().begin().await.unwrap();
    let receipt = apply_stack_delivery_mutation(&mut tx, &delivery)
        .await
        .unwrap();
    assert!(!receipt.pending);
    tx.rollback().await.unwrap();

    let stack_count: i64 = sqlx::query(
        "SELECT COUNT(*) AS count FROM item_stacks WHERE player_id = $1 AND definition_key = $2",
    )
    .bind(player_id)
    .bind(&definition_key)
    .fetch_one(store.pool())
    .await
    .unwrap()
    .try_get("count")
    .unwrap();
    assert_eq!(stack_count, 0);

    let event_count: i64 =
        sqlx::query("SELECT COUNT(*) AS count FROM asset_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap()
            .try_get("count")
            .unwrap();
    assert_eq!(event_count, 0);

    let pending_count: i64 = sqlx::query(
        "SELECT COUNT(*) AS count FROM pending_asset_deliveries WHERE operation_id = $1",
    )
    .bind(operation_id)
    .fetch_one(store.pool())
    .await
    .unwrap()
    .try_get("count")
    .unwrap();
    assert_eq!(pending_count, 0);

    let operation_state: String = sqlx::query("SELECT state FROM operations WHERE id = $1")
        .bind(operation_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
        .try_get("state")
        .unwrap();
    assert_eq!(operation_state, "PENDING");
}

#[tokio::test]
async fn concurrent_stack_deliveries_serialize_capacity_and_never_overfill_the_item_bag() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL is not set; skipping PostgreSQL integration test");
        return;
    };

    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate().await.unwrap();

    let nonce = Uuid::now_v7();
    let discord_user_id = positive_snowflake(nonce);
    let player_id = Uuid::now_v7();
    sqlx::query("INSERT INTO players (id, discord_user_id) VALUES ($1, $2)")
        .bind(player_id)
        .bind(discord_user_id)
        .execute(store.pool())
        .await
        .unwrap();

    let filler_key = format!("test.stack.tx.concurrent.filler.{nonce}");
    seed_single_version_definition(&store, &filler_key, 1).await;
    sqlx::query(
        r#"
        INSERT INTO item_stacks (
            player_id, definition_key, definition_version, location, quantity
        )
        VALUES ($1, $2, 1, 'ITEM_BAG', 35)
        "#,
    )
    .bind(player_id)
    .bind(&filler_key)
    .execute(store.pool())
    .await
    .unwrap();

    let left_key = format!("test.stack.tx.concurrent.left.{nonce}");
    let right_key = format!("test.stack.tx.concurrent.right.{nonce}");
    seed_single_version_definition(&store, &left_key, 64).await;
    seed_single_version_definition(&store, &right_key, 64).await;
    let left_operation = seed_pending_operation(&store, player_id, discord_user_id, &nonce).await;
    let right_operation = seed_pending_operation(&store, player_id, discord_user_id, &nonce).await;
    let left_request = request(
        left_operation,
        player_id,
        "concurrent:left",
        &left_key,
        1,
        64,
        "CONCURRENCY_TEST",
    );
    let right_request = request(
        right_operation,
        player_id,
        "concurrent:right",
        &right_key,
        1,
        64,
        "CONCURRENCY_TEST",
    );

    let left_store = store.clone();
    let right_store = store.clone();
    let left = async move {
        let mut tx = left_store.pool().begin().await.unwrap();
        let receipt = apply_stack_delivery_mutation(&mut tx, &left_request)
            .await
            .unwrap();
        commit_test_operation(&mut tx, left_operation).await;
        tx.commit().await.unwrap();
        receipt
    };
    let right = async move {
        let mut tx = right_store.pool().begin().await.unwrap();
        let receipt = apply_stack_delivery_mutation(&mut tx, &right_request)
            .await
            .unwrap();
        commit_test_operation(&mut tx, right_operation).await;
        tx.commit().await.unwrap();
        receipt
    };
    let (left, right) = tokio::join!(left, right);

    assert_eq!(usize::from(left.pending) + usize::from(right.pending), 1);

    let delivered_count: i64 = sqlx::query(
        r#"
        SELECT COUNT(*) AS count
          FROM item_stacks
         WHERE player_id = $1
           AND definition_key IN ($2, $3)
           AND location = 'ITEM_BAG'
        "#,
    )
    .bind(player_id)
    .bind(&left_key)
    .bind(&right_key)
    .fetch_one(store.pool())
    .await
    .unwrap()
    .try_get("count")
    .unwrap();
    assert_eq!(delivered_count, 1);

    let pending_count: i64 = sqlx::query(
        "SELECT COUNT(*) AS count FROM pending_asset_deliveries WHERE operation_id IN ($1, $2)",
    )
    .bind(left_operation)
    .bind(right_operation)
    .fetch_one(store.pool())
    .await
    .unwrap()
    .try_get("count")
    .unwrap();
    assert_eq!(pending_count, 1);
}

fn request(
    operation_id: Uuid,
    player_id: Uuid,
    mutation_key: &str,
    definition_key: &str,
    definition_version: i32,
    quantity: i64,
    source: &str,
) -> StackDeliveryMutationRequest {
    StackDeliveryMutationRequest {
        operation_id,
        player_id,
        mutation_key: mutation_key.to_owned(),
        definition_key: definition_key.to_owned(),
        definition_version,
        quantity,
        source: source.to_owned(),
        provenance: json!({
            "service":"SMELT",
            "test":true,
            "mutation_key":mutation_key,
        }),
    }
}

fn positive_snowflake(nonce: Uuid) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    i64::try_from((raw % 8_000_000_000_000_000_000_u64).max(1)).unwrap()
}

async fn seed_single_version_definition(store: &PgStore, key: &str, stack_limit: i64) {
    sqlx::query(
        r#"
        INSERT INTO item_definitions (
            key, category, stackable, active, definition_version, rarity, stack_limit, data
        )
        VALUES ($1, 'MATERIAL', TRUE, TRUE, 1, 'COMMON', $2, '{}'::jsonb)
        "#,
    )
    .bind(key)
    .bind(stack_limit)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO item_definition_versions (
            key, version, category, stackable, rarity, stack_limit, data
        )
        VALUES ($1, 1, 'MATERIAL', TRUE, 'COMMON', $2, '{}'::jsonb)
        "#,
    )
    .bind(key)
    .bind(stack_limit)
    .execute(store.pool())
    .await
    .unwrap();
}

async fn seed_two_version_definition(
    store: &PgStore,
    key: &str,
    historical_stack_limit: i64,
    current_stack_limit: i64,
) {
    sqlx::query(
        r#"
        INSERT INTO item_definitions (
            key, category, stackable, active, definition_version, rarity, stack_limit, data
        )
        VALUES ($1, 'MATERIAL', TRUE, TRUE, 2, 'COMMON', $2, '{}'::jsonb)
        "#,
    )
    .bind(key)
    .bind(current_stack_limit)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO item_definition_versions (
            key, version, category, stackable, rarity, stack_limit, data
        )
        VALUES
            ($1, 1, 'MATERIAL', TRUE, 'COMMON', $2, '{}'::jsonb),
            ($1, 2, 'MATERIAL', TRUE, 'COMMON', $3, '{}'::jsonb)
        "#,
    )
    .bind(key)
    .bind(historical_stack_limit)
    .bind(current_stack_limit)
    .execute(store.pool())
    .await
    .unwrap();
}

async fn seed_pending_operation(
    store: &PgStore,
    player_id: Uuid,
    discord_user_id: i64,
    nonce: &Uuid,
) -> Uuid {
    let operation_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO operations (
            id, external_request_key, actor_discord_user_id, player_id, kind, state,
            policy_version, request_hash, rng_root
        )
        VALUES ($1, $2, $3, $4, 'SMELT_SETTLEMENT_TEST', 'PENDING', 1, $5, $6)
        "#,
    )
    .bind(operation_id)
    .bind(format!(
        "test:transactional-stack-delivery:{nonce}:{operation_id}"
    ))
    .bind(discord_user_id)
    .bind(player_id)
    .bind([11_u8; 32].as_slice())
    .bind([13_u8; 32].as_slice())
    .execute(store.pool())
    .await
    .unwrap();
    operation_id
}

async fn commit_test_operation(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, operation_id: Uuid) {
    let result = sqlx::query(
        "UPDATE operations SET state = 'COMMITTED', result = '{}'::jsonb, committed_at = now() WHERE id = $1 AND state = 'PENDING'",
    )
    .bind(operation_id)
    .execute(&mut **tx)
    .await
    .unwrap();
    assert_eq!(result.rows_affected(), 1);
}
