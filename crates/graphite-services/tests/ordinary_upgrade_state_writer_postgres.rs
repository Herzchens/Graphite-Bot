use graphite_services::{
    ResolvedUpgradeLevelTransition, UpgradeLevelStateWriterError,
    write_resolved_upgrade_level_transition_to_owned_ordinary_equipment,
};
use graphite_store::PgStore;
use uuid::Uuid;

#[tokio::test]
async fn advance_and_downgrade_are_exact_and_preserve_sibling_structural_state() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let definition_key = format!("test.upgrade-writer.armor.{nonce}");
    seed_definition(
        &store,
        &definition_key,
        "ARMOR",
        r#"{"tier":"OBSIDIAN","slot":"ARMOR_CHEST"}"#,
    )
    .await;
    let item_id = seed_item(&store, owner_id, &definition_key, &nonce, "exact").await;
    seed_structural_state(&store, item_id, "1", "2", "3", 4, 3).await;

    let mut advance_tx = store.pool().begin().await.unwrap();
    let advanced = write_resolved_upgrade_level_transition_to_owned_ordinary_equipment(
        &mut advance_tx,
        owner_id,
        item_id,
        3,
        ResolvedUpgradeLevelTransition::AdvanceOne,
    )
    .await
    .unwrap();
    assert_eq!(advanced.previous_upgrade_level, 3);
    assert_eq!(advanced.new_upgrade_level, 4);
    assert_eq!(advanced.previous_recraft_appraisal, 1_287_479);
    assert_eq!(advanced.new_recraft_appraisal, 1_313_608);
    assert_eq!(
        advanced.previous_enhanced_canonical_appraisal,
        advanced.previous_recraft_appraisal
    );
    assert_eq!(
        advanced.new_enhanced_canonical_appraisal,
        advanced.new_recraft_appraisal
    );
    assert_eq!(
        structural_state_in_tx(&mut advance_tx, item_id).await,
        ("1".into(), "2".into(), "4".into(), 4, 3)
    );
    advance_tx.commit().await.unwrap();

    let mut downgrade_tx = store.pool().begin().await.unwrap();
    let downgraded = write_resolved_upgrade_level_transition_to_owned_ordinary_equipment(
        &mut downgrade_tx,
        owner_id,
        item_id,
        4,
        ResolvedUpgradeLevelTransition::DowngradeOne,
    )
    .await
    .unwrap();
    assert_eq!(downgraded.previous_upgrade_level, 4);
    assert_eq!(downgraded.new_upgrade_level, 3);
    assert_eq!(downgraded.previous_recraft_appraisal, 1_313_608);
    assert_eq!(downgraded.new_recraft_appraisal, 1_287_479);
    downgrade_tx.commit().await.unwrap();

    assert_eq!(
        structural_state(&store, item_id).await,
        ("1".into(), "2".into(), "3".into(), 4, 3)
    );
}

#[tokio::test]
async fn transition_is_transaction_composable_and_rollback_restores_level() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let definition_key = format!("test.upgrade-writer.rollback.{nonce}");
    seed_definition(&store, &definition_key, "PICKAXE", r#"{"tier":"OBSIDIAN"}"#).await;
    let item_id = seed_item(&store, owner_id, &definition_key, &nonce, "rollback").await;
    seed_structural_state(&store, item_id, "2", "3", "9", 4, 3).await;

    let mut tx = store.pool().begin().await.unwrap();
    let applied = write_resolved_upgrade_level_transition_to_owned_ordinary_equipment(
        &mut tx,
        owner_id,
        item_id,
        9,
        ResolvedUpgradeLevelTransition::AdvanceOne,
    )
    .await
    .unwrap();
    assert_eq!(applied.new_upgrade_level, 10);
    assert_eq!(structural_state_in_tx(&mut tx, item_id).await.2, "10");
    tx.rollback().await.unwrap();

    assert_eq!(structural_state(&store, item_id).await.2, "9");
}

