use thiserror::Error;

use crate::{MiningDepth, mining_depth_risk_policy};

/// Exact denominator used for Mining disturbance accounting.
///
/// The frozen pressure sources are all integral multiples of `0.25`, so quarter-units preserve the
/// specification exactly without choosing a floating-point or database precision for geological
/// pressure itself.
pub const MINING_PRESSURE_QUARTERS_PER_UNIT: u128 = 4;

/// Canonical appraisal represented by one whole pressure unit for an additional non-physical
/// Treasure/mutation output.
pub const MINING_PRESSURE_APPRAISAL_PER_UNIT: i64 = 1_000;

/// Maximum whole pressure units charged for one additional non-physical Treasure/mutation output.
pub const MINING_PRESSURE_MAX_NON_PHYSICAL_OUTPUT_UNITS: i64 = 25;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MiningPressureContribution {
    quarter_units: u128,
}

impl MiningPressureContribution {
    pub const ZERO: Self = Self { quarter_units: 0 };

    #[must_use]
    pub const fn quarter_units(self) -> u128 {
        self.quarter_units
    }

    /// Returns the fixed denominator for [`Self::quarter_units`].
    #[must_use]
    pub const fn pressure_unit_denominator(self) -> u128 {
        MINING_PRESSURE_QUARTERS_PER_UNIT
    }

    pub fn checked_add(self, other: Self) -> Result<Self, MiningPressurePolicyError> {
        let quarter_units = self
            .quarter_units
            .checked_add(other.quarter_units)
            .ok_or(MiningPressurePolicyError::ArithmeticOverflow)?;
        Ok(Self { quarter_units })
    }

