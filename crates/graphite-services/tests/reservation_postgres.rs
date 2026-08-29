use graphite_services::{
    ReservationRole, ServiceJobReservationError, ServiceJobReservationRequest,
    StackReservationRequest, reserve_service_job_stacks,
};
use graphite_store::PgStore;
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
async fn reservations_are_per_job_replay_safe_and_exactly_deducted() {
    let Some(store) = test_store().await else {
        return;
    };
    let fixture = Fixture::create(&store, 20).await;
    let operation_id = fixture.operation(&store, "replay").await;
    let request = fixture.request(
        operation_id,
        vec![
            stack(ReservationRole::Input, &fixture.definition_key, 6),
            stack(ReservationRole::Fuel, &fixture.definition_key, 2),
        ],
    );

    let mut tx = store.pool().begin().await.unwrap();
    let first = reserve_service_job_stacks(&mut tx, &request).await.unwrap();
    assert_eq!(first.stacks.len(), 2);
    commit_operation(&mut tx, operation_id, first.job_id).await;
    tx.commit().await.unwrap();

    assert_eq!(bag_quantity(&store, &fixture).await, 12);
    assert_eq!(reservation_count(&store, first.job_id).await, 2);
    assert_eq!(asset_event_count(&store, operation_id).await, 1);

    let mut replay_tx = store.pool().begin().await.unwrap();
    let replay = reserve_service_job_stacks(&mut replay_tx, &request)
        .await
        .unwrap();
    replay_tx.commit().await.unwrap();
    assert_eq!(replay, first);
    assert_eq!(bag_quantity(&store, &fixture).await, 12);
    assert_eq!(asset_event_count(&store, operation_id).await, 1);

    let mut conflict = request.clone();
    conflict.stacks[0].quantity = 7;
    let mut conflict_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        reserve_service_job_stacks(&mut conflict_tx, &conflict).await,
        Err(ServiceJobReservationError::ReservationConflict)
    ));
    conflict_tx.rollback().await.unwrap();
}

#[tokio::test]
async fn rollback_insufficient_and_frozen_paths_do_not_leak_assets_or_jobs() {
    let Some(store) = test_store().await else {
        return;
    };
    let fixture = Fixture::create(&store, 10).await;

    let rollback_operation = fixture.operation(&store, "rollback").await;
    let rollback_request = fixture.request(
        rollback_operation,
        vec![stack(ReservationRole::Input, &fixture.definition_key, 4)],
    );
    let mut tx = store.pool().begin().await.unwrap();
    let rollback_receipt = reserve_service_job_stacks(&mut tx, &rollback_request)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(bag_quantity(&store, &fixture).await, 10);
    assert_eq!(job_count(&store, rollback_operation).await, 0);
    assert_eq!(reservation_count(&store, rollback_receipt.job_id).await, 0);

    let insufficient_operation = fixture.operation(&store, "insufficient").await;
    let insufficient_request = fixture.request(
        insufficient_operation,
        vec![stack(ReservationRole::Input, &fixture.definition_key, 11)],
    );
    let mut insufficient_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        reserve_service_job_stacks(&mut insufficient_tx, &insufficient_request).await,
        Err(ServiceJobReservationError::InsufficientStack {
            available: 10,
            requested: 11,
            ..
        })
    ));
    insufficient_tx.rollback().await.unwrap();
    assert_eq!(bag_quantity(&store, &fixture).await, 10);
    assert_eq!(job_count(&store, insufficient_operation).await, 0);

    sqlx::query("UPDATE players SET status = 'SOFT_FROZEN' WHERE id = $1")
        .bind(fixture.player_id)
        .execute(store.pool())
        .await
        .unwrap();
    let frozen_operation = fixture.operation(&store, "frozen").await;
    let frozen_request = fixture.request(
        frozen_operation,
        vec![stack(ReservationRole::Input, &fixture.definition_key, 1)],
    );
    let mut frozen_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        reserve_service_job_stacks(&mut frozen_tx, &frozen_request).await,
        Err(ServiceJobReservationError::AccountFrozen(status)) if status == "SOFT_FROZEN"
    ));
    frozen_tx.rollback().await.unwrap();
    assert_eq!(bag_quantity(&store, &fixture).await, 10);
    assert_eq!(job_count(&store, frozen_operation).await, 0);
}

