use graphite_services::{
    OrdinaryEquipmentEnhancedResolverError, OrdinarySlotOrbPreflightResolverError,
    SlotOrbCapacityStateError, SlotOrbFamily, SlotOrbPolicyError, SlotOrbUnlock,
    lock_owned_ordinary_equipment_enhanced_appraisal,
    lock_preview_slot_orb_attempt_for_owned_ordinary_equipment, preview_slot_orb_attempt,
};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn authoritative_preflight_uses_persisted_upgrade_capacity_and_current_enhanced_appraisal() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let definition_key = format!("test.slot-orb-preflight.armor.{nonce}");
    seed_definition(&store, &definition_key).await;
    let item_id = seed_item(&store, owner_id, &definition_key, &nonce, "authoritative").await;
    seed_structural_state(&store, item_id, "5").await;
    seed_enchant(&store, item_id, "MENDING", 1).await;

    let mut tx = store.pool().begin().await.unwrap();
    let preview = lock_preview_slot_orb_attempt_for_owned_ordinary_equipment(
        &mut tx,
        owner_id,
        item_id,
        SlotOrbUnlock::Normal5,
    )
    .await
    .unwrap();
    assert_eq!(preview.current_upgrade_level, 5);
    assert_eq!(preview.policy.family, SlotOrbFamily::NormalClass);
    assert_eq!(preview.policy.target_slot_number, 5);
    assert_eq!(
        preview,
        preview_slot_orb_attempt(
            SlotOrbUnlock::Normal5,
            preview.current_upgrade_level,
            preview.current_enhanced_appraisal,
        )
        .unwrap()
    );
    tx.rollback().await.unwrap();

    let mut appraisal_tx = store.pool().begin().await.unwrap();
    let authoritative =
        lock_owned_ordinary_equipment_enhanced_appraisal(&mut appraisal_tx, owner_id, item_id)
            .await
            .unwrap();
    assert_eq!(
        preview.current_enhanced_appraisal,
        authoritative.enhanced_canonical_appraisal
    );
    assert_eq!(
        preview.current_upgrade_level,
        authoritative.recraft.upgrade_level
    );
    assert_eq!(authoritative.recraft.normal_enchant_slot_capacity, 4);
    assert_eq!(authoritative.recraft.special_enchant_slot_capacity, 3);
    appraisal_tx.rollback().await.unwrap();
}

