use graphite_services::{
    ALL_MINING_DEPTHS, EquipmentTier, MiningCapabilityClass, MiningCapabilityError,
    mining_natural_roll_drops, mining_pickaxe_allows_resource, mining_pickaxe_max_capability,
    mining_resource_capability,
};
use graphite_store::PgStore;

#[test]
fn every_active_natural_roll_resource_has_a_frozen_capability_class() {
    for depth in ALL_MINING_DEPTHS {
        for drop in mining_natural_roll_drops(depth) {
            let capability = mining_resource_capability(drop.content_key)
                .unwrap_or_else(|error| panic!("{depth:?} {}: {error}", drop.content_key));

            assert_eq!(
                mining_pickaxe_allows_resource(EquipmentTier::Graphite, drop.content_key),
                Ok(true),
                "Graphite C6 should cover every current natural-roll resource: {depth:?} {} ({capability:?})",
                drop.content_key
            );
        }
    }
}

#[test]
fn gold_boundary_matches_the_frozen_c3_side_grade_contract_across_active_tables() {
    for depth in ALL_MINING_DEPTHS {
        for drop in mining_natural_roll_drops(depth) {
            let capability = mining_resource_capability(drop.content_key).unwrap();
            let expected = capability <= MiningCapabilityClass::C3;
            assert_eq!(
                mining_pickaxe_allows_resource(EquipmentTier::Gold, drop.content_key),
                Ok(expected),
                "Gold capability mismatch at {depth:?} for {} ({capability:?})",
                drop.content_key
            );
        }
    }

    assert_eq!(
        mining_pickaxe_allows_resource(EquipmentTier::Gold, "resource.gem.sapphire"),
        Ok(true)
    );
    assert_eq!(
        mining_pickaxe_allows_resource(EquipmentTier::Gold, "resource.gem.diamond"),
        Ok(false)
    );
    assert_eq!(
        mining_pickaxe_allows_resource(EquipmentTier::Gold, "resource.ancient_debris"),
        Ok(false)
    );
    assert_eq!(
        mining_pickaxe_allows_resource(EquipmentTier::Gold, "resource.ore.platinum"),
        Ok(false)
    );
}

#[test]
fn ordinary_pickaxe_progression_stops_at_the_expected_capability_walls() {
    assert_eq!(
        mining_pickaxe_max_capability(EquipmentTier::Wood),
        Ok(MiningCapabilityClass::C0)
    );
    assert_eq!(
        mining_pickaxe_allows_resource(EquipmentTier::Wood, "resource.stone"),
        Ok(true)
    );
    assert_eq!(
        mining_pickaxe_allows_resource(EquipmentTier::Wood, "resource.coal"),
        Ok(false)
    );

    assert_eq!(
        mining_pickaxe_allows_resource(EquipmentTier::Stone, "resource.coal"),
        Ok(true)
    );
    assert_eq!(
        mining_pickaxe_allows_resource(EquipmentTier::Stone, "resource.ore.iron"),
        Ok(false)
    );

    assert_eq!(
        mining_pickaxe_allows_resource(EquipmentTier::Iron, "resource.gem.diamond"),
        Ok(true)
    );
    assert_eq!(
        mining_pickaxe_allows_resource(EquipmentTier::Iron, "resource.obsidian"),
        Ok(false)
    );

    assert_eq!(
        mining_pickaxe_allows_resource(EquipmentTier::Diamond, "resource.obsidian"),
        Ok(true)
    );
    assert_eq!(
        mining_pickaxe_allows_resource(EquipmentTier::Diamond, "resource.ore.titanium"),
        Ok(false)
    );

    assert_eq!(
        mining_pickaxe_allows_resource(EquipmentTier::Obsidian, "resource.ore.platinum"),
        Ok(true)
    );
}

#[test]
fn emerald_is_reserved_c3_but_has_no_active_natural_roll_source() {
    assert_eq!(
        mining_resource_capability("resource.gem.emerald"),
        Ok(MiningCapabilityClass::C3)
    );
    for depth in ALL_MINING_DEPTHS {
        assert!(
            mining_natural_roll_drops(depth)
                .iter()
                .all(|drop| drop.content_key != "resource.gem.emerald"),
            "Emerald must remain source-reserved at {depth:?}"
        );
    }
}

#[test]
fn invalid_tier_and_unknown_resource_fail_closed() {
    assert_eq!(
        mining_pickaxe_max_capability(EquipmentTier::StarterLeather),
        Err(MiningCapabilityError::UnsupportedPickaxeTier(
            EquipmentTier::StarterLeather
        ))
    );
    assert_eq!(
        mining_resource_capability("resource.future.unfrozen"),
        Err(MiningCapabilityError::UnknownMiningResource)
    );
    assert_eq!(
        mining_pickaxe_allows_resource(EquipmentTier::Graphite, "resource.future.unfrozen"),
        Err(MiningCapabilityError::UnknownMiningResource)
    );
}

#[tokio::test]
async fn legacy_starter_pickaxe_capability_metadata_agrees_with_frozen_policy() {
    let Some(store) = test_store().await else {
        return;
    };
    let data: serde_json::Value = sqlx::query_scalar(
        "SELECT data FROM item_definition_versions WHERE key = 'equipment.pickaxe.wood.starter' AND version = 1",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();

    assert_eq!(
        data.get("capability").and_then(serde_json::Value::as_str),
        Some("C0")
    );
    assert_eq!(
        mining_pickaxe_max_capability(EquipmentTier::Wood),
        Ok(MiningCapabilityClass::C0)
    );
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
