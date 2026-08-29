use graphite_services::{
    ORDINARY_SMELT_MICROS_PER_UNIT, ReservationRole, ServiceJobReservationRequest, SmeltFuelKind,
    SmeltingRuntimeError, SmeltingRuntimeRequest, StackReservationRequest,
    attach_smelting_job_runtime, load_smelting_job_runtime, reserve_service_job_stacks,
};
use graphite_store::PgStore;
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
async fn runtime_snapshot_is_atomic_immutable_and_replay_safe() {
    let Some(store) = test_store().await else {
        return;
    };
    let fixture = Fixture::create(&store, 20, 4).await;
    let operation_id = fixture.operation(&store, "runtime").await;

    let mut tx = store.pool().begin().await.unwrap();
    let transaction_started_at: chrono::DateTime<chrono::Utc> =
        sqlx::query("SELECT transaction_timestamp() AS tx_start")
            .fetch_one(&mut *tx)
            .await
            .unwrap()
            .try_get("tx_start")
            .unwrap();
    sqlx::query("SELECT pg_sleep(0.02)")
        .fetch_one(&mut *tx)
        .await
        .unwrap();

    let reservation =
        reserve_service_job_stacks(&mut tx, &fixture.reservation_request(operation_id, 8, 1))
            .await
            .unwrap();
    let request = runtime_request(reservation.job_id, 10, 8, 1);
    let runtime = attach_smelting_job_runtime(&mut tx, &request)
        .await
        .unwrap();
    commit_operation(&mut tx, operation_id, reservation.job_id).await;
    tx.commit().await.unwrap();

    assert!(
        runtime
            .started_at
            .signed_duration_since(transaction_started_at)
            .num_milliseconds()
            >= 10
    );
    assert_eq!(
        runtime
            .completes_at
            .signed_duration_since(runtime.started_at)
            .num_seconds(),
        160
    );
    assert_eq!(
        load_smelting_job_runtime(store.pool(), runtime.job_id)
            .await
            .unwrap(),
        Some(runtime.clone())
    );
    assert_eq!(
        bag_quantity(&store, fixture.player_id, &fixture.input_key).await,
        12
    );
    assert_eq!(
        bag_quantity(&store, fixture.player_id, &fixture.fuel_key).await,
        3
    );

    let mut replay_tx = store.pool().begin().await.unwrap();
    let replay = attach_smelting_job_runtime(&mut replay_tx, &request)
        .await
        .unwrap();
    replay_tx.commit().await.unwrap();
    assert_eq!(replay, runtime);
    assert_eq!(runtime_count(&store, runtime.job_id).await, 1);

    let mut conflict = request.clone();
    conflict.effective_unit_micros -= 1;
    let mut conflict_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        attach_smelting_job_runtime(&mut conflict_tx, &conflict).await,
        Err(SmeltingRuntimeError::RuntimeConflict)
    ));
    conflict_tx.rollback().await.unwrap();

    let mutate = sqlx::query(
        "UPDATE smelting_job_runtimes SET effective_unit_micros = effective_unit_micros + 1 WHERE job_id = $1",
    )
    .bind(runtime.job_id)
    .execute(store.pool())
    .await;
    assert!(mutate.is_err());
    let delete = sqlx::query("DELETE FROM smelting_job_runtimes WHERE job_id = $1")
        .bind(runtime.job_id)
        .execute(store.pool())
        .await;
    assert!(delete.is_err());
}

#[tokio::test]
async fn runtime_cannot_be_added_after_owning_operation_commits() {
    let Some(store) = test_store().await else {
        return;
    };
    let fixture = Fixture::create(&store, 10, 2).await;
    let operation_id = fixture.operation(&store, "late-runtime").await;

    let mut reservation_tx = store.pool().begin().await.unwrap();
    let reservation = reserve_service_job_stacks(
        &mut reservation_tx,
        &fixture.reservation_request(operation_id, 8, 1),
    )
    .await
    .unwrap();
    commit_operation(&mut reservation_tx, operation_id, reservation.job_id).await;
    reservation_tx.commit().await.unwrap();

    assert_eq!(runtime_count(&store, reservation.job_id).await, 0);
    let request = runtime_request(reservation.job_id, 8, 8, 1);
    let mut late_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        attach_smelting_job_runtime(&mut late_tx, &request).await,
        Err(SmeltingRuntimeError::OperationTerminal(state)) if state == "COMMITTED"
    ));
    late_tx.rollback().await.unwrap();
    assert_eq!(runtime_count(&store, reservation.job_id).await, 0);
    assert_eq!(
        bag_quantity(&store, fixture.player_id, &fixture.input_key).await,
        2
    );
    assert_eq!(
        bag_quantity(&store, fixture.player_id, &fixture.fuel_key).await,
        1
    );
}

