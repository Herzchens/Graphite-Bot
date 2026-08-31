use graphite_services::{
    CanonicalEnchant, EnchantAppraisalClass, OrdinaryEquipmentEnhancedResolverError,
    lock_owned_ordinary_equipment_enhanced_appraisal,
};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn enhanced_resolver_maps_persisted_identity_level_and_holds_enchant_locks() {
    let Some(store) = test_store().await else {
        return;
    };

    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let definition_key = format!("test.enhanced-resolver.armor.{nonce}");
    seed_ordinary_definition(&store, &definition_key).await;
    let item_id = seed_item(&store, owner_id, &definition_key, &nonce, "valid").await;
    seed_structural_state(&store, item_id, "1", "2", "3").await;
    seed_enchant(&store, item_id, "EFFICIENCY", 2).await;
    seed_enchant(&store, item_id, "MENDING", 1).await;
    seed_enchant(&store, item_id, "MASTER", 2).await;

    let mut tx = store.pool().begin().await.unwrap();
    let appraisal = lock_owned_ordinary_equipment_enhanced_appraisal(&mut tx, owner_id, item_id)
        .await
        .unwrap();

    assert_eq!(appraisal.recraft.recraft_appraisal, 1_287_479);
    assert_eq!(appraisal.embedded_enchant_value, 1_291_500);
    assert_eq!(appraisal.enhanced_canonical_appraisal, 2_578_979);
    assert_eq!(appraisal.embedded_enchants.len(), 3);

    assert_eq!(
        appraisal.embedded_enchants[0].enchant,
        CanonicalEnchant::Efficiency
    );
    assert_eq!(appraisal.embedded_enchants[0].level, 2);
    assert_eq!(
        appraisal.embedded_enchants[0].book_appraisal.class,
        EnchantAppraisalClass::ShopCommon
    );
    assert_eq!(appraisal.embedded_enchants[0].book_appraisal.value, 105_000);

    assert_eq!(
        appraisal.embedded_enchants[1].enchant,
        CanonicalEnchant::Master
    );
    assert_eq!(appraisal.embedded_enchants[1].level, 2);
    assert_eq!(
        appraisal.embedded_enchants[1].book_appraisal.class,
        EnchantAppraisalClass::SpecialRare
    );
    assert_eq!(
        appraisal.embedded_enchants[1].book_appraisal.value,
        1_260_000
    );

    assert_eq!(
        appraisal.embedded_enchants[2].enchant,
        CanonicalEnchant::Mending
    );
    assert_eq!(appraisal.embedded_enchants[2].level, 1);
    assert_eq!(
        appraisal.embedded_enchants[2].book_appraisal.class,
        EnchantAppraisalClass::Mending
    );
    assert_eq!(appraisal.embedded_enchants[2].book_appraisal.value, 480_000);

    let mut lock_probe = store.pool().begin().await.unwrap();
    let blocked = sqlx::query(
        r#"
        SELECT enchant_key
          FROM item_instance_embedded_enchants
         WHERE item_instance_id = $1
           AND enchant_key = 'MENDING'
         FOR UPDATE NOWAIT
        "#,
    )
    .bind(item_id)
    .fetch_one(&mut *lock_probe)
    .await;
    assert!(
        blocked.is_err(),
        "enhanced resolver must retain embedded-enchant row locks for the caller transaction"
    );
    lock_probe.rollback().await.unwrap();
    tx.rollback().await.unwrap();

    let mut after_release = store.pool().begin().await.unwrap();
    sqlx::query(
        r#"
        SELECT enchant_key
          FROM item_instance_embedded_enchants
         WHERE item_instance_id = $1
           AND enchant_key = 'MENDING'
         FOR UPDATE NOWAIT
        "#,
    )
    .bind(item_id)
    .fetch_one(&mut *after_release)
    .await
    .unwrap();
    after_release.rollback().await.unwrap();
}

