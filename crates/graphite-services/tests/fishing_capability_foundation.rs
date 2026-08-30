use graphite_services::{
    EquipmentTier, FishRarity, FishingCapabilityPolicyError, OrdinaryToolStatsError,
    STRENGTHEN_MAX_LEVEL, fishing_line_strength_foundation, ordinary_fishing_rod_stats,
    ordinary_pickaxe_stats, ordinary_sword_stats, rarity_tension_multiplier,
    strengthen_line_strength_policy,
};

#[test]
fn public_api_preserves_the_complete_ordinary_tool_table() {
    let cases = [
        (EquipmentTier::Wood, 3, 600, 1, 5, 700, 6, 600),
        (EquipmentTier::Stone, 4, 850, 2, 6, 1_000, 10, 900),
        (EquipmentTier::Copper, 5, 1_250, 4, 8, 1_500, 18, 1_400),
        (EquipmentTier::Gold, 8, 450, 12, 18, 600, 40, 550),
        (EquipmentTier::Iron, 7, 1_900, 6, 10, 2_300, 30, 2_200),
        (EquipmentTier::Diamond, 10, 2_800, 9, 14, 3_400, 55, 3_300),
        (EquipmentTier::Obsidian, 14, 4_200, 12, 18, 5_200, 85, 5_000),
        (EquipmentTier::Netherite, 19, 6_300, 15, 22, 7_800, 120, 7_600),
        (EquipmentTier::Graphite, 25, 9_000, 18, 26, 11_000, 160, 11_000),
    ];

    for (tier, damage, sword_dura, roll_min, roll_max, pick_dura, line, rod_dura) in cases {
        let sword = ordinary_sword_stats(tier).unwrap();
        let pickaxe = ordinary_pickaxe_stats(tier).unwrap();
        let rod = ordinary_fishing_rod_stats(tier).unwrap();

        assert_eq!(sword.base_damage, damage);
        assert_eq!(sword.max_durability, sword_dura);
        assert_eq!(pickaxe.natural_roll_min, roll_min);
        assert_eq!(pickaxe.natural_roll_max, roll_max);
        assert_eq!(pickaxe.max_durability, pick_dura);
        assert_eq!(rod.base_line_strength_kg_tension, line);
        assert_eq!(rod.max_durability, rod_dura);
    }
}

#[test]
fn public_api_preserves_rarity_tension_factors() {
    let cases = [
        (FishRarity::Common, 10_000),
        (FishRarity::Uncommon, 11_000),
        (FishRarity::Rare, 12_500),
        (FishRarity::Epic, 14_500),
        (FishRarity::Legendary, 17_500),
        (FishRarity::Mythic, 22_000),
    ];

    for (rarity, expected) in cases {
        assert_eq!(
            rarity_tension_multiplier(rarity).multiplier_basis_points,
            expected
        );
    }
}

#[test]
fn public_api_distinguishes_absent_strengthen_from_malformed_level_zero() {
    let absent = strengthen_line_strength_policy(None).unwrap();
    assert_eq!(absent.level, None);
    assert_eq!(absent.bonus_basis_points, 0);
    assert_eq!(absent.multiplier_basis_points, 10_000);

    let max = strengthen_line_strength_policy(Some(STRENGTHEN_MAX_LEVEL)).unwrap();
    assert_eq!(max.level, Some(10));
    assert_eq!(max.bonus_basis_points, 4_000);
    assert_eq!(max.multiplier_basis_points, 14_000);

    assert_eq!(
        strengthen_line_strength_policy(Some(0)),
        Err(FishingCapabilityPolicyError::StrengthenLevelOutOfRange(0))
    );
}

#[test]
fn public_api_keeps_gold_side_grade_stats_and_exact_unrounded_factors() {
    let gold = fishing_line_strength_foundation(EquipmentTier::Gold, Some(10)).unwrap();
    let iron = fishing_line_strength_foundation(EquipmentTier::Iron, None).unwrap();

    assert_eq!(gold.rod.base_line_strength_kg_tension, 40);
    assert_eq!(gold.rod.max_durability, 550);
    assert_eq!(gold.strengthen.multiplier_basis_points, 14_000);
    assert_eq!(gold.baseline_bait_strength_factor_basis_points, 10_000);
    assert_eq!(gold.manual_automation_strength_factor_basis_points, 10_000);

    assert_eq!(iron.rod.base_line_strength_kg_tension, 30);
    assert_eq!(iron.rod.max_durability, 2_200);
    assert_eq!(iron.strengthen.multiplier_basis_points, 10_000);
}

#[test]
fn public_api_rejects_non_tool_tier_instead_of_borrowing_starter_stats() {
    assert_eq!(
        ordinary_fishing_rod_stats(EquipmentTier::StarterLeather),
        Err(OrdinaryToolStatsError::StarterLeatherIsNotOrdinaryToolTier)
    );
    assert_eq!(
        fishing_line_strength_foundation(EquipmentTier::StarterLeather, None),
        Err(FishingCapabilityPolicyError::InvalidRodTier(
            OrdinaryToolStatsError::StarterLeatherIsNotOrdinaryToolTier
        ))
    );
}
