use graphite_services::{
    ORDINARY_SMELT_MICROS_PER_UNIT, ReservationRole, ServiceJobReservationRequest, SmeltFuelKind,
    SmeltingRuntimeRequest, StackReservationRequest, attach_smelting_job_runtime,
    reserve_service_job_stacks,
};
use graphite_store::PgStore;
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
async fn pending_unbound_operation_can_attach_runtime_then_bind_on_commit() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL is not set; skipping PostgreSQL integration test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate().await.unwrap();

    let nonce = Uuid::now_v7();
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    let discord_user_id = i64::try_from((raw % 8_000_000_000_000_000_000_u64).max(1)).unwrap();
    let player_id = Uuid::now_v7();
    let input_key = format!("test.smelting.unbound.input.{nonce}");
    let fuel_key = format!("test.smelting.unbound.fuel.{nonce}");

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

    for (key, quantity) in [(&input_key, 10_i64), (&fuel_key, 2_i64)] {
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

    let operation_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO operations (
            id, external_request_key, actor_discord_user_id, kind, state,
            policy_version, request_hash, rng_root
        )
        VALUES ($1, $2, $3, 'TEST_SMELTING_RUNTIME', 'PENDING', 1, $4, $5)
        "#,
    )
    .bind(operation_id)
    .bind(format!("test:smelting-runtime:unbound:{nonce}"))
    .bind(discord_user_id)
    .bind(vec![0xA5_u8; 32])
    .bind(vec![0x5A_u8; 32])
    .execute(store.pool())
    .await
    .unwrap();

    let mut tx = store.pool().begin().await.unwrap();
    let reservation = reserve_service_job_stacks(
        &mut tx,
        &ServiceJobReservationRequest {
            operation_id,
            player_id,
            service_kind: "SMELT".to_owned(),
            policy_version: 1,
            stacks: vec![
                StackReservationRequest {
                    role: ReservationRole::Input,
                    definition_key: input_key.clone(),
                    definition_version: 1,
                    quantity: 8,
                },
                StackReservationRequest {
                    role: ReservationRole::Fuel,
                    definition_key: fuel_key.clone(),
                    definition_version: 1,
                    quantity: 1,
                },
            ],
        },
    )
    .await
    .unwrap();

    let runtime_request = SmeltingRuntimeRequest {
        job_id: reservation.job_id,
        requested_units: 8,
        accepted_units: 8,
        fuel_kind: SmeltFuelKind::Coal,
        reserved_fuel_items: 1,
        effective_unit_micros: ORDINARY_SMELT_MICROS_PER_UNIT,
        modifier_snapshot: json!({
            "speed_bucket": [],
            "effective_unit_micros": ORDINARY_SMELT_MICROS_PER_UNIT
        }),
    };
    let runtime = attach_smelting_job_runtime(&mut tx, &runtime_request)
        .await
        .unwrap();

    let updated = sqlx::query(
        "UPDATE operations SET player_id = $1, state = 'COMMITTED', result = $2, committed_at = now() WHERE id = $3 AND state = 'PENDING'",
    )
    .bind(player_id)
    .bind(json!({ "job_id": reservation.job_id }))
    .bind(operation_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    assert_eq!(updated.rows_affected(), 1);
    tx.commit().await.unwrap();

    let bound_player: Option<Uuid> = sqlx::query("SELECT player_id FROM operations WHERE id = $1")
        .bind(operation_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
        .try_get("player_id")
        .unwrap();
    assert_eq!(bound_player, Some(player_id));

    let mut replay_tx = store.pool().begin().await.unwrap();
    let replay = attach_smelting_job_runtime(&mut replay_tx, &runtime_request)
        .await
        .unwrap();
    replay_tx.commit().await.unwrap();
    assert_eq!(replay, runtime);

    let runtime_count: i64 =
        sqlx::query("SELECT COUNT(*) AS count FROM smelting_job_runtimes WHERE job_id = $1")
            .bind(reservation.job_id)
            .fetch_one(store.pool())
            .await
            .unwrap()
            .try_get("count")
            .unwrap();
    assert_eq!(runtime_count, 1);

    for (key, expected) in [(&input_key, 2_i64), (&fuel_key, 1_i64)] {
        let quantity: i64 = sqlx::query(
            "SELECT quantity FROM item_stacks WHERE player_id = $1 AND definition_key = $2 AND definition_version = 1 AND location = 'ITEM_BAG'",
        )
        .bind(player_id)
        .bind(key)
        .fetch_one(store.pool())
        .await
        .unwrap()
        .try_get("quantity")
        .unwrap();
        assert_eq!(quantity, expected);
    }
}
