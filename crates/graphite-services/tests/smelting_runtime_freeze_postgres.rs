use graphite_services::{
    ORDINARY_SMELT_MICROS_PER_UNIT, ReservationRole, ServiceJobReservationRequest, SmeltFuelKind,
    SmeltingRuntimeError, SmeltingRuntimeRequest, StackReservationRequest,
    attach_smelting_job_runtime, reserve_service_job_stacks,
};
use graphite_store::PgStore;
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
async fn frozen_account_blocks_new_runtime_but_not_committed_exact_replay() {
    let Some(store) = test_store().await else {
        return;
    };
    let fixture = Fixture::create(&store).await;

    let committed_operation = fixture.operation(&store, "committed").await;
    let mut committed_tx = store.pool().begin().await.unwrap();
    let committed_job = reserve_service_job_stacks(
        &mut committed_tx,
        &fixture.reservation_request(committed_operation),
    )
    .await
    .unwrap();
    let committed_request = runtime_request(committed_job.job_id);
    let committed_runtime = attach_smelting_job_runtime(&mut committed_tx, &committed_request)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE operations SET state = 'COMMITTED', result = $1, committed_at = now() WHERE id = $2",
    )
    .bind(json!({"job_id": committed_job.job_id}))
    .bind(committed_operation)
    .execute(&mut *committed_tx)
    .await
    .unwrap();
    committed_tx.commit().await.unwrap();

    let pending_operation = fixture.operation(&store, "pending-reservation").await;
    let mut pending_tx = store.pool().begin().await.unwrap();
    let pending_job = reserve_service_job_stacks(
        &mut pending_tx,
        &fixture.reservation_request(pending_operation),
    )
    .await
    .unwrap();
    pending_tx.commit().await.unwrap();

    sqlx::query("UPDATE players SET status = 'SOFT_FROZEN' WHERE id = $1")
        .bind(fixture.player_id)
        .execute(store.pool())
        .await
        .unwrap();

    let mut replay_tx = store.pool().begin().await.unwrap();
    let replay = attach_smelting_job_runtime(&mut replay_tx, &committed_request)
        .await
        .unwrap();
    replay_tx.commit().await.unwrap();
    assert_eq!(replay, committed_runtime);

    let pending_request = runtime_request(pending_job.job_id);
    let mut blocked_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        attach_smelting_job_runtime(&mut blocked_tx, &pending_request).await,
        Err(SmeltingRuntimeError::AccountFrozen(state)) if state == "SOFT_FROZEN"
    ));
    blocked_tx.rollback().await.unwrap();

    let runtime_count: i64 =
        sqlx::query("SELECT COUNT(*) AS count FROM smelting_job_runtimes WHERE job_id = $1")
            .bind(pending_job.job_id)
            .fetch_one(store.pool())
            .await
            .unwrap()
            .try_get("count")
            .unwrap();
    assert_eq!(runtime_count, 0);
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

struct Fixture {
    nonce: Uuid,
    player_id: Uuid,
    discord_user_id: i64,
    input_key: String,
    fuel_key: String,
}

impl Fixture {
    async fn create(store: &PgStore) -> Self {
        let nonce = Uuid::now_v7();
        let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
        let discord_user_id = i64::try_from((raw % 8_000_000_000_000_000_000_u64).max(1)).unwrap();
        let player_id = Uuid::now_v7();
        let input_key = format!("test.smelting.freeze.input.{nonce}");
        let fuel_key = format!("test.smelting.freeze.fuel.{nonce}");

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
        for (key, quantity) in [(&input_key, 20_i64), (&fuel_key, 4_i64)] {
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
        let id = Uuid::now_v7();
        sqlx::query(
            r#"
            INSERT INTO operations (
                id, external_request_key, actor_discord_user_id, player_id, kind, state,
                policy_version, request_hash, rng_root
            )
            VALUES ($1, $2, $3, $4, 'TEST_SMELTING_FREEZE', 'PENDING', 1, $5, $6)
            "#,
        )
        .bind(id)
        .bind(format!("test:smelting-freeze:{}:{suffix}", self.nonce))
        .bind(self.discord_user_id)
        .bind(self.player_id)
        .bind(vec![0xA5_u8; 32])
        .bind(vec![0x5A_u8; 32])
        .execute(store.pool())
        .await
        .unwrap();
        id
    }

    fn reservation_request(&self, operation_id: Uuid) -> ServiceJobReservationRequest {
        ServiceJobReservationRequest {
            operation_id,
            player_id: self.player_id,
            service_kind: "SMELT".to_owned(),
            policy_version: 1,
            stacks: vec![
                StackReservationRequest {
                    role: ReservationRole::Input,
                    definition_key: self.input_key.clone(),
                    definition_version: 1,
                    quantity: 8,
                },
                StackReservationRequest {
                    role: ReservationRole::Fuel,
                    definition_key: self.fuel_key.clone(),
                    definition_version: 1,
                    quantity: 1,
                },
            ],
        }
    }
}

fn runtime_request(job_id: Uuid) -> SmeltingRuntimeRequest {
    SmeltingRuntimeRequest {
        job_id,
        requested_units: 8,
        accepted_units: 8,
        fuel_kind: SmeltFuelKind::Coal,
        reserved_fuel_items: 1,
        effective_unit_micros: ORDINARY_SMELT_MICROS_PER_UNIT,
        modifier_snapshot: json!({
            "speed_bucket": [],
            "effective_unit_micros": ORDINARY_SMELT_MICROS_PER_UNIT
        }),
    }
}
