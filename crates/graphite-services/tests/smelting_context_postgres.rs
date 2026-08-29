use chrono::{Duration, Utc};
use graphite_services::{
    ORDINARY_SMELT_MICROS_PER_UNIT, SmeltFuelKind, SmeltingSettlementContextError,
    load_smelting_settlement_context,
};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn settlement_context_preserves_exact_reserved_asset_identity_and_rejects_bad_shapes() {
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

    let input_key = format!("test.smelting.context.input.{nonce}");
    let fuel_key = format!("test.smelting.context.fuel.{nonce}");
    seed_stack_definition(&store, &input_key, 64).await;
    seed_stack_definition(&store, &fuel_key, 16).await;

    let operation_id = seed_operation(&store, player_id, discord_user_id, "SMELT", &nonce).await;
    let job_id = Uuid::now_v7();
    seed_job(&store, job_id, operation_id, player_id, "SMELT").await;
    seed_reservation(&store, job_id, "INPUT", &input_key, 1, 8).await;
    seed_reservation(&store, job_id, "FUEL", &fuel_key, 1, 1).await;
    seed_runtime(&store, job_id, 10, 8, "COAL", 1).await;

    let context = load_smelting_settlement_context(store.pool(), job_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(context.job_id, job_id);
    assert_eq!(context.operation_id, operation_id);
    assert_eq!(context.player_id, player_id);
    assert_eq!(context.policy_version, 1);
    assert_eq!(context.input.definition_key, input_key);
    assert_eq!(context.input.definition_version, 1);
    assert_eq!(context.input.quantity, 8);
    assert_eq!(context.fuel.definition_key, fuel_key);
    assert_eq!(context.fuel.definition_version, 1);
    assert_eq!(context.fuel.quantity, 1);
    assert_eq!(context.runtime.accepted_units, 8);
    assert_eq!(context.runtime.reserved_fuel_items, 1);
    assert_eq!(context.runtime.fuel_kind, SmeltFuelKind::Coal);

    assert!(
        load_smelting_settlement_context(store.pool(), Uuid::now_v7())
            .await
            .unwrap()
            .is_none()
    );

    let wrong_operation = seed_operation(
        &store,
        player_id,
        discord_user_id,
        "REPAIR",
        &Uuid::now_v7(),
    )
    .await;
    let wrong_job = Uuid::now_v7();
    seed_job(&store, wrong_job, wrong_operation, player_id, "REPAIR").await;
    assert!(matches!(
        load_smelting_settlement_context(store.pool(), wrong_job).await,
        Err(SmeltingSettlementContextError::WrongServiceKind(kind)) if kind == "REPAIR"
    ));

    let malformed_operation =
        seed_operation(&store, player_id, discord_user_id, "SMELT", &Uuid::now_v7()).await;
    let malformed_job = Uuid::now_v7();
    seed_job(
        &store,
        malformed_job,
        malformed_operation,
        player_id,
        "SMELT",
    )
    .await;
    seed_reservation(&store, malformed_job, "INPUT", &input_key, 1, 7).await;
    seed_reservation(&store, malformed_job, "FUEL", &fuel_key, 1, 1).await;
    seed_runtime(&store, malformed_job, 10, 8, "COAL", 1).await;
    assert!(matches!(
        load_smelting_settlement_context(store.pool(), malformed_job).await,
        Err(SmeltingSettlementContextError::ReservationIntegrityMismatch)
    ));
}

fn positive_snowflake(nonce: Uuid) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    i64::try_from((raw % 8_000_000_000_000_000_000_u64).max(1)).unwrap()
}

async fn seed_stack_definition(store: &PgStore, key: &str, stack_limit: i64) {
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

async fn seed_operation(
    store: &PgStore,
    player_id: Uuid,
    discord_user_id: i64,
    kind: &str,
    nonce: &Uuid,
) -> Uuid {
    let operation_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO operations (
            id, external_request_key, actor_discord_user_id, player_id, kind, state,
            policy_version, request_hash, rng_root
        )
        VALUES ($1, $2, $3, $4, $5, 'COMMITTED', 1, $6, $7)
        "#,
    )
    .bind(operation_id)
    .bind(format!("test:smelting-context:{nonce}:{operation_id}"))
    .bind(discord_user_id)
    .bind(player_id)
    .bind(kind)
    .bind([3_u8; 32].as_slice())
    .bind([5_u8; 32].as_slice())
    .execute(store.pool())
    .await
    .unwrap();
    operation_id
}

async fn seed_job(
    store: &PgStore,
    job_id: Uuid,
    operation_id: Uuid,
    player_id: Uuid,
    service_kind: &str,
) {
    sqlx::query(
        r#"
        INSERT INTO service_jobs (
            id, operation_id, player_id, service_kind, policy_version, state
        )
        VALUES ($1, $2, $3, $4, 1, 'RUNNING')
        "#,
    )
    .bind(job_id)
    .bind(operation_id)
    .bind(player_id)
    .bind(service_kind)
    .execute(store.pool())
    .await
    .unwrap();
}

async fn seed_reservation(
    store: &PgStore,
    job_id: Uuid,
    role: &str,
    definition_key: &str,
    definition_version: i32,
    quantity: i64,
) {
    sqlx::query(
        r#"
        INSERT INTO service_job_stack_reservations (
            job_id, role, definition_key, definition_version, quantity
        )
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(job_id)
    .bind(role)
    .bind(definition_key)
    .bind(definition_version)
    .bind(quantity)
    .execute(store.pool())
    .await
    .unwrap();
}

async fn seed_runtime(
    store: &PgStore,
    job_id: Uuid,
    requested_units: i64,
    accepted_units: i64,
    fuel_kind: &str,
    reserved_fuel_items: i64,
) {
    let started_at = Utc::now();
    let total_micros = accepted_units * ORDINARY_SMELT_MICROS_PER_UNIT;
    let completes_at = started_at + Duration::microseconds(total_micros);
    sqlx::query(
        r#"
        INSERT INTO smelting_job_runtimes (
            job_id, requested_units, accepted_units, fuel_kind, reserved_fuel_items,
            effective_unit_micros, modifier_snapshot, started_at, completes_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, '{}'::jsonb, $7, $8)
        "#,
    )
    .bind(job_id)
    .bind(requested_units)
    .bind(accepted_units)
    .bind(fuel_kind)
    .bind(reserved_fuel_items)
    .bind(ORDINARY_SMELT_MICROS_PER_UNIT)
    .bind(started_at)
    .bind(completes_at)
    .execute(store.pool())
    .await
    .unwrap();
}
