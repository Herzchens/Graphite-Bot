use serde::Serialize;
use thiserror::Error;

use crate::{
    FishingArea, FishingCatchBranch, FishingRarity, FishingRodEnchant, FishingRodLevelXEffect,
    FishingRodLevelXPolicyError, FishingSpecies, fishing_area_species_pool,
    fishing_base_catch_branch_policy, fishing_rod_level_x_policy, fishing_species_policy,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct LuckOfSeaXBranchWeightPreviewV1 {
    pub branch: FishingCatchBranch,
    pub base_relative_weight: u16,
    pub junk_reduction_applied: bool,
    factor_numerator: u16,
    factor_denominator: u16,
    adjusted_weight_numerator: u32,
    adjusted_weight_denominator: u16,
}

impl LuckOfSeaXBranchWeightPreviewV1 {
    #[must_use]
    pub const fn factor_numerator(self) -> u16 {
        self.factor_numerator
    }

    #[must_use]
    pub const fn factor_denominator(self) -> u16 {
        self.factor_denominator
    }

    #[must_use]
    pub const fn adjusted_weight_numerator(self) -> u32 {
        self.adjusted_weight_numerator
    }

    #[must_use]
    pub const fn adjusted_weight_denominator(self) -> u16 {
        self.adjusted_weight_denominator
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct LuckOfSeaXSpeciesWeightPreviewV1 {
    pub area: FishingArea,
    pub species: FishingSpecies,
    pub rarity: FishingRarity,
    pub base_pool_weight: u16,
    pub rare_species_boost_applied: bool,
    factor_numerator: u16,
    factor_denominator: u16,
    adjusted_weight_numerator: u32,
    adjusted_weight_denominator: u16,
}

impl LuckOfSeaXSpeciesWeightPreviewV1 {
    #[must_use]
    pub const fn factor_numerator(self) -> u16 {
        self.factor_numerator
    }

    #[must_use]
    pub const fn factor_denominator(self) -> u16 {
        self.factor_denominator
    }

    #[must_use]
    pub const fn adjusted_weight_numerator(self) -> u32 {
        self.adjusted_weight_numerator
    }

    #[must_use]
    pub const fn adjusted_weight_denominator(self) -> u16 {
        self.adjusted_weight_denominator
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ManualFishingLuckOfSeaXError {
    #[error(transparent)]
    Policy(#[from] FishingRodLevelXPolicyError),
    #[error("Luck of the Sea X scalar policy returned an unexpected effect")]
    UnexpectedPolicy,
    #[error("Luck of the Sea X must resolve before FishInstance creation")]
    InvalidResolutionStage,
    #[error("Luck of the Sea X scalar percentages exceed the supported exact range")]
    InvalidScalar,
}

/// Applies only the frozen Luck-of-the-Sea-X Junk reduction to one catch-branch row.
///
/// This is deliberately a pre-cap, pre-normalization preview. The shared Fishing
/// modifier-cap owner must compose this with Gold Rod, bait, events, or any other
/// branch source before a final weighted draw can be authorized.
pub fn preview_luck_of_sea_x_branch_weight_v1(
    branch: FishingCatchBranch,
) -> Result<LuckOfSeaXBranchWeightPreviewV1, ManualFishingLuckOfSeaXError> {
    let (rare_increase, junk_decrease) = luck_of_sea_x_scalars()?;
    let _ = rare_increase;
    if junk_decrease > 100 {
        return Err(ManualFishingLuckOfSeaXError::InvalidScalar);
    }
    let applied = branch == FishingCatchBranch::Junk;
    let (factor_numerator, factor_denominator) = if applied {
        (u16::from(100 - junk_decrease), 100)
    } else {
        (1, 1)
    };
    let base = fishing_base_catch_branch_policy(branch).relative_weight;
    Ok(LuckOfSeaXBranchWeightPreviewV1 {
        branch,
        base_relative_weight: base,
        junk_reduction_applied: applied,
        factor_numerator,
        factor_denominator,
        adjusted_weight_numerator: u32::from(base) * u32::from(factor_numerator),
        adjusted_weight_denominator: factor_denominator,
    })
}

/// Applies only the frozen Luck-of-the-Sea-X rare-or-better relative multiplier
/// to one canonical area/species row.
///
/// Rare, Epic, Legendary and Mythic rows receive exactly 1.60 (8/5). Common and
/// Uncommon stay unchanged. This function never normalizes or samples the pool.
pub fn preview_luck_of_sea_x_species_weight_v1(
    area: FishingArea,
    species: FishingSpecies,
) -> Result<Option<LuckOfSeaXSpeciesWeightPreviewV1>, ManualFishingLuckOfSeaXError> {
    let (rare_increase, _junk_decrease) = luck_of_sea_x_scalars()?;
    let base = fishing_area_species_pool(area)
        .iter()
        .find(|row| row.species == species);
    let Some(base) = base else {
        return Ok(None);
    };
    let rarity = fishing_species_policy(species).rarity;
    let applied = matches!(
        rarity,
        FishingRarity::Rare
            | FishingRarity::Epic
            | FishingRarity::Legendary
            | FishingRarity::Mythic
    );
    let (factor_numerator, factor_denominator) = if applied {
        let numerator = u16::from(
            100_u8
                .checked_add(rare_increase)
                .ok_or(ManualFishingLuckOfSeaXError::InvalidScalar)?,
        );
        (numerator, 100)
    } else {
        (1, 1)
    };
    Ok(Some(LuckOfSeaXSpeciesWeightPreviewV1 {
        area,
        species,
        rarity,
        base_pool_weight: base.pool_weight,
        rare_species_boost_applied: applied,
        factor_numerator,
        factor_denominator,
        adjusted_weight_numerator: u32::from(base.pool_weight) * u32::from(factor_numerator),
        adjusted_weight_denominator: factor_denominator,
    }))
}

fn luck_of_sea_x_scalars() -> Result<(u8, u8), ManualFishingLuckOfSeaXError> {
    let policy = fishing_rod_level_x_policy(FishingRodEnchant::LuckOfTheSea)?;
    let FishingRodLevelXEffect::LuckOfTheSea {
        rare_species_relative_weight_increase_percent,
        junk_relative_weight_decrease_percent,
        resolves_before_fish_instance_creation,
    } = policy.effect
    else {
        return Err(ManualFishingLuckOfSeaXError::UnexpectedPolicy);
    };
    if !resolves_before_fish_instance_creation {
        return Err(ManualFishingLuckOfSeaXError::InvalidResolutionStage);
    }
    Ok((
        rare_species_relative_weight_increase_percent,
        junk_relative_weight_decrease_percent,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREAS: [FishingArea; 6] = [
        FishingArea::StarterPool,
        FishingArea::River,
        FishingArea::Lake,
        FishingArea::Coast,
        FishingArea::DeepSea,
        FishingArea::Abyss,
    ];

    #[test]
    fn branch_preview_uses_exact_seven_tenths_only_for_junk() {
        let fish = preview_luck_of_sea_x_branch_weight_v1(FishingCatchBranch::Fish).unwrap();
        let junk = preview_luck_of_sea_x_branch_weight_v1(FishingCatchBranch::Junk).unwrap();
        let treasure =
            preview_luck_of_sea_x_branch_weight_v1(FishingCatchBranch::Treasure).unwrap();

        assert_eq!((fish.factor_numerator(), fish.factor_denominator()), (1, 1));
        assert_eq!(
            (junk.factor_numerator(), junk.factor_denominator()),
            (70, 100)
        );
        assert_eq!(
            (treasure.factor_numerator(), treasure.factor_denominator()),
            (1, 1)
        );
        assert_eq!(
            (
                fish.adjusted_weight_numerator(),
                junk.adjusted_weight_numerator(),
                treasure.adjusted_weight_numerator(),
            ),
            (176, 1_190, 7)
        );
    }

    #[test]
    fn species_preview_applies_exact_eight_fifths_only_to_rare_or_better_rows() {
        let mut rows = 0;
        let mut boosted = 0;
        for area in AREAS {
            for row in fishing_area_species_pool(area) {
                rows += 1;
                let preview = preview_luck_of_sea_x_species_weight_v1(area, row.species)
                    .unwrap()
                    .unwrap();
                let eligible = matches!(
                    preview.rarity,
                    FishingRarity::Rare
                        | FishingRarity::Epic
                        | FishingRarity::Legendary
                        | FishingRarity::Mythic
                );
                boosted += usize::from(eligible);
                assert_eq!(preview.rare_species_boost_applied, eligible);
                assert_eq!(
                    (preview.factor_numerator(), preview.factor_denominator()),
                    if eligible { (160, 100) } else { (1, 1) }
                );
            }
        }
        assert_eq!(rows, crate::CANONICAL_FISH_AREA_ROWS);
        assert!(boosted > 0);
    }

    #[test]
    fn species_preview_rejects_noncanonical_area_species_pair() {
        assert_eq!(
            preview_luck_of_sea_x_species_weight_v1(
                FishingArea::StarterPool,
                FishingSpecies::LeviathanFry,
            )
            .unwrap(),
            None
        );
    }
}