#[tokio::test]
async fn authoritative_preflight_requires_exact_next_slot_in_each_family() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let definition_key = format!("test.slot-orb-preflight.sequence.{nonce}");
    seed_definition(&store, &definition_key).await;
    let item_id = seed_item(&store, owner_id, &definition_key, &nonce, "sequence").await;
    seed_structural_state(&store, item_id, "15").await;

    let mut normal_missing_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_preview_slot_orb_attempt_for_owned_ordinary_equipment(
            &mut normal_missing_tx,
            owner_id,
            item_id,
            SlotOrbUnlock::Normal6,
        )
        .await,
        Err(OrdinarySlotOrbPreflightResolverError::Capacity(
            SlotOrbCapacityStateError::PredecessorSlotsLocked {
                family: SlotOrbFamily::NormalClass,
                required_unlocked_slots: 5,
                current_unlocked_slots: 4,
            }
        ))
    ));
    normal_missing_tx.rollback().await.unwrap();

    sqlx::query(
        "UPDATE item_instance_equipment_structural_state SET normal_enchant_slot_capacity = 5 WHERE item_instance_id = $1",
    )
    .bind(item_id)
    .execute(store.pool())
    .await
    .unwrap();

    let mut normal_stale_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_preview_slot_orb_attempt_for_owned_ordinary_equipment(
            &mut normal_stale_tx,
            owner_id,
            item_id,
            SlotOrbUnlock::Normal5,
        )
        .await,
        Err(OrdinarySlotOrbPreflightResolverError::Capacity(
            SlotOrbCapacityStateError::TargetSlotAlreadyUnlocked {
                family: SlotOrbFamily::NormalClass,
                target_slot_number: 5,
                current_unlocked_slots: 5,
            }
        ))
    ));
    normal_stale_tx.rollback().await.unwrap();

    let mut normal_next_tx = store.pool().begin().await.unwrap();
    let normal_next = lock_preview_slot_orb_attempt_for_owned_ordinary_equipment(
        &mut normal_next_tx,
        owner_id,
        item_id,
        SlotOrbUnlock::Normal6,
    )
    .await
    .unwrap();
    assert_eq!(normal_next.policy.target_slot_number, 6);
    normal_next_tx.rollback().await.unwrap();

    let mut special_missing_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_preview_slot_orb_attempt_for_owned_ordinary_equipment(
            &mut special_missing_tx,
            owner_id,
            item_id,
            SlotOrbUnlock::Special5,
        )
        .await,
        Err(OrdinarySlotOrbPreflightResolverError::Capacity(
            SlotOrbCapacityStateError::PredecessorSlotsLocked {
                family: SlotOrbFamily::SpecialUniversal,
                required_unlocked_slots: 4,
                current_unlocked_slots: 3,
            }
        ))
    ));
    special_missing_tx.rollback().await.unwrap();

    sqlx::query(
        "UPDATE item_instance_equipment_structural_state SET special_enchant_slot_capacity = 4 WHERE item_instance_id = $1",
    )
    .bind(item_id)
    .execute(store.pool())
    .await
    .unwrap();

    let mut special_stale_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_preview_slot_orb_attempt_for_owned_ordinary_equipment(
            &mut special_stale_tx,
            owner_id,
            item_id,
            SlotOrbUnlock::Special4,
        )
        .await,
        Err(OrdinarySlotOrbPreflightResolverError::Capacity(
            SlotOrbCapacityStateError::TargetSlotAlreadyUnlocked {
                family: SlotOrbFamily::SpecialUniversal,
                target_slot_number: 4,
                current_unlocked_slots: 4,
            }
        ))
    ));
    special_stale_tx.rollback().await.unwrap();

    let mut special_next_tx = store.pool().begin().await.unwrap();
    let special_next = lock_preview_slot_orb_attempt_for_owned_ordinary_equipment(
        &mut special_next_tx,
        owner_id,
        item_id,
        SlotOrbUnlock::Special5,
    )
    .await
    .unwrap();
    assert_eq!(special_next.policy.target_slot_number, 5);
    special_next_tx.rollback().await.unwrap();
}

#[tokio::test]
async fn authoritative_preflight_enforces_persisted_upgrade_thresholds() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let definition_key = format!("test.slot-orb-preflight.threshold.{nonce}");
    seed_definition(&store, &definition_key).await;
    let item_id = seed_item(&store, owner_id, &definition_key, &nonce, "threshold").await;
    seed_structural_state(&store, item_id, "4").await;

    let mut tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_preview_slot_orb_attempt_for_owned_ordinary_equipment(
            &mut tx,
            owner_id,
            item_id,
            SlotOrbUnlock::Normal5,
        )
        .await,
        Err(OrdinarySlotOrbPreflightResolverError::Policy(
            SlotOrbPolicyError::UpgradeLevelTooLow {
                required: 5,
                current: 4,
            }
        ))
    ));
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn authoritative_preflight_rejects_starter_and_non_enchantable_items() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let definition_key = format!("test.slot-orb-preflight.flags.{nonce}");
    seed_definition(&store, &definition_key).await;

    let starter_id = seed_item(&store, owner_id, &definition_key, &nonce, "starter").await;
    seed_structural_state(&store, starter_id, "15").await;
    sqlx::query("UPDATE item_instances SET is_starter = TRUE WHERE id = $1")
        .bind(starter_id)
        .execute(store.pool())
        .await
        .unwrap();
    let mut starter_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_preview_slot_orb_attempt_for_owned_ordinary_equipment(
            &mut starter_tx,
            owner_id,
            starter_id,
            SlotOrbUnlock::Normal5,
        )
        .await,
        Err(OrdinarySlotOrbPreflightResolverError::StarterEquipment)
    ));
    starter_tx.rollback().await.unwrap();

    let blocked_id = seed_item(&store, owner_id, &definition_key, &nonce, "blocked").await;
    seed_structural_state(&store, blocked_id, "15").await;
    sqlx::query("UPDATE item_instances SET is_enchantable = FALSE WHERE id = $1")
        .bind(blocked_id)
        .execute(store.pool())
        .await
        .unwrap();
    let mut blocked_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_preview_slot_orb_attempt_for_owned_ordinary_equipment(
            &mut blocked_tx,
            owner_id,
            blocked_id,
            SlotOrbUnlock::Normal5,
        )
        .await,
        Err(OrdinarySlotOrbPreflightResolverError::ItemNotEnchantable)
    ));
    blocked_tx.rollback().await.unwrap();
}

