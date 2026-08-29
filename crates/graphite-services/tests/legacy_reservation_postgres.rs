use graphite_store::PgStore;
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
async fn legacy_aggregate_job_reservation_location_is_rejected() {
    let Some(store) = test_store().await else {
        return;
    };

    let nonce = Uuid::now_v7();
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    let discord_user_id = i64::try_from((raw % 8_000_000_000_000_000_000_u64).max(1)).unwrap();
    let player_id = Uuid::now_v7();
    let definition_key = format!("test.service.legacy-reservation.{nonce}");

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
        "INSERT INTO item_stacks (player_id, definition_key, definition_version, location, quantity) VALUES ($1, $2, 1, 'ITEM_BAG', 5)",
    )
    .bind(player_id)
    .bind(&definition_key)
    .execute(store.pool())
    .await
    .unwrap();

    let legacy_move = sqlx::query(
        "UPDATE item_stacks SET location = 'JOB_RESERVATION' WHERE player_id = $1 AND definition_key = $2 AND definition_version = 1 AND location = 'ITEM_BAG'",
    )
    .bind(player_id)
    .bind(&definition_key)
    .execute(store.pool())
    .await;
    assert!(legacy_move.is_err());

    let legacy_insert = sqlx::query(
        "INSERT INTO item_stacks (player_id, definition_key, definition_version, location, quantity) VALUES ($1, $2, 1, 'JOB_RESERVATION', 1)",
    )
    .bind(player_id)
    .bind(&definition_key)
    .execute(store.pool())
    .await;
    assert!(legacy_insert.is_err());

    let row = sqlx::query(
        "SELECT location, quantity FROM item_stacks WHERE player_id = $1 AND definition_key = $2 AND definition_version = 1",
    )
    .bind(player_id)
    .bind(&definition_key)
    .fetch_one(store.pool())
    .await
    .unwrap();
    let location: String = row.try_get("location").unwrap();
    let quantity: i64 = row.try_get("quantity").unwrap();
    assert_eq!(location, "ITEM_BAG");
    assert_eq!(quantity, 5);
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
