use serde::Serialize;

use crate::fishing_bait::{FishingBait, FishingBaitEffect, fishing_bait_policy};

pub const CANONICAL_FISH_VARIANT_COUNT: usize = 5;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FishingVariant {
    Normal,
    Silver,
    Golden,
    Albino,
    Iridescent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FishingVariantRatio {
    numerator: u16,
    denominator: u16,
}

impl FishingVariantRatio {
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
pub struct FishingVariantPolicy {
    pub variant: FishingVariant,
    pub base_probability: FishingVariantRatio,
    pub value_multiplier: FishingVariantRatio,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct QualityBaitVariantWeightPreview {
    pub variant: FishingVariant,
    pub base_probability: FishingVariantRatio,
    pub quality_bait_applied: bool,
    relative_weight_factor_numerator: u16,
    relative_weight_factor_denominator: u16,
    adjusted_relative_weight_numerator: u32,
    adjusted_relative_weight_denominator: u32,
}

impl QualityBaitVariantWeightPreview {
    #[must_use]
    pub const fn relative_weight_factor_numerator(self) -> u16 {
        self.relative_weight_factor_numerator
    }

    #[must_use]
    pub const fn relative_weight_factor_denominator(self) -> u16 {
        self.relative_weight_factor_denominator
    }

    #[must_use]
    pub const fn adjusted_relative_weight_numerator(self) -> u32 {
        self.adjusted_relative_weight_numerator
    }

    #[must_use]
    pub const fn adjusted_relative_weight_denominator(self) -> u32 {
        self.adjusted_relative_weight_denominator
    }
}

const NORMAL_POLICY: FishingVariantPolicy = variant_policy(FishingVariant::Normal, 47, 50, 1, 1);
const SILVER_POLICY: FishingVariantPolicy = variant_policy(FishingVariant::Silver, 3, 100, 5, 4);
const GOLDEN_POLICY: FishingVariantPolicy = variant_policy(FishingVariant::Golden, 3, 200, 7, 4);
const ALBINO_POLICY: FishingVariantPolicy = variant_policy(FishingVariant::Albino, 1, 100, 2, 1);
const IRIDESCENT_POLICY: FishingVariantPolicy =
    variant_policy(FishingVariant::Iridescent, 1, 200, 3, 1);

const VARIANT_CATALOG: [FishingVariantPolicy; CANONICAL_FISH_VARIANT_COUNT] = [
    NORMAL_POLICY,
    SILVER_POLICY,
    GOLDEN_POLICY,
    ALBINO_POLICY,
    IRIDESCENT_POLICY,
];

/// Resolves the frozen base probability and NPC-value multiplier for one fish variant.
///
/// Both values are exact reduced rational numbers. The probability is the canonical unmodified
/// variant distribution only; future Luck/Quality effects may alter eligible variant weights before
/// normalization, so callers must not treat this function as a modifier-aware selector.
///
/// The value multiplier is an exact factor for the future Fish NPC valuation formula. This policy
/// performs no intermediate rounding and deliberately does not evaluate the unresolved
/// `(Weight / Wref)^0.85` term. Species rarity is not part of Money valuation because the latest
/// canonical specification already prices rarity into each species' base NPC Money value.
#[must_use]
pub const fn fishing_variant_policy(variant: FishingVariant) -> FishingVariantPolicy {
    match variant {
        FishingVariant::Normal => NORMAL_POLICY,
        FishingVariant::Silver => SILVER_POLICY,
        FishingVariant::Golden => GOLDEN_POLICY,
        FishingVariant::Albino => ALBINO_POLICY,
        FishingVariant::Iridescent => IRIDESCENT_POLICY,
    }
}

/// Returns all canonical base fish-variant rows in specification order.
#[must_use]
pub const fn fishing_variant_catalog() -> &'static [FishingVariantPolicy] {
    &VARIANT_CATALOG
}

/// Applies Quality Bait to one canonical fish-variant row without normalizing the variant pool.
///
/// Quality Bait multiplies every non-Normal variant's relative selection weight by `1.10`
/// (`11/10`) before normalization. Normal remains unchanged. The `11/10` factor is read from the
/// existing Quality Bait catalog row so this module does not create a second source of truth for bait
/// semantics.
///
/// `base_probability` is the canonical zero-modifier probability row, but after applying the factor
/// it is used only as a relative selection weight. The returned adjusted numerator/denominator are
/// therefore not a final probability and deliberately do not sum to one across the five rows.
///
/// This policy does not implement Quality Bait's separate sampled-weight-center factor because the
/// authoritative fish-weight sampler and deterministic fractional-power/log-normal evaluation are
/// still outside this slice. It also does not compose Luck, shared Fishing caps, final normalization,
/// RNG selection, FishInstance creation, bait consumption, AEXP, or settlement.
#[must_use]
pub fn preview_quality_bait_variant_weight(
    variant: FishingVariant,
) -> QualityBaitVariantWeightPreview {
    let base_policy = fishing_variant_policy(variant);
    let FishingBaitEffect::Quality {
        non_normal_variant_relative_weight_factor,
        ..
    } = fishing_bait_policy(FishingBait::Quality).effect
    else {
        unreachable!("Quality Bait catalog row returned a non-Quality effect")
    };

    let quality_bait_applied = !matches!(variant, FishingVariant::Normal);
    let (relative_weight_factor_numerator, relative_weight_factor_denominator) =
        if quality_bait_applied {
            (
                non_normal_variant_relative_weight_factor.numerator(),
                non_normal_variant_relative_weight_factor.denominator(),
            )
        } else {
            (1, 1)
        };

    QualityBaitVariantWeightPreview {
        variant,
        base_probability: base_policy.base_probability,
        quality_bait_applied,
        relative_weight_factor_numerator,
        relative_weight_factor_denominator,
        adjusted_relative_weight_numerator: u32::from(base_policy.base_probability.numerator())
            * u32::from(relative_weight_factor_numerator),
        adjusted_relative_weight_denominator: u32::from(base_policy.base_probability.denominator())
            * u32::from(relative_weight_factor_denominator),
    }
}

const fn variant_policy(
    variant: FishingVariant,
    probability_numerator: u16,
    probability_denominator: u16,
    value_numerator: u16,
    value_denominator: u16,
) -> FishingVariantPolicy {
    FishingVariantPolicy {
        variant,
        base_probability: FishingVariantRatio {
            numerator: probability_numerator,
            denominator: probability_denominator,
        },
        value_multiplier: FishingVariantRatio {
            numerator: value_numerator,
            denominator: value_denominator,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_contains_exactly_five_valid_variants() {
        let catalog = fishing_variant_catalog();
        assert_eq!(catalog.len(), CANONICAL_FISH_VARIANT_COUNT);

        for row in catalog {
            assert_eq!(*row, fishing_variant_policy(row.variant));
            assert!(row.base_probability.denominator() > 0);
            assert!(row.value_multiplier.denominator() > 0);
        }
    }

    #[test]
    fn base_probabilities_sum_to_exactly_one() {
        const COMMON_DENOMINATOR: u32 = 200;

        let numerator_sum = fishing_variant_catalog()
            .iter()
            .map(|row| {
                let ratio = row.base_probability;
                let denominator = u32::from(ratio.denominator());
                assert_eq!(COMMON_DENOMINATOR % denominator, 0);
                u32::from(ratio.numerator()) * (COMMON_DENOMINATOR / denominator)
            })
            .sum::<u32>();

        assert_eq!(numerator_sum, COMMON_DENOMINATOR);
    }

    #[test]
    fn non_normal_probability_is_exactly_six_percent() {
        const COMMON_DENOMINATOR: u32 = 200;

        let numerator_sum = fishing_variant_catalog()
            .iter()
            .filter(|row| row.variant != FishingVariant::Normal)
            .map(|row| {
                let ratio = row.base_probability;
                u32::from(ratio.numerator()) * (COMMON_DENOMINATOR / u32::from(ratio.denominator()))
            })
            .sum::<u32>();

        assert_eq!(numerator_sum, 12);
    }

    #[test]
    fn value_multipliers_match_the_frozen_table() {
        let expected = [
            (FishingVariant::Normal, 1, 1),
            (FishingVariant::Silver, 5, 4),
            (FishingVariant::Golden, 7, 4),
            (FishingVariant::Albino, 2, 1),
            (FishingVariant::Iridescent, 3, 1),
        ];

        for (variant, numerator, denominator) in expected {
            let ratio = fishing_variant_policy(variant).value_multiplier;
            assert_eq!(ratio.numerator(), numerator);
            assert_eq!(ratio.denominator(), denominator);
        }
    }

    #[test]
    fn quality_bait_boosts_only_non_normal_variant_weights_exactly() {
        let expected = [
            (FishingVariant::Normal, false, (1, 1), (47, 50)),
            (FishingVariant::Silver, true, (11, 10), (33, 1_000)),
            (FishingVariant::Golden, true, (11, 10), (33, 2_000)),
            (FishingVariant::Albino, true, (11, 10), (11, 1_000)),
            (FishingVariant::Iridescent, true, (11, 10), (11, 2_000)),
        ];

        for (variant, applied, factor, adjusted) in expected {
            let preview = preview_quality_bait_variant_weight(variant);
            assert_eq!(preview.variant, variant);
            assert_eq!(
                preview.base_probability,
                fishing_variant_policy(variant).base_probability
            );
            assert_eq!(preview.quality_bait_applied, applied);
            assert_eq!(
                (
                    preview.relative_weight_factor_numerator(),
                    preview.relative_weight_factor_denominator(),
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
    fn quality_bait_preview_remains_pre_normalization_relative_weight() {
        const COMMON_DENOMINATOR: u32 = 2_000;

        let adjusted_weight_sum = fishing_variant_catalog()
            .iter()
            .map(|row| {
                let preview = preview_quality_bait_variant_weight(row.variant);
                let denominator = preview.adjusted_relative_weight_denominator();
                assert_eq!(COMMON_DENOMINATOR % denominator, 0);
                preview.adjusted_relative_weight_numerator() * (COMMON_DENOMINATOR / denominator)
            })
            .sum::<u32>();

        assert_eq!(adjusted_weight_sum, 2_012);
        assert_ne!(adjusted_weight_sum, COMMON_DENOMINATOR);
    }
}
