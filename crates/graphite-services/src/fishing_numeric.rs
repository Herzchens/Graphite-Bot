use serde::Serialize;
use thiserror::Error;

use crate::{
    fishing_bait::{FishingBait, FishingBaitEffect, fishing_bait_policy},
    fishing_capability::FishingCapabilityRatio,
    fishing_species::{FishingSpecies, fishing_species_policy},
    fishing_variant::{FishingVariant, fishing_variant_policy},
    fishing_weight_bridge::{
        FISHING_CANONICAL_WEIGHT_SCALE, FishingCatchWeightBridgeV1, FishingWeightBridgeError,
        bridge_canonical_fish_weight_v1,
    },
};

pub const FISHING_NUMERIC_POLICY_VERSION: u16 = 1;
pub const FISHING_FIXED_POINT_SCALE: u64 = FISHING_CANONICAL_WEIGHT_SCALE;
pub const FISHING_NUMERIC_DRAW_SCALE_PPM: u32 = 1_000_000;
pub const FISHING_LINE_BREAK_PROBABILITY_SCALE_PPM: u32 = 1_000_000;

const FIXED_SCALE: i128 = FISHING_FIXED_POINT_SCALE as i128;
const FIXED_SCALE_U128: u128 = FISHING_FIXED_POINT_SCALE as u128;
const LN_2_FIXED: i128 = 693_147_180_560;
const NORMAL_SIGMA_DENOMINATOR: i128 = 4;
const NORMAL_P_LOW_FIXED: i128 = 24_250_000_000;

const WEIGHT_RATIO_MIN_FIXED: i128 = 400_000_000_000;
const WEIGHT_RATIO_MAX_FIXED: i128 = 2_500_000_000_000;
const BIOLOGICAL_NOISE_MIN_FIXED: i128 = 950_000_000_000;
const BIOLOGICAL_NOISE_SPAN_FIXED: i128 = 100_000_000_000;
const VALUE_FACTOR_MIN_FIXED: i128 = 500_000_000_000;
const VALUE_FACTOR_MAX_FIXED: i128 = 4_000_000_000_000;

const LINE_BREAK_BASE_FIXED: i128 = 2_000_000_000;
const LINE_BREAK_COEFFICIENT_FIXED: i128 = 15_000_000_000;
const LINE_BREAK_MAX_FIXED: u128 = 150_000_000_000;
const LINE_BREAK_CAP_SHORT_CIRCUIT_EXCESS: u128 = 6;
const FIXED_PER_PPM: u128 = FIXED_SCALE_U128 / FISHING_LINE_BREAK_PROBABILITY_SCALE_PPM as u128;