#[tokio::test]
async fn invalid_fuel_or_reservation_shape_rolls_back_everything() {
    let Some(store) = test_store().await else {
        return;
    };
    let fixture = Fixture::create(&store, 20, 4).await;

    let over_operation = fixture.operation(&store, "over-fuel").await;
    let mut over_tx = store.pool().begin().await.unwrap();
    let over_job = reserve_service_job_stacks(
        &mut over_tx,
        &fixture.reservation_request(over_operation, 8, 2),
    )
    .await
    .unwrap();
    let over_request = runtime_request(over_job.job_id, 8, 8, 2);
    assert!(matches!(
        attach_smelting_job_runtime(&mut over_tx, &over_request).await,
        Err(SmeltingRuntimeError::InvalidFuelReservation)
    ));
    over_tx.rollback().await.unwrap();
    assert_eq!(job_count(&store, over_operation).await, 0);
    assert_eq!(
        bag_quantity(&store, fixture.player_id, &fixture.input_key).await,
        20
    );
    assert_eq!(
        bag_quantity(&store, fixture.player_id, &fixture.fuel_key).await,
        4
    );

    let shape_operation = fixture.operation(&store, "missing-fuel-row").await;
    let mut shape_tx = store.pool().begin().await.unwrap();
    let shape_job = reserve_service_job_stacks(
        &mut shape_tx,
        &ServiceJobReservationRequest {
            operation_id: shape_operation,
            player_id: fixture.player_id,
            service_kind: "SMELT".to_owned(),
            policy_version: 1,
            stacks: vec![stack(ReservationRole::Input, &fixture.input_key, 8)],
        },
    )
    .await
    .unwrap();
    let shape_request = runtime_request(shape_job.job_id, 8, 8, 1);
    assert!(matches!(
        attach_smelting_job_runtime(&mut shape_tx, &shape_request).await,
        Err(SmeltingRuntimeError::ReservationShapeMismatch)
    ));
    shape_tx.rollback().await.unwrap();
    assert_eq!(job_count(&store, shape_operation).await, 0);
    assert_eq!(
        bag_quantity(&store, fixture.player_id, &fixture.input_key).await,
        20
    );
    assert_eq!(
        bag_quantity(&store, fixture.player_id, &fixture.fuel_key).await,
        4
    );
}

#[tokio::test]
async fn runtime_rejects_non_smelting_service_jobs() {
    let Some(store) = test_store().await else {
        return;
    };
    let fixture = Fixture::create(&store, 10, 2).await;
    let operation_id = fixture.operation(&store, "wrong-kind").await;
    let mut tx = store.pool().begin().await.unwrap();
    let reservation = reserve_service_job_stacks(
        &mut tx,
        &ServiceJobReservationRequest {
            operation_id,
            player_id: fixture.player_id,
            service_kind: "REPAIR".to_owned(),
            policy_version: 1,
            stacks: vec![
                stack(ReservationRole::Input, &fixture.input_key, 8),
                stack(ReservationRole::Fuel, &fixture.fuel_key, 1),
            ],
        },
    )
    .await
    .unwrap();
    let request = runtime_request(reservation.job_id, 8, 8, 1);
    assert!(matches!(
        attach_smelting_job_runtime(&mut tx, &request).await,
        Err(SmeltingRuntimeError::WrongServiceKind(kind)) if kind == "REPAIR"
    ));
    tx.rollback().await.unwrap();
    assert_eq!(job_count(&store, operation_id).await, 0);
}

