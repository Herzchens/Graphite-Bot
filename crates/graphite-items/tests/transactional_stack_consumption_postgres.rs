use graphite_items::{
    StackConsumptionMutationError, StackConsumptionMutationRequest,
    apply_stack_consumption_mutation,
};
use graphite_store::PgStore;
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
async fn transactional_stack_consumption_is_version_pinned_keyed_and_replay_safe() {
    let Some(store) = test_store().await else {
        return;
    };

    let nonce = Uuid::now_v7();
    let discord_user_id = positive_snowflake(nonce);
    let player_id = seed_player(&store, discord_user_id).await;
    let material_key = format!("test.stack.consume.material.{nonce}");
    seed_two_version_definition(&store, &material_key, 64, 16).await;
    seed_stack(&store, player_id, &material_key, 1, 10).await;

    let operation_id = seed_pending_operation(&store, player_id, discord_user_id, &nonce).await;
    let request = request(
        operation_id,
        player_id,
        "soulbind:onyx",
        &material_key,
        1,
        4,
        "SOULBIND_BIND",
    );

    let mut tx = store.pool().begin().await.unwrap();
    let receipt = apply_stack_consumption_mutation(&mut tx, &request)
        .await
        .unwrap();
    assert_eq!(receipt.quantity_before, 10);
    assert_eq!(receipt.quantity, 4);
    assert_eq!(receipt.quantity_after, 6);
    assert_eq!(
        stack_quantity_in_tx(&mut tx, player_id, &material_key, 1).await,
        6
    );
    assert_consumption_event_in_tx(&mut tx, &receipt, "SOULBIND_BIND").await;

    let replay = apply_stack_consumption_mutation(&mut tx, &request)
        .await
        .unwrap();
    assert_eq!(replay, receipt);
    assert_eq!(
        stack_quantity_in_tx(&mut tx, player_id, &material_key, 1).await,
        6
    );

    commit_test_operation(&mut tx, operation_id).await;
    tx.commit().await.unwrap();

    assert_eq!(stack_quantity(&store, player_id, &material_key, 1).await, 6);
    assert_eq!(stack_quantity(&store, player_id, &material_key, 2).await, 0);
    assert_eq!(asset_event_count(&store, operation_id).await, 1);

    let mut replay_tx = store.pool().begin().await.unwrap();
    assert_eq!(
        apply_stack_consumption_mutation(&mut replay_tx, &request)
            .await
            .unwrap(),
        receipt
    );
    assert_eq!(
        stack_quantity_in_tx(&mut replay_tx, player_id, &material_key, 1).await,
        6
    );

    let mut conflicting = request.clone();
    conflicting.quantity = 5;
    assert!(matches!(
        apply_stack_consumption_mutation(&mut replay_tx, &conflicting).await,
        Err(StackConsumptionMutationError::MutationConflict)
    ));

    let new_after_commit = self::request(
        operation_id,
        player_id,
        "soulbind:platinum",
        &material_key,
        1,
        1,
        "SOULBIND_BIND",
    );
    assert!(matches!(
        apply_stack_consumption_mutation(&mut replay_tx, &new_after_commit).await,
        Err(StackConsumptionMutationError::OperationTerminal(ref state)) if state == "COMMITTED"
    ));
    replay_tx.rollback().await.unwrap();
}