#[tokio::test]
async fn concurrent_jobs_cannot_double_reserve_the_same_item_bag_stack() {
    let Some(store) = test_store().await else {
        return;
    };
    let fixture = Fixture::create(&store, 10).await;
    let operation_a = fixture.operation(&store, "concurrent-a").await;
    let operation_b = fixture.operation(&store, "concurrent-b").await;
    let request_a = fixture.request(
        operation_a,
        vec![stack(ReservationRole::Input, &fixture.definition_key, 7)],
    );
    let request_b = fixture.request(
        operation_b,
        vec![stack(ReservationRole::Input, &fixture.definition_key, 7)],
    );

    let store_a = store.clone();
    let store_b = store.clone();
    let result_a = async move {
        let mut tx = store_a.pool().begin().await.unwrap();
        let result = reserve_service_job_stacks(&mut tx, &request_a).await;
        if result.is_ok() {
            tx.commit().await.unwrap();
        } else {
            tx.rollback().await.unwrap();
        }
        result
    };
    let result_b = async move {
        let mut tx = store_b.pool().begin().await.unwrap();
        let result = reserve_service_job_stacks(&mut tx, &request_b).await;
        if result.is_ok() {
            tx.commit().await.unwrap();
        } else {
            tx.rollback().await.unwrap();
        }
        result
    };
    let (a, b) = tokio::join!(result_a, result_b);
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let loser = if a.is_err() { a } else { b };
    assert!(matches!(
        loser,
        Err(ServiceJobReservationError::InsufficientStack {
            available: 3,
            requested: 7,
            ..
        })
    ));
    assert_eq!(bag_quantity(&store, &fixture).await, 3);
    assert_eq!(
        job_count(&store, operation_a).await + job_count(&store, operation_b).await,
        1
    );
}

#[tokio::test]
async fn service_job_identity_and_initial_reservations_are_immutable() {
    let Some(store) = test_store().await else {
        return;
    };
    let fixture = Fixture::create(&store, 5).await;
    let operation_id = fixture.operation(&store, "immutable").await;
    let request = fixture.request(
        operation_id,
        vec![stack(ReservationRole::Input, &fixture.definition_key, 2)],
    );
    let mut tx = store.pool().begin().await.unwrap();
    let receipt = reserve_service_job_stacks(&mut tx, &request).await.unwrap();
    tx.commit().await.unwrap();

    let mutate_reservation =
        sqlx::query("UPDATE service_job_stack_reservations SET quantity = 1 WHERE job_id = $1")
            .bind(receipt.job_id)
            .execute(store.pool())
            .await;
    assert!(mutate_reservation.is_err());

    let mutate_identity =
        sqlx::query("UPDATE service_jobs SET service_kind = 'OTHER' WHERE id = $1")
            .bind(receipt.job_id)
            .execute(store.pool())
            .await;
    assert!(mutate_identity.is_err());

    sqlx::query("UPDATE service_jobs SET state = 'COMPLETED', updated_at = now() WHERE id = $1")
        .bind(receipt.job_id)
        .execute(store.pool())
        .await
        .unwrap();
    let reopen = sqlx::query("UPDATE service_jobs SET state = 'RUNNING' WHERE id = $1")
        .bind(receipt.job_id)
        .execute(store.pool())
        .await;
    assert!(reopen.is_err());
    let delete_job = sqlx::query("DELETE FROM service_jobs WHERE id = $1")
        .bind(receipt.job_id)
        .execute(store.pool())
        .await;
    assert!(delete_job.is_err());
}

