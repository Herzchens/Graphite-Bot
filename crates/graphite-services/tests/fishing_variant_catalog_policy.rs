use graphite_services::{
    CANONICAL_FISH_VARIANT_COUNT, FishingVariant, FishingVariantPolicy, FishingVariantRatio,
    fishing_variant_catalog, fishing_variant_policy, preview_quality_bait_variant_weight,
};

#[test]
fn public_api_preserves_all_five_variant_rows() {
    let expected = [
        (FishingVariant::Normal, (47, 50), (1, 1)),
        (FishingVariant::Silver, (3, 100), (5, 4)),
        (FishingVariant::Golden, (3, 200), (7, 4)),
        (FishingVariant::Albino, (1, 100), (2, 1)),
        (FishingVariant::Iridescent, (1, 200), (3, 1)),
    ];

    assert_eq!(expected.len(), CANONICAL_FISH_VARIANT_COUNT);
    assert_eq!(fishing_variant_catalog().len(), expected.len());

    for (index, (variant, probability, value_multiplier)) in expected.into_iter().enumerate() {
        let policy = fishing_variant_policy(variant);
        assert_eq!(fishing_variant_catalog()[index], policy);
        assert_eq!(policy.variant, variant);
        assert_ratio(policy.base_probability, probability);
        assert_ratio(policy.value_multiplier, value_multiplier);
    }
}

#[test]
fn public_api_base_probabilities_form_one_exact_distribution() {
    const COMMON_DENOMINATOR: u32 = 200;

    let total = fishing_variant_catalog()
        .iter()
        .map(|row| scaled_numerator(row.base_probability, COMMON_DENOMINATOR))
        .sum::<u32>();
    let non_normal = fishing_variant_catalog()
        .iter()
        .filter(|row| row.variant != FishingVariant::Normal)
        .map(|row| scaled_numerator(row.base_probability, COMMON_DENOMINATOR))
        .sum::<u32>();

    assert_eq!(total, COMMON_DENOMINATOR);
    assert_eq!(non_normal, 12);
}

#[test]
fn public_api_keeps_variant_value_multiplier_independent_from_species_rarity() {
    let iridescent = fishing_variant_policy(FishingVariant::Iridescent);

    assert_ratio(iridescent.value_multiplier, (3, 1));
    assert_eq!(iridescent.variant, FishingVariant::Iridescent);
}

#[test]
fn public_api_quality_bait_boosts_only_non_normal_variant_weights_before_normalization() {
    let expected = [
        (FishingVariant::Normal, false, (1, 1), (47, 50)),
        (FishingVariant::Silver, true, (11, 10), (33, 1_000)),
        (FishingVariant::Golden, true, (11, 10), (33, 2_000)),
        (FishingVariant::Albino, true, (11, 10), (11, 1_000)),
        (FishingVariant::Iridescent, true, (11, 10), (11, 2_000)),
    ];

    for (variant, applied, factor, adjusted) in expected {
        let preview = preview_quality_bait_variant_weight(variant);
        assert_eq!(preview.variant, variant);
        assert_eq!(
            preview.base_probability,
            fishing_variant_policy(variant).base_probability
        );
        assert_eq!(preview.quality_bait_applied, applied);
        assert_eq!(
            (
                preview.relative_weight_factor_numerator(),
                preview.relative_weight_factor_denominator(),
            ),
            factor
        );
        assert_eq!(
            (
                preview.adjusted_relative_weight_numerator(),
                preview.adjusted_relative_weight_denominator(),
            ),
            adjusted
        );
    }

    const COMMON_DENOMINATOR: u32 = 2_000;
    let adjusted_weight_sum = fishing_variant_catalog()
        .iter()
        .map(|row| {
            let preview = preview_quality_bait_variant_weight(row.variant);
            let denominator = preview.adjusted_relative_weight_denominator();
            assert_eq!(COMMON_DENOMINATOR % denominator, 0);
            preview.adjusted_relative_weight_numerator() * (COMMON_DENOMINATOR / denominator)
        })
        .sum::<u32>();

    assert_eq!(adjusted_weight_sum, 2_012);
    assert_ne!(adjusted_weight_sum, COMMON_DENOMINATOR);
}

fn assert_ratio(actual: FishingVariantRatio, expected: (u16, u16)) {
    assert_eq!(actual.numerator(), expected.0);
    assert_eq!(actual.denominator(), expected.1);
}

fn scaled_numerator(ratio: FishingVariantRatio, common_denominator: u32) -> u32 {
    let denominator = u32::from(ratio.denominator());
    assert_eq!(common_denominator % denominator, 0);
    u32::from(ratio.numerator()) * (common_denominator / denominator)
}

#[test]
fn public_policy_shape_is_copyable_and_exact() {
    let normal: FishingVariantPolicy = fishing_variant_policy(FishingVariant::Normal);
    let copied = normal;

    assert_eq!(copied, normal);
    assert_ratio(copied.base_probability, (47, 50));
    assert_ratio(copied.value_multiplier, (1, 1));
}
