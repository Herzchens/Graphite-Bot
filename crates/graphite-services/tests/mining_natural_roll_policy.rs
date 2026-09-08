use graphite_services::{
    ALL_MINING_DEPTHS, MINING_NATURAL_ROLL_WEIGHT_SCALE, MiningDepth, mining_natural_roll_drops,
};
use graphite_store::PgStore;
use std::collections::HashSet;

const FNV_OFFSET_BASIS: u64 = 14_695_981_039_346_656_037;
const FNV_PRIME: u64 = 1_099_511_628_211;
const FROZEN_NATURAL_ROLL_TABLE_FINGERPRINT: u64 = 0x6a99_fa7f_29d9_728d;

#[test]
fn public_natural_roll_table_keeps_the_frozen_full_table_fingerprint() {
    let mut hash = FNV_OFFSET_BASIS;

    for (depth_index, depth) in ALL_MINING_DEPTHS.into_iter().enumerate() {
        hash = fnv_mix(hash, u8::try_from(depth_index).unwrap());
        let drops = mining_natural_roll_drops(depth);
        assert!(!drops.is_empty(), "{depth:?}");
        assert_eq!(
            drops.iter().map(|drop| drop.weight_units).sum::<u32>(),
            MINING_NATURAL_ROLL_WEIGHT_SCALE,
            "{depth:?}"
        );

        for drop in drops {
            for byte in drop.content_key.bytes() {
                hash = fnv_mix(hash, byte);
            }
            hash = fnv_mix(hash, 0);
            for byte in drop.weight_units.to_le_bytes() {
                hash = fnv_mix(hash, byte);
            }
        }
        hash = fnv_mix(hash, 0xff);
    }

    assert_eq!(hash, FROZEN_NATURAL_ROLL_TABLE_FINGERPRINT);
}

#[test]
fn public_table_preserves_key_depth_specific_guards() {
    let surface = mining_natural_roll_drops(MiningDepth::Surface);
    assert_eq!(weight(surface, "resource.wood.log"), Some(73_000));
    assert_eq!(weight(surface, "resource.ore.copper"), Some(300));

    let crystal = mining_natural_roll_drops(MiningDepth::CrystalDepths);
    assert_eq!(weight(crystal, "resource.gem.blood_diamond"), Some(2));

    let fringe = mining_natural_roll_drops(MiningDepth::NetherFringe);
    assert_eq!(weight(fringe, "resource.ore.platinum"), Some(1));

    for depth in ALL_MINING_DEPTHS {
        assert_eq!(
            weight(mining_natural_roll_drops(depth), "resource.gem.emerald"),
            None,
            "Emerald has no active cave source at {depth:?}"
        );
    }

    for depth in ALL_MINING_DEPTHS
        .into_iter()
        .filter(|depth| *depth != MiningDepth::ObsidianChasm)
    {
        assert_eq!(
            weight(mining_natural_roll_drops(depth), "resource.obsidian"),
            None,
            "natural Obsidian must be Obsidian Chasm-only; found at {depth:?}"
        );
    }
    assert_eq!(
        weight(
            mining_natural_roll_drops(MiningDepth::ObsidianChasm),
            "resource.obsidian"
        ),
        Some(180)
    );
}

#[tokio::test]
async fn every_natural_roll_content_key_exists_in_the_active_registry() {
    let Some(store) = test_store().await else {
        return;
    };
    let policy_version: i32 =
        sqlx::query_scalar("SELECT version FROM active_content_registry WHERE singleton = TRUE")
            .fetch_one(store.pool())
            .await
            .unwrap();

    let mut keys = HashSet::new();
    for depth in ALL_MINING_DEPTHS {
        for drop in mining_natural_roll_drops(depth) {
            keys.insert(drop.content_key);
        }
    }

    for key in keys {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM content_catalog_entries WHERE policy_version = $1 AND content_key = $2)",
        )
        .bind(policy_version)
        .bind(key)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert!(exists, "missing active content-registry key: {key}");
    }
}

fn weight(drops: &[graphite_services::MiningNaturalRollDrop], content_key: &str) -> Option<u32> {
    drops
        .iter()
        .find(|drop| drop.content_key == content_key)
        .map(|drop| drop.weight_units)
}

fn fnv_mix(hash: u64, byte: u8) -> u64 {
    (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME)
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
