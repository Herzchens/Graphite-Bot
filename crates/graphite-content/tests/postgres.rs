use graphite_content::{AppraisalMode, ContentRegistryService, ShopStockPolicy};
use graphite_store::PgStore;
use sqlx::Row;

#[tokio::test]
async fn frozen_price_registry_matches_authoritative_lattice() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL is not set; skipping PostgreSQL integration test");
        return;
    };

    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate().await.unwrap();
    let registry = ContentRegistryService::new(store.clone());

    let policy = registry.active_policy().await.unwrap();
    assert_eq!(policy.version, 1);

    let prices = registry.all_prices().await.unwrap();
    assert_eq!(prices.len(), 57);

    let shop = registry.shop_catalog().await.unwrap();
    assert_eq!(shop.len(), 33);
    assert!(shop.iter().all(|entry| entry.normal_shop_allowed));

    let tin = registry.price("resource.ingot.tin").await.unwrap().unwrap();
    assert_eq!(tin.appraisal_mode, AppraisalMode::Fixed);
    assert_eq!(tin.canonical_appraisal, Some(104));
    assert_eq!(tin.npc_buy_price, Some(104));
    assert_eq!(tin.shop_sell_price, Some(340));
    assert_eq!(tin.shop_stock_policy, ShopStockPolicy::WideOrPerUser);

    let lead = registry.price("resource.ore.lead").await.unwrap().unwrap();
    assert_eq!(lead.npc_buy_price, Some(240));
    assert_eq!(lead.shop_sell_price, Some(780));
    assert_eq!(lead.shop_stock_policy, ShopStockPolicy::WeeklyLimited);

    let graphite_layer = registry
        .price("material.graphite_layer")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(graphite_layer.canonical_appraisal, Some(103_538));
    assert_eq!(graphite_layer.npc_buy_price, None);
    assert!(!graphite_layer.npc_liquidation_allowed);
    assert_eq!(graphite_layer.shop_sell_price, None);
    assert_eq!(graphite_layer.shop_stock_policy, ShopStockPolicy::NotSold);

    let bronze = registry.price("alloy.ingot.bronze").await.unwrap().unwrap();
    assert_eq!(bronze.appraisal_mode, AppraisalMode::DerivedInput);
    assert_eq!(bronze.canonical_appraisal, None);
    assert_eq!(bronze.npc_buy_price, None);
    assert_eq!(bronze.shop_sell_price, Some(450));
    assert_eq!(bronze.shop_stock_policy, ShopStockPolicy::WeeklyLimited);

    let bronze_recipe = registry.recipe("alloy.bronze").await.unwrap().unwrap();
    assert_eq!(bronze_recipe.output_content_key, "alloy.ingot.bronze");
    assert_eq!(bronze_recipe.output_quantity, 4);
    assert_eq!(bronze_recipe.inputs.len(), 2);
    assert_eq!(bronze_recipe.inputs[0].content_key, "resource.ingot.copper");
    assert_eq!(bronze_recipe.inputs[0].quantity, 3);
    assert_eq!(bronze_recipe.inputs[1].content_key, "resource.ingot.tin");
    assert_eq!(bronze_recipe.inputs[1].quantity, 1);

    let arbitrage_count: i64 = sqlx::query(
        r#"
        SELECT COUNT(*) AS count
          FROM npc_price_entries
         WHERE npc_buy_price IS NOT NULL
           AND shop_sell_price IS NOT NULL
           AND npc_buy_price >= shop_sell_price
        "#,
    )
    .fetch_one(store.pool())
    .await
    .unwrap()
    .try_get("count")
    .unwrap();
    assert_eq!(arbitrage_count, 0);

    let forbidden_normal_shop_count: i64 = sqlx::query(
        r#"
        SELECT COUNT(*) AS count
          FROM npc_price_entries
         WHERE content_key IN (
            'resource.gem.diamond',
            'resource.obsidian',
            'resource.ore.cobalt',
            'resource.ancient_debris',
            'resource.ore.titanium',
            'resource.ore.tungsten',
            'resource.ore.platinum',
            'resource.gem.blood_diamond'
         )
           AND normal_shop_allowed = TRUE
        "#,
    )
    .fetch_one(store.pool())
    .await
    .unwrap()
    .try_get("count")
    .unwrap();
    assert_eq!(forbidden_normal_shop_count, 0);

    let mutation = sqlx::query(
        "UPDATE npc_price_entries SET shop_sell_price = 1 WHERE policy_version = 1 AND content_key = 'resource.coal'",
    )
    .execute(store.pool())
    .await;
    assert!(
        mutation.is_err(),
        "frozen registry rows must reject mutation"
    );
}
