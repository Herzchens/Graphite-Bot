use graphite_store::PgStore;
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
async fn ordinary_equipment_classification_is_versioned_constrained_and_immutable() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL is not set; skipping PostgreSQL integration test");
        return;
    };

    let store = PgStore::connect(&database_url).await.unwrap();
    store.migrate().await.unwrap();

    let starter_rows = sqlx::query(
        r#"
        SELECT COUNT(*) AS total,
               COUNT(*) FILTER (WHERE is_ordinary_equipment) AS ordinary
          FROM item_definition_versions
         WHERE key LIKE 'equipment.%.starter'
           AND version = 1
        "#,
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    let starter_total: i64 = starter_rows.try_get("total").unwrap();
    let starter_ordinary: i64 = starter_rows.try_get("ordinary").unwrap();
    assert_eq!(
        starter_total, 7,
        "the seeded starter loadout must stay complete"
    );
    assert_eq!(
        starter_ordinary, 0,
        "Starter definitions must fail closed instead of inheriting ordinary classification"
    );

    let nonce = Uuid::now_v7();
    for category in ["PICKAXE", "SWORD", "FISHING_ROD", "ARMOR"] {
        let ordinary_key = format!("test.ordinary.{}.{nonce}", category.to_ascii_lowercase());
        seed_definition_head(store.pool(), &ordinary_key, category, false, None).await;
        sqlx::query(
            r#"
            INSERT INTO item_definition_versions (
                key, version, category, stackable, rarity, stack_limit,
                is_ordinary_equipment, data
            )
            VALUES ($1, 1, $2, FALSE, 'COMMON', NULL, TRUE, '{}'::jsonb)
            "#,
        )
        .bind(&ordinary_key)
        .bind(category)
        .execute(store.pool())
        .await
        .unwrap();

        let ordinary: bool = sqlx::query(
            "SELECT is_ordinary_equipment FROM item_definition_versions WHERE key = $1 AND version = 1",
        )
        .bind(&ordinary_key)
        .fetch_one(store.pool())
        .await
        .unwrap()
        .try_get("is_ordinary_equipment")
        .unwrap();
        assert!(
            ordinary,
            "{category} must be able to opt into ordinary equipment"
        );
    }

    let non_equipment_key = format!("test.not-equipment.{nonce}");
    seed_definition_head(store.pool(), &non_equipment_key, "MATERIAL", false, None).await;
    let non_equipment_error = sqlx::query(
        r#"
        INSERT INTO item_definition_versions (
            key, version, category, stackable, rarity, stack_limit,
            is_ordinary_equipment, data
        )
        VALUES ($1, 1, 'MATERIAL', FALSE, 'COMMON', NULL, TRUE, '{}'::jsonb)
        "#,
    )
    .bind(&non_equipment_key)
    .execute(store.pool())
    .await
    .unwrap_err();
    assert_eq!(
        non_equipment_error
            .as_database_error()
            .and_then(|error| error.constraint()),
        Some("item_definition_versions_ordinary_equipment_shape")
    );

    let totem_key = format!("test.totem.{nonce}");
    seed_definition_head(store.pool(), &totem_key, "TOTEM", false, None).await;
    let totem_error = sqlx::query(
        r#"
        INSERT INTO item_definition_versions (
            key, version, category, stackable, rarity, stack_limit,
            is_ordinary_equipment, data
        )
        VALUES ($1, 1, 'TOTEM', FALSE, 'COMMON', NULL, TRUE, '{}'::jsonb)
        "#,
    )
    .bind(&totem_key)
    .execute(store.pool())
    .await
    .unwrap_err();
    assert_eq!(
        totem_error
            .as_database_error()
            .and_then(|error| error.constraint()),
        Some("item_definition_versions_ordinary_equipment_shape")
    );

    let stackable_equipment_key = format!("test.stackable-equipment.{nonce}");
    seed_definition_head(
        store.pool(),
        &stackable_equipment_key,
        "FISHING_ROD",
        true,
        Some(64),
    )
    .await;
    let stackable_error = sqlx::query(
        r#"
        INSERT INTO item_definition_versions (
            key, version, category, stackable, rarity, stack_limit,
            is_ordinary_equipment, data
        )
        VALUES ($1, 1, 'FISHING_ROD', TRUE, 'COMMON', 64, TRUE, '{}'::jsonb)
        "#,
    )
    .bind(&stackable_equipment_key)
    .execute(store.pool())
    .await
    .unwrap_err();
    assert_eq!(
        stackable_error
            .as_database_error()
            .and_then(|error| error.constraint()),
        Some("item_definition_versions_ordinary_equipment_shape")
    );

    let ordinary_key = format!("test.ordinary.fishing_rod.{nonce}");
    let immutable_error = sqlx::query(
        "UPDATE item_definition_versions SET is_ordinary_equipment = FALSE WHERE key = $1 AND version = 1",
    )
    .bind(&ordinary_key)
    .execute(store.pool())
    .await
    .unwrap_err();
    assert!(immutable_error.as_database_error().is_some());

    let ordinary_after_failed_update: bool = sqlx::query(
        "SELECT is_ordinary_equipment FROM item_definition_versions WHERE key = $1 AND version = 1",
    )
    .bind(&ordinary_key)
    .fetch_one(store.pool())
    .await
    .unwrap()
    .try_get("is_ordinary_equipment")
    .unwrap();
    assert!(ordinary_after_failed_update);
}

async fn seed_definition_head(
    pool: &sqlx::PgPool,
    key: &str,
    category: &str,
    stackable: bool,
    stack_limit: Option<i64>,
) {
    sqlx::query(
        r#"
        INSERT INTO item_definitions (
            key, category, stackable, definition_version, rarity, stack_limit, data
        )
        VALUES ($1, $2, $3, 1, 'COMMON', $4, '{}'::jsonb)
        "#,
    )
    .bind(key)
    .bind(category)
    .bind(stackable)
    .bind(stack_limit)
    .execute(pool)
    .await
    .unwrap();
}
