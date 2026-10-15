use graphite_services::{
    FishingCatchBranch, FishingNonFishPayloadDeliveryError, FishingPayload,
    FishingResolvedNonFishOutcomeV1, FishingResolvedNonFishPayloadV1, FishingTreasureResult,
    MANUAL_FISHING_OPERATION_KIND, MANUAL_FISHING_OPERATION_POLICY_VERSION,
    deliver_fishing_non_fish_outcome_v1, deliver_fishing_non_fish_payload_v1,
    lock_manual_fishing_rng_context, resolve_manual_fishing_non_fish_outcome_v1,
};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn resolved_non_fish_payload_settlement_replays_and_rolls_back_atomically() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL is not set; skipping PostgreSQL integration test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate().await.unwrap();

    let nonce = Uuid::now_v7();
    let player_id = Uuid::now_v7();
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    let discord_user_id = i64::try_from((raw % 8_000_000_000_000_000_000_u64).max(1)).unwrap();
    sqlx::query("INSERT INTO players (id, discord_user_id) VALUES ($1, $2)")
        .bind(player_id)
        .bind(discord_user_id)
        .execute(store.pool())
        .await
        .unwrap();

    let operation_id = create_operation(&store, player_id, discord_user_id).await;
    let junk = FishingResolvedNonFishPayloadV1::Junk {
        payload: FishingPayload::JunkDriftwood,
    };

    let mut tx = store.pool().begin().await.unwrap();
    let first = deliver_fishing_non_fish_payload_v1(&mut tx, operation_id, player_id, junk)
        .await
        .unwrap();
    let replay = deliver_fishing_non_fish_payload_v1(&mut tx, operation_id, player_id, junk)
        .await
        .unwrap();
    assert_eq!(first, replay);
    assert!(!first.delivery.pending);

    let wrong_replay = FishingResolvedNonFishPayloadV1::Junk {
        payload: FishingPayload::JunkRustedCan,
    };
    assert!(matches!(
        deliver_fishing_non_fish_payload_v1(&mut tx, operation_id, player_id, wrong_replay,).await,
        Err(FishingNonFishPayloadDeliveryError::Stack(_))
    ));
    tx.rollback().await.unwrap();

    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM asset_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(count, 0);

    let mut tx = store.pool().begin().await.unwrap();
    let committed = deliver_fishing_non_fish_payload_v1(&mut tx, operation_id, player_id, junk)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE operations SET state = 'COMMITTED', result = '{}'::jsonb, committed_at = now() WHERE id = $1",
    )
    .bind(operation_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let mut tx = store.pool().begin().await.unwrap();
    let replay_after_commit =
        deliver_fishing_non_fish_payload_v1(&mut tx, operation_id, player_id, junk)
            .await
            .unwrap();
    assert_eq!(committed, replay_after_commit);
    tx.commit().await.unwrap();

    let stacks: i64 = sqlx::query_scalar(
        "SELECT quantity FROM item_stacks WHERE player_id = $1 AND definition_key = $2 AND definition_version = 1 AND location = 'ITEM_BAG'",
    )
    .bind(player_id)
    .bind("fishing.junk.driftwood")
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(stacks, 1);

    let events: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM asset_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(events, 1);

    let treasure_operation_id = create_operation(&store, player_id, discord_user_id).await;
    let mixed = FishingResolvedNonFishOutcomeV1::Treasure {
        result: FishingTreasureResult::MaterialBundle,
        payloads: vec![
            FishingPayload::MaterialBundleCommon,
            FishingPayload::WeatheredCrate,
        ],
    };
    let mut tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        deliver_fishing_non_fish_outcome_v1(&mut tx, treasure_operation_id, player_id, &mixed,)
            .await,
        Err(FishingNonFishPayloadDeliveryError::InvalidTreasurePayload)
    ));
    tx.rollback().await.unwrap();

    let treasure = FishingResolvedNonFishOutcomeV1::Treasure {
        result: FishingTreasureResult::MaterialBundle,
        payloads: vec![
            FishingPayload::MaterialBundleCommon,
            FishingPayload::MaterialBundleRefined,
        ],
    };
    let mut tx = store.pool().begin().await.unwrap();
    let batch =
        deliver_fishing_non_fish_outcome_v1(&mut tx, treasure_operation_id, player_id, &treasure)
            .await
            .unwrap();
    let repeated =
        deliver_fishing_non_fish_outcome_v1(&mut tx, treasure_operation_id, player_id, &treasure)
            .await
            .unwrap();
    assert_eq!(batch, repeated);
    assert_eq!(batch.deliveries.len(), 2);
    assert_ne!(
        batch.deliveries[0].delivery.event_id,
        batch.deliveries[1].delivery.event_id
    );

    sqlx::query(
        "UPDATE operations SET state = 'COMMITTED', result = '{}'::jsonb, committed_at = now() WHERE id = $1",
    )
    .bind(treasure_operation_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let mut tx = store.pool().begin().await.unwrap();
    let committed_replay =
        deliver_fishing_non_fish_outcome_v1(&mut tx, treasure_operation_id, player_id, &treasure)
            .await
            .unwrap();
    assert_eq!(batch, committed_replay);
    tx.commit().await.unwrap();

    let treasure_events: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM asset_events WHERE operation_id = $1")
            .bind(treasure_operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(treasure_events, 2);
}

#[tokio::test]
async fn pinned_rng_junk_payload_is_delivered_by_same_operation_and_rolled_back_together() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL is not set; skipping PostgreSQL integration test");
        return;
    };
    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate().await.unwrap();

    let nonce = Uuid::now_v7();
    let player_id = Uuid::now_v7();
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    let discord_user_id = i64::try_from((raw % 8_000_000_000_000_000_000_u64).max(1)).unwrap();
    sqlx::query("INSERT INTO players (id, discord_user_id) VALUES ($1, $2)")
        .bind(player_id)
        .bind(discord_user_id)
        .execute(store.pool())
        .await
        .unwrap();
    let operation_id = create_operation(&store, player_id, discord_user_id).await;

    let mut tx = store.pool().begin().await.unwrap();
    let rng = lock_manual_fishing_rng_context(&mut tx, operation_id, player_id)
        .await
        .unwrap();
    let selected =
        resolve_manual_fishing_non_fish_outcome_v1(rng, FishingCatchBranch::Junk, None).unwrap();
    let settled = deliver_fishing_non_fish_outcome_v1(&mut tx, operation_id, player_id, &selected)
        .await
        .unwrap();
    assert_eq!(settled.deliveries.len(), 1);
    assert_eq!(settled.outcome, selected);
    assert!(!settled.deliveries[0].delivery.pending);
    tx.rollback().await.unwrap();

    let no_events: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM asset_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(no_events, 0);

    let mut retry_tx = store.pool().begin().await.unwrap();
    let retry_rng = lock_manual_fishing_rng_context(&mut retry_tx, operation_id, player_id)
        .await
        .unwrap();
    let replay_selection =
        resolve_manual_fishing_non_fish_outcome_v1(retry_rng, FishingCatchBranch::Junk, None)
            .unwrap();
    assert_eq!(selected, replay_selection);
    let replay_receipt = deliver_fishing_non_fish_outcome_v1(
        &mut retry_tx,
        operation_id,
        player_id,
        &replay_selection,
    )
    .await
    .unwrap();
    assert_eq!(settled.outcome, replay_receipt.outcome);
    assert_eq!(
        settled.deliveries[0].delivery.definition_key,
        replay_receipt.deliveries[0].delivery.definition_key
    );
    sqlx::query(
        "UPDATE operations SET state = 'COMMITTED', result = '{}'::jsonb, committed_at = now() WHERE id = $1 AND state = 'PENDING'",
    )
    .bind(operation_id)
    .execute(&mut *retry_tx)
    .await
    .unwrap();
    retry_tx.commit().await.unwrap();

    let final_events: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM asset_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(final_events, 1);
}

async fn create_operation(store: &PgStore, player_id: Uuid, actor: i64) -> Uuid {
    let operation_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO operations (
            id, external_request_key, actor_discord_user_id, player_id, kind, state,
            policy_version, request_hash, rng_root
        )
        VALUES ($1, $2, $3, $4, $5, 'PENDING', $6, $7, $8)
        "#,
    )
    .bind(operation_id)
    .bind(format!("test:fishing-non-fish-payload:{operation_id}"))
    .bind(actor)
    .bind(player_id)
    .bind(MANUAL_FISHING_OPERATION_KIND)
    .bind(MANUAL_FISHING_OPERATION_POLICY_VERSION)
    .bind([11_u8; 32].as_slice())
    .bind([13_u8; 32].as_slice())
    .execute(store.pool())
    .await
    .unwrap();
    operation_id
}
