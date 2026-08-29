use serde::Serialize;
use thiserror::Error;

const MAIN_MULTIPLIER_DENOMINATOR: u128 = 250_000;
const MAIN_LINEAR_NUMERATOR: u128 = 8_095;
const MAIN_QUADRATIC_NUMERATOR: u128 = 238;

const UPGRADE_FACTOR_DENOMINATOR: u128 = 5_000_000;
const UPGRADE_LINEAR_NUMERATOR: u128 = 89_045;
const UPGRADE_QUADRATIC_NUMERATOR: u128 = 2_618;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ExactUpgradeFactor {
    numerator: u128,
    denominator: u128,
}

impl ExactUpgradeFactor {
    pub const fn numerator(self) -> u128 {
        self.numerator
    }

    pub const fn denominator(self) -> u128 {
        self.denominator
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct UpgradeAppraisalFactors {
    pub upgrade_level: u64,
    pub main_multiplier: ExactUpgradeFactor,
    pub upgrade_factor: ExactUpgradeFactor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct UpgradeScaledBaseAppraisal {
    pub upgrade_level: u64,
    pub base_appraisal: i64,
    numerator: u128,
    denominator: u128,
}

impl UpgradeScaledBaseAppraisal {
    pub const fn numerator(self) -> u128 {
        self.numerator
    }

    pub const fn denominator(self) -> u128 {
        self.denominator
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum UpgradeAppraisalError {
    #[error("base equipment appraisal cannot be negative")]
    NegativeBaseAppraisal,
    #[error("+N appraisal-factor arithmetic exceeded supported integer bounds")]
    ArithmeticOverflow,
}

/// Returns the exact frozen +N power and appraisal factors without floating-point arithmetic.
///
/// The canonical formulas are:
///
/// `MainMult(N) = 1 + 0.03238N + 0.000952N²`
///
/// `UpgradeFactor(N) = 1 + 0.55 × (MainMult(N) - 1)`
///
/// They reduce exactly to the fixed rational forms used here. `N = 0` is valid and produces 1/1
/// semantically (represented with the fixed canonical denominators). The specification has no
/// finite gameplay +N cap, so arithmetic is checked and levels beyond supported integer bounds
/// fail closed rather than wrapping.
pub fn upgrade_appraisal_factors(
    upgrade_level: u64,
) -> Result<UpgradeAppraisalFactors, UpgradeAppraisalError> {
    let n = u128::from(upgrade_level);
    let n_squared = n
        .checked_mul(n)
        .ok_or(UpgradeAppraisalError::ArithmeticOverflow)?;

    let main_extra = MAIN_LINEAR_NUMERATOR
        .checked_mul(n)
        .and_then(|linear| {
            MAIN_QUADRATIC_NUMERATOR
                .checked_mul(n_squared)
                .and_then(|quadratic| linear.checked_add(quadratic))
        })
        .ok_or(UpgradeAppraisalError::ArithmeticOverflow)?;
    let main_numerator = MAIN_MULTIPLIER_DENOMINATOR
        .checked_add(main_extra)
        .ok_or(UpgradeAppraisalError::ArithmeticOverflow)?;

    let upgrade_extra = UPGRADE_LINEAR_NUMERATOR
        .checked_mul(n)
        .and_then(|linear| {
            UPGRADE_QUADRATIC_NUMERATOR
                .checked_mul(n_squared)
                .and_then(|quadratic| linear.checked_add(quadratic))
        })
        .ok_or(UpgradeAppraisalError::ArithmeticOverflow)?;
    let upgrade_numerator = UPGRADE_FACTOR_DENOMINATOR
        .checked_add(upgrade_extra)
        .ok_or(UpgradeAppraisalError::ArithmeticOverflow)?;

    Ok(UpgradeAppraisalFactors {
        upgrade_level,
        main_multiplier: ExactUpgradeFactor {
            numerator: main_numerator,
            denominator: MAIN_MULTIPLIER_DENOMINATOR,
        },
        upgrade_factor: ExactUpgradeFactor {
            numerator: upgrade_numerator,
            denominator: UPGRADE_FACTOR_DENOMINATOR,
        },
    })
}

/// Applies only the frozen +N appraisal factor to an already-resolved base equipment appraisal.
///
/// The result remains an exact rational and is intentionally **not rounded**. The final enhanced
/// appraisal must still multiply by the immutable creation-roll factor, add embedded-enchant value,
/// and only then apply the specification's final round-half-up rule. Returning an unrounded rational
/// here prevents an intermediate-rounding drift from becoming authoritative policy.
pub fn scale_base_appraisal_by_upgrade(
    base_appraisal: i64,
    upgrade_level: u64,
) -> Result<UpgradeScaledBaseAppraisal, UpgradeAppraisalError> {
    if base_appraisal < 0 {
        return Err(UpgradeAppraisalError::NegativeBaseAppraisal);
    }

    let factors = upgrade_appraisal_factors(upgrade_level)?;
    let numerator = u128::try_from(base_appraisal)
        .map_err(|_| UpgradeAppraisalError::NegativeBaseAppraisal)?
        .checked_mul(factors.upgrade_factor.numerator)
        .ok_or(UpgradeAppraisalError::ArithmeticOverflow)?;

    Ok(UpgradeScaledBaseAppraisal {
        upgrade_level,
        base_appraisal,
        numerator,
        denominator: factors.upgrade_factor.denominator,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_multiplier_matches_frozen_reference_levels_exactly() {
        let cases = [
            (0, 250_000),
            (5, 296_425),
            (10, 354_750),
            (15, 424_975),
            (20, 507_100),
            (30, 707_050),
            (50, 1_249_750),
        ];

        for (level, expected_numerator) in cases {
            let factors = upgrade_appraisal_factors(level).unwrap();
            assert_eq!(factors.main_multiplier.numerator(), expected_numerator);
            assert_eq!(
                factors.main_multiplier.denominator(),
                MAIN_MULTIPLIER_DENOMINATOR
            );
        }
    }

    #[test]
    fn upgrade_factor_matches_exact_formula_at_reference_levels() {
        let cases = [
            (0, 5_000_000),
            (5, 5_510_675),
            (10, 6_152_250),
            (15, 6_924_725),
            (20, 7_828_100),
            (30, 10_027_550),
            (50, 15_997_250),
        ];

        for (level, expected_numerator) in cases {
            let factors = upgrade_appraisal_factors(level).unwrap();
            assert_eq!(factors.upgrade_factor.numerator(), expected_numerator);
            assert_eq!(
                factors.upgrade_factor.denominator(),
                UPGRADE_FACTOR_DENOMINATOR
            );
        }
    }

    #[test]
    fn upgrade_factor_is_exactly_fifty_five_percent_of_main_delta() {
        for level in [0, 1, 5, 10, 20, 50, 1_000, 10_000] {
            let factors = upgrade_appraisal_factors(level).unwrap();
            let main_delta = factors.main_multiplier.numerator() - MAIN_MULTIPLIER_DENOMINATOR;
            let upgrade_delta = factors.upgrade_factor.numerator() - UPGRADE_FACTOR_DENOMINATOR;
            assert_eq!(upgrade_delta, main_delta * 11);
        }
    }

    #[test]
    fn factors_are_strictly_monotonic_for_practical_reference_range() {
        let mut previous_main = 0;
        let mut previous_upgrade = 0;

        for level in 0..=10_000 {
            let factors = upgrade_appraisal_factors(level).unwrap();
            assert!(factors.main_multiplier.numerator() > previous_main || level == 0);
            assert!(factors.upgrade_factor.numerator() > previous_upgrade || level == 0);
            previous_main = factors.main_multiplier.numerator();
            previous_upgrade = factors.upgrade_factor.numerator();
        }
    }

    #[test]
    fn scaling_base_appraisal_preserves_exact_fraction_without_rounding() {
        let scaled = scale_base_appraisal_by_upgrade(77_500, 5).unwrap();
        assert_eq!(scaled.base_appraisal, 77_500);
        assert_eq!(scaled.upgrade_level, 5);
        assert_eq!(scaled.numerator(), 427_077_312_500);
        assert_eq!(scaled.denominator(), UPGRADE_FACTOR_DENOMINATOR);

        let zero_level = scale_base_appraisal_by_upgrade(123_456, 0).unwrap();
        assert_eq!(zero_level.numerator(), 123_456 * UPGRADE_FACTOR_DENOMINATOR);
        assert_eq!(zero_level.denominator(), UPGRADE_FACTOR_DENOMINATOR);
    }

    #[test]
    fn zero_base_appraisal_remains_exact_zero() {
        let scaled = scale_base_appraisal_by_upgrade(0, 50).unwrap();
        assert_eq!(scaled.numerator(), 0);
        assert_eq!(scaled.denominator(), UPGRADE_FACTOR_DENOMINATOR);
    }

    #[test]
    fn negative_base_appraisal_is_rejected() {
        assert_eq!(
            scale_base_appraisal_by_upgrade(-1, 5),
            Err(UpgradeAppraisalError::NegativeBaseAppraisal)
        );
    }

    #[test]
    fn scaling_overflow_is_rejected_even_when_factor_itself_is_representable() {
        assert!(upgrade_appraisal_factors(1_000_000_000).is_ok());
        assert_eq!(
            scale_base_appraisal_by_upgrade(i64::MAX, 1_000_000_000),
            Err(UpgradeAppraisalError::ArithmeticOverflow)
        );
    }

    #[test]
    fn unsupported_extreme_level_fails_closed_instead_of_wrapping() {
        assert_eq!(
            upgrade_appraisal_factors(u64::MAX),
            Err(UpgradeAppraisalError::ArithmeticOverflow)
        );
    }
}
