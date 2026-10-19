use graphite_items::claim_pending_unique_item_bag_deliveries;
use graphite_services::{
    CanonicalEnchant, DirectFishingBookPool, FINISHED_ENCHANT_BOOK_DEFINITION_KEY,
    FINISHED_ENCHANT_BOOK_DEFINITION_VERSION, FishingDirectBookCandidateV1,
    FishingDirectBookDeliveryError, deliver_fishing_direct_book_v1,
};
use graphite_store::PgStore;
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
async fn fishing_finished_book_delivery_is_unique_typed_and_exactly_replay_safe() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let operation_id = seed_operation(&store, player_id, nonce, "book").await;
    let candidate = FishingDirectBookCandidateV1 {
        treasure_slot: 0,
        pool: DirectFishingBookPool::ShopCommon,
        enchant: CanonicalEnchant::Efficiency,
        level: 1,
    };

    let mut tx = store.pool().begin().await.unwrap();
    let first = deliver_fishing_direct_book_v1(&mut tx, operation_id, player_id, candidate)
        .await
        .unwrap();
    assert!(!first.delivery.pending);
    assert_eq!(first.delivery.pending_delivery_id, None);
    assert_eq!(
        deliver_fishing_direct_book_v1(&mut tx, operation_id, player_id, candidate)
            .await
            .unwrap(),
        first
    );
    commit_operation(&mut tx, operation_id).await;
    tx.commit().await.unwrap();

    let row = sqlx::query(
        r#"
        SELECT i.definition_key,
               i.definition_version,
               i.location,
               i.is_account_bound,
               i.is_tradeable,
               i.is_sellable,
               i.is_discardable,
               i.is_enchantable,
               i.is_upgradeable,
               i.is_unbreakable,
               i.is_repairable,
               b.enchant_key,
               b.level
          FROM item_instances i
          JOIN item_instance_finished_enchant_books b
            ON b.item_instance_id = i.id
         WHERE i.id = $1
        "#,
    )
    .bind(first.delivery.item_instance_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(
        row.try_get::<String, _>("definition_key").unwrap(),
        FINISHED_ENCHANT_BOOK_DEFINITION_KEY
    );
    assert_eq!(
        row.try_get::<i32, _>("definition_version").unwrap(),
        FINISHED_ENCHANT_BOOK_DEFINITION_VERSION
    );
    assert_eq!(row.try_get::<String, _>("location").unwrap(), "ITEM_BAG");
    assert!(!row.try_get::<bool, _>("is_account_bound").unwrap());
    assert!(!row.try_get::<bool, _>("is_tradeable").unwrap());
    assert!(!row.try_get::<bool, _>("is_sellable").unwrap());
    assert!(!row.try_get::<bool, _>("is_discardable").unwrap());
    assert!(!row.try_get::<bool, _>("is_enchantable").unwrap());
    assert!(!row.try_get::<bool, _>("is_upgradeable").unwrap());
    assert!(!row.try_get::<bool, _>("is_unbreakable").unwrap());
    assert!(!row.try_get::<bool, _>("is_repairable").unwrap());
    assert_eq!(
        row.try_get::<String, _>("enchant_key").unwrap(),
        candidate.enchant.persisted_key()
    );
    assert_eq!(
        row.try_get::<i16, _>("level").unwrap(),
        i16::from(candidate.level)
    );

    let mut replay = store.pool().begin().await.unwrap();
    assert_eq!(
        deliver_fishing_direct_book_v1(&mut replay, operation_id, player_id, candidate)
            .await
            .unwrap(),
        first
    );
    replay.rollback().await.unwrap();

    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM item_instances WHERE created_by_operation_id = $1 AND definition_key = $2"
        )
        .bind(operation_id)
        .bind(FINISHED_ENCHANT_BOOK_DEFINITION_KEY)
        .fetch_one(store.pool())
        .await
        .unwrap(),
        1
    );
}

#[tokio::test]
async fn fishing_finished_book_overflow_claim_preserves_same_item_and_replay_identity() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let filler_key = format!("test.fishing-book.filler.{nonce}");
    seed_stack_definition(&store, &filler_key, 1).await;
    sqlx::query(
        "INSERT INTO item_stacks (player_id, definition_key, definition_version, location, quantity) VALUES ($1, $2, 1, 'ITEM_BAG', 36)",
    )
    .bind(player_id)
    .bind(&filler_key)
    .execute(store.pool())
    .await
    .unwrap();

    let operation_id = seed_operation(&store, player_id, nonce, "overflow").await;
    let candidate = FishingDirectBookCandidateV1 {
        treasure_slot: 0,
        pool: DirectFishingBookPool::Mending,
        enchant: CanonicalEnchant::Mending,
        level: 1,
    };
    let mut tx = store.pool().begin().await.unwrap();
    let book = deliver_fishing_direct_book_v1(&mut tx, operation_id, player_id, candidate)
        .await
        .unwrap();
    assert!(book.delivery.pending);
    let pending_id = book.delivery.pending_delivery_id.expect("pending id");
    commit_operation(&mut tx, operation_id).await;
    tx.commit().await.unwrap();

    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT location FROM item_instances WHERE id = $1")
            .bind(book.delivery.item_instance_id)
            .fetch_one(store.pool())
            .await
            .unwrap(),
        "TEMP_OVERFLOW"
    );

    sqlx::query(
        "UPDATE item_stacks SET quantity = 35 WHERE player_id = $1 AND definition_key = $2 AND definition_version = 1 AND location = 'ITEM_BAG'",
    )
    .bind(player_id)
    .bind(&filler_key)
    .execute(store.pool())
    .await
    .unwrap();

    let claim_operation = seed_operation(&store, player_id, nonce, "claim").await;
    let mut claim_tx = store.pool().begin().await.unwrap();
    let claim = claim_pending_unique_item_bag_deliveries(&mut claim_tx, claim_operation, player_id)
        .await
        .unwrap();
    assert_eq!(claim.claimed.len(), 1);
    assert_eq!(claim.claimed[0].pending_delivery_id, pending_id);
    assert_eq!(
        claim.claimed[0].item_instance_id,
        book.delivery.item_instance_id
    );
    commit_operation(&mut claim_tx, claim_operation).await;
    claim_tx.commit().await.unwrap();

    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT location FROM item_instances WHERE id = $1")
            .bind(book.delivery.item_instance_id)
            .fetch_one(store.pool())
            .await
            .unwrap(),
        "ITEM_BAG"
    );

    let mut replay = store.pool().begin().await.unwrap();
    assert_eq!(
        deliver_fishing_direct_book_v1(&mut replay, operation_id, player_id, candidate)
            .await
            .unwrap(),
        book
    );
    replay.rollback().await.unwrap();
}

