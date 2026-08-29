use graphite_services::{
    ArmorEnchant, EnchantConflictDecision, FishingRodEnchant, PickaxeEnchant, SwordEnchant,
    armor_enchants_conflict, fishing_rod_enchants_conflict, pickaxe_enchants_conflict,
    sword_enchants_conflict,
};

#[test]
fn public_api_preserves_pickaxe_and_rod_conflict_boundaries() {
    assert!(pickaxe_enchants_conflict(PickaxeEnchant::Trench, PickaxeEnchant::Nuke).is_forbidden());
    assert_eq!(
        pickaxe_enchants_conflict(PickaxeEnchant::Fortune, PickaxeEnchant::Treasure),
        EnchantConflictDecision::Compatible
    );
    assert_eq!(
        fishing_rod_enchants_conflict(
            FishingRodEnchant::MultiTreasure,
            FishingRodEnchant::Multicatch,
        ),
        EnchantConflictDecision::Compatible
    );
}

#[test]
fn public_api_preserves_sword_exclusive_families_and_technique_exception() {
    for pair in [
        (SwordEnchant::Sharpness, SwordEnchant::Smite),
        (SwordEnchant::FireAspect, SwordEnchant::Freezing),
        (SwordEnchant::Annihilation, SwordEnchant::Execution),
        (SwordEnchant::SweepingEdge, SwordEnchant::Piercing),
        (SwordEnchant::SweepingEdge, SwordEnchant::ArmorPiercing),
    ] {
        assert!(sword_enchants_conflict(pair.0, pair.1).is_forbidden());
        assert!(sword_enchants_conflict(pair.1, pair.0).is_forbidden());
    }

    assert_eq!(
        sword_enchants_conflict(SwordEnchant::Piercing, SwordEnchant::ArmorPiercing),
        EnchantConflictDecision::Compatible
    );
}

#[test]
fn public_api_preserves_armor_conflicts_and_walker_compatibility() {
    for pair in [
        (ArmorEnchant::Cat, ArmorEnchant::Dog),
        (ArmorEnchant::Angel, ArmorEnchant::Evil),
        (ArmorEnchant::Guardian, ArmorEnchant::Phoenix),
        (ArmorEnchant::NineLife, ArmorEnchant::Phoenix),
        (ArmorEnchant::Thorn, ArmorEnchant::Reinforce),
    ] {
        assert!(armor_enchants_conflict(pair.0, pair.1).is_forbidden());
        assert!(armor_enchants_conflict(pair.1, pair.0).is_forbidden());
    }

    for pair in [
        (ArmorEnchant::DayWalker, ArmorEnchant::NightWalker),
        (ArmorEnchant::DayWalker, ArmorEnchant::ShadowWalker),
        (ArmorEnchant::NightWalker, ArmorEnchant::ShadowWalker),
    ] {
        assert_eq!(
            armor_enchants_conflict(pair.0, pair.1),
            EnchantConflictDecision::Compatible
        );
    }
}
