use graphite_services::{
    EquipmentTier, FISHING_FIXED_POINT_SCALE, FISHING_LINE_BREAK_PROBABILITY_SCALE_PPM,
    FISHING_NUMERIC_DRAW_SCALE_PPM, FISHING_NUMERIC_POLICY_VERSION, FishingNumericError,
    FishingSpecies, FishingVariant, fishing_catch_load, fishing_tension,
    manual_fishing_capability_ratio, manual_fishing_line_strength,
    preview_manual_fishing_line_break_chance_v1, resolve_fish_instance_numeric_v1,
};

#[derive(Clone, Copy)]
struct FishGolden {
    species: FishingSpecies,
    variant: FishingVariant,
    weight_draw: u32,
    noise_draw: u32,
    quality_bait: bool,
    weight_ratio_scaled: u64,
    weight_scaled_grams: u64,
    biological_noise_scaled: u64,
    length_scaled_millimeters: u64,
    npc_value_money: i64,
}

#[test]
fn fish_instance_numeric_v1_golden_vectors_are_bit_stable() {
    let vectors = [
        FishGolden {
            species: FishingSpecies::Bluegill,
            variant: FishingVariant::Normal,
            weight_draw: 0,
            noise_draw: 0,
            quality_bait: false,
            weight_ratio_scaled: 400_000_000_000,
            weight_scaled_grams: 160_000_000_000_000,
            biological_noise_scaled: 950_000_050_000,
            length_scaled_millimeters: 139_993_204_316_193,
            npc_value_money: 25,
        },
        FishGolden {
            species: FishingSpecies::Bluegill,
            variant: FishingVariant::Normal,
            weight_draw: 500_000,
            noise_draw: 500_000,
            quality_bait: false,
            weight_ratio_scaled: 1_000_000_313_329,
            weight_scaled_grams: 400_000_125_331_600,
            biological_noise_scaled: 1_000_000_050_000,
            length_scaled_millimeters: 200_000_030_888_601,
            npc_value_money: 50,
        },
        FishGolden {
            species: FishingSpecies::Bluegill,
            variant: FishingVariant::Iridescent,
            weight_draw: 999_999,
            noise_draw: 999_999,
            quality_bait: false,
            weight_ratio_scaled: 2_500_000_000_000,
            weight_scaled_grams: 1_000_000_000_000_000,
            biological_noise_scaled: 1_049_999_950_000,
            length_scaled_millimeters: 285_013_836_170_702,
            npc_value_money: 327,
        },
        FishGolden {
            species: FishingSpecies::Tuna,
            variant: FishingVariant::Golden,
            weight_draw: 500_000,
            noise_draw: 0,
            quality_bait: true,
            weight_ratio_scaled: 1_050_000_328_995,
            weight_scaled_grams: 15_750_004_934_925_000,
            biological_noise_scaled: 950_000_050_000,
            length_scaled_millimeters: 1_158_692_028_768_448,
            npc_value_money: 1_277,
        },
        FishGolden {
            species: FishingSpecies::LeviathanFry,
            variant: FishingVariant::Normal,
            weight_draw: 999_999,
            noise_draw: 500_000,
            quality_bait: false,
            weight_ratio_scaled: 2_500_000_000_000,
            weight_scaled_grams: 250_000_000_000_000_000,
            biological_noise_scaled: 1_000_000_050_000,
            length_scaled_millimeters: 3_393_022_190_398_601,
            npc_value_money: 21_790,
        },
    ];

    for vector in vectors {
        let resolved = resolve_fish_instance_numeric_v1(
            vector.species,
            vector.variant,
            vector.weight_draw,
            vector.noise_draw,
            vector.quality_bait,
        )
        .unwrap();
        assert_eq!(resolved.policy_version, FISHING_NUMERIC_POLICY_VERSION);
        assert_eq!(resolved.fixed_point_scale(), FISHING_FIXED_POINT_SCALE);
        assert_eq!(resolved.weight_ratio_scaled(), vector.weight_ratio_scaled);
        assert_eq!(resolved.weight_scaled_grams(), vector.weight_scaled_grams);
        assert_eq!(
            resolved.biological_noise_scaled(),
            vector.biological_noise_scaled
        );
        assert_eq!(
            resolved.length_scaled_millimeters(),
            vector.length_scaled_millimeters
        );
        assert_eq!(resolved.npc_value_money, vector.npc_value_money);
    }
}

#[test]
fn weight_sampling_is_single_draw_then_clamp_and_noise_is_independent() {
    let low = resolve_fish_instance_numeric_v1(
        FishingSpecies::Bluegill,
        FishingVariant::Normal,
        0,
        0,
        false,
    )
    .unwrap();
    let low_other_noise = resolve_fish_instance_numeric_v1(
        FishingSpecies::Bluegill,
        FishingVariant::Normal,
        0,
        999_999,
        false,
    )
    .unwrap();
    let high = resolve_fish_instance_numeric_v1(
        FishingSpecies::Bluegill,
        FishingVariant::Normal,
        999_999,
        0,
        false,
    )
    .unwrap();

    assert_eq!(low.weight_ratio_scaled(), 400_000_000_000);
    assert_eq!(high.weight_ratio_scaled(), 2_500_000_000_000);
    assert_eq!(
        low.weight_scaled_grams(),
        low_other_noise.weight_scaled_grams()
    );
    assert_eq!(low.npc_value_money, low_other_noise.npc_value_money);
    assert_ne!(
        low.length_scaled_millimeters(),
        low_other_noise.length_scaled_millimeters()
    );
}