fn runtime_request(
    job_id: Uuid,
    requested_units: i64,
    accepted_units: i64,
    reserved_fuel_items: i64,
) -> SmeltingRuntimeRequest {
    SmeltingRuntimeRequest {
        job_id,
        requested_units,
        accepted_units,
        fuel_kind: SmeltFuelKind::Coal,
        reserved_fuel_items,
        effective_unit_micros: ORDINARY_SMELT_MICROS_PER_UNIT,
        modifier_snapshot: json!({
            "speed_bucket": [],
            "effective_unit_micros": ORDINARY_SMELT_MICROS_PER_UNIT
        }),
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

#[derive(Clone)]
struct Fixture {
    nonce: Uuid,
    player_id: Uuid,
    discord_user_id: i64,
    input_key: String,
    fuel_key: String,
}

impl Fixture {
    async fn create(store: &PgStore, input_quantity: i64, fuel_quantity: i64) -> Self {
        let nonce = Uuid::now_v7();
        let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
        let discord_user_id = i64::try_from((raw % 8_000_000_000_000_000_000_u64).max(1)).unwrap();
        let player_id = Uuid::now_v7();
        let input_key = format!("test.smelting.input.{nonce}");
        let fuel_key = format!("test.smelting.fuel.{nonce}");

        sqlx::query("INSERT INTO players (id, discord_user_id) VALUES ($1, $2)")
            .bind(player_id)
            .bind(discord_user_id)
            .execute(store.pool())
            .await
            .unwrap();
        for key in [&input_key, &fuel_key] {
            sqlx::query(
                "INSERT INTO item_definitions (key, category, stackable, rarity, stack_limit) VALUES ($1, 'TEST_RESOURCE', TRUE, 'COMMON', 64)",
            )
            .bind(key)
            .execute(store.pool())
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO item_definition_versions (key, version, category, stackable, rarity, stack_limit) VALUES ($1, 1, 'TEST_RESOURCE', TRUE, 'COMMON', 64)",
            )
            .bind(key)
            .execute(store.pool())
            .await
            .unwrap();
        }
        for (key, quantity) in [(&input_key, input_quantity), (&fuel_key, fuel_quantity)] {
            sqlx::query(
                "INSERT INTO item_stacks (player_id, definition_key, definition_version, location, quantity) VALUES ($1, $2, 1, 'ITEM_BAG', $3)",
            )
            .bind(player_id)
            .bind(key)
            .bind(quantity)
            .execute(store.pool())
            .await
            .unwrap();
        }
        Self {
            nonce,
            player_id,
            discord_user_id,
            input_key,
            fuel_key,
        }
    }

    async fn operation(&self, store: &PgStore, suffix: &str) -> Uuid {
        let operation_id = Uuid::now_v7();
        sqlx::query(
            r#"
            INSERT INTO operations (
                id, external_request_key, actor_discord_user_id, player_id, kind, state,
                policy_version, request_hash, rng_root
            )
            VALUES ($1, $2, $3, $4, 'TEST_SMELTING_RUNTIME', 'PENDING', 1, $5, $6)
            "#,
        )
        .bind(operation_id)
        .bind(format!("test:smelting-runtime:{}:{suffix}", self.nonce))
        .bind(self.discord_user_id)
        .bind(self.player_id)
        .bind(vec![0xA5_u8; 32])
        .bind(vec![0x5A_u8; 32])
        .execute(store.pool())
        .await
        .unwrap();
        operation_id
    }

    fn reservation_request(
        &self,
        operation_id: Uuid,
        accepted_units: i64,
        fuel_items: i64,
    ) -> ServiceJobReservationRequest {
        ServiceJobReservationRequest {
            operation_id,
            player_id: self.player_id,
            service_kind: "SMELT".to_owned(),
            policy_version: 1,
            stacks: vec![
                stack(ReservationRole::Input, &self.input_key, accepted_units),
                stack(ReservationRole::Fuel, &self.fuel_key, fuel_items),
            ],
        }
    }
}

fn stack(role: ReservationRole, definition_key: &str, quantity: i64) -> StackReservationRequest {
    StackReservationRequest {
        role,
        definition_key: definition_key.to_owned(),
        definition_version: 1,
        quantity,
    }
}

async fn commit_operation(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    operation_id: Uuid,
    job_id: Uuid,
) {
    sqlx::query(
        "UPDATE operations SET state = 'COMMITTED', result = $1, committed_at = now() WHERE id = $2",
    )
    .bind(json!({ "job_id": job_id }))
    .bind(operation_id)
    .execute(&mut **tx)
    .await
    .unwrap();
}

async fn bag_quantity(store: &PgStore, player_id: Uuid, definition_key: &str) -> i64 {
    sqlx::query(
        "SELECT quantity FROM item_stacks WHERE player_id = $1 AND definition_key = $2 AND definition_version = 1 AND location = 'ITEM_BAG'",
    )
    .bind(player_id)
    .bind(definition_key)
    .fetch_optional(store.pool())
    .await
    .unwrap()
    .map(|row| row.try_get("quantity").unwrap())
    .unwrap_or(0)
}

async fn job_count(store: &PgStore, operation_id: Uuid) -> i64 {
    sqlx::query("SELECT COUNT(*) AS count FROM service_jobs WHERE operation_id = $1")
        .bind(operation_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
        .try_get("count")
        .unwrap()
}

async fn runtime_count(store: &PgStore, job_id: Uuid) -> i64 {
    sqlx::query("SELECT COUNT(*) AS count FROM smelting_job_runtimes WHERE job_id = $1")
        .bind(job_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
        .try_get("count")
        .unwrap()
}
