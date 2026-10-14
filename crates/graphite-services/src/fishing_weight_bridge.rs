use serde::Serialize;
use thiserror::Error;

pub const FISHING_CANONICAL_WEIGHT_SCALE: u64 = 1_000_000_000_000;
pub const FISHING_WEIGHT_BRIDGE_POLICY_VERSION: u16 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FishingCatchWeightBridgeV1 {
    pub policy_version: u16,
    pub canonical_weight_scaled_grams: u64,
    pub catch_bag_weight_grams: u64,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum FishingWeightBridgeError {
    #[error("canonical fish weight must be positive")]
    ZeroCanonicalWeight,
    #[error("canonical fish weight rounded to zero whole grams")]
    RoundedWholeGramWeightIsZero,
    #[error("Fishing weight bridge arithmetic exceeded supported integer bounds")]
    ArithmeticOverflow,
}

/// Bridges canonical sub-gram FishInstance weight into the legacy whole-gram CatchBag surface.
///
/// Gameplay authority remains canonical_weight_scaled_grams at FISHING_CANONICAL_WEIGHT_SCALE
/// units per gram. CatchBag capacity/display uses one round-half-up conversion to whole grams;
/// callers must not feed that rounded value back into FishTension, line-break, valuation, or other
/// gameplay math.
pub fn bridge_canonical_fish_weight_v1(
    canonical_weight_scaled_grams: u64,
) -> Result<FishingCatchWeightBridgeV1, FishingWeightBridgeError> {
    if canonical_weight_scaled_grams == 0 {
        return Err(FishingWeightBridgeError::ZeroCanonicalWeight);
    }

    let whole = canonical_weight_scaled_grams / FISHING_CANONICAL_WEIGHT_SCALE;
    let remainder = canonical_weight_scaled_grams % FISHING_CANONICAL_WEIGHT_SCALE;
    let round_up = remainder >= FISHING_CANONICAL_WEIGHT_SCALE / 2;
    let catch_bag_weight_grams = whole
        .checked_add(u64::from(round_up))
        .ok_or(FishingWeightBridgeError::ArithmeticOverflow)?;

    if catch_bag_weight_grams == 0 {
        return Err(FishingWeightBridgeError::RoundedWholeGramWeightIsZero);
    }

    Ok(FishingCatchWeightBridgeV1 {
        policy_version: FISHING_WEIGHT_BRIDGE_POLICY_VERSION,
        canonical_weight_scaled_grams,
        catch_bag_weight_grams,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_up_boundary_is_exact() {
        let scale = FISHING_CANONICAL_WEIGHT_SCALE;
        assert_eq!(
            bridge_canonical_fish_weight_v1(6_000 * scale + scale / 2 - 1)
                .unwrap()
                .catch_bag_weight_grams,
            6_000
        );
        assert_eq!(
            bridge_canonical_fish_weight_v1(6_000 * scale + scale / 2)
                .unwrap()
                .catch_bag_weight_grams,
            6_001
        );
    }

    #[test]
    fn zero_and_sub_half_gram_fail_closed_when_whole_gram_storage_cannot_represent_them() {
        assert_eq!(
            bridge_canonical_fish_weight_v1(0),
            Err(FishingWeightBridgeError::ZeroCanonicalWeight)
        );
        assert_eq!(
            bridge_canonical_fish_weight_v1(FISHING_CANONICAL_WEIGHT_SCALE / 2 - 1),
            Err(FishingWeightBridgeError::RoundedWholeGramWeightIsZero)
        );
    }
}