#[test]
fn quality_bait_shifts_the_weight_center_before_the_same_wref_clamp() {
    let normal = resolve_fish_instance_numeric_v1(
        FishingSpecies::Tuna,
        FishingVariant::Normal,
        500_000,
        500_000,
        false,
    )
    .unwrap();
    let quality = resolve_fish_instance_numeric_v1(
        FishingSpecies::Tuna,
        FishingVariant::Normal,
        500_000,
        500_000,
        true,
    )
    .unwrap();

    assert_eq!(normal.weight_ratio_scaled(), 1_000_000_313_329);
    assert_eq!(quality.weight_ratio_scaled(), 1_050_000_328_995);

    let quality_high = resolve_fish_instance_numeric_v1(
        FishingSpecies::Tuna,
        FishingVariant::Normal,
        999_999,
        500_000,
        true,
    )
    .unwrap();
    assert_eq!(quality_high.weight_ratio_scaled(), 2_500_000_000_000);
}

#[test]
fn variant_changes_money_without_rewriting_weight_or_length() {
    let normal = resolve_fish_instance_numeric_v1(
        FishingSpecies::Koi,
        FishingVariant::Normal,
        750_000,
        125_000,
        false,
    )
    .unwrap();
    let iridescent = resolve_fish_instance_numeric_v1(
        FishingSpecies::Koi,
        FishingVariant::Iridescent,
        750_000,
        125_000,
        false,
    )
    .unwrap();

    assert_eq!(
        normal.weight_scaled_grams(),
        iridescent.weight_scaled_grams()
    );
    assert_eq!(
        normal.length_scaled_millimeters(),
        iridescent.length_scaled_millimeters()
    );
    assert_eq!(normal.npc_value_money, 462);
    assert_eq!(iridescent.npc_value_money, 1_385);
}

fn manual_wood_ratio(common_weight_grams: u64) -> graphite_services::FishingCapabilityRatio {
    let tensions = [fishing_tension(
        common_weight_grams,
        graphite_services::FishingRarity::Common,
    )
    .unwrap()];
    let load = fishing_catch_load(&tensions).unwrap();
    let strength = manual_fishing_line_strength(EquipmentTier::Wood, true, None, false).unwrap();
    manual_fishing_capability_ratio(load, strength).unwrap()
}

#[test]
fn line_break_v1_golden_vectors_use_half_up_ppm_thresholds() {
    assert_eq!(FISHING_LINE_BREAK_PROBABILITY_SCALE_PPM, 1_000_000);

    let ten_percent_over =
        preview_manual_fishing_line_break_chance_v1(manual_wood_ratio(6_600), false).unwrap();
    assert_eq!(ten_percent_over.base_probability_ppm, 2_752);
    assert_eq!(ten_percent_over.final_probability_ppm, 2_752);
    assert!(!ten_percent_over.base_cap_applied);

    let ten_percent_over_sturdy =
        preview_manual_fishing_line_break_chance_v1(manual_wood_ratio(6_600), true).unwrap();
    assert_eq!(ten_percent_over_sturdy.base_probability_ppm, 2_752);
    assert_eq!(ten_percent_over_sturdy.final_probability_ppm, 2_477);

    let double_load =
        preview_manual_fishing_line_break_chance_v1(manual_wood_ratio(12_000), false).unwrap();
    assert_eq!(double_load.base_probability_ppm, 17_000);
    assert_eq!(double_load.final_probability_ppm, 17_000);

    let capped =
        preview_manual_fishing_line_break_chance_v1(manual_wood_ratio(42_000), true).unwrap();
    assert!(capped.base_cap_applied);
    assert_eq!(capped.base_probability_ppm, 150_000);
    assert_eq!(capped.final_probability_ppm, 135_000);
}

#[test]
fn line_break_rejects_within_capability_and_draw_domain_rejects_one_million() {
    assert_eq!(
        preview_manual_fishing_line_break_chance_v1(manual_wood_ratio(6_000), false),
        Err(FishingNumericError::CapabilityRatioNotOverCap)
    );
    assert_eq!(
        resolve_fish_instance_numeric_v1(
            FishingSpecies::Bluegill,
            FishingVariant::Normal,
            FISHING_NUMERIC_DRAW_SCALE_PPM,
            0,
            false,
        ),
        Err(FishingNumericError::DrawOutOfRange(
            FISHING_NUMERIC_DRAW_SCALE_PPM
        ))
    );
}
