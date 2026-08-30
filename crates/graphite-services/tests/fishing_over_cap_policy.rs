use graphite_services::{
    EquipmentTier, FishingCapabilityRatio, FishingOverCapError, FishingRarity,
    OVER_CAP_CATCH_CHANCE_MAX_PERCENT, OVER_CAP_CATCH_CHANCE_MIN_PERCENT, OverCapCatchChanceBound,
    SHARP_HOOK_MAX_LEVEL, SHARP_HOOK_PERCENTAGE_POINTS_PER_LEVEL, fishing_catch_load,
    fishing_tension, manual_fishing_capability_ratio, manual_fishing_line_strength,
    preview_over_cap_catch_chance,
};

fn manual_ratio(common_weight_grams: u64) -> FishingCapabilityRatio {
    let candidate = [fishing_tension(common_weight_grams, FishingRarity::Common).unwrap()];
    let load = fishing_catch_load(&candidate).unwrap();
    let strength = manual_fishing_line_strength(EquipmentTier::Wood, true, None, false).unwrap();
    manual_fishing_capability_ratio(load, strength).unwrap()
}

#[test]
fn public_api_exposes_the_frozen_sharp_hook_and_clamp_contract() {
    assert_eq!(SHARP_HOOK_MAX_LEVEL, 10);
    assert_eq!(SHARP_HOOK_PERCENTAGE_POINTS_PER_LEVEL, 2);
    assert_eq!(OVER_CAP_CATCH_CHANCE_MIN_PERCENT, 15);
    assert_eq!(OVER_CAP_CATCH_CHANCE_MAX_PERCENT, 95);
}

#[test]
fn public_api_preserves_exact_interior_probability() {
    let chance = preview_over_cap_catch_chance(manual_ratio(6_600), None).unwrap();
    assert_eq!(chance.sharp_hook_level, None);
    assert_eq!(chance.bound, OverCapCatchChanceBound::Interior);
    assert_eq!(chance.numerator(), 33);
    assert_eq!(chance.denominator(), 40);
}

#[test]
fn public_api_applies_sharp_hook_as_percentage_points_and_clamps_exactly() {
    let upper = preview_over_cap_catch_chance(manual_ratio(8_400), Some(10)).unwrap();
    assert_eq!(upper.sharp_hook_level, Some(10));
    assert_eq!(upper.bound, OverCapCatchChanceBound::Maximum);
    assert_eq!((upper.numerator(), upper.denominator()), (19, 20));

    let lower = preview_over_cap_catch_chance(manual_ratio(22_800), None).unwrap();
    assert_eq!(lower.bound, OverCapCatchChanceBound::Minimum);
    assert_eq!((lower.numerator(), lower.denominator()), (3, 20));
}

#[test]
fn public_api_fails_closed_outside_the_over_cap_and_level_domains() {
    assert_eq!(
        preview_over_cap_catch_chance(manual_ratio(6_000), None),
        Err(FishingOverCapError::CapabilityRatioNotOverCap)
    );

    let over = manual_ratio(6_600);
    for level in [0, 11, u8::MAX] {
        assert_eq!(
            preview_over_cap_catch_chance(over, Some(level)),
            Err(FishingOverCapError::SharpHookLevelOutOfRange(level))
        );
    }
}
