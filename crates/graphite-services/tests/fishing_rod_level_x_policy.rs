use graphite_services::{
    FishingCatchBranch, FishingRodEnchant, FishingRodLevelXEffect, FishingRodLevelXPolicy,
    FishingRodLevelXPolicyError, fishing_rod_level_x_policy,
    preview_treasure_level_x_branch_weight,
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

#[test]
fn public_api_treasure_level_x_transforms_only_treasure_branch_before_normalization() {
    let expected = [
        (FishingCatchBranch::Fish, 176, false, (1, 1), (176, 1)),
        (FishingCatchBranch::Junk, 17, false, (1, 1), (17, 1)),
        (FishingCatchBranch::Treasure, 7, true, (9, 5), (63, 5)),
    ];

    for (branch, base, applied, factor, adjusted) in expected {
        let preview = preview_treasure_level_x_branch_weight(branch).unwrap();
        assert_eq!(preview.branch, branch);
        assert_eq!(preview.base_relative_weight, base);
        assert_eq!(preview.treasure_level_x_applied, applied);
        assert_eq!(
            (
                preview.relative_weight_multiplier_numerator(),
                preview.relative_weight_multiplier_denominator(),
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