#[tokio::test]
async fn transactional_stack_consumption_rolls_back_and_deletes_exactly_consumed_stack() {
    let Some(store) = test_store().await else {
        return;
    };

    let nonce = Uuid::now_v7();
    let discord_user_id = positive_snowflake(nonce);
    let player_id = seed_player(&store, discord_user_id).await;
    let material_key = format!("test.stack.consume.rollback.{nonce}");
    seed_single_version_definition(&store, &material_key, 64, true).await;
    seed_stack(&store, player_id, &material_key, 1, 3).await;
    let operation_id = seed_pending_operation(&store, player_id, discord_user_id, &nonce).await;
    let request = request(
        operation_id,
        player_id,
        "rollback:all",
        &material_key,
        1,
        3,
        "ROLLBACK_TEST",
    );

    let mut tx = store.pool().begin().await.unwrap();
    let receipt = apply_stack_consumption_mutation(&mut tx, &request)
        .await
        .unwrap();
    assert_eq!(receipt.quantity_after, 0);
    assert_eq!(
        stack_row_count_in_tx(&mut tx, player_id, &material_key, 1).await,
        0
    );
    assert_eq!(asset_event_count_in_tx(&mut tx, operation_id).await, 1);
    tx.rollback().await.unwrap();

    assert_eq!(stack_quantity(&store, player_id, &material_key, 1).await, 3);
    assert_eq!(asset_event_count(&store, operation_id).await, 0);
    assert_eq!(operation_state(&store, operation_id).await, "PENDING");
}

#[tokio::test]
async fn stack_consumption_fails_closed_for_inventory_definition_and_account_guards() {
    let Some(store) = test_store().await else {
        return;
    };

    let nonce = Uuid::now_v7();
    let discord_user_id = positive_snowflake(nonce);
    let player_id = seed_player(&store, discord_user_id).await;
    let stackable_key = format!("test.stack.consume.guards.stackable.{nonce}");
    seed_single_version_definition(&store, &stackable_key, 64, true).await;
    seed_stack(&store, player_id, &stackable_key, 1, 2).await;
    let non_stackable_key = format!("test.stack.consume.guards.nonstack.{nonce}");
    seed_single_version_definition(&store, &non_stackable_key, 1, false).await;

    let insufficient_operation =
        seed_pending_operation(&store, player_id, discord_user_id, &nonce).await;
    let insufficient = request(
        insufficient_operation,
        player_id,
        "guard:insufficient",
        &stackable_key,
        1,
        3,
        "GUARD_TEST",
    );
    let mut insufficient_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        apply_stack_consumption_mutation(&mut insufficient_tx, &insufficient).await,
        Err(StackConsumptionMutationError::InsufficientStack {
            available: 2,
            requested: 3,
            ..
        })
    ));
    assert_eq!(
        stack_quantity_in_tx(&mut insufficient_tx, player_id, &stackable_key, 1).await,
        2
    );
    assert_eq!(
        asset_event_count_in_tx(&mut insufficient_tx, insufficient_operation).await,
        0
    );
    insufficient_tx.rollback().await.unwrap();

    let invalid_definition_operation =
        seed_pending_operation(&store, player_id, discord_user_id, &nonce).await;
    let invalid_definition = request(
        invalid_definition_operation,
        player_id,
        "guard:nonstackable",
        &non_stackable_key,
        1,
        1,
        "GUARD_TEST",
    );
    let mut invalid_definition_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        apply_stack_consumption_mutation(&mut invalid_definition_tx, &invalid_definition).await,
        Err(StackConsumptionMutationError::InvalidStackDefinition)
    ));
    invalid_definition_tx.rollback().await.unwrap();

    sqlx::query("UPDATE players SET status = 'SOFT_FROZEN' WHERE id = $1")
        .bind(player_id)
        .execute(store.pool())
        .await
        .unwrap();
    let frozen_operation = seed_pending_operation(&store, player_id, discord_user_id, &nonce).await;
    let frozen = request(
        frozen_operation,
        player_id,
        "guard:frozen",
        &stackable_key,
        1,
        1,
        "GUARD_TEST",
    );
    let mut frozen_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        apply_stack_consumption_mutation(&mut frozen_tx, &frozen).await,
        Err(StackConsumptionMutationError::AccountFrozen(ref status)) if status == "SOFT_FROZEN"
    ));
    frozen_tx.rollback().await.unwrap();
}

