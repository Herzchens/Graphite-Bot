use serde::Serialize;
use thiserror::Error;

use crate::{
    equipment_policy::EquipmentTier,
    fishing_bait::{
        FishingBait, FishingBaitEffect, FishingRarity, MAX_FISH_PER_CAST, fishing_bait_policy,
    },
};

pub const NORMAL_ROD_DURABILITY_PER_COMPLETED_CAST_ATTEMPT: u8 = 1;
pub const STRENGTHEN_MAX_LEVEL: u8 = 10;

const STRENGTHEN_FACTOR_DENOMINATOR: u128 = 25;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FishingRodBaseStats {
    pub tier: EquipmentTier,
    pub base_line_strength_grams_tension: u32,
    pub base_durability: u32,
    pub gold_side_grade: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FishingTensionRatio {
    numerator: u16,
    denominator: u16,
}

impl FishingTensionRatio {
    #[must_use]
    pub const fn numerator(self) -> u16 {
        self.numerator
    }

    #[must_use]
    pub const fn denominator(self) -> u16 {
        self.denominator
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FishingTension {
    pub rarity: FishingRarity,
    pub source_weight_grams: u64,
    numerator_gram_tension: u64,
    denominator: u16,
}

impl FishingTension {
    #[must_use]
    pub const fn numerator_gram_tension(self) -> u64 {
        self.numerator_gram_tension
    }

    #[must_use]
    pub const fn denominator(self) -> u16 {
        self.denominator
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FishingCatchLoad {
    pub fish_count: u8,
    numerator_gram_tension: u128,
    denominator: u16,
}

impl FishingCatchLoad {
    #[must_use]
    pub const fn numerator_gram_tension(self) -> u128 {
        self.numerator_gram_tension
    }

    #[must_use]
    pub const fn denominator(self) -> u16 {
        self.denominator
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ManualFishingLineStrength {
    pub tier: EquipmentTier,
    pub strengthen_level: Option<u8>,
    pub sturdy_bait_active: bool,
    numerator_gram_tension: u128,
    denominator: u16,
}

impl ManualFishingLineStrength {
    #[must_use]
    pub const fn numerator_gram_tension(self) -> u128 {
        self.numerator_gram_tension
    }

    #[must_use]
    pub const fn denominator(self) -> u16 {
        self.denominator
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FishingCapabilityClassification {
    WithinRodCapability,
    OverRodCapability,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FishingCapabilityRatio {
    pub classification: FishingCapabilityClassification,
    numerator: u128,
    denominator: u128,
}

impl FishingCapabilityRatio {
    #[must_use]
    pub const fn numerator(self) -> u128 {
        self.numerator
    }

    #[must_use]
    pub const fn denominator(self) -> u128 {
        self.denominator
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum FishingCapabilityError {
    #[error("Fishing Rod definition is not an ordinary Rod")]
    NotOrdinaryFishingRod,
    #[error("Starter Leather is not an ordinary Fishing Rod tier")]
    StarterLeatherIsNotOrdinaryRodTier,
    #[error("fish weight must be positive")]
    ZeroFishWeight,
    #[error("fish candidate count must be between 1 and {MAX_FISH_PER_CAST}; got {0}")]
    FishCountOutOfRange(usize),
    #[error("Strengthen level must be between I and X when present; got {0}")]
    StrengthenLevelOutOfRange(u8),
    #[error("fish tension arithmetic exceeded supported integer bounds")]
    ArithmeticOverflow,
}

/// Resolves the frozen base line strength and ordinary durability for an ordinary Fishing Rod.
///
/// `is_ordinary_rod` must be derived from authoritative versioned item-definition state before this
/// function is called. Tier alone is insufficient because the separate system-bound Starter Basic
/// Rod currently carries Wood-like metadata but must not inherit the ordinary Wood durability row.
///
/// Line strength is represented exactly as integer gram-tension instead of the specification's
/// kilogram-tension display unit. Gold remains an explicit side-grade, so callers must not derive
/// progression order from the numeric line-strength or durability values.
pub const fn ordinary_fishing_rod_base_stats(
    tier: EquipmentTier,
    is_ordinary_rod: bool,
) -> Result<FishingRodBaseStats, FishingCapabilityError> {
    if !is_ordinary_rod {
        return Err(FishingCapabilityError::NotOrdinaryFishingRod);
    }

    let (base_line_strength_grams_tension, base_durability, gold_side_grade) = match tier {
        EquipmentTier::StarterLeather => {
            return Err(FishingCapabilityError::StarterLeatherIsNotOrdinaryRodTier);
        }
        EquipmentTier::Wood => (6_000, 600, false),
        EquipmentTier::Stone => (10_000, 900, false),
        EquipmentTier::Copper => (18_000, 1_400, false),
        EquipmentTier::Gold => (40_000, 550, true),
        EquipmentTier::Iron => (30_000, 2_200, false),
        EquipmentTier::Diamond => (55_000, 3_300, false),
        EquipmentTier::Obsidian => (85_000, 5_000, false),
        EquipmentTier::Netherite => (120_000, 7_600, false),
        EquipmentTier::Graphite => (160_000, 11_000, false),
    };

    Ok(FishingRodBaseStats {
        tier,
        base_line_strength_grams_tension,
        base_durability,
        gold_side_grade,
    })
}

/// Resolves the frozen rarity-to-tension multiplier as an exact reduced rational number.
#[must_use]
pub const fn fishing_rarity_tension_multiplier(rarity: FishingRarity) -> FishingTensionRatio {
    let (numerator, denominator) = match rarity {
        FishingRarity::Common => (1, 1),
        FishingRarity::Uncommon => (11, 10),
        FishingRarity::Rare => (5, 4),
        FishingRarity::Epic => (29, 20),
        FishingRarity::Legendary => (7, 4),
        FishingRarity::Mythic => (11, 5),
    };

    FishingTensionRatio {
        numerator,
        denominator,
    }
}

/// Computes exact FishTension for a positive fish weight without rounding.
///
/// The result is normalized to a reduced rational number in gram-tension. This is a deterministic
/// prerequisite for CatchLoad and Rod-capability evaluation; it deliberately does not evaluate the
/// unresolved `(R - 1)^1.30` line-break term or perform any RNG draw.
pub fn fishing_tension(
    weight_grams: u64,
    rarity: FishingRarity,
) -> Result<FishingTension, FishingCapabilityError> {
    if weight_grams == 0 {
        return Err(FishingCapabilityError::ZeroFishWeight);
    }

    let multiplier = fishing_rarity_tension_multiplier(rarity);
    let numerator = weight_grams
        .checked_mul(u64::from(multiplier.numerator()))
        .ok_or(FishingCapabilityError::ArithmeticOverflow)?;
    let denominator = u64::from(multiplier.denominator());
    let divisor = gcd_u64(numerator, denominator);

    Ok(FishingTension {
        rarity,
        source_weight_grams: weight_grams,
        numerator_gram_tension: numerator / divisor,
        denominator: u16::try_from(denominator / divisor)
            .map_err(|_| FishingCapabilityError::ArithmeticOverflow)?,
    })
}

/// Sums all fish tensions in one candidate fish catch exactly.
///
/// A fish branch must contain between one and five fish. Treasure and junk branches deliberately do
/// not use this API because they have no line tension. The five-fish ceiling is the shared Fishing
/// cap after Multi Catch and School Bait composition.
pub fn fishing_catch_load(
    tensions: &[FishingTension],
) -> Result<FishingCatchLoad, FishingCapabilityError> {
    if tensions.is_empty() || tensions.len() > usize::from(MAX_FISH_PER_CAST) {
        return Err(FishingCapabilityError::FishCountOutOfRange(tensions.len()));
    }

    let mut numerator = 0_u128;
    let mut denominator = 1_u128;

    for tension in tensions {
        let tension_denominator = u128::from(tension.denominator());
        let denominator_gcd = gcd_u128(denominator, tension_denominator);
        let common_denominator = denominator
            .checked_div(denominator_gcd)
            .and_then(|value| value.checked_mul(tension_denominator))
            .ok_or(FishingCapabilityError::ArithmeticOverflow)?;
        let left_scale = common_denominator / denominator;
        let right_scale = common_denominator / tension_denominator;
        let scaled_left = numerator
            .checked_mul(left_scale)
            .ok_or(FishingCapabilityError::ArithmeticOverflow)?;
        let scaled_right = u128::from(tension.numerator_gram_tension())
            .checked_mul(right_scale)
            .ok_or(FishingCapabilityError::ArithmeticOverflow)?;

        numerator = scaled_left
            .checked_add(scaled_right)
            .ok_or(FishingCapabilityError::ArithmeticOverflow)?;
        denominator = common_denominator;

        let divisor = gcd_u128(numerator, denominator);
        numerator /= divisor;
        denominator /= divisor;
    }

    Ok(FishingCatchLoad {
        fish_count: u8::try_from(tensions.len())
            .map_err(|_| FishingCapabilityError::ArithmeticOverflow)?,
        numerator_gram_tension: numerator,
        denominator: u16::try_from(denominator)
            .map_err(|_| FishingCapabilityError::ArithmeticOverflow)?,
    })
}

/// Computes manual EffectiveLineStrength exactly for an authoritative ordinary Rod definition.
///
/// Strengthen contributes +4% line strength per canonical level I-X. Sturdy Bait contributes its
/// catalog-owned x1.10 line-strength factor. Manual fishing fixes AutomationStrengthFactor at 1, so
/// Auto Fisher Strength is intentionally outside this API until its numeric policy is authoritative.
/// `None` means the Rod has no Strengthen enchant; a present level must be I-X and is never silently
/// clamped.
pub fn manual_fishing_line_strength(
    tier: EquipmentTier,
    is_ordinary_rod: bool,
    strengthen_level: Option<u8>,
    sturdy_bait_active: bool,
) -> Result<ManualFishingLineStrength, FishingCapabilityError> {
    let base_stats = ordinary_fishing_rod_base_stats(tier, is_ordinary_rod)?;
    let strengthen_level_value = match strengthen_level {
        None => 0,
        Some(level @ 1..=STRENGTHEN_MAX_LEVEL) => level,
        Some(level) => return Err(FishingCapabilityError::StrengthenLevelOutOfRange(level)),
    };

    let strengthen_numerator = STRENGTHEN_FACTOR_DENOMINATOR
        .checked_add(u128::from(strengthen_level_value))
        .ok_or(FishingCapabilityError::ArithmeticOverflow)?;
    let mut numerator = u128::from(base_stats.base_line_strength_grams_tension)
        .checked_mul(strengthen_numerator)
        .ok_or(FishingCapabilityError::ArithmeticOverflow)?;
    let mut denominator = STRENGTHEN_FACTOR_DENOMINATOR;

    if sturdy_bait_active {
        let FishingBaitEffect::Sturdy {
            line_strength_factor,
            ..
        } = fishing_bait_policy(FishingBait::Sturdy).effect
        else {
            unreachable!("Sturdy Bait must expose the Sturdy effect variant");
        };

        numerator = numerator
            .checked_mul(u128::from(line_strength_factor.numerator()))
            .ok_or(FishingCapabilityError::ArithmeticOverflow)?;
        denominator = denominator
            .checked_mul(u128::from(line_strength_factor.denominator()))
            .ok_or(FishingCapabilityError::ArithmeticOverflow)?;
    }

    let divisor = gcd_u128(numerator, denominator);
    numerator /= divisor;
    denominator /= divisor;

    Ok(ManualFishingLineStrength {
        tier,
        strengthen_level,
        sturdy_bait_active,
        numerator_gram_tension: numerator,
        denominator: u16::try_from(denominator)
            .map_err(|_| FishingCapabilityError::ArithmeticOverflow)?,
    })
}

/// Computes `R = CatchLoad / EffectiveLineStrength` exactly for manual fishing.
///
/// `WithinRodCapability` means `R <= 1`, so the frozen capability rule grants the fish load with
/// 100% capability success and zero line-break chance. `OverRodCapability` means the later resolver
/// must continue into the unresolved fractional-power line-break kernel before any over-cap catch
/// roll can occur.
pub fn manual_fishing_capability_ratio(
    catch_load: FishingCatchLoad,
    line_strength: ManualFishingLineStrength,
) -> Result<FishingCapabilityRatio, FishingCapabilityError> {
    let numerator = catch_load
        .numerator_gram_tension()
        .checked_mul(u128::from(line_strength.denominator()))
        .ok_or(FishingCapabilityError::ArithmeticOverflow)?;
    let denominator = u128::from(catch_load.denominator())
        .checked_mul(line_strength.numerator_gram_tension())
        .ok_or(FishingCapabilityError::ArithmeticOverflow)?;
    let divisor = gcd_u128(numerator, denominator);
    let numerator = numerator / divisor;
    let denominator = denominator / divisor;
    let classification = if numerator <= denominator {
        FishingCapabilityClassification::WithinRodCapability
    } else {
        FishingCapabilityClassification::OverRodCapability
    };

    Ok(FishingCapabilityRatio {
        classification,
        numerator,
        denominator,
    })
}

const fn gcd_u64(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

const fn gcd_u128(mut left: u128, mut right: u128) -> u128 {
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
    fn ordinary_rod_table_matches_every_frozen_tier() {
        let expected = [
            (EquipmentTier::Wood, 6_000, 600, false),
            (EquipmentTier::Stone, 10_000, 900, false),
            (EquipmentTier::Copper, 18_000, 1_400, false),
            (EquipmentTier::Gold, 40_000, 550, true),
            (EquipmentTier::Iron, 30_000, 2_200, false),
            (EquipmentTier::Diamond, 55_000, 3_300, false),
            (EquipmentTier::Obsidian, 85_000, 5_000, false),
            (EquipmentTier::Netherite, 120_000, 7_600, false),
            (EquipmentTier::Graphite, 160_000, 11_000, false),
        ];

        for (tier, line_strength, durability, gold_side_grade) in expected {
            assert_eq!(
                ordinary_fishing_rod_base_stats(tier, true),
                Ok(FishingRodBaseStats {
                    tier,
                    base_line_strength_grams_tension: line_strength,
                    base_durability: durability,
                    gold_side_grade,
                })
            );
        }
    }

    #[test]
    fn non_ordinary_wood_definition_cannot_borrow_the_ordinary_wood_row() {
        assert_eq!(
            ordinary_fishing_rod_base_stats(EquipmentTier::Wood, false),
            Err(FishingCapabilityError::NotOrdinaryFishingRod)
        );
    }

    #[test]
    fn starter_leather_cannot_be_interpreted_as_an_ordinary_rod() {
        assert_eq!(
            ordinary_fishing_rod_base_stats(EquipmentTier::StarterLeather, true),
            Err(FishingCapabilityError::StarterLeatherIsNotOrdinaryRodTier)
        );
    }

    #[test]
    fn rarity_tension_multipliers_match_the_frozen_table() {
        let expected = [
            (FishingRarity::Common, 1, 1),
            (FishingRarity::Uncommon, 11, 10),
            (FishingRarity::Rare, 5, 4),
            (FishingRarity::Epic, 29, 20),
            (FishingRarity::Legendary, 7, 4),
            (FishingRarity::Mythic, 11, 5),
        ];

        for (rarity, numerator, denominator) in expected {
            let multiplier = fishing_rarity_tension_multiplier(rarity);
            assert_eq!(multiplier.numerator(), numerator);
            assert_eq!(multiplier.denominator(), denominator);
        }
    }

    #[test]
    fn fish_tension_is_exact_and_reduced() {
        let legendary = fishing_tension(400, FishingRarity::Legendary).unwrap();
        assert_eq!(legendary.numerator_gram_tension(), 700);
        assert_eq!(legendary.denominator(), 1);

        let uncommon = fishing_tension(1, FishingRarity::Uncommon).unwrap();
        assert_eq!(uncommon.numerator_gram_tension(), 11);
        assert_eq!(uncommon.denominator(), 10);
    }

    #[test]
    fn fish_tension_rejects_impossible_or_unsupported_inputs() {
        assert_eq!(
            fishing_tension(0, FishingRarity::Common),
            Err(FishingCapabilityError::ZeroFishWeight)
        );
        assert_eq!(
            fishing_tension(u64::MAX, FishingRarity::Mythic),
            Err(FishingCapabilityError::ArithmeticOverflow)
        );
    }

    #[test]
    fn catch_load_sums_fractional_tensions_exactly() {
        let tensions = [
            fishing_tension(1, FishingRarity::Uncommon).unwrap(),
            fishing_tension(1, FishingRarity::Epic).unwrap(),
        ];
        let load = fishing_catch_load(&tensions).unwrap();

        assert_eq!(load.fish_count, 2);
        assert_eq!(load.numerator_gram_tension(), 51);
        assert_eq!(load.denominator(), 20);
    }

    #[test]
    fn catch_load_rejects_non_fish_or_over_cap_candidates() {
        assert_eq!(
            fishing_catch_load(&[]),
            Err(FishingCapabilityError::FishCountOutOfRange(0))
        );

        let tension = fishing_tension(1, FishingRarity::Common).unwrap();
        let six_fish = [tension; 6];
        assert_eq!(
            fishing_catch_load(&six_fish),
            Err(FishingCapabilityError::FishCountOutOfRange(6))
        );
    }

    #[test]
    fn manual_line_strength_composes_strengthen_and_sturdy_exactly() {
        let base = manual_fishing_line_strength(EquipmentTier::Wood, true, None, false).unwrap();
        assert_eq!(base.numerator_gram_tension(), 6_000);
        assert_eq!(base.denominator(), 1);

        let strengthen_x =
            manual_fishing_line_strength(EquipmentTier::Wood, true, Some(10), false).unwrap();
        assert_eq!(strengthen_x.numerator_gram_tension(), 8_400);
        assert_eq!(strengthen_x.denominator(), 1);

        let strengthen_x_sturdy =
            manual_fishing_line_strength(EquipmentTier::Wood, true, Some(10), true).unwrap();
        assert_eq!(strengthen_x_sturdy.numerator_gram_tension(), 9_240);
        assert_eq!(strengthen_x_sturdy.denominator(), 1);
    }

    #[test]
    fn manual_line_strength_rejects_noncanonical_strengthen_levels() {
        for level in [0, STRENGTHEN_MAX_LEVEL + 1, u8::MAX] {
            assert_eq!(
                manual_fishing_line_strength(EquipmentTier::Wood, true, Some(level), false),
                Err(FishingCapabilityError::StrengthenLevelOutOfRange(level))
            );
        }
    }

    #[test]
    fn manual_capability_ratio_classifies_exact_boundary_without_float() {
        let mythic = [fishing_tension(3_000, FishingRarity::Mythic).unwrap()];
        let load = fishing_catch_load(&mythic).unwrap();

        let base = manual_fishing_line_strength(EquipmentTier::Wood, true, None, false).unwrap();
        let over_cap = manual_fishing_capability_ratio(load, base).unwrap();
        assert_eq!(over_cap.numerator(), 11);
        assert_eq!(over_cap.denominator(), 10);
        assert_eq!(
            over_cap.classification,
            FishingCapabilityClassification::OverRodCapability
        );

        let sturdy = manual_fishing_line_strength(EquipmentTier::Wood, true, None, true).unwrap();
        let boundary = manual_fishing_capability_ratio(load, sturdy).unwrap();
        assert_eq!(boundary.numerator(), 1);
        assert_eq!(boundary.denominator(), 1);
        assert_eq!(
            boundary.classification,
            FishingCapabilityClassification::WithinRodCapability
        );
    }

    #[test]
    fn normal_rod_cast_cost_is_one_durability_event() {
        assert_eq!(NORMAL_ROD_DURABILITY_PER_COMPLETED_CAST_ATTEMPT, 1);
    }
}
