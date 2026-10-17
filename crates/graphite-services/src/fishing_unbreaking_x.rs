use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    FishingRodDurabilityPolicyError, MANUAL_FISHING_RNG_PPM_SCALE, ManualFishingPpmDrawV1,
    ManualFishingRngContext, ManualFishingRngDrawError, fishing_unbreaking_level_x_policy,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ManualFishingUnbreakingXWearPreviewV1 {
    pub draw_ppm: u32,
    pub chance_ppm: u32,
    pub prevented: bool,
}

#[derive(Debug, Error)]
pub enum ManualFishingUnbreakingXError {
    #[error(transparent)]
    Policy(#[from] FishingRodDurabilityPolicyError),
    #[error(transparent)]
    Rng(#[from] ManualFishingRngDrawError),
    #[error(
        "Unbreaking X prevention probability is not exactly representable on the manual Fishing PPM scale"
    )]
    UnsupportedPpmScale,
}

/// Resolves the frozen Unbreaking X ordinary-wear prevention draw.
///
/// The scalar owner freezes exactly 20% (1/5) for the one normal Rod durability
/// event. The versioned manual Fishing RNG already owns an independent
/// Unbreaking-ordinary-wear PPM domain. This helper proves the exact rational
/// chance is representable on that scale before comparing draw < chance.
///
/// Line-break destruction must never call this helper: Unbreaking does not prevent
/// line breaks. The caller also owns authoritative verification that the equipped
/// Rod has exactly Unbreaking X and no other modifier affecting this narrow path.
pub fn resolve_manual_fishing_unbreaking_x_wear_v1(
    rng: ManualFishingRngContext,
) -> Result<ManualFishingUnbreakingXWearPreviewV1, ManualFishingUnbreakingXError> {
    let policy = fishing_unbreaking_level_x_policy()?;
    let numerator = u64::from(policy.ordinary_event_prevention_probability_numerator());
    let denominator = u64::from(policy.ordinary_event_prevention_probability_denominator());
    let scaled = u64::from(MANUAL_FISHING_RNG_PPM_SCALE)
        .checked_mul(numerator)
        .ok_or(ManualFishingUnbreakingXError::UnsupportedPpmScale)?;
    if denominator == 0 || !scaled.is_multiple_of(denominator) {
        return Err(ManualFishingUnbreakingXError::UnsupportedPpmScale);
    }
    let chance_ppm = u32::try_from(scaled / denominator)
        .map_err(|_| ManualFishingUnbreakingXError::UnsupportedPpmScale)?;
    let draw_ppm = rng.sample_ppm_v1(ManualFishingPpmDrawV1::UnbreakingOrdinaryWear)?;
    Ok(ManualFishingUnbreakingXWearPreviewV1 {
        draw_ppm,
        chance_ppm,
        prevented: draw_ppm < chance_ppm,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_unbreaking_x_ratio_maps_exactly_to_two_hundred_thousand_ppm() {
        let policy = fishing_unbreaking_level_x_policy().unwrap();
        let numerator = u64::from(policy.ordinary_event_prevention_probability_numerator());
        let denominator = u64::from(policy.ordinary_event_prevention_probability_denominator());
        let scaled = u64::from(MANUAL_FISHING_RNG_PPM_SCALE) * numerator;
        assert_eq!(scaled % denominator, 0);
        assert_eq!(scaled / denominator, 200_000);
    }
}
