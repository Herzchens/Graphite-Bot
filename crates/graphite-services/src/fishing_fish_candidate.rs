use thiserror::Error;

use crate::{
    FishingArea, FishingFishInstanceDeliveryRequestV1, FishingNumericError, ManualFishingPpmDrawV1,
    ManualFishingRngContext, ManualFishingRngDrawError, ManualFishingWeightedDrawV1,
    fishing_area_species_pool, fishing_variant_catalog, resolve_fish_instance_numeric_v1,
};

pub const BASE_FISH_VARIANT_WEIGHT_SCALE_V1: u64 = 200;

#[derive(Debug, Error)]
pub enum ManualFishingUnmodifiedFishCandidateError {
    #[error(transparent)]
    Rng(#[from] ManualFishingRngDrawError),
    #[error(transparent)]
    Numeric(#[from] FishingNumericError),
    #[error("canonical Fishing area species pool is empty")]
    EmptySpeciesPool,
    #[error("canonical Fishing variant table is empty")]
    EmptyVariantPool,
    #[error("canonical Fishing variant probability cannot be represented on the v1 integer scale")]
    UnsupportedVariantProbability,
}

/// Previews exactly one *unmodified* fish candidate from the operation's pinned RNG domains.
///
/// This selector is valid only when the owning cast has verified that no active bait, Gold Rod
/// side-grade or embedded enchant can influence species, variant, weight or biological value.
/// All other modifier combinations must use a later capped composer; this helper MUST NOT
/// be invoked on modified casts or used to bypass capability, line-break or progression.
///
/// Every fish slot gets a distinct species/variant/weight/biological draw domain. Repeating a
/// preview with the same operation and slot returns identical canonical numeric state and
/// cannot mint assets; FishInstance issuance remains a separate caller-owned transaction step.
pub fn preview_manual_fishing_unmodified_fish_candidate_v1(
    rng: ManualFishingRngContext,
    area: FishingArea,
    fish_slot: u8,
) -> Result<FishingFishInstanceDeliveryRequestV1, ManualFishingUnmodifiedFishCandidateError> {
    let species_pool = fishing_area_species_pool(area);
    if species_pool.is_empty() {
        return Err(ManualFishingUnmodifiedFishCandidateError::EmptySpeciesPool);
    }
    let species_weights = species_pool
        .iter()
        .map(|row| u64::from(row.pool_weight))
        .collect::<Vec<_>>();
    let species_index = rng.sample_weighted_v1(
        ManualFishingWeightedDrawV1::FishSpecies { fish_slot },
        &species_weights,
    )?;
    let species = species_pool[species_index].species;

    let variants = fishing_variant_catalog();
    if variants.is_empty() {
        return Err(ManualFishingUnmodifiedFishCandidateError::EmptyVariantPool);
    }
    let mut variant_weights = Vec::with_capacity(variants.len());
    for variant in variants {
        let base = variant.base_probability;
        let numerator = u64::from(base.numerator()) * BASE_FISH_VARIANT_WEIGHT_SCALE_V1;
        let denominator = u64::from(base.denominator());
        if denominator == 0 || !numerator.is_multiple_of(denominator) {
            return Err(ManualFishingUnmodifiedFishCandidateError::UnsupportedVariantProbability);
        }
        variant_weights.push(numerator / denominator);
    }
    let variant_index = rng.sample_weighted_v1(
        ManualFishingWeightedDrawV1::FishVariant { fish_slot },
        &variant_weights,
    )?;
    let variant = variants[variant_index].variant;

    let weight_quantile_ppm =
        rng.sample_ppm_v1(ManualFishingPpmDrawV1::FishWeight { fish_slot })?;
    let biological_noise_quantile_ppm =
        rng.sample_ppm_v1(ManualFishingPpmDrawV1::FishBiologicalNoise { fish_slot })?;
    let numeric = resolve_fish_instance_numeric_v1(
        species,
        variant,
        weight_quantile_ppm,
        biological_noise_quantile_ppm,
        false,
    )?;

    Ok(FishingFishInstanceDeliveryRequestV1 {
        fish_slot,
        area,
        numeric,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_canonical_variant_has_exact_positive_v1_integer_weight() {
        let catalog = fishing_variant_catalog();
        let mut weights = Vec::new();
        for row in catalog {
            let numerator =
                u64::from(row.base_probability.numerator()) * BASE_FISH_VARIANT_WEIGHT_SCALE_V1;
            let denominator = u64::from(row.base_probability.denominator());
            assert!(denominator > 0);
            assert_eq!(numerator % denominator, 0);
            weights.push(numerator / denominator);
        }
        assert_eq!(weights, [188, 6, 3, 2, 1]);
        assert_eq!(
            weights.iter().sum::<u64>(),
            BASE_FISH_VARIANT_WEIGHT_SCALE_V1
        );
    }

    #[test]
    fn every_area_has_nonempty_canonical_species_weights() {
        use FishingArea as A;
        for area in [
            A::StarterPool,
            A::River,
            A::Lake,
            A::Coast,
            A::DeepSea,
            A::Abyss,
        ] {
            let rows = fishing_area_species_pool(area);
            assert!(!rows.is_empty());
            assert!(rows.iter().all(|row| row.pool_weight > 0));
        }
    }
}
