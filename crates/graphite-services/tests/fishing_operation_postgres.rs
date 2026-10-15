use graphite_services::{
    FishingArea, MANUAL_FISHING_OPERATION_KIND, MANUAL_FISHING_OPERATION_POLICY_VERSION,
    ManualFishingOperationError, lock_or_create_manual_fishing_operation_v1,
    manual_fishing_request_hash_v1,
};
use graphite_store::PgStore;
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
async fn creates_replays_and_locks_one_canonical_fishing_operation() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let actor = positive_snowflake(nonce);
    let player_id = seed_player(&store, actor).await;
    let key = format!("test:fishing-operation:{nonce}:replay");

    let mut create = store.pool().begin().await.unwrap();
    let first = lock_or_create_manual_fishing_operation_v1(
        &mut create,
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap();
    assert_eq!(first.player_id, player_id);
    assert_eq!(first.area, FishingArea::River);
    let row = sqlx::query(
        "SELECT kind, policy_version, request_hash, octet_length(rng_root) AS root_len FROM operations WHERE id = $1",
    )
    .bind(first.operation_id)
    .fetch_one(&mut *create)
    .await
    .unwrap();
    assert_eq!(
        row.try_get::<String, _>("kind").unwrap(),
        MANUAL_FISHING_OPERATION_KIND
    );
    assert_eq!(
        row.try_get::<i32, _>("policy_version").unwrap(),
        MANUAL_FISHING_OPERATION_POLICY_VERSION
    );
    assert_eq!(
        row.try_get::<Vec<u8>, _>("request_hash").unwrap(),
        manual_fishing_request_hash_v1(FishingArea::River)
    );
    assert_eq!(row.try_get::<i32, _>("root_len").unwrap(), 32);
    create.commit().await.unwrap();

    let mut retry = store.pool().begin().await.unwrap();
    let replay = lock_or_create_manual_fishing_operation_v1(
        &mut retry,
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::River,
    )
    .await
    .unwrap();
    assert_eq!(replay, first);
    retry.rollback().await.unwrap();
}

#[tokio::test]
async fn external_key_reuse_with_changed_area_actor_or_contract_fails_closed() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let actor = positive_snowflake(nonce);
    let other_actor = positive_snowflake_with_offset(nonce, 1);
    seed_player(&store, actor).await;
    seed_player(&store, other_actor).await;
    let key = format!("test:fishing-operation:{nonce}:conflict");

    let mut create = store.pool().begin().await.unwrap();
    let first = lock_or_create_manual_fishing_operation_v1(
        &mut create,
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::Lake,
    )
    .await
    .unwrap();
    create.commit().await.unwrap();

    let mut changed_area = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_or_create_manual_fishing_operation_v1(
            &mut changed_area,
            u64::try_from(actor).unwrap(),
            &key,
            FishingArea::Coast,
        )
        .await,
        Err(ManualFishingOperationError::IdempotencyConflict)
    ));
    changed_area.rollback().await.unwrap();

    let mut changed_actor = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_or_create_manual_fishing_operation_v1(
            &mut changed_actor,
            u64::try_from(other_actor).unwrap(),
            &key,
            FishingArea::Lake,
        )
        .await,
        Err(ManualFishingOperationError::IdempotencyConflict)
    ));
    changed_actor.rollback().await.unwrap();

    sqlx::query("UPDATE operations SET policy_version = 2 WHERE id = $1")
        .bind(first.operation_id)
        .execute(store.pool())
        .await
        .unwrap();
    let mut wrong_contract = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_or_create_manual_fishing_operation_v1(
            &mut wrong_contract,
            u64::try_from(actor).unwrap(),
            &key,
            FishingArea::Lake,
        )
        .await,
        Err(ManualFishingOperationError::IdempotencyConflict)
    ));
    wrong_contract.rollback().await.unwrap();
}

#[tokio::test]
async fn terminal_operation_is_not_opaque_replayed_and_creation_rolls_back() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let actor = positive_snowflake(nonce);
    seed_player(&store, actor).await;

    let rolled_back_key = format!("test:fishing-operation:{nonce}:rollback");
    let rolled_back_id = {
        let mut tx = store.pool().begin().await.unwrap();
        let receipt = lock_or_create_manual_fishing_operation_v1(
            &mut tx,
            u64::try_from(actor).unwrap(),
            &rolled_back_key,
            FishingArea::StarterPool,
        )
        .await
        .unwrap();
        tx.rollback().await.unwrap();
        receipt.operation_id
    };
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM operations WHERE id = $1)")
        .bind(rolled_back_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert!(!exists);

    let key = format!("test:fishing-operation:{nonce}:terminal");
    let mut create = store.pool().begin().await.unwrap();
    let receipt = lock_or_create_manual_fishing_operation_v1(
        &mut create,
        u64::try_from(actor).unwrap(),
        &key,
        FishingArea::StarterPool,
    )
    .await
    .unwrap();
    create.commit().await.unwrap();
    sqlx::query(
        "UPDATE operations SET state = 'COMMITTED', result = '{}'::jsonb, committed_at = now() WHERE id = $1",
    )
    .bind(receipt.operation_id)
    .execute(store.pool())
    .await
    .unwrap();

    let mut terminal = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_or_create_manual_fishing_operation_v1(
            &mut terminal,
            u64::try_from(actor).unwrap(),
            &key,
            FishingArea::StarterPool,
        )
        .await,
        Err(ManualFishingOperationError::OperationTerminal(state)) if state == "COMMITTED"
    ));
    terminal.rollback().await.unwrap();
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

async fn seed_player(store: &PgStore, actor: i64) -> Uuid {
    let player_id = Uuid::now_v7();
    sqlx::query("INSERT INTO players (id, discord_user_id) VALUES ($1, $2)")
        .bind(player_id)
        .bind(actor)
        .execute(store.pool())
        .await
        .unwrap();
    player_id
}

fn positive_snowflake(nonce: Uuid) -> i64 {
    positive_snowflake_with_offset(nonce, 0)
}

fn positive_snowflake_with_offset(nonce: Uuid, offset: u64) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    let value = (raw % 7_999_999_999_999_999_000_u64)
        .saturating_add(1)
        .saturating_add(offset);
    i64::try_from(value).unwrap()
}