#[tokio::test]
async fn malformed_requests_are_rejected_before_asset_mutation() {
    let Some(store) = test_store().await else {
        return;
    };
    let fixture = Fixture::create(&store, 5).await;
    let operation_id = fixture.operation(&store, "malformed").await;
    let mut request = fixture.request(
        operation_id,
        vec![stack(ReservationRole::Input, &fixture.definition_key, 1)],
    );

    request.stacks[0].quantity = 0;
    let mut tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        reserve_service_job_stacks(&mut tx, &request).await,
        Err(ServiceJobReservationError::InvalidReservation)
    ));
    tx.rollback().await.unwrap();

    let duplicate = stack(ReservationRole::Input, &fixture.definition_key, 1);
    request.stacks = vec![duplicate.clone(), duplicate];
    let mut tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        reserve_service_job_stacks(&mut tx, &request).await,
        Err(ServiceJobReservationError::DuplicateReservation)
    ));
    tx.rollback().await.unwrap();
    assert_eq!(bag_quantity(&store, &fixture).await, 5);
    assert_eq!(job_count(&store, operation_id).await, 0);
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
    definition_key: String,
}

impl Fixture {
    async fn create(store: &PgStore, quantity: i64) -> Self {
        let nonce = Uuid::now_v7();
        let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
        let discord_user_id = i64::try_from((raw % 8_000_000_000_000_000_000_u64).max(1)).unwrap();
        let player_id = Uuid::now_v7();
        let definition_key = format!("test.service.stack.{nonce}");
        sqlx::query("INSERT INTO players (id, discord_user_id) VALUES ($1, $2)")
            .bind(player_id)
            .bind(discord_user_id)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO item_definitions (key, category, stackable, rarity, stack_limit) VALUES ($1, 'TEST_RESOURCE', TRUE, 'COMMON', 64)",
        )
        .bind(&definition_key)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO item_definition_versions (key, version, category, stackable, rarity, stack_limit) VALUES ($1, 1, 'TEST_RESOURCE', TRUE, 'COMMON', 64)",
        )
        .bind(&definition_key)
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO item_stacks (player_id, definition_key, definition_version, location, quantity) VALUES ($1, $2, 1, 'ITEM_BAG', $3)",
        )
        .bind(player_id)
        .bind(&definition_key)
        .bind(quantity)
        .execute(store.pool())
        .await
        .unwrap();
        Self {
            nonce,
            player_id,
            discord_user_id,
            definition_key,
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
            VALUES ($1, $2, $3, $4, 'TEST_SERVICE_JOB', 'PENDING', 1, $5, $6)
            "#,
        )
        .bind(operation_id)
        .bind(format!("test:service-job:{}:{suffix}", self.nonce))
        .bind(self.discord_user_id)
        .bind(self.player_id)
        .bind(vec![0xA5_u8; 32])
        .bind(vec![0x5A_u8; 32])
        .execute(store.pool())
        .await
        .unwrap();
        operation_id
    }

    fn request(
        &self,
        operation_id: Uuid,
        stacks: Vec<StackReservationRequest>,
    ) -> ServiceJobReservationRequest {
        ServiceJobReservationRequest {
            operation_id,
            player_id: self.player_id,
            service_kind: "SMELT".to_owned(),
            policy_version: 1,
            stacks,
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
    .bind(serde_json::json!({ "job_id": job_id }))
    .bind(operation_id)
    .execute(&mut **tx)
    .await
    .unwrap();
}

async fn bag_quantity(store: &PgStore, fixture: &Fixture) -> i64 {
    sqlx::query(
        "SELECT quantity FROM item_stacks WHERE player_id = $1 AND definition_key = $2 AND definition_version = 1 AND location = 'ITEM_BAG'",
    )
    .bind(fixture.player_id)
    .bind(&fixture.definition_key)
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

async fn reservation_count(store: &PgStore, job_id: Uuid) -> i64 {
    sqlx::query("SELECT COUNT(*) AS count FROM service_job_stack_reservations WHERE job_id = $1")
        .bind(job_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
        .try_get("count")
        .unwrap()
}

async fn asset_event_count(store: &PgStore, operation_id: Uuid) -> i64 {
    sqlx::query(
        "SELECT COUNT(*) AS count FROM asset_events WHERE operation_id = $1 AND event_kind = 'SERVICE_JOB_STACKS_RESERVED'",
    )
    .bind(operation_id)
    .fetch_one(store.pool())
    .await
    .unwrap()
    .try_get("count")
    .unwrap()
}
