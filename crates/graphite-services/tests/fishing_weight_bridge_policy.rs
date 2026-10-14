use graphite_services::{
    EquipmentTier, FISHING_CANONICAL_WEIGHT_SCALE, FishingCapabilityClassification, FishingRarity,
    FishingSpecies, FishingVariant, FishingWeightBridgeError, bridge_canonical_fish_weight_v1,
    fishing_catch_load, fishing_catch_load_from_scaled_tensions, fishing_tension,
    fishing_tension_from_canonical_scaled_weight, manual_fishing_capability_ratio,
    manual_fishing_line_strength, resolve_fish_instance_numeric_v1,
};

#[test]
fn catch_bag_bridge_rounds_half_up_once_without_redefining_canonical_weight() {
    let scale = FISHING_CANONICAL_WEIGHT_SCALE;

    let below_half = bridge_canonical_fish_weight_v1(6_000 * scale + 400_000_000_000).unwrap();
    assert_eq!(
        below_half.canonical_weight_scaled_grams,
        6_000_400_000_000_000
    );
    assert_eq!(below_half.catch_bag_weight_grams, 6_000);

    let exact_half = bridge_canonical_fish_weight_v1(6_000 * scale + scale / 2).unwrap();
    assert_eq!(exact_half.catch_bag_weight_grams, 6_001);
}

#[test]
fn scaled_weight_remains_gameplay_authority_even_when_catch_bag_rounding_hides_the_fraction() {
    let scale = FISHING_CANONICAL_WEIGHT_SCALE;
    let canonical_weight = 6_000 * scale + 400_000_000_000;

    let scaled_tension =
        fishing_tension_from_canonical_scaled_weight(canonical_weight, FishingRarity::Common)
            .unwrap();
    let scaled_load = fishing_catch_load_from_scaled_tensions(&[scaled_tension]).unwrap();
    let wood = manual_fishing_line_strength(EquipmentTier::Wood, true, None, false).unwrap();
    let scaled_ratio = manual_fishing_capability_ratio(scaled_load, wood).unwrap();
    assert_eq!(
        scaled_ratio.classification,
        FishingCapabilityClassification::OverRodCapability
    );
    assert_eq!(
        (scaled_ratio.numerator(), scaled_ratio.denominator()),
        (15_001, 15_000)
    );

    let bridged = bridge_canonical_fish_weight_v1(canonical_weight).unwrap();
    assert_eq!(bridged.catch_bag_weight_grams, 6_000);

    let rounded_tension =
        fishing_tension(bridged.catch_bag_weight_grams, FishingRarity::Common).unwrap();
    let rounded_load = fishing_catch_load(&[rounded_tension]).unwrap();
    let rounded_ratio = manual_fishing_capability_ratio(rounded_load, wood).unwrap();
    assert_eq!(
        rounded_ratio.classification,
        FishingCapabilityClassification::WithinRodCapability
    );
}

#[test]
fn exact_whole_gram_inputs_match_the_legacy_whole_gram_capability_path() {
    let scale = FISHING_CANONICAL_WEIGHT_SCALE;
    let scaled =
        fishing_tension_from_canonical_scaled_weight(6_600 * scale, FishingRarity::Epic).unwrap();
    let scaled_load = fishing_catch_load_from_scaled_tensions(&[scaled]).unwrap();

    let legacy = fishing_tension(6_600, FishingRarity::Epic).unwrap();
    let legacy_load = fishing_catch_load(&[legacy]).unwrap();

    assert_eq!(
        (
            scaled_load.numerator_gram_tension(),
            scaled_load.denominator()
        ),
        (
            legacy_load.numerator_gram_tension(),
            legacy_load.denominator()
        )
    );
}

#[test]
fn numeric_fish_instance_exposes_the_same_owner_approved_bridge() {
    let fish = resolve_fish_instance_numeric_v1(
        FishingSpecies::Bluegill,
        FishingVariant::Normal,
        500_000,
        500_000,
        false,
    )
    .unwrap();
    let bridge = fish.catch_weight_bridge().unwrap();

    assert_eq!(
        bridge.canonical_weight_scaled_grams,
        fish.weight_scaled_grams()
    );
    assert_eq!(bridge.catch_bag_weight_grams, 400);
}

#[test]
fn impossible_zero_weight_fails_closed() {
    assert_eq!(
        bridge_canonical_fish_weight_v1(0),
        Err(FishingWeightBridgeError::ZeroCanonicalWeight)
    );
}
