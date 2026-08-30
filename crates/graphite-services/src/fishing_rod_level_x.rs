use serde::Serialize;
use thiserror::Error;

use crate::{
    FishingRodEnchant,
    fishing_droptable::{FishingCatchBranch, fishing_base_catch_branch_policy},
};

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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct TreasureLevelXBranchWeightPreview {
    pub branch: FishingCatchBranch,
    pub base_relative_weight: u16,
    pub treasure_level_x_applied: bool,
    relative_weight_multiplier_numerator: u16,
    relative_weight_multiplier_denominator: u16,
    adjusted_relative_weight_numerator: u32,
    adjusted_relative_weight_denominator: u16,
}

impl TreasureLevelXBranchWeightPreview {
    #[must_use]
    pub const fn relative_weight_multiplier_numerator(self) -> u16 {
        self.relative_weight_multiplier_numerator
    }

    #[must_use]
    pub const fn relative_weight_multiplier_denominator(self) -> u16 {
        self.relative_weight_multiplier_denominator
    }

    #[must_use]
    pub const fn adjusted_relative_weight_numerator(self) -> u32 {
        self.adjusted_relative_weight_numerator
    }

    #[must_use]
    pub const fn adjusted_relative_weight_denominator(self) -> u16 {
        self.adjusted_relative_weight_denominator
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum FishingRodLevelXPolicyError {
    #[error(
        "Fishing Rod enchant {0:?} is not owned by the scalar Level X modifier policy; use its dedicated policy"
    )]
    DedicatedPolicy(FishingRodEnchant),
    #[error("Treasure Level X scalar policy returned an unexpected non-Treasure effect")]
    UnexpectedTreasurePolicy,
    #[error("Treasure Level X unexpectedly affects internal Treasure result weights")]
    TreasureAffectsInternalResultWeights,
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

/// Applies the frozen Treasure-enchant Level X scalar to one canonical catch-branch row exactly.
///
/// The `+80%` value remains owned by [`fishing_rod_level_x_policy`]. This preview reads that scalar,
/// reduces it to the exact `9/5` relative-weight multiplier, and applies it only to the Treasure
/// branch. Fish and Junk remain unchanged. The canonical base weights continue to come from
/// [`fishing_base_catch_branch_policy`].
///
/// This is a Treasure-X-only pre-normalization preview. It does not extrapolate Levels I-IX, compose
/// Treasure Bait or Gold Rod, apply shared Fishing caps, normalize the branch table, perform RNG, or
/// alter the internal within-Treasure result table. If the scalar owner later stops representing
/// Treasure with these frozen composition semantics, this preview fails closed instead of panicking
/// or silently changing the internal Treasure result table.
pub fn preview_treasure_level_x_branch_weight(
    branch: FishingCatchBranch,
) -> Result<TreasureLevelXBranchWeightPreview, FishingRodLevelXPolicyError> {
    let policy = fishing_rod_level_x_policy(FishingRodEnchant::Treasure)?;
    let FishingRodLevelXEffect::Treasure {
        treasure_branch_relative_weight_increase_percent,
        affects_internal_treasure_result_weights,
    } = policy.effect
    else {
        return Err(FishingRodLevelXPolicyError::UnexpectedTreasurePolicy);
    };
    if affects_internal_treasure_result_weights {
        return Err(FishingRodLevelXPolicyError::TreasureAffectsInternalResultWeights);
    }

    let base_relative_weight = fishing_base_catch_branch_policy(branch).relative_weight;
    let treasure_level_x_applied = branch == FishingCatchBranch::Treasure;
    let (relative_weight_multiplier_numerator, relative_weight_multiplier_denominator) =
        if treasure_level_x_applied {
            relative_weight_multiplier_from_increase_percent(
                treasure_branch_relative_weight_increase_percent,
            )
        } else {
            (1, 1)
        };

    Ok(TreasureLevelXBranchWeightPreview {
        branch,
        base_relative_weight,
        treasure_level_x_applied,
        relative_weight_multiplier_numerator,
        relative_weight_multiplier_denominator,
        adjusted_relative_weight_numerator: u32::from(base_relative_weight)
            * u32::from(relative_weight_multiplier_numerator),
        adjusted_relative_weight_denominator: relative_weight_multiplier_denominator,
    })
}

const fn relative_weight_multiplier_from_increase_percent(increase_percent: u8) -> (u16, u16) {
    let numerator = 100_u16 + increase_percent as u16;
    let denominator = 100_u16;
    let divisor = gcd(numerator, denominator);
    (numerator / divisor, denominator / divisor)
}

const fn gcd(mut left: u16, mut right: u16) -> u16 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
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

    #[test]
    fn treasure_level_x_boosts_only_treasure_branch_before_normalization() {
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
}
