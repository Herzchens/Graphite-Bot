use graphite_services::{
    FishingRodEnchant, FishingRodLevelXEffect, FishingRodLevelXPolicy, FishingRodLevelXPolicyError,
    fishing_rod_level_x_policy,
};

#[test]
fn public_api_exposes_every_frozen_scalar_level_x_row_exactly() {
    let expected = [
        (
            FishingRodEnchant::Lure,
            FishingRodLevelXEffect::Lure {
                action_speed_rating_percent: 15,
                uses_shared_fishing_speed_bucket: true,
            },
        ),
        (
            FishingRodEnchant::LuckOfTheSea,
            FishingRodLevelXEffect::LuckOfTheSea {
                rare_species_relative_weight_increase_percent: 60,
                junk_relative_weight_decrease_percent: 30,
                resolves_before_fish_instance_creation: true,
            },
        ),
        (
            FishingRodEnchant::Treasure,
            FishingRodLevelXEffect::Treasure {
                treasure_branch_relative_weight_increase_percent: 80,
                affects_internal_treasure_result_weights: false,
            },
        ),
        (
            FishingRodEnchant::Luck,
            FishingRodLevelXEffect::Luck {
                expected_size_weight_variant_quality_value_increase_percent: 20,
                alters_species_rarity: false,
                resolves_before_fish_instance_creation: true,
            },
        ),
        (
            FishingRodEnchant::Unbreaking,
            FishingRodLevelXEffect::Unbreaking {
                ignore_normal_rod_durability_event_chance_percent: 20,
            },
        ),
    ];

    for (enchant, effect) in expected {
        assert_eq!(
            fishing_rod_level_x_policy(enchant),
            Ok(FishingRodLevelXPolicy { enchant, effect })
        );
    }
}

#[test]
fn public_api_does_not_duplicate_dedicated_fishing_rod_policy_owners() {
    for enchant in [
        FishingRodEnchant::MultiTreasure,
        FishingRodEnchant::Mending,
        FishingRodEnchant::Multicatch,
        FishingRodEnchant::Strengthen,
        FishingRodEnchant::SharpHook,
        FishingRodEnchant::BaitRack,
    ] {
        assert_eq!(
            fishing_rod_level_x_policy(enchant),
            Err(FishingRodLevelXPolicyError::DedicatedPolicy(enchant))
        );
    }
}