#[tokio::test]
async fn starter_non_upgradeable_stale_and_zero_downgrade_fail_before_mutation() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let definition_key = format!("test.upgrade-writer.guards.{nonce}");
    seed_definition(&store, &definition_key, "SWORD", r#"{"tier":"OBSIDIAN"}"#).await;

    let starter = seed_item(&store, owner_id, &definition_key, &nonce, "starter").await;
    seed_structural_state(&store, starter, "1", "2", "3", 4, 3).await;
    sqlx::query("UPDATE item_instances SET is_starter = TRUE, is_upgradeable = TRUE WHERE id = $1")
        .bind(starter)
        .execute(store.pool())
        .await
        .unwrap();
    let mut starter_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        write_resolved_upgrade_level_transition_to_owned_ordinary_equipment(
            &mut starter_tx,
            owner_id,
            starter,
            3,
            ResolvedUpgradeLevelTransition::AdvanceOne,
        )
        .await,
        Err(UpgradeLevelStateWriterError::StarterEquipment)
    ));
    starter_tx.rollback().await.unwrap();
    assert_eq!(structural_state(&store, starter).await.2, "3");

    let disabled = seed_item(&store, owner_id, &definition_key, &nonce, "disabled").await;
    seed_structural_state(&store, disabled, "1", "2", "3", 4, 3).await;
    sqlx::query("UPDATE item_instances SET is_upgradeable = FALSE WHERE id = $1")
        .bind(disabled)
        .execute(store.pool())
        .await
        .unwrap();
    let mut disabled_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        write_resolved_upgrade_level_transition_to_owned_ordinary_equipment(
            &mut disabled_tx,
            owner_id,
            disabled,
            3,
            ResolvedUpgradeLevelTransition::AdvanceOne,
        )
        .await,
        Err(UpgradeLevelStateWriterError::ItemNotUpgradeable)
    ));
    disabled_tx.rollback().await.unwrap();
    assert_eq!(structural_state(&store, disabled).await.2, "3");

    let stale = seed_item(&store, owner_id, &definition_key, &nonce, "stale").await;
    seed_structural_state(&store, stale, "1", "2", "4", 4, 3).await;
    let mut stale_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        write_resolved_upgrade_level_transition_to_owned_ordinary_equipment(
            &mut stale_tx,
            owner_id,
            stale,
            3,
            ResolvedUpgradeLevelTransition::AdvanceOne,
        )
        .await,
        Err(UpgradeLevelStateWriterError::UpgradeLevelChanged {
            expected_level: 3,
            actual_level: 4,
        })
    ));
    stale_tx.rollback().await.unwrap();
    assert_eq!(structural_state(&store, stale).await.2, "4");

    let zero = seed_item(&store, owner_id, &definition_key, &nonce, "zero").await;
    seed_structural_state(&store, zero, "1", "2", "0", 4, 3).await;
    let mut zero_tx = store.pool().begin().await.unwrap();
    assert!(matches!(
        write_resolved_upgrade_level_transition_to_owned_ordinary_equipment(
            &mut zero_tx,
            owner_id,
            zero,
            0,
            ResolvedUpgradeLevelTransition::DowngradeOne,
        )
        .await,
        Err(UpgradeLevelStateWriterError::CannotDowngradeZero)
    ));
    zero_tx.rollback().await.unwrap();
    assert_eq!(structural_state(&store, zero).await.2, "0");
}

#[tokio::test]
async fn transition_preserves_embedded_enchants_and_returns_enhanced_appraisal_context() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let definition_key = format!("test.upgrade-writer.enchanted.{nonce}");
    seed_definition(
        &store,
        &definition_key,
        "ARMOR",
        r#"{"tier":"OBSIDIAN","slot":"ARMOR_CHEST"}"#,
    )
    .await;
    let item_id = seed_item(&store, owner_id, &definition_key, &nonce, "enchanted").await;
    seed_structural_state(&store, item_id, "1", "2", "3", 4, 3).await;
    seed_enchant(&store, item_id, "PROTECTION", 1).await;

    let before_rows = enchant_rows(&store, item_id).await;
    let mut tx = store.pool().begin().await.unwrap();
    let applied = write_resolved_upgrade_level_transition_to_owned_ordinary_equipment(
        &mut tx,
        owner_id,
        item_id,
        3,
        ResolvedUpgradeLevelTransition::AdvanceOne,
    )
    .await
    .unwrap();
    let before_enchant_value = applied
        .previous_enhanced_canonical_appraisal
        .checked_sub(applied.previous_recraft_appraisal)
        .unwrap();
    let after_enchant_value = applied
        .new_enhanced_canonical_appraisal
        .checked_sub(applied.new_recraft_appraisal)
        .unwrap();
    assert!(before_enchant_value > 0);
    assert_eq!(after_enchant_value, before_enchant_value);
    tx.commit().await.unwrap();

    assert_eq!(enchant_rows(&store, item_id).await, before_rows);
    assert_eq!(structural_state(&store, item_id).await.2, "4");
}

