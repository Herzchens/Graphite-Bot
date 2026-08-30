use serde::Serialize;
use thiserror::Error;

use crate::FishingRodEnchant;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FishingRodLevelXEffect {
    Lure {
        action_speed_rating_percent: u8,
        uses_shared_fishing_speed_bucket: bool,
    },
    LuckOfTheSea {
        rare_species_relative_weight_increase_percent: u8,
        junk_relative_weight_decrease_percent: u8,
        resolves_before_fish_instance_creation: bool,
    },
    Treasure {
        treasure_branch_relative_weight_increase_percent: u8,
        affects_internal_treasure_result_weights: bool,
    },
    Luck {
        expected_size_weight_variant_quality_value_increase_percent: u8,
        alters_species_rarity: bool,
        resolves_before_fish_instance_creation: bool,
    },
    Unbreaking {
        ignore_normal_rod_durability_event_chance_percent: u8,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FishingRodLevelXPolicy {
    pub enchant: FishingRodEnchant,
    pub effect: FishingRodLevelXEffect,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum FishingRodLevelXPolicyError {
    #[error(
        "Fishing Rod enchant {0:?} is not owned by the scalar Level X modifier policy; use its dedicated policy"
    )]
    DedicatedPolicy(FishingRodEnchant),
}

/// Returns the exact final-Level-X scalar policy for Fishing Rod effects that do not yet have a
/// dedicated numeric owner.
///
/// The current master freezes only the **Level X** values represented here. It does not define an
/// interpolation table or per-level formula for Levels I-IX, so this API deliberately accepts no
/// enchant level and callers must not extrapolate these values downward.
///
/// This policy freezes scalar inputs and composition-stage semantics only:
/// - Lure contributes +15% action-speed rating to the shared Fishing speed bucket. It does not turn
///   that rating into a duration or apply the shared speed cap/floor.
/// - Luck of the Sea contributes +60% relative rare-species weight and -30% relative Junk weight
///   before immutable FishInstance creation. It does not normalize or sample either pool.
/// - Treasure contributes +80% relative Treasure-branch weight and does not alter the internal
///   within-Treasure result table.
/// - Luck targets +20% expected size/weight/variant quality value before FishInstance creation and
///   explicitly does not alter species rarity. The distribution transform that realizes this target
///   remains outside this scalar policy.
/// - Unbreaking has a 20% chance to ignore one normal Rod durability event. This function performs
///   no RNG draw and does not mutate durability.
///
/// Multi Treasure, Mending, Multicatch, Strengthen, SharpHook, and Bait Rack already have dedicated
/// policy owners in this crate and therefore fail closed here instead of creating a second source of
/// truth.
pub const fn fishing_rod_level_x_policy(
    enchant: FishingRodEnchant,
) -> Result<FishingRodLevelXPolicy, FishingRodLevelXPolicyError> {
    let effect = match enchant {
        FishingRodEnchant::Lure => FishingRodLevelXEffect::Lure {
            action_speed_rating_percent: 15,
            uses_shared_fishing_speed_bucket: true,
        },
        FishingRodEnchant::LuckOfTheSea => FishingRodLevelXEffect::LuckOfTheSea {
            rare_species_relative_weight_increase_percent: 60,
            junk_relative_weight_decrease_percent: 30,
            resolves_before_fish_instance_creation: true,
        },
        FishingRodEnchant::Treasure => FishingRodLevelXEffect::Treasure {
            treasure_branch_relative_weight_increase_percent: 80,
            affects_internal_treasure_result_weights: false,
        },
        FishingRodEnchant::Luck => FishingRodLevelXEffect::Luck {
            expected_size_weight_variant_quality_value_increase_percent: 20,
            alters_species_rarity: false,
            resolves_before_fish_instance_creation: true,
        },
        FishingRodEnchant::Unbreaking => FishingRodLevelXEffect::Unbreaking {
            ignore_normal_rod_durability_event_chance_percent: 20,
        },
        FishingRodEnchant::MultiTreasure
        | FishingRodEnchant::Mending
        | FishingRodEnchant::Multicatch
        | FishingRodEnchant::Strengthen
        | FishingRodEnchant::SharpHook
        | FishingRodEnchant::BaitRack => {
            return Err(FishingRodLevelXPolicyError::DedicatedPolicy(enchant));
        }
    };

    Ok(FishingRodLevelXPolicy { enchant, effect })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_scalar_level_x_rows_match_the_current_master() {
        assert_eq!(
            fishing_rod_level_x_policy(FishingRodEnchant::Lure),
            Ok(FishingRodLevelXPolicy {
                enchant: FishingRodEnchant::Lure,
                effect: FishingRodLevelXEffect::Lure {
                    action_speed_rating_percent: 15,
                    uses_shared_fishing_speed_bucket: true,
                },
            })
        );
        assert_eq!(
            fishing_rod_level_x_policy(FishingRodEnchant::LuckOfTheSea),
            Ok(FishingRodLevelXPolicy {
                enchant: FishingRodEnchant::LuckOfTheSea,
                effect: FishingRodLevelXEffect::LuckOfTheSea {
                    rare_species_relative_weight_increase_percent: 60,
                    junk_relative_weight_decrease_percent: 30,
                    resolves_before_fish_instance_creation: true,
                },
            })
        );
        assert_eq!(
            fishing_rod_level_x_policy(FishingRodEnchant::Treasure),
            Ok(FishingRodLevelXPolicy {
                enchant: FishingRodEnchant::Treasure,
                effect: FishingRodLevelXEffect::Treasure {
                    treasure_branch_relative_weight_increase_percent: 80,
                    affects_internal_treasure_result_weights: false,
                },
            })
        );
        assert_eq!(
            fishing_rod_level_x_policy(FishingRodEnchant::Luck),
            Ok(FishingRodLevelXPolicy {
                enchant: FishingRodEnchant::Luck,
                effect: FishingRodLevelXEffect::Luck {
                    expected_size_weight_variant_quality_value_increase_percent: 20,
                    alters_species_rarity: false,
                    resolves_before_fish_instance_creation: true,
                },
            })
        );
        assert_eq!(
            fishing_rod_level_x_policy(FishingRodEnchant::Unbreaking),
            Ok(FishingRodLevelXPolicy {
                enchant: FishingRodEnchant::Unbreaking,
                effect: FishingRodLevelXEffect::Unbreaking {
                    ignore_normal_rod_durability_event_chance_percent: 20,
                },
            })
        );
    }

    #[test]
    fn dedicated_policy_enchants_fail_closed() {
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
}
