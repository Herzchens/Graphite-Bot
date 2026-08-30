use serde::Serialize;
use thiserror::Error;

use crate::fishing_capability::{FishingCapabilityClassification, FishingCapabilityRatio};

pub const SHARP_HOOK_MAX_LEVEL: u8 = 10;
pub const SHARP_HOOK_PERCENTAGE_POINTS_PER_LEVEL: u8 = 2;
pub const OVER_CAP_CATCH_CHANCE_MIN_PERCENT: u8 = 15;
pub const OVER_CAP_CATCH_CHANCE_MAX_PERCENT: u8 = 95;

const PERCENT_DENOMINATOR: u128 = 100;
const BASE_OVER_CAP_CATCH_PERCENT: u128 = 85;
const OVER_CAP_PENALTY_PERCENT_PER_EXCESS_RATIO: u128 = 25;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OverCapCatchChanceBound {
    Minimum,
    Interior,
    Maximum,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct OverCapCatchChancePolicy {
    pub sharp_hook_level: Option<u8>,
    pub bound: OverCapCatchChanceBound,
    numerator: u128,
    denominator: u128,
}

impl OverCapCatchChancePolicy {
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
pub enum FishingOverCapError {
    #[error("over-cap catch chance requires a capability ratio strictly greater than one")]
    CapabilityRatioNotOverCap,
    #[error("SharpHook level must be between I and X when present; got {0}")]
    SharpHookLevelOutOfRange(u8),
    #[error("over-cap catch-chance arithmetic exceeded supported integer bounds")]
    ArithmeticOverflow,
}

/// Previews the exact catch chance used after an over-cap candidate survives the line-break step.
///
/// The canonical formula is:
///
/// `clamp(0.85 - 0.25 × (R - 1) + 0.02 × SharpHookLevel, 0.15, 0.95)`.
///
/// This function accepts only `R > 1`, evaluates the formula as checked rational arithmetic, and
/// returns an exact reduced probability. `None` means no SharpHook enchant; a present level must be
/// canonical I-X. The generic normal-Shop Level V ceiling is acquisition metadata and is not the
/// gameplay max: the Fishing numeric-cap table and this formula explicitly freeze SharpHook X.
///
/// Resolution ordering remains external and mandatory. The caller may use this probability only
/// after the candidate has survived the preceding line-break roll. This policy performs no RNG draw
/// and does not bypass the still-unresolved `(R - 1)^1.30` line-break calculation.
pub fn preview_over_cap_catch_chance(
    capability_ratio: FishingCapabilityRatio,
    sharp_hook_level: Option<u8>,
) -> Result<OverCapCatchChancePolicy, FishingOverCapError> {
    if capability_ratio.classification != FishingCapabilityClassification::OverRodCapability
        || capability_ratio.numerator() <= capability_ratio.denominator()
    {
        return Err(FishingOverCapError::CapabilityRatioNotOverCap);
    }

    let sharp_hook_level_value = match sharp_hook_level {
        None => 0,
        Some(level @ 1..=SHARP_HOOK_MAX_LEVEL) => level,
        Some(level) => return Err(FishingOverCapError::SharpHookLevelOutOfRange(level)),
    };

    let ratio_numerator = capability_ratio.numerator();
    let ratio_denominator = capability_ratio.denominator();
    let penalty_scaled_numerator = ratio_numerator
        .checked_mul(OVER_CAP_PENALTY_PERCENT_PER_EXCESS_RATIO)
        .ok_or(FishingOverCapError::ArithmeticOverflow)?;
    let sharp_hook_bonus_percent = u128::from(sharp_hook_level_value)
        .checked_mul(u128::from(SHARP_HOOK_PERCENTAGE_POINTS_PER_LEVEL))
        .ok_or(FishingOverCapError::ArithmeticOverflow)?;
    let intercept_percent = BASE_OVER_CAP_CATCH_PERCENT
        .checked_add(OVER_CAP_PENALTY_PERCENT_PER_EXCESS_RATIO)
        .ok_or(FishingOverCapError::ArithmeticOverflow)?;

    // raw chance <= minimum iff 25R >= (110 - minimum) + 2L.
    let minimum_threshold_factor = intercept_percent
        .checked_sub(u128::from(OVER_CAP_CATCH_CHANCE_MIN_PERCENT))
        .and_then(|value| value.checked_add(sharp_hook_bonus_percent))
        .ok_or(FishingOverCapError::ArithmeticOverflow)?;
    let minimum_threshold = ratio_denominator
        .checked_mul(minimum_threshold_factor)
        .ok_or(FishingOverCapError::ArithmeticOverflow)?;
    if penalty_scaled_numerator >= minimum_threshold {
        return Ok(percent_policy(
            sharp_hook_level,
            OverCapCatchChanceBound::Minimum,
            OVER_CAP_CATCH_CHANCE_MIN_PERCENT,
        ));
    }

    // raw chance >= maximum iff 25R <= (110 - maximum) + 2L.
    let maximum_threshold_factor = intercept_percent
        .checked_sub(u128::from(OVER_CAP_CATCH_CHANCE_MAX_PERCENT))
        .and_then(|value| value.checked_add(sharp_hook_bonus_percent))
        .ok_or(FishingOverCapError::ArithmeticOverflow)?;
    let maximum_threshold = ratio_denominator
        .checked_mul(maximum_threshold_factor)
        .ok_or(FishingOverCapError::ArithmeticOverflow)?;
    if penalty_scaled_numerator <= maximum_threshold {
        return Ok(percent_policy(
            sharp_hook_level,
            OverCapCatchChanceBound::Maximum,
            OVER_CAP_CATCH_CHANCE_MAX_PERCENT,
        ));
    }

    // 0.85 - 0.25(R - 1) + 0.02L = (110 + 2L - 25R) / 100.
    let positive_factor = intercept_percent
        .checked_add(sharp_hook_bonus_percent)
        .ok_or(FishingOverCapError::ArithmeticOverflow)?;
    let positive_term = ratio_denominator
        .checked_mul(positive_factor)
        .ok_or(FishingOverCapError::ArithmeticOverflow)?;
    let numerator = positive_term
        .checked_sub(penalty_scaled_numerator)
        .ok_or(FishingOverCapError::ArithmeticOverflow)?;
    let denominator = ratio_denominator
        .checked_mul(PERCENT_DENOMINATOR)
        .ok_or(FishingOverCapError::ArithmeticOverflow)?;
    let divisor = gcd(numerator, denominator);

    Ok(OverCapCatchChancePolicy {
        sharp_hook_level,
        bound: OverCapCatchChanceBound::Interior,
        numerator: numerator / divisor,
        denominator: denominator / divisor,
    })
}

fn percent_policy(
    sharp_hook_level: Option<u8>,
    bound: OverCapCatchChanceBound,
    percent: u8,
) -> OverCapCatchChancePolicy {
    let numerator = u128::from(percent);
    let divisor = gcd(numerator, PERCENT_DENOMINATOR);
    OverCapCatchChancePolicy {
        sharp_hook_level,
        bound,
        numerator: numerator / divisor,
        denominator: PERCENT_DENOMINATOR / divisor,
    }
}

const fn gcd(mut left: u128, mut right: u128) -> u128 {
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
    use crate::{
        EquipmentTier, FishingRarity, fishing_catch_load, fishing_tension,
        manual_fishing_capability_ratio, manual_fishing_line_strength,
    };

    fn manual_ratio(common_weight_grams: u64) -> FishingCapabilityRatio {
        let catch = [fishing_tension(common_weight_grams, FishingRarity::Common).unwrap()];
        let load = fishing_catch_load(&catch).unwrap();
        let strength =
            manual_fishing_line_strength(EquipmentTier::Wood, true, None, false).unwrap();
        manual_fishing_capability_ratio(load, strength).unwrap()
    }

    #[test]
    fn interior_probability_is_exact_without_sharp_hook() {
        let chance = preview_over_cap_catch_chance(manual_ratio(6_600), None).unwrap();
        assert_eq!(chance.bound, OverCapCatchChanceBound::Interior);
        assert_eq!(chance.numerator(), 33);
        assert_eq!(chance.denominator(), 40);
    }

    #[test]
    fn sharp_hook_x_reaches_the_upper_bound_when_applicable() {
        let chance = preview_over_cap_catch_chance(manual_ratio(6_600), Some(10)).unwrap();
        assert_eq!(chance.bound, OverCapCatchChanceBound::Maximum);
        assert_eq!(chance.numerator(), 19);
        assert_eq!(chance.denominator(), 20);
    }

    #[test]
    fn extreme_over_cap_ratio_reaches_the_lower_bound() {
        let chance = preview_over_cap_catch_chance(manual_ratio(30_000), None).unwrap();
        assert_eq!(chance.bound, OverCapCatchChanceBound::Minimum);
        assert_eq!(chance.numerator(), 3);
        assert_eq!(chance.denominator(), 20);
    }

    #[test]
    fn exact_clamp_boundaries_are_stable() {
        let upper = preview_over_cap_catch_chance(manual_ratio(8_400), Some(10)).unwrap();
        assert_eq!(upper.bound, OverCapCatchChanceBound::Maximum);
        assert_eq!((upper.numerator(), upper.denominator()), (19, 20));

        let lower = preview_over_cap_catch_chance(manual_ratio(22_800), None).unwrap();
        assert_eq!(lower.bound, OverCapCatchChanceBound::Minimum);
        assert_eq!((lower.numerator(), lower.denominator()), (3, 20));
    }

    #[test]
    fn within_capability_and_invalid_sharp_hook_levels_fail_closed() {
        let within = manual_ratio(6_000);
        assert_eq!(
            preview_over_cap_catch_chance(within, None),
            Err(FishingOverCapError::CapabilityRatioNotOverCap)
        );

        let over = manual_ratio(6_600);
        for level in [0, 11, u8::MAX] {
            assert_eq!(
                preview_over_cap_catch_chance(over, Some(level)),
                Err(FishingOverCapError::SharpHookLevelOutOfRange(level))
            );
        }
    }
}