const INVERSE_NORMAL_A: [i128; 6] = [
    -39_696_830_286_654,
    220_946_098_424_521,
    -275_928_510_446_969,
    138_357_751_867_269,
    -30_664_798_066_147,
    2_506_628_277_459,
];
const INVERSE_NORMAL_B: [i128; 5] = [
    -54_476_098_798_224,
    161_585_836_858_041,
    -155_698_979_859_887,
    66_801_311_887_720,
    -13_280_681_552_886,
];
const INVERSE_NORMAL_C: [i128; 6] = [
    -7_784_894_002,
    -322_396_458_041,
    -2_400_758_277_162,
    -2_549_732_539_344,
    4_374_664_141_465,
    2_938_163_982_699,
];
const INVERSE_NORMAL_D: [i128; 4] = [
    7_784_695_709,
    322_467_129_070,
    2_445_134_137_143,
    3_754_408_661_907,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FishingFishInstanceNumericV1 {
    pub policy_version: u16,
    pub species: FishingSpecies,
    pub variant: FishingVariant,
    pub quality_bait_active: bool,
    pub weight_quantile_ppm: u32,
    pub biological_noise_quantile_ppm: u32,
    weight_ratio_scaled: u64,
    weight_scaled_grams: u64,
    biological_noise_scaled: u64,
    length_scaled_millimeters: u64,
    pub npc_value_money: i64,
}

impl FishingFishInstanceNumericV1 {
    #[must_use]
    pub const fn fixed_point_scale(self) -> u64 {
        FISHING_FIXED_POINT_SCALE
    }

    #[must_use]
    pub const fn weight_ratio_scaled(self) -> u64 {
        self.weight_ratio_scaled
    }

    #[must_use]
    pub const fn weight_scaled_grams(self) -> u64 {
        self.weight_scaled_grams
    }

    pub fn catch_weight_bridge(
        self,
    ) -> Result<FishingCatchWeightBridgeV1, FishingWeightBridgeError> {
        bridge_canonical_fish_weight_v1(self.weight_scaled_grams)
    }

    #[must_use]
    pub const fn biological_noise_scaled(self) -> u64 {
        self.biological_noise_scaled
    }

    #[must_use]
    pub const fn length_scaled_millimeters(self) -> u64 {
        self.length_scaled_millimeters
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ManualFishingLineBreakChanceV1 {
    pub policy_version: u16,
    pub sturdy_bait_active: bool,
    pub base_cap_applied: bool,
    pub base_probability_ppm: u32,
    pub final_probability_ppm: u32,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum FishingNumericError {
    #[error("Fishing numeric draw must be in 0..{FISHING_NUMERIC_DRAW_SCALE_PPM}; got {0}")]
    DrawOutOfRange(u32),
    #[error("line-break probability requires an exact capability ratio strictly greater than one")]
    CapabilityRatioNotOverCap,
    #[error("Fishing deterministic numeric arithmetic exceeded supported integer bounds")]
    ArithmeticOverflow,
    #[error("Fishing deterministic numeric approximation received an invalid domain")]
    InvalidNumericDomain,
}

/// Resolves the owner-approved deterministic numeric FishInstance contract, version 1.
///
/// The supplied draw values are already-authoritative uniform PPM buckets. This function does not
/// derive them from an operation RNG root; the future Fishing lifecycle still owns domain-separated
/// draw derivation and ordering.
///
/// Weight uses a log-normal distribution whose median is Wref: mu = ln(Wref), sigma = 0.25. The
/// uniform weight bucket is mapped through its midpoint to the standard-normal inverse CDF, sampled
/// exactly once, optionally shifts the center by Quality Bait x1.05, then clamps the final weight to
/// [0.40 Wref, 2.50 Wref]. Biological noise independently maps its uniform bucket midpoint to
/// [0.95, 1.05].
///
/// All transcendental/fractional evaluation is performed by the versioned integer fixed-point
/// kernel below. There is no native floating-point gameplay authority.
pub fn resolve_fish_instance_numeric_v1(
    species: FishingSpecies,
    variant: FishingVariant,
    weight_quantile_ppm: u32,
    biological_noise_quantile_ppm: u32,
    quality_bait_active: bool,
) -> Result<FishingFishInstanceNumericV1, FishingNumericError> {
    validate_draw(weight_quantile_ppm)?;
    validate_draw(biological_noise_quantile_ppm)?;

    let species_policy = fishing_species_policy(species);
    let variant_policy = fishing_variant_policy(variant);

    let normal_z = inverse_standard_normal_from_ppm_midpoint(weight_quantile_ppm)?;
    let sigma_z = round_div_signed(normal_z, NORMAL_SIGMA_DENOMINATOR)?;
    let mut weight_ratio = exp_fixed(sigma_z)?;

    if quality_bait_active {
        let FishingBaitEffect::Quality {
            sampled_fish_weight_center_factor,
            ..
        } = fishing_bait_policy(FishingBait::Quality).effect
        else {
            unreachable!("Quality Bait must expose the Quality effect variant");
        };
        weight_ratio = mul_fixed_by_ratio(
            weight_ratio,
            i128::from(sampled_fish_weight_center_factor.numerator()),
            i128::from(sampled_fish_weight_center_factor.denominator()),
        )?;
    }

    weight_ratio = weight_ratio.clamp(WEIGHT_RATIO_MIN_FIXED, WEIGHT_RATIO_MAX_FIXED);
    let weight_ratio_u128 =
        u128::try_from(weight_ratio).map_err(|_| FishingNumericError::ArithmeticOverflow)?;
    let weight_scaled_grams = u128::from(species_policy.reference_weight_grams)
        .checked_mul(weight_ratio_u128)
        .ok_or(FishingNumericError::ArithmeticOverflow)?;

    let noise_unit = unit_midpoint_from_ppm(biological_noise_quantile_ppm)?;
    let noise_delta = mul_fixed(noise_unit, BIOLOGICAL_NOISE_SPAN_FIXED)?;
    let biological_noise = BIOLOGICAL_NOISE_MIN_FIXED
        .checked_add(noise_delta)
        .ok_or(FishingNumericError::ArithmeticOverflow)?;

    let cube_root_ratio = pow_positive_fixed(weight_ratio, 1, 3)?;
    let cube_root_u128 =
        u128::try_from(cube_root_ratio).map_err(|_| FishingNumericError::ArithmeticOverflow)?;
    let biological_noise_u128 =
        u128::try_from(biological_noise).map_err(|_| FishingNumericError::ArithmeticOverflow)?;
    let length_numerator = u128::from(species_policy.reference_length_millimeters)
        .checked_mul(cube_root_u128)
        .and_then(|value| value.checked_mul(biological_noise_u128))
        .ok_or(FishingNumericError::ArithmeticOverflow)?;
    let length_scaled_millimeters = round_div_u128(length_numerator, FIXED_SCALE_U128)?;

    let size_factor = pow_positive_fixed(weight_ratio, 17, 20)?
        .clamp(VALUE_FACTOR_MIN_FIXED, VALUE_FACTOR_MAX_FIXED);
    let size_factor_u128 =
        u128::try_from(size_factor).map_err(|_| FishingNumericError::ArithmeticOverflow)?;
    let base_value = u128::try_from(species_policy.base_npc_value_money)
        .map_err(|_| FishingNumericError::InvalidNumericDomain)?;
    let value_multiplier = variant_policy.value_multiplier;
    let value_numerator = base_value
        .checked_mul(size_factor_u128)
        .and_then(|value| value.checked_mul(u128::from(value_multiplier.numerator())))
        .ok_or(FishingNumericError::ArithmeticOverflow)?;
    let value_denominator = FIXED_SCALE_U128
        .checked_mul(u128::from(value_multiplier.denominator()))
        .ok_or(FishingNumericError::ArithmeticOverflow)?;
    let npc_value_money = round_div_u128(value_numerator, value_denominator)?;

    Ok(FishingFishInstanceNumericV1 {
        policy_version: FISHING_NUMERIC_POLICY_VERSION,
        species,
        variant,
        quality_bait_active,
        weight_quantile_ppm,
        biological_noise_quantile_ppm,
        weight_ratio_scaled: u64::try_from(weight_ratio_u128)
            .map_err(|_| FishingNumericError::ArithmeticOverflow)?,
        weight_scaled_grams: u64::try_from(weight_scaled_grams)
            .map_err(|_| FishingNumericError::ArithmeticOverflow)?,
        biological_noise_scaled: u64::try_from(biological_noise)
            .map_err(|_| FishingNumericError::ArithmeticOverflow)?,
        length_scaled_millimeters: u64::try_from(length_scaled_millimeters)
            .map_err(|_| FishingNumericError::ArithmeticOverflow)?,
        npc_value_money: i64::try_from(npc_value_money)
            .map_err(|_| FishingNumericError::ArithmeticOverflow)?,
    })
}

/// Resolves the canonical manual Fishing line-break probability as a PPM RNG threshold.
///
/// The exact R > 1 capability ratio remains the authority. Version 1 evaluates
/// min(0.15, 0.002 + 0.015 * (R - 1)^1.30) in the deterministic fixed-point kernel, applies Sturdy
/// Bait's x0.90 final-probability factor afterwards, clamps again to [0, 0.15], then rounds half-up
/// to the canonical 1,000,000-PPM RNG threshold.
pub fn preview_manual_fishing_line_break_chance_v1(
    capability_ratio: FishingCapabilityRatio,
    sturdy_bait_active: bool,
) -> Result<ManualFishingLineBreakChanceV1, FishingNumericError> {
    let ratio_numerator = capability_ratio.numerator();
    let ratio_denominator = capability_ratio.denominator();
    if ratio_denominator == 0 || ratio_numerator <= ratio_denominator {
        return Err(FishingNumericError::CapabilityRatioNotOverCap);
    }

    let excess_numerator = ratio_numerator - ratio_denominator;
    let (base_probability_fixed, base_cap_applied) = if excess_numerator / ratio_denominator
        >= LINE_BREAK_CAP_SHORT_CIRCUIT_EXCESS
    {
        (LINE_BREAK_MAX_FIXED, true)
    } else {
        let excess_scaled_numerator = excess_numerator
            .checked_mul(FIXED_SCALE_U128)
            .ok_or(FishingNumericError::ArithmeticOverflow)?;
        let excess_fixed = round_div_u128(excess_scaled_numerator, ratio_denominator)?;
        let powered = if excess_fixed == 0 {
            0
        } else {
            pow_positive_fixed(
                i128::try_from(excess_fixed)
                    .map_err(|_| FishingNumericError::ArithmeticOverflow)?,
                13,
                10,
            )?
        };
        let powered_term = mul_fixed(LINE_BREAK_COEFFICIENT_FIXED, powered)?;
        let raw = LINE_BREAK_BASE_FIXED
            .checked_add(powered_term)
            .ok_or(FishingNumericError::ArithmeticOverflow)?;
        let raw_u128 = u128::try_from(raw).map_err(|_| FishingNumericError::ArithmeticOverflow)?;
        (
            raw_u128.min(LINE_BREAK_MAX_FIXED),
            raw_u128 >= LINE_BREAK_MAX_FIXED,
        )
    };

    let final_probability_fixed = if sturdy_bait_active {
        let FishingBaitEffect::Sturdy {
            final_line_break_chance_factor,
            ..
        } = fishing_bait_policy(FishingBait::Sturdy).effect
        else {
            unreachable!("Sturdy Bait must expose the Sturdy effect variant");
        };
        round_div_u128(
            base_probability_fixed
                .checked_mul(u128::from(final_line_break_chance_factor.numerator()))
                .ok_or(FishingNumericError::ArithmeticOverflow)?,
            u128::from(final_line_break_chance_factor.denominator()),
        )?
    } else {
        base_probability_fixed
    }
    .min(LINE_BREAK_MAX_FIXED);

    let base_probability_ppm =
        u32::try_from(round_div_u128(base_probability_fixed, FIXED_PER_PPM)?)
            .map_err(|_| FishingNumericError::ArithmeticOverflow)?;
    let final_probability_ppm =
        u32::try_from(round_div_u128(final_probability_fixed, FIXED_PER_PPM)?)
            .map_err(|_| FishingNumericError::ArithmeticOverflow)?;

    Ok(ManualFishingLineBreakChanceV1 {
        policy_version: FISHING_NUMERIC_POLICY_VERSION,
        sturdy_bait_active,
        base_cap_applied,
        base_probability_ppm,
        final_probability_ppm,
    })
}

const fn validate_draw(draw: u32) -> Result<(), FishingNumericError> {
    if draw >= FISHING_NUMERIC_DRAW_SCALE_PPM {
        Err(FishingNumericError::DrawOutOfRange(draw))
    } else {
        Ok(())
    }
}

fn unit_midpoint_from_ppm(draw: u32) -> Result<i128, FishingNumericError> {
    validate_draw(draw)?;
    let odd_bucket = i128::from(draw)
        .checked_mul(2)
        .and_then(|value| value.checked_add(1))
        .ok_or(FishingNumericError::ArithmeticOverflow)?;
    let numerator = odd_bucket
        .checked_mul(FIXED_SCALE)
        .ok_or(FishingNumericError::ArithmeticOverflow)?;
    round_div_signed(numerator, i128::from(FISHING_NUMERIC_DRAW_SCALE_PPM) * 2)
}

fn inverse_standard_normal_from_ppm_midpoint(draw: u32) -> Result<i128, FishingNumericError> {
    let probability = unit_midpoint_from_ppm(draw)?;

    if probability < NORMAL_P_LOW_FIXED {
        let q = sqrt_fixed(
            natural_log_fixed(probability)?
                .checked_mul(-2)
                .ok_or(FishingNumericError::ArithmeticOverflow)?,
        )?;
        let numerator = horner_fixed(&INVERSE_NORMAL_C, q)?;
        let denominator = mul_fixed(horner_fixed(&INVERSE_NORMAL_D, q)?, q)?
            .checked_add(FIXED_SCALE)
            .ok_or(FishingNumericError::ArithmeticOverflow)?;
        return div_fixed(numerator, denominator);
    }

    if probability > FIXED_SCALE - NORMAL_P_LOW_FIXED {
        let tail_probability = FIXED_SCALE
            .checked_sub(probability)
            .ok_or(FishingNumericError::ArithmeticOverflow)?;
        let q = sqrt_fixed(
            natural_log_fixed(tail_probability)?
                .checked_mul(-2)
                .ok_or(FishingNumericError::ArithmeticOverflow)?,
        )?;
        let numerator = horner_fixed(&INVERSE_NORMAL_C, q)?;
        let denominator = mul_fixed(horner_fixed(&INVERSE_NORMAL_D, q)?, q)?
            .checked_add(FIXED_SCALE)
            .ok_or(FishingNumericError::ArithmeticOverflow)?;
        return div_fixed(numerator, denominator)?
            .checked_neg()
            .ok_or(FishingNumericError::ArithmeticOverflow);
    }

    let q = probability - FIXED_SCALE / 2;
    let r = mul_fixed(q, q)?;
    let numerator = mul_fixed(horner_fixed(&INVERSE_NORMAL_A, r)?, q)?;
    let denominator = mul_fixed(horner_fixed(&INVERSE_NORMAL_B, r)?, r)?
        .checked_add(FIXED_SCALE)
        .ok_or(FishingNumericError::ArithmeticOverflow)?;
    div_fixed(numerator, denominator)
}

fn pow_positive_fixed(
    value: i128,
    exponent_numerator: i128,
    exponent_denominator: i128,
) -> Result<i128, FishingNumericError> {
    if value <= 0 || exponent_denominator <= 0 {
        return Err(FishingNumericError::InvalidNumericDomain);
    }
    let logarithm = natural_log_fixed(value)?;
    let exponentiated_log = round_div_signed(
        logarithm
            .checked_mul(exponent_numerator)
            .ok_or(FishingNumericError::ArithmeticOverflow)?,
        exponent_denominator,
    )?;
    exp_fixed(exponentiated_log)
}

fn natural_log_fixed(value: i128) -> Result<i128, FishingNumericError> {
    if value <= 0 {
        return Err(FishingNumericError::InvalidNumericDomain);
    }

    let mut mantissa = value;
    let mut power_of_two = 0_i32;

    while mantissa < FIXED_SCALE {
        mantissa = mantissa
            .checked_mul(2)
            .ok_or(FishingNumericError::ArithmeticOverflow)?;
        power_of_two = power_of_two
            .checked_sub(1)
            .ok_or(FishingNumericError::ArithmeticOverflow)?;
    }
    while mantissa >= FIXED_SCALE * 2 {
        mantissa = round_div_signed(mantissa, 2)?;
        power_of_two = power_of_two
            .checked_add(1)
            .ok_or(FishingNumericError::ArithmeticOverflow)?;
    }

    let y = div_fixed(mantissa - FIXED_SCALE, mantissa + FIXED_SCALE)?;
    let y_squared = mul_fixed(y, y)?;
    let mut term = y;
    let mut series = 0_i128;

    for index in 0_i128..30 {
        let divisor = index
            .checked_mul(2)
            .and_then(|value| value.checked_add(1))
            .ok_or(FishingNumericError::ArithmeticOverflow)?;
        series = series
            .checked_add(round_div_signed(term, divisor)?)
            .ok_or(FishingNumericError::ArithmeticOverflow)?;
        term = mul_fixed(term, y_squared)?;
    }

    series
        .checked_mul(2)
        .and_then(|value| {
            i128::from(power_of_two)
                .checked_mul(LN_2_FIXED)
                .and_then(|offset| value.checked_add(offset))
        })
        .ok_or(FishingNumericError::ArithmeticOverflow)
}

fn exp_fixed(value: i128) -> Result<i128, FishingNumericError> {
    let power_of_two = round_div_signed(value, LN_2_FIXED)?;
    let power_of_two =
        i32::try_from(power_of_two).map_err(|_| FishingNumericError::ArithmeticOverflow)?;
    let reduced = value
        .checked_sub(
            i128::from(power_of_two)
                .checked_mul(LN_2_FIXED)
                .ok_or(FishingNumericError::ArithmeticOverflow)?,
        )
        .ok_or(FishingNumericError::ArithmeticOverflow)?;

    let mut term = FIXED_SCALE;
    let mut sum = FIXED_SCALE;
    for divisor in 1_i128..=34 {
        term = round_div_signed(mul_fixed(term, reduced)?, divisor)?;
        sum = sum
            .checked_add(term)
            .ok_or(FishingNumericError::ArithmeticOverflow)?;
        if term == 0 {
            break;
        }
    }

    if power_of_two >= 0 {
        let shift =
            u32::try_from(power_of_two).map_err(|_| FishingNumericError::ArithmeticOverflow)?;
        if shift >= 127 {
            return Err(FishingNumericError::ArithmeticOverflow);
        }
        sum.checked_mul(1_i128 << shift)
            .ok_or(FishingNumericError::ArithmeticOverflow)
    } else {
        let shift = power_of_two
            .checked_neg()
            .and_then(|value| u32::try_from(value).ok())
            .ok_or(FishingNumericError::ArithmeticOverflow)?;
        if shift >= 127 {
            return Ok(0);
        }
        round_div_signed(sum, 1_i128 << shift)
    }
}

fn sqrt_fixed(value: i128) -> Result<i128, FishingNumericError> {
    if value < 0 {
        return Err(FishingNumericError::InvalidNumericDomain);
    }
    let radicand = u128::try_from(value)
        .map_err(|_| FishingNumericError::ArithmeticOverflow)?
        .checked_mul(FIXED_SCALE_U128)
        .ok_or(FishingNumericError::ArithmeticOverflow)?;
    i128::try_from(integer_sqrt_u128(radicand)).map_err(|_| FishingNumericError::ArithmeticOverflow)
}

fn integer_sqrt_u128(mut value: u128) -> u128 {
    let mut result = 0_u128;
    let mut bit = 1_u128 << 126;

    while bit > value {
        bit >>= 2;
    }

    while bit != 0 {
        let candidate = result + bit;
        if value >= candidate {
            value -= candidate;
            result = (result >> 1) + bit;
        } else {
            result >>= 1;
        }
        bit >>= 2;
    }

    result
}

fn horner_fixed(coefficients: &[i128], value: i128) -> Result<i128, FishingNumericError> {
    let (&first, rest) = coefficients
        .split_first()
        .ok_or(FishingNumericError::InvalidNumericDomain)?;
    rest.iter().try_fold(first, |accumulator, coefficient| {
        mul_fixed(accumulator, value)?
            .checked_add(*coefficient)
            .ok_or(FishingNumericError::ArithmeticOverflow)
    })
}

fn mul_fixed(left: i128, right: i128) -> Result<i128, FishingNumericError> {
    round_div_signed(
        left.checked_mul(right)
            .ok_or(FishingNumericError::ArithmeticOverflow)?,
        FIXED_SCALE,
    )
}

fn mul_fixed_by_ratio(
    value: i128,
    numerator: i128,
    denominator: i128,
) -> Result<i128, FishingNumericError> {
    if denominator <= 0 {
        return Err(FishingNumericError::InvalidNumericDomain);
    }
    round_div_signed(
        value
            .checked_mul(numerator)
            .ok_or(FishingNumericError::ArithmeticOverflow)?,
        denominator,
    )
}

fn div_fixed(numerator: i128, denominator: i128) -> Result<i128, FishingNumericError> {
    if denominator == 0 {
        return Err(FishingNumericError::InvalidNumericDomain);
    }
    let (numerator, denominator) = if denominator < 0 {
        (
            numerator
                .checked_neg()
                .ok_or(FishingNumericError::ArithmeticOverflow)?,
            denominator
                .checked_neg()
                .ok_or(FishingNumericError::ArithmeticOverflow)?,
        )
    } else {
        (numerator, denominator)
    };
    round_div_signed(
        numerator
            .checked_mul(FIXED_SCALE)
            .ok_or(FishingNumericError::ArithmeticOverflow)?,
        denominator,
    )
}

fn round_div_signed(numerator: i128, denominator: i128) -> Result<i128, FishingNumericError> {
    if denominator <= 0 {
        return Err(FishingNumericError::InvalidNumericDomain);
    }
    let negative = numerator < 0;
    let magnitude = numerator
        .checked_abs()
        .ok_or(FishingNumericError::ArithmeticOverflow)?;
    let quotient = magnitude / denominator;
    let remainder = magnitude % denominator;
    let threshold = denominator / 2 + denominator % 2;
    let rounded = if remainder >= threshold {
        quotient
            .checked_add(1)
            .ok_or(FishingNumericError::ArithmeticOverflow)?
    } else {
        quotient
    };
    if negative {
        rounded
            .checked_neg()
            .ok_or(FishingNumericError::ArithmeticOverflow)
    } else {
        Ok(rounded)
    }
}

fn round_div_u128(numerator: u128, denominator: u128) -> Result<u128, FishingNumericError> {
    if denominator == 0 {
        return Err(FishingNumericError::InvalidNumericDomain);
    }
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    let threshold = denominator / 2 + denominator % 2;
    if remainder >= threshold {
        quotient
            .checked_add(1)
            .ok_or(FishingNumericError::ArithmeticOverflow)
    } else {
        Ok(quotient)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_normal_midpoint_grid_is_symmetric() {
        for draw in [0, 1, 100, 1_000, 24_250, 499_999] {
            assert_eq!(
                inverse_standard_normal_from_ppm_midpoint(draw).unwrap(),
                -inverse_standard_normal_from_ppm_midpoint(
                    FISHING_NUMERIC_DRAW_SCALE_PPM - 1 - draw
                )
                .unwrap()
            );
        }
    }

    #[test]
    fn fixed_fractional_power_vectors_are_stable() {
        assert_eq!(
            pow_positive_fixed(400_000_000_000, 17, 20).unwrap(),
            458_934_802_226
        );
        assert_eq!(
            pow_positive_fixed(2_500_000_000_000, 1, 3).unwrap(),
            1_357_208_808_299
        );
        assert_eq!(
            pow_positive_fixed(6_000_000_000_000, 13, 10).unwrap(),
            10_270_619_156_448
        );
    }

    #[test]
    fn out_of_range_draws_fail_closed() {
        assert_eq!(
            resolve_fish_instance_numeric_v1(
                FishingSpecies::Bluegill,
                FishingVariant::Normal,
                FISHING_NUMERIC_DRAW_SCALE_PPM,
                0,
                false,
            ),
            Err(FishingNumericError::DrawOutOfRange(
                FISHING_NUMERIC_DRAW_SCALE_PPM
            ))
        );
    }
}
