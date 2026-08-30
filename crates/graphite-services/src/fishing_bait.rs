use serde::Serialize;
use thiserror::Error;

use crate::fishing_droptable::{FishingCatchBranch, fishing_base_catch_branch_policy};
pub(crate) use crate::fishing_limits::MAX_FISH_PER_CAST;

pub const BAIT_RACK_MAX_LEVEL: u8 = 3;
pub const NATIVE_ACTIVE_BAIT_CATEGORY_SLOTS: u8 = 3;
pub const BAIT_RACK_ACTIVE_SLOTS_PER_LEVEL: u8 = 1;
pub const MAX_ACTIVE_BAIT_CATEGORY_SLOTS: u8 = 6;
pub const BAIT_UNITS_CONSUMED_PER_ACTIVE_CATEGORY_PER_CAST: u8 = 1;

const SCHOOL_BAIT_EXTRA_FISH_COUNT: u8 = 1;

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

const TREASURE_BAIT_FISH_BRANCH_WEIGHT_FACTOR: FishingBaitRatio = ratio(1, 1);
const TREASURE_BAIT_JUNK_BRANCH_WEIGHT_FACTOR: FishingBaitRatio = ratio(9, 10);
const TREASURE_BAIT_TREASURE_BRANCH_WEIGHT_FACTOR: FishingBaitRatio = ratio(23, 20);

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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SchoolBaitProcResolution {
    NotTriggered,
    Triggered,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SchoolBaitNoExtraFishReason {
    ProcNotTriggered,
    GlobalFishCapReached,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SchoolBaitQuantityResolution {
    Unchanged {
        fish_count: u8,
        reason: SchoolBaitNoExtraFishReason,
    },
    AddOneSameAreaFish {
        initial_fish_count: u8,
        final_fish_count: u8,
    },
}

impl SchoolBaitQuantityResolution {
    #[must_use]
    pub const fn final_fish_count(self) -> u8 {
        match self {
            Self::Unchanged { fish_count, .. } => fish_count,
            Self::AddOneSameAreaFish {
                final_fish_count, ..
            } => final_fish_count,
        }
    }

    #[must_use]
    pub const fn extra_fish_count(self) -> u8 {
        match self {
            Self::Unchanged { .. } => 0,
            Self::AddOneSameAreaFish { .. } => SCHOOL_BAIT_EXTRA_FISH_COUNT,
        }
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum SchoolBaitQuantityError {
    #[error("School Bait quantity resolution requires a Fish result; got {0:?}")]
    RequiresFishResult(FishingCatchBranch),
    #[error("School Bait current fish count must be between 1 and {MAX_FISH_PER_CAST}; got {0}")]
    FishCountOutOfRange(u8),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct TreasureBaitBranchWeightPreview {
    pub branch: FishingCatchBranch,
    pub base_relative_weight: u16,
    pub relative_weight_factor: FishingBaitRatio,
    adjusted_relative_weight_numerator: u32,
    adjusted_relative_weight_denominator: u16,
}

impl TreasureBaitBranchWeightPreview {
    #[must_use]
    pub const fn adjusted_relative_weight_numerator(self) -> u32 {
        self.adjusted_relative_weight_numerator
    }

    #[must_use]
    pub const fn adjusted_relative_weight_denominator(self) -> u16 {
        self.adjusted_relative_weight_denominator
    }
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
                extra_fish_count: SCHOOL_BAIT_EXTRA_FISH_COUNT,
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
                treasure_branch_relative_weight_factor: TREASURE_BAIT_TREASURE_BRANCH_WEIGHT_FACTOR,
                junk_branch_relative_weight_factor: TREASURE_BAIT_JUNK_BRANCH_WEIGHT_FACTOR,
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

/// Applies one already-authoritative School Bait proc result to an existing Fish-result count.
///
/// School Bait is eligible only after a Fish branch has produced at least one candidate fish. A
/// triggered proc adds exactly one extra **same-area** fish, never recursively, while the shared
/// global five-fish ceiling remains authoritative. A trigger at the ceiling therefore adds nothing
/// rather than overflowing or replacing an existing fish.
///
/// `current_fish_count` is intentionally agnostic to how prior independent quantity policy reached
/// that count. In particular, this kernel does not choose an ordering between School Bait and
/// Multicatch, and it does not decide whether any future quantity stage runs before or after it.
/// The future owning Fishing lifecycle must compose those independent stages once under the shared
/// cap. The proc evidence is supplied by the caller because this function does not draw RNG.
///
/// `AddOneSameAreaFish` is a count/area requirement only. This policy does not select the additional
/// fish's species, rarity, weight, variant, biological noise, AEXP contribution, or FishInstance
/// identity, and it does not consume bait or activate `/fish`.
pub fn resolve_school_bait_quantity(
    branch: FishingCatchBranch,
    current_fish_count: u8,
    proc_resolution: SchoolBaitProcResolution,
) -> Result<SchoolBaitQuantityResolution, SchoolBaitQuantityError> {
    if branch != FishingCatchBranch::Fish {
        return Err(SchoolBaitQuantityError::RequiresFishResult(branch));
    }
    if !(1..=MAX_FISH_PER_CAST).contains(&current_fish_count) {
        return Err(SchoolBaitQuantityError::FishCountOutOfRange(
            current_fish_count,
        ));
    }

    Ok(match proc_resolution {
        SchoolBaitProcResolution::NotTriggered => SchoolBaitQuantityResolution::Unchanged {
            fish_count: current_fish_count,
            reason: SchoolBaitNoExtraFishReason::ProcNotTriggered,
        },
        SchoolBaitProcResolution::Triggered if current_fish_count == MAX_FISH_PER_CAST => {
            SchoolBaitQuantityResolution::Unchanged {
                fish_count: current_fish_count,
                reason: SchoolBaitNoExtraFishReason::GlobalFishCapReached,
            }
        }
        SchoolBaitProcResolution::Triggered => SchoolBaitQuantityResolution::AddOneSameAreaFish {
            initial_fish_count: current_fish_count,
            final_fish_count: current_fish_count + SCHOOL_BAIT_EXTRA_FISH_COUNT,
        },
    })
}

/// Applies Treasure Bait to one row of the frozen zero-modifier catch-branch table exactly.
///
/// Treasure Bait changes relative selection weights **before normalization**: Treasure is multiplied
/// by 1.15 (`23/20`), Junk by 0.90 (`9/10`), and Fish is unchanged (`1/1`). This preview keeps the
/// transformed weight as an exact rational, so `7 × 23/20` remains `161/20` rather than being
/// rounded to an integer or misread as a percentage-point change.
///
/// The input baseline comes from [`fishing_base_catch_branch_policy`]. This function intentionally
/// represents Treasure Bait in isolation: it does not compose the Gold Rod Treasure modifier,
/// shared Fishing caps, final branch normalization, RNG selection, bait consumption, or settlement.
/// A later owner must compose all active modifier buckets according to the shared Fishing pipeline
/// before normalizing once.
#[must_use]
pub const fn preview_treasure_bait_base_branch_weight(
    branch: FishingCatchBranch,
) -> TreasureBaitBranchWeightPreview {
    let base_relative_weight = fishing_base_catch_branch_policy(branch).relative_weight;
    let relative_weight_factor = match branch {
        FishingCatchBranch::Fish => TREASURE_BAIT_FISH_BRANCH_WEIGHT_FACTOR,
        FishingCatchBranch::Junk => TREASURE_BAIT_JUNK_BRANCH_WEIGHT_FACTOR,
        FishingCatchBranch::Treasure => TREASURE_BAIT_TREASURE_BRANCH_WEIGHT_FACTOR,
    };

    TreasureBaitBranchWeightPreview {
        branch,
        base_relative_weight,
        relative_weight_factor,
        adjusted_relative_weight_numerator: base_relative_weight as u32
            * relative_weight_factor.numerator as u32,
        adjusted_relative_weight_denominator: relative_weight_factor.denominator,
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
        assert_eq!(extra_fish_count, SCHOOL_BAIT_EXTRA_FISH_COUNT);
        assert!(non_recursive);
        assert_eq!(max_total_fish_per_cast, MAX_FISH_PER_CAST);

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

    #[test]
    fn school_bait_quantity_requires_a_valid_fish_result_count() {
        for branch in [FishingCatchBranch::Junk, FishingCatchBranch::Treasure] {
            assert_eq!(
                resolve_school_bait_quantity(branch, 1, SchoolBaitProcResolution::Triggered),
                Err(SchoolBaitQuantityError::RequiresFishResult(branch))
            );
        }
        for fish_count in [0, MAX_FISH_PER_CAST + 1, u8::MAX] {
            assert_eq!(
                resolve_school_bait_quantity(
                    FishingCatchBranch::Fish,
                    fish_count,
                    SchoolBaitProcResolution::Triggered,
                ),
                Err(SchoolBaitQuantityError::FishCountOutOfRange(fish_count))
            );
        }
    }

    #[test]
    fn school_bait_no_proc_preserves_every_valid_existing_count() {
        for fish_count in 1..=MAX_FISH_PER_CAST {
            let resolution = resolve_school_bait_quantity(
                FishingCatchBranch::Fish,
                fish_count,
                SchoolBaitProcResolution::NotTriggered,
            )
            .unwrap();
            assert_eq!(
                resolution,
                SchoolBaitQuantityResolution::Unchanged {
                    fish_count,
                    reason: SchoolBaitNoExtraFishReason::ProcNotTriggered,
                }
            );
            assert_eq!(resolution.final_fish_count(), fish_count);
            assert_eq!(resolution.extra_fish_count(), 0);
        }
    }

    #[test]
    fn school_bait_trigger_adds_one_same_area_fish_until_global_cap() {
        for fish_count in 1..MAX_FISH_PER_CAST {
            let resolution = resolve_school_bait_quantity(
                FishingCatchBranch::Fish,
                fish_count,
                SchoolBaitProcResolution::Triggered,
            )
            .unwrap();
            assert_eq!(
                resolution,
                SchoolBaitQuantityResolution::AddOneSameAreaFish {
                    initial_fish_count: fish_count,
                    final_fish_count: fish_count + SCHOOL_BAIT_EXTRA_FISH_COUNT,
                }
            );
            assert_eq!(
                resolution.final_fish_count(),
                fish_count + SCHOOL_BAIT_EXTRA_FISH_COUNT
            );
            assert_eq!(resolution.extra_fish_count(), SCHOOL_BAIT_EXTRA_FISH_COUNT);
        }

        let capped = resolve_school_bait_quantity(
            FishingCatchBranch::Fish,
            MAX_FISH_PER_CAST,
            SchoolBaitProcResolution::Triggered,
        )
        .unwrap();
        assert_eq!(
            capped,
            SchoolBaitQuantityResolution::Unchanged {
                fish_count: MAX_FISH_PER_CAST,
                reason: SchoolBaitNoExtraFishReason::GlobalFishCapReached,
            }
        );
        assert_eq!(capped.final_fish_count(), MAX_FISH_PER_CAST);
        assert_eq!(capped.extra_fish_count(), 0);
    }

    #[test]
    fn treasure_bait_base_branch_weights_remain_exact_before_normalization() {
        let fish = preview_treasure_bait_base_branch_weight(FishingCatchBranch::Fish);
        assert_eq!(fish.base_relative_weight, 176);
        assert_eq!(
            (
                fish.relative_weight_factor.numerator(),
                fish.relative_weight_factor.denominator()
            ),
            (1, 1)
        );
        assert_eq!(
            (
                fish.adjusted_relative_weight_numerator(),
                fish.adjusted_relative_weight_denominator()
            ),
            (176, 1)
        );

        let junk = preview_treasure_bait_base_branch_weight(FishingCatchBranch::Junk);
        assert_eq!(junk.base_relative_weight, 17);
        assert_eq!(
            (
                junk.relative_weight_factor.numerator(),
                junk.relative_weight_factor.denominator()
            ),
            (9, 10)
        );
        assert_eq!(
            (
                junk.adjusted_relative_weight_numerator(),
                junk.adjusted_relative_weight_denominator()
            ),
            (153, 10)
        );

        let treasure = preview_treasure_bait_base_branch_weight(FishingCatchBranch::Treasure);
        assert_eq!(treasure.base_relative_weight, 7);
        assert_eq!(
            (
                treasure.relative_weight_factor.numerator(),
                treasure.relative_weight_factor.denominator()
            ),
            (23, 20)
        );
        assert_eq!(
            (
                treasure.adjusted_relative_weight_numerator(),
                treasure.adjusted_relative_weight_denominator()
            ),
            (161, 20)
        );
    }
}