#[tokio::test]
async fn authoritative_preflight_propagates_invalid_embedded_enchant_state() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let definition_key = format!("test.slot-orb-preflight.invalid.{nonce}");
    seed_definition(&store, &definition_key).await;
    let item_id = seed_item(&store, owner_id, &definition_key, &nonce, "invalid").await;
    seed_structural_state(&store, item_id, "15").await;
    seed_enchant(&store, item_id, "FUTURE_UNKNOWN", 1).await;

    let mut tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_preview_slot_orb_attempt_for_owned_ordinary_equipment(
            &mut tx,
            owner_id,
            item_id,
            SlotOrbUnlock::Normal5,
        )
        .await,
        Err(OrdinarySlotOrbPreflightResolverError::Enhanced(
            OrdinaryEquipmentEnhancedResolverError::UnknownEmbeddedEnchantKey(key)
        )) if key == "FUTURE_UNKNOWN"
    ));
    tx.rollback().await.unwrap();
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

async fn seed_definition(store: &PgStore, key: &str) {
    let data = r#"{"tier":"OBSIDIAN","slot":"ARMOR_CHEST"}"#;
    sqlx::query("INSERT INTO item_definitions (key, category, stackable, active, definition_version, rarity, stack_limit, data) VALUES ($1, 'ARMOR', FALSE, TRUE, 1, 'COMMON', NULL, $2::jsonb)")
        .bind(key)
        .bind(data)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO item_definition_versions (key, version, category, stackable, rarity, stack_limit, is_ordinary_equipment, data) VALUES ($1, 1, 'ARMOR', FALSE, 'COMMON', NULL, TRUE, $2::jsonb)")
        .bind(key)
        .bind(data)
        .execute(store.pool())
        .await
        .unwrap();
}

async fn seed_item(
    store: &PgStore,
    player_id: Uuid,
    definition_key: &str,
    nonce: &Uuid,
    suffix: &str,
) -> Uuid {
    let operation_id = Uuid::now_v7();
    let discord_user_id: i64 =
        sqlx::query_scalar("SELECT discord_user_id FROM players WHERE id = $1")
            .bind(player_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    sqlx::query("INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'SLOT_ORB_PREFLIGHT_TEST', 'PENDING', 1, $5, $6)")
        .bind(operation_id)
        .bind(format!("test:slot-orb-preflight:{nonce}:{suffix}:{operation_id}"))
        .bind(discord_user_id)
        .bind(player_id)
        .bind([131_u8; 32].as_slice())
        .bind([137_u8; 32].as_slice())
        .execute(store.pool())
        .await
        .unwrap();
    let item_id = Uuid::now_v7();
    sqlx::query("INSERT INTO item_instances (id, definition_key, owner_player_id, created_by_operation_id, location, definition_version) VALUES ($1, $2, $3, $4, 'TOOL_LOCKER', 1)")
        .bind(item_id)
        .bind(definition_key)
        .bind(player_id)
        .bind(operation_id)
        .execute(store.pool())
        .await
        .unwrap();
    item_id
}

async fn seed_structural_state(store: &PgStore, item_id: Uuid, upgrade_level: &str) {
    sqlx::query("INSERT INTO item_instance_equipment_structural_state (item_instance_id, creation_roll_numerator, creation_roll_denominator, upgrade_level) VALUES ($1, 1, 2, $2::NUMERIC)")
        .bind(item_id)
        .bind(upgrade_level)
        .execute(store.pool())
        .await
        .unwrap();
}

async fn seed_enchant(store: &PgStore, item_id: Uuid, key: &str, level: i16) {
    sqlx::query("INSERT INTO item_instance_embedded_enchants (item_instance_id, enchant_key, level) VALUES ($1, $2, $3)")
        .bind(item_id)
        .bind(key)
        .bind(level)
        .execute(store.pool())
        .await
        .unwrap();
}

fn positive_snowflake(nonce: Uuid) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    i64::try_from((raw % 8_000_000_000_000_000_000_u64).max(1)).unwrap()
}
