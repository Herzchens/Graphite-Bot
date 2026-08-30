use serde::Serialize;
use thiserror::Error;

use crate::{equipment_policy::EquipmentTier, fishing_bait::FishingRarity};

pub const NORMAL_ROD_DURABILITY_PER_COMPLETED_CAST_ATTEMPT: u8 = 1;

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

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum FishingCapabilityError {
    #[error("Starter Leather is not an ordinary Fishing Rod tier")]
    StarterLeatherIsNotOrdinaryRodTier,
    #[error("fish weight must be positive")]
    ZeroFishWeight,
    #[error("fish tension arithmetic exceeded supported integer bounds")]
    ArithmeticOverflow,
}

/// Resolves the frozen base line strength and ordinary durability for an ordinary Fishing Rod.
///
/// Line strength is represented exactly as integer gram-tension instead of the specification's
/// kilogram-tension display unit. The Starter Basic Rod is intentionally excluded: it is a separate
/// system-bound, unbreakable definition and must not inherit the ordinary Wood durability row.
/// Gold remains an explicit side-grade, so callers must not derive progression order from the
/// numeric line-strength or durability values.
pub const fn ordinary_fishing_rod_base_stats(
    tier: EquipmentTier,
) -> Result<FishingRodBaseStats, FishingCapabilityError> {
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
/// prerequisite for future CatchLoad and Rod-capability evaluation; it deliberately does not
/// evaluate the unresolved `(R - 1)^1.30` line-break term or perform any RNG draw.
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
    let divisor = gcd(numerator, denominator);

    Ok(FishingTension {
        rarity,
        source_weight_grams: weight_grams,
        numerator_gram_tension: numerator / divisor,
        denominator: u16::try_from(denominator / divisor)
            .map_err(|_| FishingCapabilityError::ArithmeticOverflow)?,
    })
}

const fn gcd(mut left: u64, mut right: u64) -> u64 {
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
                ordinary_fishing_rod_base_stats(tier),
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
    fn starter_leather_cannot_be_interpreted_as_an_ordinary_rod() {
        assert_eq!(
            ordinary_fishing_rod_base_stats(EquipmentTier::StarterLeather),
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
    fn normal_rod_cast_cost_is_one_durability_event() {
        assert_eq!(NORMAL_ROD_DURABILITY_PER_COMPLETED_CAST_ATTEMPT, 1);
    }
}
