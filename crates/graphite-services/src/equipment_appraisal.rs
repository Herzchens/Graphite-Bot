use serde::Serialize;
use thiserror::Error;

use crate::{BaseEquipmentAppraisal, UpgradeAppraisalError, scale_base_appraisal_by_upgrade};

const ROLL_FACTOR_BASE_DENOMINATOR: u128 = 25;
const ROLL_FACTOR_QUADRATIC_NUMERATOR: u128 = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct CreationRoll {
    numerator: u64,
    denominator: u64,
}

impl CreationRoll {
    pub const ZERO: Self = Self {
        numerator: 0,
        denominator: 1,
    };
    pub const ONE: Self = Self {
        numerator: 1,
        denominator: 1,
    };

    pub fn new(numerator: u64, denominator: u64) -> Result<Self, CreationRollError> {
        if denominator == 0 {
            return Err(CreationRollError::ZeroDenominator);
        }
        if numerator > denominator {
            return Err(CreationRollError::OutOfRange);
        }
        if numerator == 0 {
            return Ok(Self::ZERO);
        }

        let divisor = gcd_u64(numerator, denominator);
        Ok(Self {
            numerator: numerator / divisor,
            denominator: denominator / divisor,
        })
    }

    pub const fn numerator(self) -> u64 {
        self.numerator
    }

