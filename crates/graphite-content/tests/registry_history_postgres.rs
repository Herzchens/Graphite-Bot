use graphite_store::PgStore;
use sqlx::Row;

#[tokio::test]
async fn registry_versions_preserve_frozen_history_across_v1_v2_v3() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL is not set; skipping PostgreSQL integration test");
        return;
    };

    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate().await.unwrap();

    for (older, newer) in [(1_i32, 2_i32), (2_i32, 3_i32)] {
        let catalog_mismatch_count: i64 = sqlx::query(
            r#"
            SELECT COUNT(*) AS count
              FROM content_catalog_entries old
              FULL JOIN content_catalog_entries new
                ON new.policy_version = $2
               AND new.content_key = old.content_key
             WHERE old.policy_version = $1
               AND (
                   new.content_key IS NULL
                   OR old.display_name IS DISTINCT FROM new.display_name
                   OR old.content_kind IS DISTINCT FROM new.content_kind
                   OR old.source_class IS DISTINCT FROM new.source_class
                   OR old.metadata IS DISTINCT FROM new.metadata
               )
            "#,
        )
        .bind(older)
        .bind(newer)
        .fetch_one(store.pool())
        .await
        .unwrap()
        .try_get("count")
        .unwrap();
        assert_eq!(
            catalog_mismatch_count, 0,
            "catalog history changed from policy v{older} to v{newer}"
        );

        let price_mismatch_count: i64 = sqlx::query(
            r#"
            SELECT COUNT(*) AS count
              FROM npc_price_entries old
              FULL JOIN npc_price_entries new
                ON new.policy_version = $2
               AND new.content_key = old.content_key
             WHERE old.policy_version = $1
               AND (
                   new.content_key IS NULL
                   OR old.appraisal_mode IS DISTINCT FROM new.appraisal_mode
                   OR old.canonical_appraisal IS DISTINCT FROM new.canonical_appraisal
                   OR old.npc_buy_price IS DISTINCT FROM new.npc_buy_price
                   OR old.npc_liquidation_allowed IS DISTINCT FROM new.npc_liquidation_allowed
                   OR old.shop_sell_price IS DISTINCT FROM new.shop_sell_price
                   OR old.normal_shop_allowed IS DISTINCT FROM new.normal_shop_allowed
                   OR old.shop_stock_policy IS DISTINCT FROM new.shop_stock_policy
                   OR old.shop_class IS DISTINCT FROM new.shop_class
               )
            "#,
        )
        .bind(older)
        .bind(newer)
        .fetch_one(store.pool())
        .await
        .unwrap()
        .try_get("count")
        .unwrap();
        assert_eq!(
            price_mismatch_count, 0,
            "price history changed from policy v{older} to v{newer}"
        );
    }

    let copied_recipe_mismatch_count: i64 = sqlx::query(
        r#"
        SELECT COUNT(*) AS count
          FROM content_recipes v2
          LEFT JOIN content_recipes v3
            ON v3.policy_version = 3
           AND v3.recipe_key = v2.recipe_key
         WHERE v2.policy_version = 2
           AND (
               v3.recipe_key IS NULL
               OR v2.recipe_kind IS DISTINCT FROM v3.recipe_kind
               OR v2.output_content_key IS DISTINCT FROM v3.output_content_key
               OR v2.output_quantity IS DISTINCT FROM v3.output_quantity
               OR v2.metadata IS DISTINCT FROM v3.metadata
           )
        "#,
    )
    .fetch_one(store.pool())
    .await
    .unwrap()
    .try_get("count")
    .unwrap();
    assert_eq!(copied_recipe_mismatch_count, 0);

    let copied_input_mismatch_count: i64 = sqlx::query(
        r#"
        SELECT COUNT(*) AS count
          FROM content_recipe_inputs v2
          LEFT JOIN content_recipe_inputs v3
            ON v3.policy_version = 3
           AND v3.recipe_key = v2.recipe_key
           AND v3.sequence = v2.sequence
         WHERE v2.policy_version = 2
           AND (
               v3.recipe_key IS NULL
               OR v2.content_key IS DISTINCT FROM v3.content_key
               OR v2.quantity IS DISTINCT FROM v3.quantity
           )
        "#,
    )
    .fetch_one(store.pool())
    .await
    .unwrap()
    .try_get("count")
    .unwrap();
    assert_eq!(copied_input_mismatch_count, 0);

    let v2_recipe_count: i64 =
        sqlx::query("SELECT COUNT(*) AS count FROM content_recipes WHERE policy_version = 2")
            .fetch_one(store.pool())
            .await
            .unwrap()
            .try_get("count")
            .unwrap();
    let v3_recipe_count: i64 =
        sqlx::query("SELECT COUNT(*) AS count FROM content_recipes WHERE policy_version = 3")
            .fetch_one(store.pool())
            .await
            .unwrap()
            .try_get("count")
            .unwrap();
    assert_eq!(v2_recipe_count, 18);
    assert_eq!(v3_recipe_count, v2_recipe_count + 4);

    let v2_input_count: i64 =
        sqlx::query("SELECT COUNT(*) AS count FROM content_recipe_inputs WHERE policy_version = 2")
            .fetch_one(store.pool())
            .await
            .unwrap()
            .try_get("count")
            .unwrap();
    let v3_input_count: i64 =
        sqlx::query("SELECT COUNT(*) AS count FROM content_recipe_inputs WHERE policy_version = 3")
            .fetch_one(store.pool())
            .await
            .unwrap()
            .try_get("count")
            .unwrap();
    assert_eq!(v3_input_count, v2_input_count + 9);
}