#[tokio::test]
async fn structural_writer_does_not_turn_the_v1_probability_table_boundary_into_a_level_cap() {
    let Some(store) = test_store().await else {
        return;
    };
    let nonce = Uuid::now_v7();
    let owner_id = seed_player(&store, positive_snowflake(nonce)).await;
    let definition_key = format!("test.upgrade-writer.above-table.{nonce}");
    seed_definition(&store, &definition_key, "PICKAXE", r#"{"tier":"OBSIDIAN"}"#).await;
    let item_id = seed_item(&store, owner_id, &definition_key, &nonce, "above-table").await;
    seed_structural_state(&store, item_id, "1", "2", "20", 6, 6).await;

    // This proves only that structural persistence is not capped at +20. The current v1 outcome
    // policy still has no authoritative probability row that could resolve a real +21 attempt.
    let mut tx = store.pool().begin().await.unwrap();
    let applied = write_resolved_upgrade_level_transition_to_owned_ordinary_equipment(
        &mut tx,
        owner_id,
        item_id,
        20,
        ResolvedUpgradeLevelTransition::AdvanceOne,
    )
    .await
    .unwrap();
    assert_eq!(applied.new_upgrade_level, 21);
    tx.commit().await.unwrap();
    assert_eq!(structural_state(&store, item_id).await.2, "21");
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

async fn seed_definition(store: &PgStore, key: &str, category: &str, data: &str) {
    sqlx::query("INSERT INTO item_definitions (key, category, stackable, active, definition_version, rarity, stack_limit, data) VALUES ($1, $2, FALSE, TRUE, 1, 'COMMON', NULL, $3::jsonb)")
        .bind(key)
        .bind(category)
        .bind(data)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO item_definition_versions (key, version, category, stackable, rarity, stack_limit, is_ordinary_equipment, data) VALUES ($1, 1, $2, FALSE, 'COMMON', NULL, TRUE, $3::jsonb)")
        .bind(key)
        .bind(category)
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
    sqlx::query("INSERT INTO operations (id, external_request_key, actor_discord_user_id, player_id, kind, state, policy_version, request_hash, rng_root) VALUES ($1, $2, $3, $4, 'UPGRADE_STATE_WRITER_TEST', 'PENDING', 1, $5, $6)")
        .bind(operation_id)
        .bind(format!("test:upgrade-state-writer:{nonce}:{suffix}:{operation_id}"))
        .bind(discord_user_id)
        .bind(player_id)
        .bind([157_u8; 32].as_slice())
        .bind([163_u8; 32].as_slice())
        .execute(store.pool())
        .await
        .unwrap();
    let item_id = Uuid::now_v7();
    sqlx::query("INSERT INTO item_instances (id, definition_key, owner_player_id, created_by_operation_id, location, definition_version, is_upgradeable) VALUES ($1, $2, $3, $4, 'TOOL_LOCKER', 1, TRUE)")
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
    normal_capacity: i16,
    special_capacity: i16,
) {
    sqlx::query("INSERT INTO item_instance_equipment_structural_state (item_instance_id, creation_roll_numerator, creation_roll_denominator, upgrade_level, normal_enchant_slot_capacity, special_enchant_slot_capacity) VALUES ($1, $2::NUMERIC, $3::NUMERIC, $4::NUMERIC, $5, $6)")
        .bind(item_id)
        .bind(numerator)
        .bind(denominator)
        .bind(upgrade_level)
        .bind(normal_capacity)
        .bind(special_capacity)
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

async fn structural_state(store: &PgStore, item_id: Uuid) -> (String, String, String, i16, i16) {
    sqlx::query_as("SELECT trim_scale(creation_roll_numerator)::TEXT, trim_scale(creation_roll_denominator)::TEXT, trim_scale(upgrade_level)::TEXT, normal_enchant_slot_capacity, special_enchant_slot_capacity FROM item_instance_equipment_structural_state WHERE item_instance_id = $1")
        .bind(item_id)
        .fetch_one(store.pool())
        .await
        .unwrap()
}

async fn structural_state_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    item_id: Uuid,
) -> (String, String, String, i16, i16) {
    sqlx::query_as("SELECT trim_scale(creation_roll_numerator)::TEXT, trim_scale(creation_roll_denominator)::TEXT, trim_scale(upgrade_level)::TEXT, normal_enchant_slot_capacity, special_enchant_slot_capacity FROM item_instance_equipment_structural_state WHERE item_instance_id = $1")
        .bind(item_id)
        .fetch_one(&mut **tx)
        .await
        .unwrap()
}

async fn enchant_rows(store: &PgStore, item_id: Uuid) -> Vec<(String, i16)> {
    sqlx::query_as("SELECT enchant_key, level FROM item_instance_embedded_enchants WHERE item_instance_id = $1 ORDER BY enchant_key")
        .bind(item_id)
        .fetch_all(store.pool())
        .await
        .unwrap()
}

fn positive_snowflake(nonce: Uuid) -> i64 {
    let raw = u64::from_be_bytes(nonce.as_bytes()[..8].try_into().unwrap());
    i64::try_from((raw % 8_000_000_000_000_000_000_u64).max(1)).unwrap()
}
