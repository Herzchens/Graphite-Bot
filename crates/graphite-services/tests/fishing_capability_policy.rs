use graphite_services::{
    EquipmentTier, FishingCapabilityClassification, FishingCapabilityError, FishingRarity,
    FishingRodBaseStats, NORMAL_ROD_DURABILITY_PER_COMPLETED_CAST_ATTEMPT, STRENGTHEN_MAX_LEVEL,
    fishing_catch_load, fishing_rarity_tension_multiplier, fishing_tension,
    manual_fishing_capability_ratio, manual_fishing_line_strength, ordinary_fishing_rod_base_stats,
};

#[test]
fn public_api_preserves_every_ordinary_rod_base_row() {
    let expected = [
        (EquipmentTier::Wood, 6_000, 600, false),
        (EquipmentTier::Stone, 10_000, 900, false),
        (EquipmentTier::Copper, 18_000, 1_400, false),
        (EquipmentTier::Gold, 40_000, 550, true),
        (EquipmentTier::Iron, 30_000, 2_200, false),
        (EquipmentTier::Diamond, 55_000, 3_300, false),
        (EquipmentTier::Obsidian, 85_000, 5_000, false),
        (EquipmentTier::Netherite, 120_000, 7_600, false),
        (EquipmentTier::Graphite, 160_000, 11_000, false),
    ];

    for (tier, line_strength, durability, gold_side_grade) in expected {
        assert_eq!(
            ordinary_fishing_rod_base_stats(tier, true),
            Ok(FishingRodBaseStats {
                tier,
                base_line_strength_grams_tension: line_strength,
                base_durability: durability,
                gold_side_grade,
            })
        );
    }
}

#[test]
fn public_api_requires_authoritative_ordinary_rod_classification() {
    assert_eq!(
        ordinary_fishing_rod_base_stats(EquipmentTier::Wood, false),
        Err(FishingCapabilityError::NotOrdinaryFishingRod)
    );
    assert_eq!(
        manual_fishing_line_strength(EquipmentTier::Wood, false, None, false),
        Err(FishingCapabilityError::NotOrdinaryFishingRod)
    );
}

#[test]
fn public_api_rejects_non_rod_starter_leather_tier() {
    assert_eq!(
        ordinary_fishing_rod_base_stats(EquipmentTier::StarterLeather, true),
        Err(FishingCapabilityError::StarterLeatherIsNotOrdinaryRodTier)
    );
}

#[test]
fn public_api_preserves_exact_rarity_tension_multipliers() {
    let expected = [
        (FishingRarity::Common, 1, 1),
        (FishingRarity::Uncommon, 11, 10),
        (FishingRarity::Rare, 5, 4),
        (FishingRarity::Epic, 29, 20),
        (FishingRarity::Legendary, 7, 4),
        (FishingRarity::Mythic, 11, 5),
    ];

    for (rarity, numerator, denominator) in expected {
        let multiplier = fishing_rarity_tension_multiplier(rarity);
        assert_eq!(multiplier.numerator(), numerator);
        assert_eq!(multiplier.denominator(), denominator);
    }
}

#[test]
fn public_api_computes_exact_reduced_fish_tension_without_rounding() {
    let epic = fishing_tension(1_000, FishingRarity::Epic).unwrap();
    assert_eq!(epic.source_weight_grams, 1_000);
    assert_eq!(epic.rarity, FishingRarity::Epic);
    assert_eq!(epic.numerator_gram_tension(), 1_450);
    assert_eq!(epic.denominator(), 1);

    let uncommon = fishing_tension(1, FishingRarity::Uncommon).unwrap();
    assert_eq!(uncommon.numerator_gram_tension(), 11);
    assert_eq!(uncommon.denominator(), 10);
}

#[test]
fn public_api_sums_candidate_fish_load_exactly_and_enforces_global_cap() {
    let tensions = [
        fishing_tension(1, FishingRarity::Uncommon).unwrap(),
        fishing_tension(1, FishingRarity::Epic).unwrap(),
    ];
    let load = fishing_catch_load(&tensions).unwrap();
    assert_eq!(load.fish_count, 2);
    assert_eq!(load.numerator_gram_tension(), 51);
    assert_eq!(load.denominator(), 20);

    assert_eq!(
        fishing_catch_load(&[]),
        Err(FishingCapabilityError::FishCountOutOfRange(0))
    );

    let common = fishing_tension(1, FishingRarity::Common).unwrap();
    assert_eq!(
        fishing_catch_load(&[common; 6]),
        Err(FishingCapabilityError::FishCountOutOfRange(6))
    );
}

#[test]
fn public_api_composes_manual_strengthen_and_sturdy_factors_exactly() {
    assert_eq!(STRENGTHEN_MAX_LEVEL, 10);

    let base = manual_fishing_line_strength(EquipmentTier::Wood, true, None, false).unwrap();
    assert_eq!(base.numerator_gram_tension(), 6_000);
    assert_eq!(base.denominator(), 1);

    let strengthen_x =
        manual_fishing_line_strength(EquipmentTier::Wood, true, Some(10), false).unwrap();
    assert_eq!(strengthen_x.numerator_gram_tension(), 8_400);
    assert_eq!(strengthen_x.denominator(), 1);

    let strengthen_x_sturdy =
        manual_fishing_line_strength(EquipmentTier::Wood, true, Some(10), true).unwrap();
    assert_eq!(strengthen_x_sturdy.numerator_gram_tension(), 9_240);
    assert_eq!(strengthen_x_sturdy.denominator(), 1);

    for level in [0, 11, u8::MAX] {
        assert_eq!(
            manual_fishing_line_strength(EquipmentTier::Wood, true, Some(level), false),
            Err(FishingCapabilityError::StrengthenLevelOutOfRange(level))
        );
    }
}

#[test]
fn public_api_classifies_exact_manual_capability_boundary_without_float() {
    let mythic = [fishing_tension(3_000, FishingRarity::Mythic).unwrap()];
    let load = fishing_catch_load(&mythic).unwrap();

    let base = manual_fishing_line_strength(EquipmentTier::Wood, true, None, false).unwrap();
    let over_cap = manual_fishing_capability_ratio(load, base).unwrap();
    assert_eq!(over_cap.numerator(), 11);
    assert_eq!(over_cap.denominator(), 10);
    assert_eq!(
        over_cap.classification,
        FishingCapabilityClassification::OverRodCapability
    );

    let sturdy = manual_fishing_line_strength(EquipmentTier::Wood, true, None, true).unwrap();
    let boundary = manual_fishing_capability_ratio(load, sturdy).unwrap();
    assert_eq!(boundary.numerator(), 1);
    assert_eq!(boundary.denominator(), 1);
    assert_eq!(
        boundary.classification,
        FishingCapabilityClassification::WithinRodCapability
    );
}

#[test]
fn public_api_records_one_normal_durability_event_per_completed_cast_attempt() {
    assert_eq!(NORMAL_ROD_DURABILITY_PER_COMPLETED_CAST_ATTEMPT, 1);
}