    const fn from_quarter_units(quarter_units: u128) -> Self {
        Self { quarter_units }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MiningPressureIncrementRatio {
    numerator: u128,
    denominator: u128,
}

impl MiningPressureIncrementRatio {
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
pub enum MiningPressurePolicyError {
    #[error("canonical appraisal for a non-physical Mining output cannot be negative")]
    NegativeCanonicalAppraisal,
    #[error("Mining pressure contribution arithmetic exceeded supported integer bounds")]
    ArithmeticOverflow,
}

/// Returns the frozen `1.00` pressure-unit contribution for each actual physical block disturbed.
///
/// This is intentionally source-local. The future committed-result owner must ensure a physical
/// block is represented exactly once when composing ordinary, Trench, and Nuke result bookkeeping.
#[must_use]
pub fn physical_block_pressure_contribution(
    actual_physical_blocks: u64,
) -> MiningPressureContribution {
    whole_block_pressure_contribution(actual_physical_blocks)
}

/// Returns the frozen `0.25` pressure-unit contribution for each Fortune-created extra material unit.
#[must_use]
pub fn fortune_extra_material_pressure_contribution(
    extra_material_units: u64,
) -> MiningPressureContribution {
    MiningPressureContribution::from_quarter_units(u128::from(extra_material_units))
}

/// Returns the frozen `1.00` pressure-unit contribution for each Trench extra block.
///
/// This function does not aggregate a complete committed Mining result, so callers must not also
/// count the same Trench block through another physical-block bucket.
#[must_use]
pub fn trench_extra_block_pressure_contribution(extra_blocks: u64) -> MiningPressureContribution {
    whole_block_pressure_contribution(extra_blocks)
}

/// Returns the frozen Nuke contribution equal to the actual number of blast blocks.
///
/// This function does not aggregate a complete committed Mining result, so callers must not also
/// count the same blast block through another physical-block bucket.
#[must_use]
pub fn nuke_blast_block_pressure_contribution(
    actual_blast_blocks: u64,
) -> MiningPressureContribution {
    whole_block_pressure_contribution(actual_blast_blocks)
}

/// Returns the frozen value-based pressure charge for an additional non-physical Treasure/mutation
/// output.
///
/// `None` means no additional non-physical output and therefore contributes zero. `Some(appraisal)`
/// means such an output exists and applies
/// `min(25, max(1, ceil(canonical_appraisal / 1_000)))` whole pressure units. A natural ore block
/// already charged as physical disturbance must remain on the physical path and must not call this
/// value-charge path a second time.
pub fn additional_non_physical_output_pressure_contribution(
    canonical_appraisal: Option<i64>,
) -> Result<MiningPressureContribution, MiningPressurePolicyError> {
    let Some(canonical_appraisal) = canonical_appraisal else {
        return Ok(MiningPressureContribution::ZERO);
    };
    if canonical_appraisal < 0 {
        return Err(MiningPressurePolicyError::NegativeCanonicalAppraisal);
    }

    let quotient = canonical_appraisal / MINING_PRESSURE_APPRAISAL_PER_UNIT;
    let remainder = canonical_appraisal % MINING_PRESSURE_APPRAISAL_PER_UNIT;
    let rounded_up = quotient + if remainder == 0 { 0 } else { 1 };
    let whole_units = rounded_up.clamp(1, MINING_PRESSURE_MAX_NON_PHYSICAL_OUTPUT_UNITS);
    let whole_units =
        u128::try_from(whole_units).map_err(|_| MiningPressurePolicyError::ArithmeticOverflow)?;
    Ok(MiningPressureContribution::from_quarter_units(
        whole_units * MINING_PRESSURE_QUARTERS_PER_UNIT,
    ))
}

/// Returns the exact disturbance term `pressure_units / SeamCapacity(depth)` as a reduced rational.
///
/// This function deliberately stops before combining the disturbance term with stored pressure or
/// elapsed-time recovery. It therefore chooses no geological-pressure persistence precision, clock
/// resolution, rounding rule, density-band boundary behavior, or `/mine` lifecycle semantics.
#[must_use]
pub fn mining_pressure_increment_ratio(
    depth: MiningDepth,
    contribution: MiningPressureContribution,
) -> MiningPressureIncrementRatio {
    if contribution.quarter_units == 0 {
        return MiningPressureIncrementRatio {
            numerator: 0,
            denominator: 1,
        };
    }

    let seam_capacity = u128::from(mining_depth_risk_policy(depth).seam_capacity);
    let denominator = MINING_PRESSURE_QUARTERS_PER_UNIT * seam_capacity;
    let divisor = gcd_u128(contribution.quarter_units, denominator);
    MiningPressureIncrementRatio {
        numerator: contribution.quarter_units / divisor,
        denominator: denominator / divisor,
    }
}

fn whole_block_pressure_contribution(blocks: u64) -> MiningPressureContribution {
    MiningPressureContribution::from_quarter_units(
        u128::from(blocks) * MINING_PRESSURE_QUARTERS_PER_UNIT,
    )
}

const fn gcd_u128(mut left: u128, mut right: u128) -> u128 {
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

    #[test]
    fn frozen_source_contributions_are_exact_quarter_units() {
        assert_eq!(physical_block_pressure_contribution(3).quarter_units(), 12);
        assert_eq!(
            fortune_extra_material_pressure_contribution(3).quarter_units(),
            3
        );
        assert_eq!(
            trench_extra_block_pressure_contribution(2).quarter_units(),
            8
        );
        assert_eq!(
            nuke_blast_block_pressure_contribution(5).quarter_units(),
            20
        );
    }

    #[test]
    fn non_physical_output_value_charge_uses_frozen_ceiling_floor_and_cap() {
        let cases = [
            (None, 0),
            (Some(0), 4),
            (Some(1), 4),
            (Some(1_000), 4),
            (Some(1_001), 8),
            (Some(24_000), 96),
            (Some(24_001), 100),
            (Some(25_000), 100),
            (Some(25_001), 100),
            (Some(i64::MAX), 100),
        ];

        for (appraisal, expected_quarters) in cases {
            assert_eq!(
                additional_non_physical_output_pressure_contribution(appraisal)
                    .unwrap()
                    .quarter_units(),
                expected_quarters
            );
        }
        assert_eq!(
            additional_non_physical_output_pressure_contribution(Some(-1)),
            Err(MiningPressurePolicyError::NegativeCanonicalAppraisal)
        );
    }

    #[test]
    fn checked_add_preserves_exact_units_and_fails_closed_on_overflow() {
        let combined = physical_block_pressure_contribution(1)
            .checked_add(fortune_extra_material_pressure_contribution(1))
            .unwrap();
        assert_eq!(combined.quarter_units(), 5);
        assert_eq!(combined.pressure_unit_denominator(), 4);

        assert_eq!(
            MiningPressureContribution::from_quarter_units(u128::MAX)
                .checked_add(MiningPressureContribution::from_quarter_units(1)),
            Err(MiningPressurePolicyError::ArithmeticOverflow)
        );
    }

    #[test]
    fn pressure_increment_uses_exact_depth_seam_capacity_without_rounding() {
        let surface = mining_pressure_increment_ratio(
            MiningDepth::Surface,
            physical_block_pressure_contribution(1),
        );
        assert_eq!((surface.numerator(), surface.denominator()), (1, 3_800));

        let shallow = mining_pressure_increment_ratio(
            MiningDepth::Shallow,
            fortune_extra_material_pressure_contribution(1),
        );
        assert_eq!((shallow.numerator(), shallow.denominator()), (1, 20_800));

        let abyssal = mining_pressure_increment_ratio(
            MiningDepth::AbyssalDepths,
            additional_non_physical_output_pressure_contribution(Some(i64::MAX)).unwrap(),
        );
        assert_eq!((abyssal.numerator(), abyssal.denominator()), (1, 1_640));

        let zero = mining_pressure_increment_ratio(
            MiningDepth::AbyssalDepths,
            MiningPressureContribution::ZERO,
        );
        assert_eq!((zero.numerator(), zero.denominator()), (0, 1));
    }
}
