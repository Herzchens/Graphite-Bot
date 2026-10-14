use graphite_items::ItemService;
use graphite_services::{
    FISHING_FISH_ITEM_DEFINITION_VERSION, FishingArea, FishingFishDeliveryLocation,
    FishingFishInstanceDeliveryError, FishingFishInstanceDeliveryRequestV1, FishingSpecies,
    FishingVariant, claim_pending_fishing_catches_v1, deliver_fishing_fish_instance_v1,
    fishing_species_policy, resolve_fish_instance_numeric_v1,
};
use graphite_store::PgStore;
use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
async fn unique_fish_delivery_is_exact_replay_safe_and_non_transferable() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let discord_user_id = positive_snowflake(nonce);
    let player_id = seed_player(&store, discord_user_id).await;
    let operation_id = seed_operation(&store, player_id, nonce, "fish-slot-zero").await;
    let operation_created_at: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT created_at FROM operations WHERE id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap();

    let request = FishingFishInstanceDeliveryRequestV1 {
        fish_slot: 0,
        area: FishingArea::StarterPool,
        numeric: resolve_fish_instance_numeric_v1(
            FishingSpecies::Bluegill,
            FishingVariant::Normal,
            500_000,
            500_000,
            false,
        )
        .unwrap(),
    };

    let mut tx = store.pool().begin().await.unwrap();
    let first = deliver_fishing_fish_instance_v1(&mut tx, operation_id, player_id, request)
        .await
        .unwrap();
    let replay = deliver_fishing_fish_instance_v1(&mut tx, operation_id, player_id, request)
        .await
        .unwrap();
    assert_eq!(first, replay);
    assert_eq!(first.location, FishingFishDeliveryLocation::CatchBag);
    assert_eq!(first.pending_delivery_id, None);
    assert_eq!(first.definition_key, "fishing.fish.bluegill");
    assert_eq!(
        first.definition_version,
        FISHING_FISH_ITEM_DEFINITION_VERSION
    );
    assert_eq!(first.caught_at, operation_created_at);

    sqlx::query("UPDATE operations SET state = 'COMMITTED', committed_at = now() WHERE id = $1")
        .bind(operation_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let row = sqlx::query(
        r#"
        SELECT location, is_account_bound, is_tradeable, is_sellable, is_enchantable,
               is_upgradeable, is_repairable, catch_weight_grams, catch_weight_scaled_grams, state
          FROM item_instances
         WHERE id = $1
        "#,
    )
    .bind(first.item_instance_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(row.try_get::<String, _>("location").unwrap(), "CATCH_BAG");
    assert!(row.try_get::<bool, _>("is_account_bound").unwrap());
    assert!(!row.try_get::<bool, _>("is_tradeable").unwrap());
    assert!(row.try_get::<bool, _>("is_sellable").unwrap());
    assert!(!row.try_get::<bool, _>("is_enchantable").unwrap());
    assert!(!row.try_get::<bool, _>("is_upgradeable").unwrap());
    assert!(!row.try_get::<bool, _>("is_repairable").unwrap());
    assert_eq!(
        row.try_get::<i64, _>("catch_weight_grams").unwrap(),
        first.catch_weight_grams
    );
    assert_eq!(
        row.try_get::<i64, _>("catch_weight_scaled_grams").unwrap(),
        first.canonical_weight_scaled_grams
    );
    let state: Value = row.try_get("state").unwrap();
    assert_eq!(state["provenance"], "FISHING_CATCH");
    assert_eq!(state["species"], "BLUEGILL");
    assert_eq!(state["variant"], "NORMAL");
    assert_eq!(state["area"], "STARTER_POOL");
    assert_eq!(state["npc_value_money"], request.numeric.npc_value_money);
    assert_eq!(
        state["biological_noise_quantile_ppm"],
        request.numeric.biological_noise_quantile_ppm
    );

    let mut committed_replay = store.pool().begin().await.unwrap();
    assert_eq!(
        deliver_fishing_fish_instance_v1(&mut committed_replay, operation_id, player_id, request)
            .await
            .unwrap(),
        first
    );
    committed_replay.rollback().await.unwrap();

    let catch_bag = ItemService::new(store.clone())
        .catch_bag(u64::try_from(discord_user_id).unwrap())
        .await
        .unwrap();
    assert_eq!(catch_bag.pending_deliveries, 0);
    assert_eq!(catch_bag.catches.len(), 1);
    assert_eq!(
        catch_bag.catches[0].canonical_weight_scaled_grams,
        Some(first.canonical_weight_scaled_grams)
    );
}

#[tokio::test]
async fn full_catchbag_preserves_unique_fish_as_one_pending_overflow_asset() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let discord_user_id = positive_snowflake(nonce);
    let player_id = seed_player(&store, discord_user_id).await;
    seed_near_full_catch_bag(&store, player_id, nonce).await;

    let operation_id = seed_operation(&store, player_id, nonce, "overflow").await;
    let request = FishingFishInstanceDeliveryRequestV1 {
        fish_slot: 0,
        area: FishingArea::StarterPool,
        numeric: resolve_fish_instance_numeric_v1(
            FishingSpecies::Bluegill,
            FishingVariant::Normal,
            500_000,
            500_000,
            false,
        )
        .unwrap(),
    };

    let mut tx = store.pool().begin().await.unwrap();
    let receipt = deliver_fishing_fish_instance_v1(&mut tx, operation_id, player_id, request)
        .await
        .unwrap();
    assert_eq!(
        receipt.location,
        FishingFishDeliveryLocation::PendingCatchBag
    );
    let pending_id = receipt.pending_delivery_id.expect("pending delivery id");
    sqlx::query("UPDATE operations SET state = 'COMMITTED', committed_at = now() WHERE id = $1")
        .bind(operation_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let item_location: String =
        sqlx::query_scalar("SELECT location FROM item_instances WHERE id = $1")
            .bind(receipt.item_instance_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(item_location, "TEMP_OVERFLOW");

    let pending = sqlx::query(
        r#"
        SELECT item_instance_id, quantity, desired_location, reason, state
          FROM pending_asset_deliveries
         WHERE id = $1
        "#,
    )
    .bind(pending_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(
        pending.try_get::<Uuid, _>("item_instance_id").unwrap(),
        receipt.item_instance_id
    );
    assert_eq!(pending.try_get::<i64, _>("quantity").unwrap(), 1);
    assert_eq!(
        pending.try_get::<String, _>("desired_location").unwrap(),
        "CATCH_BAG"
    );
    assert_eq!(
        pending.try_get::<String, _>("reason").unwrap(),
        "CATCH_BAG_CAPACITY"
    );
    assert_eq!(pending.try_get::<String, _>("state").unwrap(), "PENDING");

    let catch_bag = ItemService::new(store.clone())
        .catch_bag(u64::try_from(discord_user_id).unwrap())
        .await
        .unwrap();
    assert_eq!(catch_bag.used_grams, 999_900);
    assert_eq!(catch_bag.pending_deliveries, 1);

    let next_operation = seed_operation(&store, player_id, nonce, "blocked-next-cast").await;
    let mut blocked = store.pool().begin().await.unwrap();
    assert!(matches!(
        deliver_fishing_fish_instance_v1(&mut blocked, next_operation, player_id, request).await,
        Err(FishingFishInstanceDeliveryError::ExistingPendingCatchDelivery)
    ));
    blocked.rollback().await.unwrap();

    sqlx::query(
        "DELETE FROM item_instances WHERE owner_player_id = $1 AND state->>'provenance' = 'TEST_FILLER'",
    )
    .bind(player_id)
    .execute(store.pool())
    .await
    .unwrap();

    let claim_operation = seed_operation(&store, player_id, nonce, "claim-overflow").await;
    let mut claim_tx = store.pool().begin().await.unwrap();
    let claim = claim_pending_fishing_catches_v1(&mut claim_tx, claim_operation, player_id)
        .await
        .unwrap();
    assert_eq!(claim.claimed.len(), 1);
    assert_eq!(claim.claimed[0].pending_delivery_id, pending_id);
    assert_eq!(claim.claimed[0].item_instance_id, receipt.item_instance_id);
    assert_eq!(claim.blocked_pending_delivery_id, None);
    assert_eq!(claim.used_grams_after, receipt.catch_weight_grams);
    assert_eq!(
        claim_pending_fishing_catches_v1(&mut claim_tx, claim_operation, player_id)
            .await
            .unwrap(),
        claim
    );
    sqlx::query("UPDATE operations SET state = 'COMMITTED', committed_at = now() WHERE id = $1")
        .bind(claim_operation)
        .execute(&mut *claim_tx)
        .await
        .unwrap();
    claim_tx.commit().await.unwrap();

    let claimed_location: String =
        sqlx::query_scalar("SELECT location FROM item_instances WHERE id = $1")
            .bind(receipt.item_instance_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(claimed_location, "CATCH_BAG");
    let pending_state: String =
        sqlx::query_scalar("SELECT state FROM pending_asset_deliveries WHERE id = $1")
            .bind(pending_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(pending_state, "CLAIMED");

    let catch_bag_after_claim = ItemService::new(store.clone())
        .catch_bag(u64::try_from(discord_user_id).unwrap())
        .await
        .unwrap();
    assert_eq!(catch_bag_after_claim.pending_deliveries, 0);
    assert_eq!(catch_bag_after_claim.used_grams, receipt.catch_weight_grams);

    let mut committed_claim_replay = store.pool().begin().await.unwrap();
    assert_eq!(
        claim_pending_fishing_catches_v1(&mut committed_claim_replay, claim_operation, player_id)
            .await
            .unwrap(),
        claim
    );
    committed_claim_replay.rollback().await.unwrap();
}

#[tokio::test]
async fn caller_rollback_removes_fish_pending_and_audit_before_retry() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let player_id = seed_player(&store, positive_snowflake(nonce)).await;
    let operation_id = seed_operation(&store, player_id, nonce, "rollback").await;
    let request = FishingFishInstanceDeliveryRequestV1 {
        fish_slot: 0,
        area: FishingArea::River,
        numeric: resolve_fish_instance_numeric_v1(
            FishingSpecies::Trout,
            FishingVariant::Silver,
            725_000,
            125_000,
            false,
        )
        .unwrap(),
    };

    let mut tx = store.pool().begin().await.unwrap();
    let first = deliver_fishing_fish_instance_v1(&mut tx, operation_id, player_id, request)
        .await
        .unwrap();
    tx.rollback().await.unwrap();

    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM item_instances WHERE created_by_operation_id = $1"
        )
        .bind(operation_id)
        .fetch_one(store.pool())
        .await
        .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM asset_events WHERE operation_id = $1")
            .bind(operation_id)
            .fetch_one(store.pool())
            .await
            .unwrap(),
        0
    );

    let mut retry = store.pool().begin().await.unwrap();
    let second = deliver_fishing_fish_instance_v1(&mut retry, operation_id, player_id, request)
        .await
        .unwrap();
    assert_ne!(first.item_instance_id, second.item_instance_id);
    retry.commit().await.unwrap();
}

#[tokio::test]
async fn all_canonical_species_have_matching_active_unique_fish_definitions() {
    let Some(store) = test_store().await else {
        return;
    };

    let species = [
        FishingSpecies::Bluegill,
        FishingSpecies::Carp,
        FishingSpecies::Catfish,
        FishingSpecies::Koi,
        FishingSpecies::Trout,
        FishingSpecies::Salmon,
        FishingSpecies::Pike,
        FishingSpecies::Sturgeon,
        FishingSpecies::Bass,
        FishingSpecies::Mackerel,
        FishingSpecies::Snapper,
        FishingSpecies::Tuna,
        FishingSpecies::Pufferfish,
        FishingSpecies::Swordfish,
        FishingSpecies::Marlin,
        FishingSpecies::GiantGrouper,
        FishingSpecies::Shark,
        FishingSpecies::Coelacanth,
        FishingSpecies::Anglerfish,
        FishingSpecies::AbyssEel,
        FishingSpecies::Moonfish,
        FishingSpecies::LeviathanFry,
    ];

    for fish in species {
        let row = sqlx::query(
            r#"
            SELECT d.active, d.definition_version, v.category, v.stackable, v.rarity
              FROM item_definitions d
              JOIN item_definition_versions v
                ON v.key = d.key
               AND v.version = d.definition_version
             WHERE d.key = $1
            "#,
        )
        .bind(fish.item_definition_key())
        .fetch_one(store.pool())
        .await
        .unwrap();

        assert!(row.try_get::<bool, _>("active").unwrap());
        assert_eq!(
            row.try_get::<i32, _>("definition_version").unwrap(),
            FISHING_FISH_ITEM_DEFINITION_VERSION
        );
        assert_eq!(row.try_get::<String, _>("category").unwrap(), "FISH");
        assert!(!row.try_get::<bool, _>("stackable").unwrap());
        assert_eq!(
            row.try_get::<String, _>("rarity").unwrap(),
            fishing_species_policy(fish).rarity.persisted_key()
        );
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

async fn seed_operation(store: &PgStore, player_id: Uuid, nonce: Uuid, suffix: &str) -> Uuid {
    let operation_id = Uuid::now_v7();
    let discord_user_id: i64 =
        sqlx::query_scalar("SELECT discord_user_id FROM players WHERE id = $1")
            .bind(player_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'FISHING_FISH_DELIVERY_TEST', 'PENDING', 1, $5, $6)",
    )
    .bind(operation_id)
    .bind(format!("test:fishing-fish-delivery:{nonce}:{suffix}:{operation_id}"))
    .bind(discord_user_id)
    .bind(player_id)
    .bind([111_u8; 32].as_slice())
    .bind([113_u8; 32].as_slice())
    .execute(store.pool())
    .await
    .unwrap();
    operation_id
}

async fn seed_near_full_catch_bag(store: &PgStore, player_id: Uuid, nonce: Uuid) {
    let operation_id = seed_operation(store, player_id, nonce, "near-full").await;
    sqlx::query(
        r#"
        INSERT INTO item_instances (
            id, definition_key, definition_version, owner_player_id, created_by_operation_id,
            location, is_account_bound, is_tradeable, is_sellable, is_enchantable,
            is_upgradeable, is_repairable, catch_weight_grams, state
        )
        VALUES (
            $1, 'fishing.fish.bluegill', 1, $2, $3,
            'CATCH_BAG', TRUE, FALSE, TRUE, FALSE,
            FALSE, FALSE, 999900, '{"provenance":"TEST_FILLER"}'::jsonb
        )
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(player_id)
    .bind(operation_id)
    .execute(store.pool())
    .await
    .unwrap();
}

fn positive_snowflake(nonce: Uuid) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    let value = (raw % 7_999_999_999_999_999_000_u64).saturating_add(1);
    i64::try_from(value).unwrap()
}