#[tokio::test]
async fn fishing_finished_book_rejects_pool_identity_mismatch_before_asset_creation() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let operation_id = seed_operation(&store, player_id, nonce, "invalid").await;
    let invalid = FishingDirectBookCandidateV1 {
        treasure_slot: 0,
        pool: DirectFishingBookPool::Mythic,
        enchant: CanonicalEnchant::Efficiency,
        level: 1,
    };

    let mut tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        deliver_fishing_direct_book_v1(&mut tx, operation_id, player_id, invalid).await,
        Err(FishingDirectBookDeliveryError::InvalidCandidate)
    ));
    tx.rollback().await.unwrap();

    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM item_instances WHERE created_by_operation_id = $1"
        )
        .bind(operation_id)
        .fetch_one(store.pool())
        .await
        .unwrap(),
        0
    );
}

#[tokio::test]
async fn finished_book_identity_cannot_be_detached_or_repinned_but_parent_delete_cascades() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let operation_id = seed_operation(&store, player_id, nonce, "identity-guards").await;
    let candidate = FishingDirectBookCandidateV1 {
        treasure_slot: 0,
        pool: DirectFishingBookPool::ShopCommon,
        enchant: CanonicalEnchant::Efficiency,
        level: 1,
    };

    let mut issue_tx = store.pool().begin().await.unwrap();
    let book = deliver_fishing_direct_book_v1(&mut issue_tx, operation_id, player_id, candidate)
        .await
        .unwrap();
    commit_operation(&mut issue_tx, operation_id).await;
    issue_tx.commit().await.unwrap();

    let repin_error = sqlx::query("UPDATE item_instances SET definition_version = 2 WHERE id = $1")
        .bind(book.delivery.item_instance_id)
        .execute(store.pool())
        .await
        .unwrap_err();
    assert!(
        repin_error
            .to_string()
            .contains("Finished Enchant Book definition pin is immutable")
    );

    let mut detach_tx = store.pool().begin().await.unwrap();
    sqlx::query("DELETE FROM item_instance_finished_enchant_books WHERE item_instance_id = $1")
        .bind(book.delivery.item_instance_id)
        .execute(&mut *detach_tx)
        .await
        .unwrap();
    let detach_error = detach_tx.commit().await.unwrap_err();
    assert!(
        detach_error
            .to_string()
            .contains("typed identity cannot be detached")
    );

    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM item_instance_finished_enchant_books WHERE item_instance_id = $1",
        )
        .bind(book.delivery.item_instance_id)
        .fetch_one(store.pool())
        .await
        .unwrap(),
        1
    );

    let mut delete_parent_tx = store.pool().begin().await.unwrap();
    sqlx::query("DELETE FROM item_instances WHERE id = $1")
        .bind(book.delivery.item_instance_id)
        .execute(&mut *delete_parent_tx)
        .await
        .unwrap();
    delete_parent_tx.commit().await.unwrap();

    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM item_instances WHERE id = $1",)
            .bind(book.delivery.item_instance_id)
            .fetch_one(store.pool())
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM item_instance_finished_enchant_books WHERE item_instance_id = $1",
        )
        .bind(book.delivery.item_instance_id)
        .fetch_one(store.pool())
        .await
        .unwrap(),
        0
    );
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

async fn seed_operation(store: &PgStore, player_id: Uuid, nonce: Uuid, suffix: &str) -> Uuid {
    let operation_id = Uuid::now_v7();
    let discord_user_id: i64 =
        sqlx::query_scalar("SELECT discord_user_id FROM players WHERE id = $1")
            .bind(player_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'FISHING_BOOK_DELIVERY_TEST', 'PENDING', 1, $5, $6)",
    )
    .bind(operation_id)
    .bind(format!("test:fishing-book-delivery:{nonce}:{suffix}:{operation_id}"))
    .bind(discord_user_id)
    .bind(player_id)
    .bind([61_u8; 32].as_slice())
    .bind([67_u8; 32].as_slice())
    .execute(store.pool())
    .await
    .unwrap();
    operation_id
}

async fn commit_operation(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, operation_id: Uuid) {
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
