use graphite_services::{
    FishingTreasureResult, fishing_junk_payloads, fishing_non_book_treasure_payloads,
    fishing_payload_policy,
};
use graphite_store::PgStore;
use sqlx::Row;

#[tokio::test]
async fn fishing_payload_item_definitions_match_the_frozen_policy() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL is not set; skipping PostgreSQL integration test");
        return;
    };

    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate().await.unwrap();

    let mut payloads = Vec::new();
    payloads.extend_from_slice(fishing_junk_payloads());
    for result in [
        FishingTreasureResult::MaterialBundle,
        FishingTreasureResult::CrateOrChest,
        FishingTreasureResult::OrbOrCatalystFragmentOrItem,
        FishingTreasureResult::RareBaitOrUtilityItem,
        FishingTreasureResult::RelicOrCollectible,
    ] {
        payloads.extend_from_slice(fishing_non_book_treasure_payloads(result).unwrap());
    }

    for payload in payloads {
        let policy = fishing_payload_policy(payload);
        let row = sqlx::query(
            r#"
            SELECT d.active,
                   d.definition_version,
                   d.stackable,
                   d.stack_limit,
                   v.version,
                   v.category,
                   v.rarity,
                   v.stackable AS version_stackable,
                   v.stack_limit AS version_stack_limit
              FROM item_definitions d
              JOIN item_definition_versions v
                ON v.key = d.key
               AND v.version = d.definition_version
             WHERE d.key = $1
            "#,
        )
        .bind(policy.item_definition_key)
        .fetch_one(store.pool())
        .await
        .unwrap();

        assert!(row.try_get::<bool, _>("active").unwrap());
        assert_eq!(row.try_get::<i32, _>("definition_version").unwrap(), 1);
        assert_eq!(row.try_get::<i32, _>("version").unwrap(), 1);
        assert!(row.try_get::<bool, _>("stackable").unwrap());
        assert!(row.try_get::<bool, _>("version_stackable").unwrap());
        assert!(row.try_get::<i64, _>("stack_limit").unwrap() > 0);
        assert!(row.try_get::<i64, _>("version_stack_limit").unwrap() > 0);
        assert!(!row.try_get::<String, _>("category").unwrap().is_empty());
        assert!(!row.try_get::<String, _>("rarity").unwrap().is_empty());
    }
}
