use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    FishingBait, FishingBaitEffect, FishingCatchBranch, MANUAL_FISHING_RNG_PPM_SCALE,
    ManualFishingPpmDrawV1, ManualFishingRngContext, ManualFishingRngDrawError,
    SchoolBaitProcResolution, SchoolBaitQuantityError, SchoolBaitQuantityResolution,
    fishing_bait_policy, resolve_school_bait_quantity,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ManualFishingSchoolBaitQuantityV1 {
    pub draw_ppm: u32,
    pub chance_ppm: u32,
    pub proc_resolution: SchoolBaitProcResolution,
    pub quantity: SchoolBaitQuantityResolution,
}

#[derive(Debug, Error)]
pub enum ManualFishingSchoolBaitError {
    #[error(transparent)]
    Rng(#[from] ManualFishingRngDrawError),
    #[error(transparent)]
    Quantity(#[from] SchoolBaitQuantityError),
    #[error(
        "School Bait proc probability is not exactly representable on the manual Fishing PPM scale"
    )]
    UnsupportedPpmScale,
}

/// Resolves one canonical School Bait quantity proc from the operation-pinned
/// semantic PPM domain.
///
/// The frozen School chance is 2/25 and therefore exactly 80,000 PPM. The helper
/// derives that threshold from the bait catalog rather than duplicating a gameplay
/// constant, proves exact representability, and delegates the shared five-fish cap
/// behavior to the existing School quantity policy.
///
/// The current fish count represents fish already selected by earlier quantity
/// stages. This helper does not choose ordering with Multicatch; the School-only
/// cast owner passes exactly one baseline fish.
pub fn resolve_manual_fishing_school_bait_quantity_v1(
    rng: ManualFishingRngContext,
    current_fish_count: u8,
) -> Result<ManualFishingSchoolBaitQuantityV1, ManualFishingSchoolBaitError> {
    let FishingBaitEffect::School {
        extra_same_area_fish_chance,
        ..
    } = fishing_bait_policy(FishingBait::School).effect
    else {
        unreachable!("School Bait must expose the School effect variant");
    };
    let numerator = u64::from(extra_same_area_fish_chance.numerator());
    let denominator = u64::from(extra_same_area_fish_chance.denominator());
    let scaled = u64::from(MANUAL_FISHING_RNG_PPM_SCALE)
        .checked_mul(numerator)
        .ok_or(ManualFishingSchoolBaitError::UnsupportedPpmScale)?;
    if denominator == 0 || !scaled.is_multiple_of(denominator) {
        return Err(ManualFishingSchoolBaitError::UnsupportedPpmScale);
    }
    let chance_ppm = u32::try_from(scaled / denominator)
        .map_err(|_| ManualFishingSchoolBaitError::UnsupportedPpmScale)?;
    let draw_ppm = rng.sample_ppm_v1(ManualFishingPpmDrawV1::SchoolBaitProc)?;
    let proc_resolution = if draw_ppm < chance_ppm {
        SchoolBaitProcResolution::Triggered
    } else {
        SchoolBaitProcResolution::NotTriggered
    };
    let quantity = resolve_school_bait_quantity(
        FishingCatchBranch::Fish,
        current_fish_count,
        proc_resolution,
    )?;
    Ok(ManualFishingSchoolBaitQuantityV1 {
        draw_ppm,
        chance_ppm,
        proc_resolution,
        quantity,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn school_bait_ratio_maps_exactly_to_eighty_thousand_ppm() {
        let FishingBaitEffect::School {
            extra_same_area_fish_chance,
            ..
        } = fishing_bait_policy(FishingBait::School).effect
        else {
            panic!("wrong School Bait policy variant");
        };
        let scaled = u64::from(MANUAL_FISHING_RNG_PPM_SCALE)
            * u64::from(extra_same_area_fish_chance.numerator());
        let denominator = u64::from(extra_same_area_fish_chance.denominator());
        assert_eq!(scaled % denominator, 0);
        assert_eq!(scaled / denominator, 80_000);
    }
}