#[tokio::test]
async fn enhanced_resolver_fails_closed_on_unknown_or_impossible_persisted_enchants() {
    let Some(store) = test_store().await else {
        return;
    };

    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let definition_key = format!("test.enhanced-resolver.invalid.armor.{nonce}");
    seed_ordinary_definition(&store, &definition_key).await;

    let unknown_item = seed_item(&store, owner_id, &definition_key, &nonce, "unknown").await;
    seed_structural_state(&store, unknown_item, "1", "2", "0").await;
    seed_enchant(&store, unknown_item, "FUTURE_UNKNOWN", 1).await;
    let mut unknown_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        lock_owned_ordinary_equipment_enhanced_appraisal(&mut unknown_tx, owner_id, unknown_item)
            .await,
        Err(OrdinaryEquipmentEnhancedResolverError::UnknownEmbeddedEnchantKey(key))
            if key == "FUTURE_UNKNOWN"
    ));
    unknown_tx.rollback().await.unwrap();

    for (suffix, key, level, expected_enchant, max_level) in [
        ("mending", "MENDING", 2_i16, CanonicalEnchant::Mending, 1_u8),
        (
            "bait-rack",
            "BAIT_RACK",
            4_i16,
            CanonicalEnchant::BaitRack,
            3_u8,
        ),
        (
            "nine-life",
            "NINE_LIFE",
            10_i16,
            CanonicalEnchant::NineLife,
            9_u8,
        ),
        ("phoenix", "PHOENIX", 2_i16, CanonicalEnchant::Phoenix, 1_u8),
        ("carving", "CARVING", 2_i16, CanonicalEnchant::Carving, 1_u8),
        ("master", "MASTER", 3_i16, CanonicalEnchant::Master, 2_u8),
    ] {
        let item_id = seed_item(&store, owner_id, &definition_key, &nonce, suffix).await;
        seed_structural_state(&store, item_id, "1", "2", "0").await;
        seed_enchant(&store, item_id, key, level).await;
        let mut tx = store.pool().begin().await.unwrap();
        assert!(matches!(
            lock_owned_ordinary_equipment_enhanced_appraisal(&mut tx, owner_id, item_id).await,
            Err(OrdinaryEquipmentEnhancedResolverError::InvalidEmbeddedEnchantLevel {
                enchant,
                level: stored_level,
                max_level: stored_max,
            }) if enchant == expected_enchant && stored_level == level && stored_max == max_level
        ));
        tx.rollback().await.unwrap();
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

async fn seed_ordinary_definition(store: &PgStore, key: &str) {
    sqlx::query(
        r#"
        INSERT INTO item_definitions (
            key, category, stackable, active, definition_version, rarity, stack_limit, data
        )
        VALUES ($1, 'ARMOR', FALSE, TRUE, 1, 'COMMON', NULL,
                '{"tier":"OBSIDIAN","slot":"ARMOR_CHEST"}'::jsonb)
        "#,
    )
    .bind(key)
    .execute(store.pool())
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO item_definition_versions (
            key, version, category, stackable, rarity, stack_limit,
            is_ordinary_equipment, data
        )
        VALUES ($1, 1, 'ARMOR', FALSE, 'COMMON', NULL, TRUE,
                '{"tier":"OBSIDIAN","slot":"ARMOR_CHEST"}'::jsonb)
        "#,
    )
    .bind(key)
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
    sqlx::query(
        r#"
        INSERT INTO operations (
            id, external_request_key, actor_discord_user_id, player_id, kind, state,
            policy_version, request_hash, rng_root
        )
        VALUES ($1, $2, $3, $4, 'ORDINARY_ENHANCED_RESOLVER_TEST', 'PENDING', 1, $5, $6)
        "#,
    )
    .bind(operation_id)
    .bind(format!(
        "test:ordinary-enhanced-resolver:{nonce}:{suffix}:{operation_id}"
    ))
    .bind(discord_user_id)
    .bind(player_id)
    .bind([79_u8; 32].as_slice())
    .bind([83_u8; 32].as_slice())
    .execute(store.pool())
    .await
    .unwrap();

    let item_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO item_instances (
            id, definition_key, owner_player_id, created_by_operation_id,
            location, definition_version
        )
        VALUES ($1, $2, $3, $4, 'TOOL_LOCKER', 1)
        "#,
    )
    .bind(item_id)
    .bind(definition_key)
    .bind(player_id)
    .bind(operation_id)
    .execute(store.pool())
    .await
    .unwrap();
    item_id
}

async fn seed_structural_state(
    store: &PgStore,
    item_id: Uuid,
    numerator: &str,
    denominator: &str,
    upgrade_level: &str,
) {
    sqlx::query(
        r#"
        INSERT INTO item_instance_equipment_structural_state (
            item_instance_id,
            creation_roll_numerator,
            creation_roll_denominator,
            upgrade_level
        )
        VALUES ($1, $2::NUMERIC, $3::NUMERIC, $4::NUMERIC)
        "#,
    )
    .bind(item_id)
    .bind(numerator)
    .bind(denominator)
    .bind(upgrade_level)
    .execute(store.pool())
    .await
    .unwrap();
}

async fn seed_enchant(store: &PgStore, item_id: Uuid, enchant_key: &str, level: i16) {
    sqlx::query(
        r#"
        INSERT INTO item_instance_embedded_enchants (item_instance_id, enchant_key, level)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(item_id)
    .bind(enchant_key)
    .bind(level)
    .execute(store.pool())
    .await
    .unwrap();
}

fn positive_snowflake(nonce: Uuid) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    i64::try_from((raw % 8_000_000_000_000_000_000_u64).max(1)).unwrap()
}
