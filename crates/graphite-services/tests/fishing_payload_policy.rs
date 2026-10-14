use std::collections::HashSet;

use graphite_services::{
    FishingBait, FishingPayload, FishingPayloadPolicyError, FishingTreasureResult,
    fishing_bait_item_definition_key, fishing_junk_payloads, fishing_non_book_treasure_payloads,
    fishing_payload_policy,
};

#[test]
fn every_non_book_payload_table_is_exact_and_normalized() {
    assert_table(
        fishing_junk_payloads(),
        &[
            ("fishing.junk.driftwood", 1, 40),
            ("fishing.junk.rusted_can", 1, 30),
            ("fishing.junk.torn_net", 1, 20),
            ("fishing.junk.broken_hook", 1, 10),
        ],
    );

    for (result, expected) in [
        (
            FishingTreasureResult::MaterialBundle,
            &[
                ("fishing.material_bundle.common", 1, 65),
                ("fishing.material_bundle.refined", 1, 25),
                ("fishing.material_bundle.deep", 1, 10),
            ][..],
        ),
        (
            FishingTreasureResult::CrateOrChest,
            &[
                ("fishing.container.weathered_crate", 1, 70),
                ("fishing.container.ironbound_chest", 1, 25),
                ("fishing.container.sunken_cache", 1, 5),
            ][..],
        ),
        (
            FishingTreasureResult::OrbOrCatalystFragmentOrItem,
            &[
                ("fishing.fragment.enchant_catalyst", 1, 60),
                ("fishing.fragment.normal_slot_orb", 1, 25),
                ("fishing.fragment.special_slot_orb", 1, 15),
            ][..],
        ),
        (
            FishingTreasureResult::RareBaitOrUtilityItem,
            &[
                ("fishing.bait.school", 1, 10),
                ("fishing.bait.quality", 1, 15),
                ("fishing.bait.rare", 1, 30),
                ("fishing.bait.treasure", 1, 25),
                ("fishing.bait.sturdy", 1, 20),
            ][..],
        ),
        (
            FishingTreasureResult::RelicOrCollectible,
            &[
                ("fishing.relic.ancient_coin", 1, 50),
                ("fishing.relic.sea_glass", 1, 30),
                ("fishing.relic.fossil_shell", 1, 15),
                ("fishing.relic.abyssal_idol", 1, 5),
            ][..],
        ),
    ] {
        assert_table(
            fishing_non_book_treasure_payloads(result).unwrap(),
            expected,
        );
    }

    assert_eq!(
        fishing_non_book_treasure_payloads(FishingTreasureResult::EnchantBook),
        Err(FishingPayloadPolicyError::EnchantBookUsesDedicatedPolicy)
    );
}

#[test]
fn payload_item_keys_are_unique_and_baits_share_one_identity_map() {
    let mut keys = HashSet::new();
    let mut all_payloads = Vec::new();
    all_payloads.extend_from_slice(fishing_junk_payloads());
    for result in [
        FishingTreasureResult::MaterialBundle,
        FishingTreasureResult::CrateOrChest,
        FishingTreasureResult::OrbOrCatalystFragmentOrItem,
        FishingTreasureResult::RareBaitOrUtilityItem,
        FishingTreasureResult::RelicOrCollectible,
    ] {
        all_payloads.extend_from_slice(fishing_non_book_treasure_payloads(result).unwrap());
    }

    for payload in all_payloads {
        let policy = fishing_payload_policy(payload);
        assert!(policy.quantity > 0);
        assert!(policy.relative_weight > 0);
        assert!(keys.insert(policy.item_definition_key));
    }

    for (bait, payload) in [
        (FishingBait::School, FishingPayload::SchoolBait),
        (FishingBait::Quality, FishingPayload::QualityBait),
        (FishingBait::Rare, FishingPayload::RareBait),
        (FishingBait::Treasure, FishingPayload::TreasureBait),
        (FishingBait::Sturdy, FishingPayload::SturdyBait),
    ] {
        assert_eq!(
            fishing_bait_item_definition_key(bait),
            fishing_payload_policy(payload).item_definition_key
        );
    }
}

fn assert_table(payloads: &[FishingPayload], expected: &[(&str, u16, u16)]) {
    assert_eq!(payloads.len(), expected.len());
    let actual = payloads
        .iter()
        .map(|payload| {
            let policy = fishing_payload_policy(*payload);
            (
                policy.item_definition_key,
                policy.quantity,
                policy.relative_weight,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
    assert_eq!(
        actual
            .iter()
            .map(|(_, _, weight)| u32::from(*weight))
            .sum::<u32>(),
        100
    );
}