#[tokio::test]
async fn concurrent_stack_consumptions_serialize_and_never_overconsume() {
    let Some(store) = test_store().await else {
        return;
    };

    let nonce = Uuid::now_v7();
    let discord_user_id = positive_snowflake(nonce);
    let player_id = seed_player(&store, discord_user_id).await;
    let material_key = format!("test.stack.consume.concurrent.{nonce}");
    seed_single_version_definition(&store, &material_key, 64, true).await;
    seed_stack(&store, player_id, &material_key, 1, 5).await;

    let left_operation = seed_pending_operation(&store, player_id, discord_user_id, &nonce).await;
    let right_operation = seed_pending_operation(&store, player_id, discord_user_id, &nonce).await;
    let left_request = request(
        left_operation,
        player_id,
        "concurrent:left",
        &material_key,
        1,
        4,
        "CONCURRENCY_TEST",
    );
    let right_request = request(
        right_operation,
        player_id,
        "concurrent:right",
        &material_key,
        1,
        4,
        "CONCURRENCY_TEST",
    );

    let left_store = store.clone();
    let right_store = store.clone();
    let left = async move {
        let mut tx = left_store.pool().begin().await.unwrap();
        let result = apply_stack_consumption_mutation(&mut tx, &left_request).await;
        if result.is_ok() {
            commit_test_operation(&mut tx, left_operation).await;
            tx.commit().await.unwrap();
        } else {
            tx.rollback().await.unwrap();
        }
        result
    };
    let right = async move {
        let mut tx = right_store.pool().begin().await.unwrap();
        let result = apply_stack_consumption_mutation(&mut tx, &right_request).await;
        if result.is_ok() {
            commit_test_operation(&mut tx, right_operation).await;
            tx.commit().await.unwrap();
        } else {
            tx.rollback().await.unwrap();
        }
        result
    };
    let (left, right) = tokio::join!(left, right);

    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    for result in [left, right] {
        if let Err(error) = result {
            assert!(matches!(
                error,
                StackConsumptionMutationError::InsufficientStack {
                    available: 1,
                    requested: 4,
                    ..
                }
            ));
        }
    }
    assert_eq!(stack_quantity(&store, player_id, &material_key, 1).await, 1);
    let total_events = asset_event_count(&store, left_operation).await
        + asset_event_count(&store, right_operation).await;
    assert_eq!(total_events, 1);
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

fn request(
    operation_id: Uuid,
    player_id: Uuid,
    mutation_key: &str,
    definition_key: &str,
    definition_version: i32,
    quantity: i64,
    source: &str,
) -> StackConsumptionMutationRequest {
    StackConsumptionMutationRequest {
        operation_id,
        player_id,
        mutation_key: mutation_key.to_owned(),
        definition_key: definition_key.to_owned(),
        definition_version,
        quantity,
        source: source.to_owned(),
        provenance: json!({
            "service": source,
            "test": true,
            "mutation_key": mutation_key,
        }),
    }
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

async fn seed_single_version_definition(
    store: &PgStore,
    key: &str,
    stack_limit: i64,
    stackable: bool,
) {
    sqlx::query(
        r#"
        INSERT INTO item_definitions (
            key, category, stackable, active, definition_version, rarity, stack_limit, data
        )
        VALUES (
            $1, 'MATERIAL', $3, TRUE, 1, 'COMMON',
            CASE WHEN $3 THEN $2 ELSE NULL END,
            '{}'::jsonb
        )
        "#,
    )
    .bind(key)
    .bind(stack_limit)
    .bind(stackable)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO item_definition_versions (
            key, version, category, stackable, rarity, stack_limit, data
        )
        VALUES (
            $1, 1, 'MATERIAL', $3, 'COMMON',
            CASE WHEN $3 THEN $2 ELSE NULL END,
            '{}'::jsonb
        )
        "#,
    )
    .bind(key)
    .bind(stack_limit)
    .bind(stackable)
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

async fn seed_stack(
    store: &PgStore,
    player_id: Uuid,
    definition_key: &str,
    definition_version: i32,
    quantity: i64,
) {
    sqlx::query(
        "INSERT INTO item_stacks (player_id, definition_key, definition_version, location, quantity) VALUES ($1, $2, $3, 'ITEM_BAG', $4)",
    )
    .bind(player_id)
    .bind(definition_key)
    .bind(definition_version)
    .bind(quantity)
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
        VALUES ($1, $2, $3, $4, 'STACK_CONSUMPTION_TEST', 'PENDING', 1, $5, $6)
        "#,
    )
    .bind(operation_id)
    .bind(format!(
        "test:transactional-stack-consumption:{nonce}:{operation_id}"
    ))
    .bind(discord_user_id)
    .bind(player_id)
    .bind([17_u8; 32].as_slice())
    .bind([19_u8; 32].as_slice())
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

async fn stack_quantity(
    store: &PgStore,
    player_id: Uuid,
    definition_key: &str,
    definition_version: i32,
) -> i64 {
    sqlx::query(
        "SELECT quantity FROM item_stacks WHERE player_id = $1 AND definition_key = $2 AND definition_version = $3 AND location = 'ITEM_BAG'",
    )
    .bind(player_id)
    .bind(definition_key)
    .bind(definition_version)
    .fetch_optional(store.pool())
    .await
    .unwrap()
    .map(|row| row.try_get("quantity").unwrap())
    .unwrap_or(0)
}

async fn stack_quantity_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    player_id: Uuid,
    definition_key: &str,
    definition_version: i32,
) -> i64 {
    sqlx::query(
        "SELECT quantity FROM item_stacks WHERE player_id = $1 AND definition_key = $2 AND definition_version = $3 AND location = 'ITEM_BAG'",
    )
    .bind(player_id)
    .bind(definition_key)
    .bind(definition_version)
    .fetch_optional(&mut **tx)
    .await
    .unwrap()
    .map(|row| row.try_get("quantity").unwrap())
    .unwrap_or(0)
}

async fn stack_row_count_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    player_id: Uuid,
    definition_key: &str,
    definition_version: i32,
) -> i64 {
    sqlx::query(
        "SELECT COUNT(*) AS count FROM item_stacks WHERE player_id = $1 AND definition_key = $2 AND definition_version = $3 AND location = 'ITEM_BAG'",
    )
    .bind(player_id)
    .bind(definition_key)
    .bind(definition_version)
    .fetch_one(&mut **tx)
    .await
    .unwrap()
    .try_get("count")
    .unwrap()
}

