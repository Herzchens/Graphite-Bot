use serde::Serialize;
use thiserror::Error;

pub const BAIT_RACK_MAX_LEVEL: u8 = 3;
pub const NATIVE_ACTIVE_BAIT_CATEGORY_SLOTS: u8 = 3;
pub const BAIT_RACK_ACTIVE_SLOTS_PER_LEVEL: u8 = 1;
pub const MAX_ACTIVE_BAIT_CATEGORY_SLOTS: u8 = 6;
pub const BAIT_UNITS_CONSUMED_PER_ACTIVE_CATEGORY_PER_CAST: u8 = 1;
pub const MAX_FISH_PER_CAST: u8 = 5;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct BaitRackCapacityPolicy {
    pub bait_rack_level: Option<u8>,
    pub native_active_bait_category_slots: u8,
    pub additional_active_bait_category_slots: u8,
    pub active_bait_category_slots: u8,
    pub max_active_bait_category_slots: u8,
    pub rod_only: bool,
    pub occupies_one_normal_rod_enchant_slot: bool,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum BaitRackPolicyError {
    #[error("Bait Rack level must be between I and III when present; got {0}")]
    LevelOutOfRange(u8),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FishingBait {
    School,
    Quality,
    Rare,
    Treasure,
    Sturdy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FishingBaitCategory {
    Quantity,
    Quality,
    SpeciesQuality,
    Treasure,
    Safety,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FishingRarity {
    Common,
    Uncommon,
    Rare,
    Epic,
    Legendary,
    Mythic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FishingBaitRatio {
    numerator: u16,
    denominator: u16,
}

impl FishingBaitRatio {
    pub const fn numerator(self) -> u16 {
        self.numerator
    }

    pub const fn denominator(self) -> u16 {
        self.denominator
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FishingBaitEffect {
    School {
        requires_fish_result: bool,
        extra_same_area_fish_chance: FishingBaitRatio,
        extra_fish_count: u8,
        non_recursive: bool,
        max_total_fish_per_cast: u8,
    },
    Quality {
        sampled_fish_weight_center_factor: FishingBaitRatio,
        non_normal_variant_relative_weight_factor: FishingBaitRatio,
    },
    Rare {
        affected_species_rarities: [FishingRarity; 4],
        eligible_species_relative_weight_factor: FishingBaitRatio,
    },
    Treasure {
        treasure_branch_relative_weight_factor: FishingBaitRatio,
        junk_branch_relative_weight_factor: FishingBaitRatio,
    },
    Sturdy {
        line_strength_factor: FishingBaitRatio,
        final_line_break_chance_factor: FishingBaitRatio,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FishingBaitPolicy {
    pub bait: FishingBait,
    pub category: FishingBaitCategory,
    pub shop_price_per_unit: i64,
    pub units_consumed_per_cast: u8,
    pub optional_money_sink: bool,
    pub effects_apply_on_independent_axes_before_shared_caps: bool,
    pub multi_catch_consumes_extra_bait: bool,
    pub multi_treasure_consumes_extra_bait: bool,
    pub effect: FishingBaitEffect,
}

/// Resolves the frozen active bait-category capacity for a normal Fishing Rod.
///
/// Normal fishing starts with three native active bait-category slots. Bait Rack is a Rod-only
/// normal enchant that adds exactly one active bait category per level through Level III, producing
/// capacities four, five, and six. The enchant occupies exactly one normal Rod enchant slot
/// regardless of level.
///
/// `None` represents a Rod without Bait Rack. A present level must be canonical I-III; `Some(0)` and
/// levels above III are rejected instead of being treated as absence or silently clamped. This pure
/// policy does not validate authoritative ItemInstance enchant state, select bait categories,
/// consume bait, or activate Fishing gameplay.
pub fn bait_rack_capacity_policy(
    bait_rack_level: Option<u8>,
) -> Result<BaitRackCapacityPolicy, BaitRackPolicyError> {
    let level = match bait_rack_level {
        None => 0,
        Some(level @ 1..=BAIT_RACK_MAX_LEVEL) => level,
        Some(level) => return Err(BaitRackPolicyError::LevelOutOfRange(level)),
    };

    let additional_active_bait_category_slots = level * BAIT_RACK_ACTIVE_SLOTS_PER_LEVEL;
    let active_bait_category_slots =
        NATIVE_ACTIVE_BAIT_CATEGORY_SLOTS + additional_active_bait_category_slots;

    debug_assert!(active_bait_category_slots <= MAX_ACTIVE_BAIT_CATEGORY_SLOTS);

    Ok(BaitRackCapacityPolicy {
        bait_rack_level,
        native_active_bait_category_slots: NATIVE_ACTIVE_BAIT_CATEGORY_SLOTS,
        additional_active_bait_category_slots,
        active_bait_category_slots,
        max_active_bait_category_slots: MAX_ACTIVE_BAIT_CATEGORY_SLOTS,
        rod_only: true,
        occupies_one_normal_rod_enchant_slot: bait_rack_level.is_some(),
    })
}

/// Returns the frozen normal-Fishing bait catalog row.
///
/// The returned ratios are exact policy factors, not already-normalized probabilities or a runtime
/// sampling algorithm. Quality/Rare/Treasure effects are intended to transform their documented
/// relative weights before the future Fishing resolver normalizes the eligible pool. Sturdy exposes
/// only its exact line-strength and final line-break multipliers; this function deliberately does
/// not evaluate the unresolved fractional-power base line-break formula.
///
/// One unit of every active bait category is consumed per cast. Multi Catch and Multi Treasure do
/// not cause extra bait consumption. Baits are optional Money sinks; this pure catalog does not
/// purchase, select, reserve, consume, or persist bait and does not activate `/fish`.
pub const fn fishing_bait_policy(bait: FishingBait) -> FishingBaitPolicy {
    let (category, shop_price_per_unit, effect) = match bait {
        FishingBait::School => (
            FishingBaitCategory::Quantity,
            35,
            FishingBaitEffect::School {
                requires_fish_result: true,
                extra_same_area_fish_chance: ratio(2, 25),
                extra_fish_count: 1,
                non_recursive: true,
                max_total_fish_per_cast: MAX_FISH_PER_CAST,
            },
        ),
        FishingBait::Quality => (
            FishingBaitCategory::Quality,
            50,
            FishingBaitEffect::Quality {
                sampled_fish_weight_center_factor: ratio(21, 20),
                non_normal_variant_relative_weight_factor: ratio(11, 10),
            },
        ),
        FishingBait::Rare => (
            FishingBaitCategory::SpeciesQuality,
            75,
            FishingBaitEffect::Rare {
                affected_species_rarities: [
                    FishingRarity::Rare,
                    FishingRarity::Epic,
                    FishingRarity::Legendary,
                    FishingRarity::Mythic,
                ],
                eligible_species_relative_weight_factor: ratio(28, 25),
            },
        ),
        FishingBait::Treasure => (
            FishingBaitCategory::Treasure,
            100,
            FishingBaitEffect::Treasure {
                treasure_branch_relative_weight_factor: ratio(23, 20),
                junk_branch_relative_weight_factor: ratio(9, 10),
            },
        ),
        FishingBait::Sturdy => (
            FishingBaitCategory::Safety,
            50,
            FishingBaitEffect::Sturdy {
                line_strength_factor: ratio(11, 10),
                final_line_break_chance_factor: ratio(9, 10),
            },
        ),
    };

    FishingBaitPolicy {
        bait,
        category,
        shop_price_per_unit,
        units_consumed_per_cast: BAIT_UNITS_CONSUMED_PER_ACTIVE_CATEGORY_PER_CAST,
        optional_money_sink: true,
        effects_apply_on_independent_axes_before_shared_caps: true,
        multi_catch_consumes_extra_bait: false,
        multi_treasure_consumes_extra_bait: false,
        effect,
    }
}

const fn ratio(numerator: u16, denominator: u16) -> FishingBaitRatio {
    FishingBaitRatio {
        numerator,
        denominator,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_bait_rack_preserves_three_native_active_categories() {
        assert_eq!(
            bait_rack_capacity_policy(None).unwrap(),
            BaitRackCapacityPolicy {
                bait_rack_level: None,
                native_active_bait_category_slots: 3,
                additional_active_bait_category_slots: 0,
                active_bait_category_slots: 3,
                max_active_bait_category_slots: 6,
                rod_only: true,
                occupies_one_normal_rod_enchant_slot: false,
            }
        );
    }

    #[test]
    fn levels_one_through_three_expand_capacity_to_four_through_six() {
        for (level, expected_capacity) in [(1, 4), (2, 5), (3, 6)] {
            let policy = bait_rack_capacity_policy(Some(level)).unwrap();
            assert_eq!(policy.bait_rack_level, Some(level));
            assert_eq!(policy.native_active_bait_category_slots, 3);
            assert_eq!(policy.additional_active_bait_category_slots, level);
            assert_eq!(policy.active_bait_category_slots, expected_capacity);
            assert_eq!(policy.max_active_bait_category_slots, 6);
            assert!(policy.rod_only);
            assert!(policy.occupies_one_normal_rod_enchant_slot);
        }
    }

    #[test]
    fn present_noncanonical_levels_fail_closed() {
        for level in [0, 4, u8::MAX] {
            assert_eq!(
                bait_rack_capacity_policy(Some(level)),
                Err(BaitRackPolicyError::LevelOutOfRange(level))
            );
        }
    }

    #[test]
    fn bait_catalog_preserves_exact_categories_prices_and_consumption() {
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
            assert_eq!(policy.units_consumed_per_cast, 1);
            assert!(policy.optional_money_sink);
            assert!(policy.effects_apply_on_independent_axes_before_shared_caps);
            assert!(!policy.multi_catch_consumes_extra_bait);
            assert!(!policy.multi_treasure_consumes_extra_bait);
        }
    }

    #[test]
    fn school_and_sturdy_effects_preserve_exact_frozen_ratios() {
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
        assert_eq!(max_total_fish_per_cast, 5);

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

    #[test]
    fn quality_rare_and_treasure_effects_preserve_relative_weight_factors() {
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
    }
}
