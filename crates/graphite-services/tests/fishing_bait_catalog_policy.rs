use graphite_services::{
    BAIT_UNITS_CONSUMED_PER_ACTIVE_CATEGORY_PER_CAST, FishingBait, FishingBaitCategory,
    FishingBaitEffect, FishingRarity, MAX_FISH_PER_CAST, fishing_bait_policy,
};

#[test]
fn public_api_preserves_the_five_frozen_bait_rows() {
    for (bait, category, price) in [
        (FishingBait::School, FishingBaitCategory::Quantity, 35),
        (FishingBait::Quality, FishingBaitCategory::Quality, 50),
        (FishingBait::Rare, FishingBaitCategory::SpeciesQuality, 75),
        (FishingBait::Treasure, FishingBaitCategory::Treasure, 100),
        (FishingBait::Sturdy, FishingBaitCategory::Safety, 50),
    ] {
        let policy = fishing_bait_policy(bait);
        assert_eq!(policy.bait, bait);
        assert_eq!(policy.category, category);
        assert_eq!(policy.shop_price_per_unit, price);
        assert_eq!(
            policy.units_consumed_per_cast,
            BAIT_UNITS_CONSUMED_PER_ACTIVE_CATEGORY_PER_CAST
        );
        assert!(policy.optional_money_sink);
        assert!(policy.effects_apply_on_independent_axes_before_shared_caps);
        assert!(!policy.multi_catch_consumes_extra_bait);
        assert!(!policy.multi_treasure_consumes_extra_bait);
    }
}

#[test]
fn public_api_preserves_school_bait_cardinality_and_probability() {
    let FishingBaitEffect::School {
        requires_fish_result,
        extra_same_area_fish_chance,
        extra_fish_count,
        non_recursive,
        max_total_fish_per_cast,
    } = fishing_bait_policy(FishingBait::School).effect
    else {
        panic!("School Bait returned the wrong effect variant");
    };

    assert!(requires_fish_result);
    assert_eq!(
        (
            extra_same_area_fish_chance.numerator(),
            extra_same_area_fish_chance.denominator()
        ),
        (2, 25)
    );
    assert_eq!(extra_fish_count, 1);
    assert!(non_recursive);
    assert_eq!(MAX_FISH_PER_CAST, 5);
    assert_eq!(max_total_fish_per_cast, MAX_FISH_PER_CAST);
}

#[test]
fn public_api_preserves_quality_rare_treasure_and_sturdy_factors() {
    let FishingBaitEffect::Quality {
        sampled_fish_weight_center_factor,
        non_normal_variant_relative_weight_factor,
    } = fishing_bait_policy(FishingBait::Quality).effect
    else {
        panic!("Quality Bait returned the wrong effect variant");
    };
    assert_eq!(
        (
            sampled_fish_weight_center_factor.numerator(),
            sampled_fish_weight_center_factor.denominator()
        ),
        (21, 20)
    );
    assert_eq!(
        (
            non_normal_variant_relative_weight_factor.numerator(),
            non_normal_variant_relative_weight_factor.denominator()
        ),
        (11, 10)
    );

    let FishingBaitEffect::Rare {
        affected_species_rarities,
        eligible_species_relative_weight_factor,
    } = fishing_bait_policy(FishingBait::Rare).effect
    else {
        panic!("Rare Bait returned the wrong effect variant");
    };
    assert_eq!(
        affected_species_rarities,
        [
            FishingRarity::Rare,
            FishingRarity::Epic,
            FishingRarity::Legendary,
            FishingRarity::Mythic,
        ]
    );
    assert_eq!(
        (
            eligible_species_relative_weight_factor.numerator(),
            eligible_species_relative_weight_factor.denominator()
        ),
        (28, 25)
    );

    let FishingBaitEffect::Treasure {
        treasure_branch_relative_weight_factor,
        junk_branch_relative_weight_factor,
    } = fishing_bait_policy(FishingBait::Treasure).effect
    else {
        panic!("Treasure Bait returned the wrong effect variant");
    };
    assert_eq!(
        (
            treasure_branch_relative_weight_factor.numerator(),
            treasure_branch_relative_weight_factor.denominator()
        ),
        (23, 20)
    );
    assert_eq!(
        (
            junk_branch_relative_weight_factor.numerator(),
            junk_branch_relative_weight_factor.denominator()
        ),
        (9, 10)
    );

    let FishingBaitEffect::Sturdy {
        line_strength_factor,
        final_line_break_chance_factor,
    } = fishing_bait_policy(FishingBait::Sturdy).effect
    else {
        panic!("Sturdy Bait returned the wrong effect variant");
    };
    assert_eq!(
        (
            line_strength_factor.numerator(),
            line_strength_factor.denominator()
        ),
        (11, 10)
    );
    assert_eq!(
        (
            final_line_break_chance_factor.numerator(),
            final_line_break_chance_factor.denominator()
        ),
        (9, 10)
    );
}