async fn asset_event_count(store: &PgStore, operation_id: Uuid) -> i64 {
    sqlx::query("SELECT COUNT(*) AS count FROM asset_events WHERE operation_id = $1")
        .bind(operation_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
        .try_get("count")
        .unwrap()
}

async fn asset_event_count_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    operation_id: Uuid,
) -> i64 {
    sqlx::query("SELECT COUNT(*) AS count FROM asset_events WHERE operation_id = $1")
        .bind(operation_id)
        .fetch_one(&mut **tx)
        .await
        .unwrap()
        .try_get("count")
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

async fn assert_consumption_event_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    receipt: &graphite_items::StackConsumptionMutationReceipt,
    expected_source: &str,
) {
    let row = sqlx::query(
        "SELECT event_kind, payload FROM asset_events WHERE id = $1 AND operation_id = $2",
    )
    .bind(receipt.event_id)
    .bind(receipt.operation_id)
    .fetch_one(&mut **tx)
    .await
    .unwrap();
    assert_eq!(
        row.try_get::<String, _>("event_kind").unwrap(),
        "STACK_SUBCONSUMPTION_CONSUMED"
    );
    let payload: serde_json::Value = row.try_get("payload").unwrap();
    assert_eq!(payload["source"], expected_source);
    assert_eq!(
        payload["receipt"]["quantity_before"],
        receipt.quantity_before
    );
    assert_eq!(payload["receipt"]["quantity_after"], receipt.quantity_after);
}

fn positive_snowflake(nonce: Uuid) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    i64::try_from((raw % 8_000_000_000_000_000_000_u64).max(1)).unwrap()
}