    pub const fn denominator(self) -> u64 {
        self.denominator
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum CreationRollError {
    #[error("creation-roll denominator must be positive")]
    ZeroDenominator,
    #[error("creation roll must be an exact percentile in the inclusive range [0, 1]")]
    OutOfRange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct CanonicalEquipmentAppraisal {
    pub base: BaseEquipmentAppraisal,
    pub creation_roll: CreationRoll,
    pub upgrade_level: u64,
    pub embedded_enchant_value: i64,
    pub recraft_appraisal: i64,
    pub enhanced_canonical_appraisal: i64,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum CanonicalEquipmentAppraisalError {
    #[error("base equipment appraisal cannot be negative")]
    NegativeBaseAppraisal,
    #[error("embedded enchant value cannot be negative")]
    NegativeEmbeddedEnchantValue,
    #[error("canonical equipment appraisal arithmetic exceeded supported integer bounds")]
    ArithmeticOverflow,
}

/// Composes the frozen structural and enhanced equipment appraisals from already-resolved inputs.
///
/// `creation_roll` is an exact rational percentile in `[0, 1]`; this policy deliberately does not
/// choose a persistence precision for the immutable roll. The structural value is evaluated as the
/// exact rational `BaseEquipmentAppraisal × RollFactor(q) × UpgradeFactor(N)` and rounded half-up
/// once to produce `RecraftAppraisal`.
///
/// `EmbeddedEnchantValue` is already an integer after its own canonical 70% round-half-up step.
/// Therefore final `round_half_up(structural + EmbeddedEnchantValue)` is algebraically identical to
/// `RecraftAppraisal + EmbeddedEnchantValue`. Using that equivalent form avoids another large
/// denominator multiplication while preserving the frozen result exactly.
pub fn compose_canonical_equipment_appraisal(
    base: BaseEquipmentAppraisal,
    creation_roll: CreationRoll,
    upgrade_level: u64,
    embedded_enchant_value: i64,
) -> Result<CanonicalEquipmentAppraisal, CanonicalEquipmentAppraisalError> {
    if base.value < 0 {
        return Err(CanonicalEquipmentAppraisalError::NegativeBaseAppraisal);
    }
    if embedded_enchant_value < 0 {
        return Err(CanonicalEquipmentAppraisalError::NegativeEmbeddedEnchantValue);
    }

    let upgrade_scaled =
        scale_base_appraisal_by_upgrade(base.value, upgrade_level).map_err(map_upgrade_error)?;
    let (roll_numerator, roll_denominator) = creation_roll_factor(creation_roll)?;
    let (structural_numerator, structural_denominator) = multiply_fractions(
        upgrade_scaled.numerator(),
        upgrade_scaled.denominator(),
        roll_numerator,
        roll_denominator,
    )?;
    let recraft_appraisal = round_half_up_fraction(structural_numerator, structural_denominator)?;
    let enhanced_canonical_appraisal = recraft_appraisal
        .checked_add(embedded_enchant_value)
        .ok_or(CanonicalEquipmentAppraisalError::ArithmeticOverflow)?;

    Ok(CanonicalEquipmentAppraisal {
        base,
        creation_roll,
        upgrade_level,
        embedded_enchant_value,
        recraft_appraisal,
        enhanced_canonical_appraisal,
    })
}

fn creation_roll_factor(
    creation_roll: CreationRoll,
) -> Result<(u128, u128), CanonicalEquipmentAppraisalError> {
    let numerator = u128::from(creation_roll.numerator);
    let denominator = u128::from(creation_roll.denominator);
    let numerator_squared = numerator
        .checked_mul(numerator)
        .ok_or(CanonicalEquipmentAppraisalError::ArithmeticOverflow)?;
    let denominator_squared = denominator
        .checked_mul(denominator)
        .ok_or(CanonicalEquipmentAppraisalError::ArithmeticOverflow)?;
    let base = denominator_squared
        .checked_mul(ROLL_FACTOR_BASE_DENOMINATOR)
        .ok_or(CanonicalEquipmentAppraisalError::ArithmeticOverflow)?;
    let quadratic = numerator_squared
        .checked_mul(ROLL_FACTOR_QUADRATIC_NUMERATOR)
        .ok_or(CanonicalEquipmentAppraisalError::ArithmeticOverflow)?;
    let roll_numerator = base
        .checked_add(quadratic)
        .ok_or(CanonicalEquipmentAppraisalError::ArithmeticOverflow)?;
    Ok(reduce_fraction(roll_numerator, base))
}

fn multiply_fractions(
    left_numerator: u128,
    left_denominator: u128,
    right_numerator: u128,
    right_denominator: u128,
) -> Result<(u128, u128), CanonicalEquipmentAppraisalError> {
    debug_assert!(left_denominator > 0 && right_denominator > 0);
    let (mut left_numerator, mut left_denominator) =
        reduce_fraction(left_numerator, left_denominator);
    let (mut right_numerator, mut right_denominator) =
        reduce_fraction(right_numerator, right_denominator);

    let cross_left = gcd_u128(left_numerator, right_denominator);
    left_numerator /= cross_left;
    right_denominator /= cross_left;

    let cross_right = gcd_u128(right_numerator, left_denominator);
    right_numerator /= cross_right;
    left_denominator /= cross_right;

    let numerator = left_numerator
        .checked_mul(right_numerator)
        .ok_or(CanonicalEquipmentAppraisalError::ArithmeticOverflow)?;
    let denominator = left_denominator
        .checked_mul(right_denominator)
        .ok_or(CanonicalEquipmentAppraisalError::ArithmeticOverflow)?;
    Ok(reduce_fraction(numerator, denominator))
}

fn round_half_up_fraction(
    numerator: u128,
    denominator: u128,
) -> Result<i64, CanonicalEquipmentAppraisalError> {
    debug_assert!(denominator > 0);
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    let half_threshold = denominator / 2 + denominator % 2;
    let rounded = quotient
        .checked_add(u128::from(remainder >= half_threshold))
        .ok_or(CanonicalEquipmentAppraisalError::ArithmeticOverflow)?;
    i64::try_from(rounded).map_err(|_| CanonicalEquipmentAppraisalError::ArithmeticOverflow)
}

fn reduce_fraction(numerator: u128, denominator: u128) -> (u128, u128) {
    debug_assert!(denominator > 0);
    if numerator == 0 {
        return (0, 1);
    }
    let divisor = gcd_u128(numerator, denominator);
    (numerator / divisor, denominator / divisor)
}

const fn gcd_u64(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

const fn gcd_u128(mut left: u128, mut right: u128) -> u128 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

const fn map_upgrade_error(error: UpgradeAppraisalError) -> CanonicalEquipmentAppraisalError {
    match error {
        UpgradeAppraisalError::NegativeBaseAppraisal => {
            CanonicalEquipmentAppraisalError::NegativeBaseAppraisal
        }
        UpgradeAppraisalError::ArithmeticOverflow => {
            CanonicalEquipmentAppraisalError::ArithmeticOverflow
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BaseEquipmentAppraisalSource, EquipmentSlot, EquipmentTier};

    fn base(value: i64) -> BaseEquipmentAppraisal {
        BaseEquipmentAppraisal {
            tier: EquipmentTier::Iron,
            slot: EquipmentSlot::Pickaxe,
            value,
            source: BaseEquipmentAppraisalSource::DefinitionOverride,
        }
    }

    #[test]
    fn creation_roll_is_validated_and_normalized_without_freezing_storage_precision() {
        assert_eq!(CreationRoll::new(0, 100).unwrap(), CreationRoll::ZERO);
        assert_eq!(
            CreationRoll::new(50, 100).unwrap(),
            CreationRoll::new(1, 2).unwrap()
        );
        assert_eq!(CreationRoll::new(100, 100).unwrap(), CreationRoll::ONE);
        assert_eq!(
            CreationRoll::new(1, 0),
            Err(CreationRollError::ZeroDenominator)
        );
        assert_eq!(
            CreationRoll::new(101, 100),
            Err(CreationRollError::OutOfRange)
        );
    }

    #[test]
    fn roll_factor_matches_frozen_quadratic_exactly() {
        assert_eq!(creation_roll_factor(CreationRoll::ZERO).unwrap(), (1, 1));
        assert_eq!(
            creation_roll_factor(CreationRoll::new(1, 2).unwrap()).unwrap(),
            (103, 100)
        );
        assert_eq!(creation_roll_factor(CreationRoll::ONE).unwrap(), (28, 25));
    }

    #[test]
    fn no_roll_upgrade_or_enchants_preserves_base_value() {
        let appraisal =
            compose_canonical_equipment_appraisal(base(77_500), CreationRoll::ZERO, 0, 0).unwrap();
        assert_eq!(appraisal.recraft_appraisal, 77_500);
        assert_eq!(appraisal.enhanced_canonical_appraisal, 77_500);
    }

    #[test]
    fn final_rounding_is_half_up_and_happens_after_exact_structural_math() {
        let appraisal =
            compose_canonical_equipment_appraisal(base(50), CreationRoll::new(1, 2).unwrap(), 0, 0)
                .unwrap();
        assert_eq!(appraisal.recraft_appraisal, 52, "50 × 1.03 = 51.5");
    }

    #[test]
    fn combined_roll_upgrade_and_embedded_value_match_exact_reference_math() {
        let appraisal = compose_canonical_equipment_appraisal(
            base(77_500),
            CreationRoll::new(1, 2).unwrap(),
            5,
            1_921_500,
        )
        .unwrap();
        assert_eq!(appraisal.recraft_appraisal, 87_978);
        assert_eq!(appraisal.enhanced_canonical_appraisal, 2_009_478);
    }

    #[test]
    fn embedded_integer_value_is_an_exact_additive_shift_after_structural_rounding() {
        for embedded in [0, 1, 60_000, 1_921_500, 7_200_000] {
            let appraisal = compose_canonical_equipment_appraisal(
                base(77_500),
                CreationRoll::new(37, 100).unwrap(),
                13,
                embedded,
            )
            .unwrap();
            assert_eq!(
                appraisal.enhanced_canonical_appraisal,
                appraisal.recraft_appraisal + embedded
            );
            assert!(appraisal.recraft_appraisal <= appraisal.enhanced_canonical_appraisal);
        }
    }

    #[test]
    fn invariants_hold_across_dense_roll_and_upgrade_boundaries() {
        for denominator in 1_u64..=32 {
            for numerator in 0..=denominator {
                let roll = CreationRoll::new(numerator, denominator).unwrap();
                for upgrade_level in [0, 1, 5, 10, 20, 50] {
                    for base_value in [0, 50, 3_600, 77_500, 8_050_000] {
                        let appraisal = compose_canonical_equipment_appraisal(
                            base(base_value),
                            roll,
                            upgrade_level,
                            123_456,
                        )
                        .unwrap();
                        assert!(appraisal.recraft_appraisal >= base_value);
                        assert_eq!(
                            appraisal.enhanced_canonical_appraisal,
                            appraisal.recraft_appraisal + 123_456
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn largest_base_is_supported_when_factors_are_identity() {
        let appraisal =
            compose_canonical_equipment_appraisal(base(i64::MAX), CreationRoll::ZERO, 0, 0)
                .unwrap();
        assert_eq!(appraisal.recraft_appraisal, i64::MAX);
        assert_eq!(appraisal.enhanced_canonical_appraisal, i64::MAX);
    }

    #[test]
    fn invalid_and_unrepresentable_inputs_fail_closed() {
        assert_eq!(
            compose_canonical_equipment_appraisal(base(-1), CreationRoll::ZERO, 0, 0),
            Err(CanonicalEquipmentAppraisalError::NegativeBaseAppraisal)
        );
        assert_eq!(
            compose_canonical_equipment_appraisal(base(1), CreationRoll::ZERO, 0, -1),
            Err(CanonicalEquipmentAppraisalError::NegativeEmbeddedEnchantValue)
        );
        assert_eq!(
            compose_canonical_equipment_appraisal(base(i64::MAX), CreationRoll::ZERO, 0, 1),
            Err(CanonicalEquipmentAppraisalError::ArithmeticOverflow)
        );
        assert_eq!(
            compose_canonical_equipment_appraisal(
                base(1),
                CreationRoll::new(1, u64::MAX).unwrap(),
                0,
                0,
            ),
            Err(CanonicalEquipmentAppraisalError::ArithmeticOverflow)
        );
    }
}
