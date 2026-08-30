use serde::Serialize;
use thiserror::Error;

use crate::{
    equipment_policy::EquipmentTier,
    fishing_area::FishingArea,
    fishing_bait::FishingRarity,
    fishing_capability::{FishingCapabilityError, ordinary_fishing_rod_base_stats},
    fishing_droptable::{FishingCatchBranch, fishing_base_catch_branch_policy},
    fishing_species::{FishingSpecies, fishing_area_species_pool, fishing_species_policy},
};

pub const GOLD_ROD_ACTION_SPEED_RATING_PERCENT: u8 = 10;
pub const GOLD_ROD_RARE_OR_BETTER_RELATIVE_WEIGHT_PERCENT: u8 = 15;
pub const GOLD_ROD_TREASURE_RELATIVE_WEIGHT_PERCENT: u8 = 15;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FishingRelativeWeightMultiplier {
    numerator: u16,
    denominator: u16,
}

impl FishingRelativeWeightMultiplier {
    #[must_use]
    pub const fn numerator(self) -> u16 {
        self.numerator
    }

    #[must_use]
    pub const fn denominator(self) -> u16 {
        self.denominator
    }
}

const IDENTITY_RELATIVE_WEIGHT_MULTIPLIER: FishingRelativeWeightMultiplier =
    FishingRelativeWeightMultiplier {
        numerator: 1,
        denominator: 1,
    };

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GoldFishingRodModifierStage {
    BeforeSharedFishingCaps,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct GoldFishingRodSideGradePolicy {
    pub action_speed_rating_percent: u8,
    pub rare_or_better_species_relative_weight_multiplier: FishingRelativeWeightMultiplier,
    pub treasure_branch_relative_weight_multiplier: FishingRelativeWeightMultiplier,
    pub modifier_stage: GoldFishingRodModifierStage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct GoldFishingRodCatchBranchWeightPreview {
    pub branch: FishingCatchBranch,
    pub base_relative_weight: u16,
    pub gold_modifier_applied: bool,
    pub relative_weight_multiplier: FishingRelativeWeightMultiplier,
    adjusted_relative_weight_numerator: u32,
    adjusted_relative_weight_denominator: u16,
}

impl GoldFishingRodCatchBranchWeightPreview {
    #[must_use]
    pub const fn adjusted_relative_weight_numerator(self) -> u32 {
        self.adjusted_relative_weight_numerator
    }

    #[must_use]
    pub const fn adjusted_relative_weight_denominator(self) -> u16 {
        self.adjusted_relative_weight_denominator
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct GoldFishingRodSpeciesWeightPreview {
    pub area: FishingArea,
    pub species: FishingSpecies,
    pub rarity: FishingRarity,
    pub base_pool_weight: u16,
    pub gold_modifier_applied: bool,
    pub relative_weight_multiplier: FishingRelativeWeightMultiplier,
    adjusted_pool_weight_numerator: u32,
    adjusted_pool_weight_denominator: u16,
}

impl GoldFishingRodSpeciesWeightPreview {
    #[must_use]
    pub const fn adjusted_pool_weight_numerator(self) -> u32 {
        self.adjusted_pool_weight_numerator
    }

    #[must_use]
    pub const fn adjusted_pool_weight_denominator(self) -> u16 {
        self.adjusted_pool_weight_denominator
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum GoldFishingRodPolicyError {
    #[error(transparent)]
    InvalidOrdinaryRod(#[from] FishingCapabilityError),
    #[error("ordinary Fishing Rod is not the Gold side-grade")]
    NotGoldFishingRod,
    #[error("species {species:?} is not in Fishing area {area:?}")]
    SpeciesNotInArea {
        area: FishingArea,
        species: FishingSpecies,
    },
}

/// Resolves the fixed pre-cap modifiers of the ordinary Gold Fishing Rod side-grade.
///
/// The caller must supply authoritative ordinary-Rod classification. This reuses the ordinary Rod
/// capability owner so a system-bound or special definition cannot obtain Gold side-grade bonuses
/// merely by carrying Gold-like tier metadata.
///
/// The +10% value is an action-speed **rating**, not a direct duration multiplier. The two +15%
/// values are relative selection-weight multipliers (23/20), not +15 percentage points of final
/// probability. All three modifiers feed their respective shared Fishing modifier buckets before
/// shared caps. This policy intentionally does not normalize weights, apply shared caps, or convert
/// speed rating into a cooldown/duration because those composition rules are not owned here.
pub fn gold_fishing_rod_side_grade_policy(
    tier: EquipmentTier,
    is_ordinary_rod: bool,
) -> Result<GoldFishingRodSideGradePolicy, GoldFishingRodPolicyError> {
    let base_stats = ordinary_fishing_rod_base_stats(tier, is_ordinary_rod)?;
    if !base_stats.gold_side_grade {
        return Err(GoldFishingRodPolicyError::NotGoldFishingRod);
    }

    let relative_weight_multiplier =
        relative_weight_multiplier_from_percent(GOLD_ROD_RARE_OR_BETTER_RELATIVE_WEIGHT_PERCENT);
    let treasure_relative_weight_multiplier =
        relative_weight_multiplier_from_percent(GOLD_ROD_TREASURE_RELATIVE_WEIGHT_PERCENT);

    Ok(GoldFishingRodSideGradePolicy {
        action_speed_rating_percent: GOLD_ROD_ACTION_SPEED_RATING_PERCENT,
        rare_or_better_species_relative_weight_multiplier: relative_weight_multiplier,
        treasure_branch_relative_weight_multiplier: treasure_relative_weight_multiplier,
        modifier_stage: GoldFishingRodModifierStage::BeforeSharedFishingCaps,
    })
}

/// Applies the ordinary Gold Fishing Rod's Treasure-branch side-grade to one canonical catch row.
///
/// Only the Treasure branch receives the Gold Rod's catalog-owned `23/20` relative-weight
/// multiplier; Fish and Junk remain unchanged. The caller must prove this is an authoritative
/// ordinary Gold Fishing Rod through the same boundary as [`gold_fishing_rod_side_grade_policy`].
///
/// This preview is Gold-Rod-only and intentionally remains before shared Fishing caps and final
/// normalization. It does not compose Treasure Bait or any later modifier, perform RNG, consume Rod
/// durability, or settle a catch.
pub fn preview_gold_fishing_rod_catch_branch_weight(
    tier: EquipmentTier,
    is_ordinary_rod: bool,
    branch: FishingCatchBranch,
) -> Result<GoldFishingRodCatchBranchWeightPreview, GoldFishingRodPolicyError> {
    let policy = gold_fishing_rod_side_grade_policy(tier, is_ordinary_rod)?;
    let base_relative_weight = fishing_base_catch_branch_policy(branch).relative_weight;
    let gold_modifier_applied = branch == FishingCatchBranch::Treasure;
    let relative_weight_multiplier = if gold_modifier_applied {
        policy.treasure_branch_relative_weight_multiplier
    } else {
        IDENTITY_RELATIVE_WEIGHT_MULTIPLIER
    };

    Ok(GoldFishingRodCatchBranchWeightPreview {
        branch,
        base_relative_weight,
        gold_modifier_applied,
        relative_weight_multiplier,
        adjusted_relative_weight_numerator: u32::from(base_relative_weight)
            * u32::from(relative_weight_multiplier.numerator()),
        adjusted_relative_weight_denominator: relative_weight_multiplier.denominator(),
    })
}

/// Applies the ordinary Gold Fishing Rod's rare-or-better side-grade to one canonical area row.
///
/// Rare, Epic, Legendary, and Mythic species receive the Gold Rod's catalog-owned `23/20`
/// relative-weight multiplier; Common and Uncommon species remain unchanged. The base pool weight
/// and rarity are re-derived from canonical Fishing owners so callers cannot fabricate an
/// area/species weight pair. A species absent from the requested area fails closed.
///
/// This preview is Gold-Rod-only and intentionally remains before shared Fishing caps and final
/// species-pool normalization. It does not compose Rare Bait, Luck, RNG selection, FishInstance
/// creation, bait consumption, AEXP, or settlement.
pub fn preview_gold_fishing_rod_species_weight(
    tier: EquipmentTier,
    is_ordinary_rod: bool,
    area: FishingArea,
    species: FishingSpecies,
) -> Result<GoldFishingRodSpeciesWeightPreview, GoldFishingRodPolicyError> {
    let policy = gold_fishing_rod_side_grade_policy(tier, is_ordinary_rod)?;
    let base_row = fishing_area_species_pool(area)
        .iter()
        .find(|row| row.species == species)
        .ok_or(GoldFishingRodPolicyError::SpeciesNotInArea { area, species })?;
    let rarity = fishing_species_policy(species).rarity;
    let gold_modifier_applied = is_rare_or_better(rarity);
    let relative_weight_multiplier = if gold_modifier_applied {
        policy.rare_or_better_species_relative_weight_multiplier
    } else {
        IDENTITY_RELATIVE_WEIGHT_MULTIPLIER
    };

    Ok(GoldFishingRodSpeciesWeightPreview {
        area,
        species,
        rarity,
        base_pool_weight: base_row.pool_weight,
        gold_modifier_applied,
        relative_weight_multiplier,
        adjusted_pool_weight_numerator: u32::from(base_row.pool_weight)
            * u32::from(relative_weight_multiplier.numerator()),
        adjusted_pool_weight_denominator: relative_weight_multiplier.denominator(),
    })
}

const fn is_rare_or_better(rarity: FishingRarity) -> bool {
    matches!(
        rarity,
        FishingRarity::Rare
            | FishingRarity::Epic
            | FishingRarity::Legendary
            | FishingRarity::Mythic
    )
}

const fn relative_weight_multiplier_from_percent(
    relative_increase_percent: u8,
) -> FishingRelativeWeightMultiplier {
    let numerator = 100_u16 + relative_increase_percent as u16;
    let denominator = 100_u16;
    let divisor = gcd(numerator, denominator);

    FishingRelativeWeightMultiplier {
        numerator: numerator / divisor,
        denominator: denominator / divisor,
    }
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

    const ALL_AREAS: [FishingArea; 6] = [
        FishingArea::StarterPool,
        FishingArea::River,
        FishingArea::Lake,
        FishingArea::Coast,
        FishingArea::DeepSea,
        FishingArea::Abyss,
    ];

    #[test]
    fn gold_rod_policy_matches_all_frozen_side_grade_inputs() {
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
    fn non_gold_ordinary_rod_cannot_borrow_side_grade_modifiers() {
        assert_eq!(
            gold_fishing_rod_side_grade_policy(EquipmentTier::Diamond, true),
            Err(GoldFishingRodPolicyError::NotGoldFishingRod)
        );
    }

    #[test]
    fn non_ordinary_gold_like_definition_fails_closed() {
        assert_eq!(
            gold_fishing_rod_side_grade_policy(EquipmentTier::Gold, false),
            Err(GoldFishingRodPolicyError::InvalidOrdinaryRod(
                FishingCapabilityError::NotOrdinaryFishingRod
            ))
        );
    }

    #[test]
    fn gold_rod_boosts_only_the_treasure_branch_before_normalization() {
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
    fn gold_rod_boosts_every_rare_or_better_canonical_species_row_only() {
        let mut rows_seen = 0;

        for area in ALL_AREAS {
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
                let eligible = is_rare_or_better(rarity);
                let expected_factor = if eligible { (23, 20) } else { (1, 1) };

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

        assert_eq!(rows_seen, crate::CANONICAL_FISH_AREA_ROWS);
    }

    #[test]
    fn gold_rod_species_preview_rejects_noncanonical_area_pairs() {
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
    }

    #[test]
    fn weight_previews_preserve_gold_rod_trust_boundary() {
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
}
