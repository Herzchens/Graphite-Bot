use serde::Serialize;
use thiserror::Error;

use crate::{
    EquippedFishingRodKind, FishingArea, FishingCapabilityError, FishingCapabilityRatio,
    FishingCapabilityResolutionError, FishingCatchBranch, FishingFishInstanceNumericV1,
    FishingNumericError, FishingOverCapError, FishingRoutedFishCapabilityStage,
    ManualFishingLineBreakChanceV1, OverCapCatchChancePolicy, fishing_area_species_pool,
    fishing_capability_routing_policy, fishing_catch_load_from_scaled_tensions,
    fishing_species_policy, fishing_tension_from_canonical_scaled_weight,
    manual_fishing_capability_ratio, manual_fishing_line_strength,
    preview_manual_fishing_line_break_chance_v1, preview_over_cap_catch_chance,
    resolve_routed_fish_capability_stage,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "stage", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ManualFishingUnmodifiedCapabilityPreviewV1 {
    StarterPoolGuaranteedLanded,
    WithinRodCapability {
        ratio: FishingCapabilityRatio,
    },
    OverCapPending {
        ratio: FishingCapabilityRatio,
        line_break: ManualFishingLineBreakChanceV1,
        catch_chance: OverCapCatchChancePolicy,
    },
}

#[derive(Debug, Error)]
pub enum ManualFishingUnmodifiedCapabilityError {
    #[error("Fish species is not in the specified canonical Fishing area")]
    SpeciesOutsideArea,
    #[error("non-Starter-Pool Fishing requires an ordinary Rod")]
    StarterRodOutsidePool,
    #[error(transparent)]
    Capability(#[from] FishingCapabilityError),
    #[error(transparent)]
    Routing(#[from] FishingCapabilityResolutionError),
    #[error(transparent)]
    Numeric(#[from] FishingNumericError),
    #[error(transparent)]
    OverCap(#[from] FishingOverCapError),
}

/// Previews the exact physical capability route of one unmodified manual Fish candidate.
///
/// Requires a candidate already resolved in the same canonical Fishing area. Starter Pool Fish
/// bypasses tension regardless of Rod tier. Elsewhere the precise scaled FishInstance weight,
/// rarity and ordinary Rod tier feed the versioned rational capability kernel.
///
/// For R>1 this produces both frozen *probability previews* but intentionally draws neither
/// line-break nor over-cap catch RNG. An owning cast must resolve them in order (line break
/// first, catch/escape only after survival), with its approved final RNG probability mapping,
/// and must still settle bait, wear, progression, result and audit/outbox before commit.
pub fn preview_manual_unmodified_fish_capability_v1(
    area: FishingArea,
    rod: EquippedFishingRodKind,
    candidate: FishingFishInstanceNumericV1,
) -> Result<ManualFishingUnmodifiedCapabilityPreviewV1, ManualFishingUnmodifiedCapabilityError> {
    if !fishing_area_species_pool(area)
        .iter()
        .any(|row| row.species == candidate.species)
    {
        return Err(ManualFishingUnmodifiedCapabilityError::SpeciesOutsideArea);
    }

    let route = fishing_capability_routing_policy(area, FishingCatchBranch::Fish);
    if route.guaranteed_capability_outcome().is_some() {
        return Ok(ManualFishingUnmodifiedCapabilityPreviewV1::StarterPoolGuaranteedLanded);
    }

    let EquippedFishingRodKind::Ordinary { tier } = rod else {
        return Err(ManualFishingUnmodifiedCapabilityError::StarterRodOutsidePool);
    };
    let rarity = fishing_species_policy(candidate.species).rarity;
    let tension =
        fishing_tension_from_canonical_scaled_weight(candidate.weight_scaled_grams(), rarity)?;
    let load = fishing_catch_load_from_scaled_tensions(&[tension])?;
    let strength = manual_fishing_line_strength(tier, true, None, false)?;
    let ratio = manual_fishing_capability_ratio(load, strength)?;

    Ok(match resolve_routed_fish_capability_stage(route, ratio)? {
        FishingRoutedFishCapabilityStage::LandedWithinRodCapability => {
            ManualFishingUnmodifiedCapabilityPreviewV1::WithinRodCapability { ratio }
        }
        FishingRoutedFishCapabilityStage::ResolveOverCap(required) => {
            let ratio = required.capability_ratio();
            ManualFishingUnmodifiedCapabilityPreviewV1::OverCapPending {
                ratio,
                line_break: preview_manual_fishing_line_break_chance_v1(ratio, false)?,
                catch_chance: preview_over_cap_catch_chance(ratio, None)?,
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FishingSpecies, FishingVariant, resolve_fish_instance_numeric_v1};

    #[test]
    fn starter_pool_fish_bypasses_capability_and_invalid_species_fail_closed() {
        let species = fishing_area_species_pool(FishingArea::StarterPool)[0].species;
        let candidate = resolve_fish_instance_numeric_v1(
            species,
            FishingVariant::Normal,
            500_000,
            500_000,
            false,
        )
        .unwrap();
        assert_eq!(
            preview_manual_unmodified_fish_capability_v1(
                FishingArea::StarterPool,
                EquippedFishingRodKind::StarterBasic,
                candidate,
            )
            .unwrap(),
            ManualFishingUnmodifiedCapabilityPreviewV1::StarterPoolGuaranteedLanded
        );

        let elsewhere = resolve_fish_instance_numeric_v1(
            FishingSpecies::LeviathanFry,
            FishingVariant::Normal,
            500_000,
            500_000,
            false,
        )
        .unwrap();
        assert!(matches!(
            preview_manual_unmodified_fish_capability_v1(
                FishingArea::StarterPool,
                EquippedFishingRodKind::StarterBasic,
                elsewhere,
            ),
            Err(ManualFishingUnmodifiedCapabilityError::SpeciesOutsideArea)
        ));
    }

    #[test]
    fn non_pool_preview_requires_ordinary_rod_before_capability_math() {
        let area = FishingArea::River;
        let species = fishing_area_species_pool(area)[0].species;
        let candidate = resolve_fish_instance_numeric_v1(
            species,
            FishingVariant::Normal,
            500_000,
            500_000,
            false,
        )
        .unwrap();
        assert!(matches!(
            preview_manual_unmodified_fish_capability_v1(
                area,
                EquippedFishingRodKind::StarterBasic,
                candidate,
            ),
            Err(ManualFishingUnmodifiedCapabilityError::StarterRodOutsidePool)
        ));
    }
}
