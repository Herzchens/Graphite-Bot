use graphite_services::{
    CANONICAL_FISH_AREA_ROWS, EquipmentTier, FishingArea, FishingCapabilityError,
    FishingCatchBranch, FishingRarity, FishingSpecies, GOLD_ROD_ACTION_SPEED_RATING_PERCENT,
    GOLD_ROD_RARE_OR_BETTER_RELATIVE_WEIGHT_PERCENT, GOLD_ROD_TREASURE_RELATIVE_WEIGHT_PERCENT,
    GoldFishingRodModifierStage, GoldFishingRodPolicyError, fishing_area_species_pool,
    fishing_species_policy, gold_fishing_rod_side_grade_policy,
    preview_gold_fishing_rod_catch_branch_weight, preview_gold_fishing_rod_species_weight,
};

#[test]
fn public_api_exposes_the_frozen_gold_rod_side_grade_values() {
    assert_eq!(GOLD_ROD_ACTION_SPEED_RATING_PERCENT, 10);
    assert_eq!(GOLD_ROD_RARE_OR_BETTER_RELATIVE_WEIGHT_PERCENT, 15);
    assert_eq!(GOLD_ROD_TREASURE_RELATIVE_WEIGHT_PERCENT, 15);

    let policy = gold_fishing_rod_side_grade_policy(EquipmentTier::Gold, true).unwrap();
    assert_eq!(policy.action_speed_rating_percent, 10);
    assert_eq!(
        (
            policy
                .rare_or_better_species_relative_weight_multiplier
                .numerator(),
            policy
                .rare_or_better_species_relative_weight_multiplier
                .denominator(),
        ),
        (23, 20)
    );
    assert_eq!(
        (
            policy
                .treasure_branch_relative_weight_multiplier
                .numerator(),
            policy
                .treasure_branch_relative_weight_multiplier
                .denominator(),
        ),
        (23, 20)
    );
    assert_eq!(
        policy.modifier_stage,
        GoldFishingRodModifierStage::BeforeSharedFishingCaps
    );
}

#[test]
fn public_api_does_not_convert_gold_speed_rating_into_a_duration_factor() {
    let policy = gold_fishing_rod_side_grade_policy(EquipmentTier::Gold, true).unwrap();

    assert_eq!(
        policy.action_speed_rating_percent,
        GOLD_ROD_ACTION_SPEED_RATING_PERCENT
    );
}

#[test]
fn public_api_rejects_non_gold_or_non_ordinary_definitions() {
    assert_eq!(
        gold_fishing_rod_side_grade_policy(EquipmentTier::Diamond, true),
        Err(GoldFishingRodPolicyError::NotGoldFishingRod)
    );
    assert_eq!(
        gold_fishing_rod_side_grade_policy(EquipmentTier::Gold, false),
        Err(GoldFishingRodPolicyError::InvalidOrdinaryRod(
            FishingCapabilityError::NotOrdinaryFishingRod
        ))
    );
}

#[test]
fn public_api_gold_rod_boosts_only_treasure_branch_before_shared_caps() {
    let expected = [
        (FishingCatchBranch::Fish, 176, false, (1, 1), (176, 1)),
        (FishingCatchBranch::Junk, 17, false, (1, 1), (17, 1)),
        (FishingCatchBranch::Treasure, 7, true, (23, 20), (161, 20)),
    ];

    for (branch, base, applied, factor, adjusted) in expected {
        let preview =
            preview_gold_fishing_rod_catch_branch_weight(EquipmentTier::Gold, true, branch)
                .unwrap();
        assert_eq!(preview.branch, branch);
        assert_eq!(preview.base_relative_weight, base);
        assert_eq!(preview.gold_modifier_applied, applied);
        assert_eq!(
            (
                preview.relative_weight_multiplier.numerator(),
                preview.relative_weight_multiplier.denominator(),
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
}

#[test]
fn public_api_gold_rod_boosts_all_rare_or_better_canonical_species_rows_only() {
    let areas = [
        FishingArea::StarterPool,
        FishingArea::River,
        FishingArea::Lake,
        FishingArea::Coast,
        FishingArea::DeepSea,
        FishingArea::Abyss,
    ];
    let mut rows_seen = 0;

    for area in areas {
        for row in fishing_area_species_pool(area) {
            rows_seen += 1;
            let preview = preview_gold_fishing_rod_species_weight(
                EquipmentTier::Gold,
                true,
                area,
                row.species,
            )
            .unwrap();
            let rarity = fishing_species_policy(row.species).rarity;
            let eligible = matches!(
                rarity,
                FishingRarity::Rare
                    | FishingRarity::Epic
                    | FishingRarity::Legendary
                    | FishingRarity::Mythic
            );
            let expected_factor = if eligible { (23_u16, 20_u16) } else { (1, 1) };

            assert_eq!(preview.area, area);
            assert_eq!(preview.species, row.species);
            assert_eq!(preview.rarity, rarity);
            assert_eq!(preview.base_pool_weight, row.pool_weight);
            assert_eq!(preview.gold_modifier_applied, eligible);
            assert_eq!(
                (
                    preview.relative_weight_multiplier.numerator(),
                    preview.relative_weight_multiplier.denominator(),
                ),
                expected_factor
            );
            assert_eq!(
                preview.adjusted_pool_weight_numerator(),
                u32::from(row.pool_weight) * u32::from(expected_factor.0)
            );
            assert_eq!(
                preview.adjusted_pool_weight_denominator(),
                expected_factor.1
            );
        }
    }

    assert_eq!(rows_seen, CANONICAL_FISH_AREA_ROWS);
}

#[test]
fn public_api_gold_rod_previews_fail_closed_on_invalid_authority() {
    assert_eq!(
        preview_gold_fishing_rod_species_weight(
            EquipmentTier::Gold,
            true,
            FishingArea::StarterPool,
            FishingSpecies::LeviathanFry,
        ),
        Err(GoldFishingRodPolicyError::SpeciesNotInArea {
            area: FishingArea::StarterPool,
            species: FishingSpecies::LeviathanFry,
        })
    );
    assert_eq!(
        preview_gold_fishing_rod_catch_branch_weight(
            EquipmentTier::Diamond,
            true,
            FishingCatchBranch::Treasure,
        ),
        Err(GoldFishingRodPolicyError::NotGoldFishingRod)
    );
    assert_eq!(
        preview_gold_fishing_rod_species_weight(
            EquipmentTier::Gold,
            false,
            FishingArea::StarterPool,
            FishingSpecies::Koi,
        ),
        Err(GoldFishingRodPolicyError::InvalidOrdinaryRod(
            FishingCapabilityError::NotOrdinaryFishingRod
        ))
    );
}
