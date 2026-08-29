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
    assert_eq!(policy.version, 3);

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

    let expected_smelt_recipes = [
        ("smelt.tin", "resource.ore.tin", "resource.ingot.tin"),
        (
            "smelt.copper",
            "resource.ore.copper",
            "resource.ingot.copper",
        ),
        ("smelt.zinc", "resource.ore.zinc", "resource.ingot.zinc"),
        (
            "smelt.aluminum",
            "resource.bauxite",
            "resource.ingot.aluminum",
        ),
        ("smelt.iron", "resource.ore.iron", "resource.ingot.iron"),
        ("smelt.lead", "resource.ore.lead", "resource.ingot.lead"),
        (
            "smelt.silver",
            "resource.ore.silver",
            "resource.ingot.silver",
        ),
        (
            "smelt.nickel",
            "resource.ore.nickel",
            "resource.ingot.nickel",
        ),
        ("smelt.gold", "resource.ore.gold", "resource.ingot.gold"),
        (
            "smelt.cobalt",
            "resource.ore.cobalt",
            "resource.ingot.cobalt",
        ),
        (
            "smelt.titanium",
            "resource.ore.titanium",
            "resource.ingot.titanium",
        ),
        (
            "smelt.tungsten",
            "resource.ore.tungsten",
            "resource.ingot.tungsten",
        ),
        (
            "smelt.netherite-scrap",
            "resource.ancient_debris",
            "resource.netherite_scrap",
        ),
        (
            "smelt.platinum",
            "resource.ore.platinum",
            "resource.ingot.platinum",
        ),
    ];

    for (recipe_key, input_key, output_key) in expected_smelt_recipes {
        let recipe = registry.recipe(recipe_key).await.unwrap().unwrap();
        assert_eq!(recipe.recipe_kind, "SMELT", "recipe {recipe_key}");
        assert_eq!(recipe.output_content_key, output_key, "recipe {recipe_key}");
        assert_eq!(recipe.output_quantity, 1, "recipe {recipe_key}");
        assert_eq!(recipe.inputs.len(), 1, "recipe {recipe_key}");
        assert_eq!(
            recipe.inputs[0].content_key, input_key,
            "recipe {recipe_key}"
        );
        assert_eq!(recipe.inputs[0].quantity, 1, "recipe {recipe_key}");
        assert_eq!(
            recipe.metadata,
            serde_json::json!({}),
            "global ordinary-Smelting policy must not be duplicated into recipe metadata for {recipe_key}"
        );
    }

    let expected_forge_recipes = [
        (
            "forge.netherite-billet",
            "material.netherite_billet",
            vec![("resource.netherite_scrap", 4), ("resource.ingot.gold", 4)],
        ),
        (
            "forge.graphite-precursor",
            "material.graphitic_precursor",
            vec![
                ("resource.ingot.titanium", 1),
                ("resource.ingot.tungsten", 1),
                ("resource.gem.onyx", 2),
                ("resource.gem.diamond", 2),
                ("resource.coal", 16),
            ],
        ),
        (
            "forge.graphite-layer",
            "material.graphite_layer",
            vec![("material.graphitic_precursor", 1)],
        ),
        (
            "forge.graphite-billet",
            "material.graphite_billet",
            vec![("material.graphite_layer", 20)],
        ),
    ];

    for (recipe_key, output_key, expected_inputs) in expected_forge_recipes {
        let recipe = registry.recipe(recipe_key).await.unwrap().unwrap();
        assert_eq!(recipe.recipe_kind, "FORGE", "recipe {recipe_key}");
        assert_eq!(recipe.output_content_key, output_key, "recipe {recipe_key}");
        assert_eq!(recipe.output_quantity, 1, "recipe {recipe_key}");
        assert_eq!(
            recipe.inputs.len(),
            expected_inputs.len(),
            "recipe {recipe_key}"
        );
        for (input, (expected_key, expected_quantity)) in recipe.inputs.iter().zip(expected_inputs)
        {
            assert_eq!(input.content_key, expected_key, "recipe {recipe_key}");
            assert_eq!(input.quantity, expected_quantity, "recipe {recipe_key}");
        }
        assert_eq!(
            recipe.metadata,
            serde_json::json!({}),
            "advanced Forge economic/runtime policy must not be duplicated into recipe metadata for {recipe_key}"
        );
    }

    let version_one_catalog_count: i64 = sqlx::query(
        "SELECT COUNT(*) AS count FROM content_catalog_entries WHERE policy_version = 1",
    )
    .fetch_one(store.pool())
    .await
    .unwrap()
    .try_get("count")
    .unwrap();
    let version_two_catalog_count: i64 = sqlx::query(
        "SELECT COUNT(*) AS count FROM content_catalog_entries WHERE policy_version = 2",
    )
    .fetch_one(store.pool())
    .await
    .unwrap()
    .try_get("count")
    .unwrap();
    let version_three_catalog_count: i64 = sqlx::query(
        "SELECT COUNT(*) AS count FROM content_catalog_entries WHERE policy_version = 3",
    )
    .fetch_one(store.pool())
    .await
    .unwrap()
    .try_get("count")
    .unwrap();
    assert_eq!(version_one_catalog_count, 57);
    assert_eq!(version_two_catalog_count, 57);
    assert_eq!(version_three_catalog_count, 57);

    let copied_catalog_mismatch_count: i64 = sqlx::query(
        r#"
        SELECT COUNT(*) AS count
          FROM content_catalog_entries v2
          FULL JOIN content_catalog_entries v3
            ON v3.policy_version = 3
           AND v3.content_key = v2.content_key
         WHERE v2.policy_version = 2
           AND (
               v3.content_key IS NULL
               OR v2.display_name IS DISTINCT FROM v3.display_name
               OR v2.content_kind IS DISTINCT FROM v3.content_kind
               OR v2.source_class IS DISTINCT FROM v3.source_class
               OR v2.metadata IS DISTINCT FROM v3.metadata
           )
        "#,
    )
    .fetch_one(store.pool())
    .await
    .unwrap()
    .try_get("count")
    .unwrap();
    assert_eq!(copied_catalog_mismatch_count, 0);

    let version_one_price_count: i64 =
        sqlx::query("SELECT COUNT(*) AS count FROM npc_price_entries WHERE policy_version = 1")
            .fetch_one(store.pool())
            .await
            .unwrap()
            .try_get("count")
            .unwrap();
    let version_two_price_count: i64 =
        sqlx::query("SELECT COUNT(*) AS count FROM npc_price_entries WHERE policy_version = 2")
            .fetch_one(store.pool())
            .await
            .unwrap()
            .try_get("count")
            .unwrap();
    let version_three_price_count: i64 =
        sqlx::query("SELECT COUNT(*) AS count FROM npc_price_entries WHERE policy_version = 3")
            .fetch_one(store.pool())
            .await
            .unwrap()
            .try_get("count")
            .unwrap();
    assert_eq!(version_one_price_count, 57);
    assert_eq!(version_two_price_count, 57);
    assert_eq!(version_three_price_count, 57);

    let copied_price_mismatch_count: i64 = sqlx::query(
        r#"
        SELECT COUNT(*) AS count
          FROM npc_price_entries v2
          FULL JOIN npc_price_entries v3
            ON v3.policy_version = 3
           AND v3.content_key = v2.content_key
         WHERE v2.policy_version = 2
           AND (
               v3.content_key IS NULL
               OR v2.appraisal_mode IS DISTINCT FROM v3.appraisal_mode
               OR v2.canonical_appraisal IS DISTINCT FROM v3.canonical_appraisal
               OR v2.npc_buy_price IS DISTINCT FROM v3.npc_buy_price
               OR v2.npc_liquidation_allowed IS DISTINCT FROM v3.npc_liquidation_allowed
               OR v2.shop_sell_price IS DISTINCT FROM v3.shop_sell_price
               OR v2.normal_shop_allowed IS DISTINCT FROM v3.normal_shop_allowed
               OR v2.shop_stock_policy IS DISTINCT FROM v3.shop_stock_policy
               OR v2.shop_class IS DISTINCT FROM v3.shop_class
           )
        "#,
    )
    .fetch_one(store.pool())
    .await
    .unwrap()
    .try_get("count")
    .unwrap();
    assert_eq!(copied_price_mismatch_count, 0);

    let version_one_recipe_count: i64 =
        sqlx::query("SELECT COUNT(*) AS count FROM content_recipes WHERE policy_version = 1")
            .fetch_one(store.pool())
            .await
            .unwrap()
            .try_get("count")
            .unwrap();
    let version_two_recipe_count: i64 =
        sqlx::query("SELECT COUNT(*) AS count FROM content_recipes WHERE policy_version = 2")
            .fetch_one(store.pool())
            .await
            .unwrap()
            .try_get("count")
            .unwrap();
    let version_three_recipe_count: i64 =
        sqlx::query("SELECT COUNT(*) AS count FROM content_recipes WHERE policy_version = 3")
            .fetch_one(store.pool())
            .await
            .unwrap()
            .try_get("count")
            .unwrap();
    let version_one_smelt_count: i64 = sqlx::query(
        "SELECT COUNT(*) AS count FROM content_recipes WHERE policy_version = 1 AND recipe_kind = 'SMELT'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap()
    .try_get("count")
    .unwrap();
    let version_two_smelt_count: i64 = sqlx::query(
        "SELECT COUNT(*) AS count FROM content_recipes WHERE policy_version = 2 AND recipe_kind = 'SMELT'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap()
    .try_get("count")
    .unwrap();
    let version_three_smelt_count: i64 = sqlx::query(
        "SELECT COUNT(*) AS count FROM content_recipes WHERE policy_version = 3 AND recipe_kind = 'SMELT'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap()
    .try_get("count")
    .unwrap();
    let version_three_forge_count: i64 = sqlx::query(
        "SELECT COUNT(*) AS count FROM content_recipes WHERE policy_version = 3 AND recipe_kind = 'FORGE'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap()
    .try_get("count")
    .unwrap();
    assert_eq!(version_one_recipe_count, 4);
    assert_eq!(version_two_recipe_count, 18);
    assert_eq!(version_three_recipe_count, 22);
    assert_eq!(version_one_smelt_count, 0);
    assert_eq!(version_two_smelt_count, 14);
    assert_eq!(version_three_smelt_count, 14);
    assert_eq!(version_three_forge_count, 4);

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
        "historical frozen registry rows must reject mutation"
    );

    let active_mutation = sqlx::query(
        "UPDATE npc_price_entries SET shop_sell_price = 1 WHERE policy_version = 3 AND content_key = 'resource.coal'",
    )
    .execute(store.pool())
    .await;
    assert!(
        active_mutation.is_err(),
        "active frozen registry rows must also reject mutation"
    );

    let recipe_mutation = sqlx::query(
        "UPDATE content_recipes SET output_quantity = 2 WHERE policy_version = 3 AND recipe_key = 'forge.graphite-layer'",
    )
    .execute(store.pool())
    .await;
    assert!(
        recipe_mutation.is_err(),
        "active advanced Forge recipe mappings must remain immutable"
    );
}
