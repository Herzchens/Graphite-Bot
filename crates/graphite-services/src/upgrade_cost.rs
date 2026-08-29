use crate::equipment_policy::{
    BaseEquipmentAppraisal, EquipmentAppraisalError, EquipmentMaterial, EquipmentSlot,
    EquipmentTier, base_equipment_appraisal,
};
use serde::Serialize;
use thiserror::Error;

const TABLE_RATE_DENOMINATOR: i128 = 1_000;
const MONEY_ROUNDING_UNIT: i128 = 100;
const CONTINUATION_DECIMAL_MULTIPLIER: u32 = 135;
const MAX_CONTINUATION_EXPONENT_TO_EVALUATE: u64 = 144;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct UpgradeAttemptResourceCostPreview {
    pub target_level: u64,
    pub base_appraisal: BaseEquipmentAppraisal,
    pub money_cost: i64,
    pub reinforcement_material: EquipmentMaterial,
    pub reinforcement_units: u64,
    pub committed_failure_consumes_money_and_material: bool,
    pub downgrade_modifiers_do_not_change_base_attempt_cost: bool,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum UpgradeAttemptCostError {
    #[error("+N target level must be at least +1")]
    TargetLevelZero,
    #[error("starter equipment is non-upgradeable")]
    StarterEquipmentNotUpgradeable,
    #[error("Gold +N does not support equipment slot {0:?} in current-v1")]
    UnsupportedGoldSlot(EquipmentSlot),
    #[error("base equipment appraisal failed: {0}")]
    EquipmentAppraisal(EquipmentAppraisalError),
    #[error("+N attempt cost arithmetic exceeded supported integer bounds")]
    ArithmeticOverflow,
}

impl From<EquipmentAppraisalError> for UpgradeAttemptCostError {
    fn from(value: EquipmentAppraisalError) -> Self {
        Self::EquipmentAppraisal(value)
    }
}

/// Previews the frozen Money and reinforcement-material portion of one +N attempt.
///
/// `definition_override` is the authoritative ItemDefinition `base_appraisal` override when one
/// exists. The function resolves the pre-attempt base appraisal internally so the cost never uses
/// Creation Roll, current +N, embedded enchant value, Market price, or another already-enhanced
/// appraisal. Starter equipment is rejected because it is canonically non-upgradeable; Gold armor
/// slots are rejected because current-v1 defines Gold only for Pickaxe/Sword/Fishing Rod.
///
/// The future stateful owner must lock the authoritative ItemInstance and prove `target_level` is
/// exactly the next enhancement level (`current + 1`) before reserving any cost; this pure preview
/// prices a target and does not authorize skipping +N levels.
///
/// This preview deliberately excludes `UpgradeAEXP(N) = round10(20 × N^1.55)`: the active
/// specification freezes the formula but not a deterministic fractional-power evaluation algorithm.
/// It also does not reserve or consume assets. Money and reinforcement material are consumed only by
/// the future owning transaction after Confirm, including on a committed failed attempt.
pub fn preview_upgrade_attempt_resource_cost(
    tier: EquipmentTier,
    slot: EquipmentSlot,
    definition_override: Option<i64>,
    target_level: u64,
) -> Result<UpgradeAttemptResourceCostPreview, UpgradeAttemptCostError> {
    validate_upgrade_target(tier, slot, target_level)?;
    let base_appraisal = base_equipment_appraisal(tier, slot, definition_override)?;
    let money_cost = upgrade_money_cost(base_appraisal.value, target_level)?;

    Ok(UpgradeAttemptResourceCostPreview {
        target_level,
        base_appraisal,
        money_cost,
        reinforcement_material: tier.material(),
        reinforcement_units: reinforcement_units(target_level),
        committed_failure_consumes_money_and_material: true,
        downgrade_modifiers_do_not_change_base_attempt_cost: true,
    })
}

fn validate_upgrade_target(
    tier: EquipmentTier,
    slot: EquipmentSlot,
    target_level: u64,
) -> Result<(), UpgradeAttemptCostError> {
    if target_level == 0 {
        return Err(UpgradeAttemptCostError::TargetLevelZero);
    }
    if tier == EquipmentTier::StarterLeather {
        return Err(UpgradeAttemptCostError::StarterEquipmentNotUpgradeable);
    }
    if tier == EquipmentTier::Gold
        && matches!(
            slot,
            EquipmentSlot::Helmet
                | EquipmentSlot::Chestplate
                | EquipmentSlot::Leggings
                | EquipmentSlot::Boots
        )
    {
        return Err(UpgradeAttemptCostError::UnsupportedGoldSlot(slot));
    }
    Ok(())
}

fn reinforcement_units(target_level: u64) -> u64 {
    match target_level {
        1..=10 => 1,
        11..=15 => 2,
        16..=20 => 3,
        _ => 3 + (target_level - 20) / 5,
    }
}

fn upgrade_money_cost(
    base_appraisal: i64,
    target_level: u64,
) -> Result<i64, UpgradeAttemptCostError> {
    if base_appraisal < 0 {
        return Err(UpgradeAttemptCostError::ArithmeticOverflow);
    }
    if target_level == 0 {
        return Err(UpgradeAttemptCostError::TargetLevelZero);
    }

    if target_level <= 20 {
        return table_upgrade_money_cost(base_appraisal, target_level);
    }
    continuation_upgrade_money_cost(base_appraisal, target_level - 20)
}

fn table_upgrade_money_cost(
    base_appraisal: i64,
    target_level: u64,
) -> Result<i64, UpgradeAttemptCostError> {
    let rate_thousandths: i128 = match target_level {
        1 => 5,
        2 => 6,
        3 => 8,
        4 => 10,
        5 => 13,
        6 => 17,
        7 => 22,
        8 => 28,
        9 => 36,
        10 => 45,
        11 => 60,
        12 => 80,
        13 => 110,
        14 => 160,
        15 => 240,
        16 => 360,
        17 => 550,
        18 => 850,
        19 => 1_300,
        20 => 2_000,
        _ => return Err(UpgradeAttemptCostError::TargetLevelZero),
    };

    let numerator = i128::from(base_appraisal)
        .checked_mul(rate_thousandths)
        .ok_or(UpgradeAttemptCostError::ArithmeticOverflow)?;
    let round100_denominator = TABLE_RATE_DENOMINATOR
        .checked_mul(MONEY_ROUNDING_UNIT)
        .ok_or(UpgradeAttemptCostError::ArithmeticOverflow)?;
    let rounded_units = round_half_up_nonnegative(numerator, round100_denominator)?;
    let money = rounded_units
        .checked_mul(MONEY_ROUNDING_UNIT)
        .ok_or(UpgradeAttemptCostError::ArithmeticOverflow)?;
    i64::try_from(money).map_err(|_| UpgradeAttemptCostError::ArithmeticOverflow)
}

/// Evaluates `round100(base × 2 × 1.35^exponent)` exactly without floating point.
///
/// Since `1.35 = 135 / 100`, the denominator is a pure power of ten. We multiply only decimal
/// digits by 135, then perform the final decimal shift and one half-up rounding step. For every
/// positive integer base appraisal, exponent 144 already rounds above `i64::MAX`; larger exponents
/// therefore fail immediately by monotonicity instead of allocating work proportional to an
/// untrusted target level. A zero appraisal remains exactly zero at every exponent.
fn continuation_upgrade_money_cost(
    base_appraisal: i64,
    exponent: u64,
) -> Result<i64, UpgradeAttemptCostError> {
    if base_appraisal == 0 {
        return Ok(0);
    }
    if exponent > MAX_CONTINUATION_EXPONENT_TO_EVALUATE {
        return Err(UpgradeAttemptCostError::ArithmeticOverflow);
    }

    let doubled = u128::try_from(base_appraisal)
        .map_err(|_| UpgradeAttemptCostError::ArithmeticOverflow)?
        .checked_mul(2)
        .ok_or(UpgradeAttemptCostError::ArithmeticOverflow)?;
    let mut digits = decimal_digits_little_endian(doubled);
    for _ in 0..exponent {
        multiply_decimal_digits(&mut digits, CONTINUATION_DECIMAL_MULTIPLIER);
    }

    let decimal_shift = usize::try_from(
        exponent
            .checked_mul(2)
            .and_then(|value| value.checked_add(2))
            .ok_or(UpgradeAttemptCostError::ArithmeticOverflow)?,
    )
    .map_err(|_| UpgradeAttemptCostError::ArithmeticOverflow)?;

    let mut rounded_units = decimal_integer_part(&digits, decimal_shift)?;
    if decimal_remainder_is_at_least_half(&digits, decimal_shift) {
        rounded_units = rounded_units
            .checked_add(1)
            .ok_or(UpgradeAttemptCostError::ArithmeticOverflow)?;
    }
    let money = rounded_units
        .checked_mul(100)
        .ok_or(UpgradeAttemptCostError::ArithmeticOverflow)?;
    i64::try_from(money).map_err(|_| UpgradeAttemptCostError::ArithmeticOverflow)
}

fn round_half_up_nonnegative(
    numerator: i128,
    denominator: i128,
) -> Result<i128, UpgradeAttemptCostError> {
    if numerator < 0 || denominator <= 0 {
        return Err(UpgradeAttemptCostError::ArithmeticOverflow);
    }
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    let round_up = remainder
        .checked_mul(2)
        .ok_or(UpgradeAttemptCostError::ArithmeticOverflow)?
        >= denominator;
    quotient
        .checked_add(i128::from(round_up))
        .ok_or(UpgradeAttemptCostError::ArithmeticOverflow)
}

fn decimal_digits_little_endian(mut value: u128) -> Vec<u8> {
    if value == 0 {
        return vec![0];
    }
    let mut digits = Vec::new();
    while value != 0 {
        digits.push((value % 10) as u8);
        value /= 10;
    }
    digits
}

fn multiply_decimal_digits(digits: &mut Vec<u8>, multiplier: u32) {
    let mut carry = 0_u32;
    for digit in digits.iter_mut() {
        let value = u32::from(*digit) * multiplier + carry;
        *digit = (value % 10) as u8;
        carry = value / 10;
    }
    while carry != 0 {
        digits.push((carry % 10) as u8);
        carry /= 10;
    }
}

fn decimal_integer_part(
    digits: &[u8],
    decimal_shift: usize,
) -> Result<u128, UpgradeAttemptCostError> {
    if digits.len() <= decimal_shift {
        return Ok(0);
    }

    let mut value = 0_u128;
    for &digit in digits[decimal_shift..].iter().rev() {
        value = value
            .checked_mul(10)
            .and_then(|current| current.checked_add(u128::from(digit)))
            .ok_or(UpgradeAttemptCostError::ArithmeticOverflow)?;
    }
    Ok(value)
}

fn decimal_remainder_is_at_least_half(digits: &[u8], decimal_shift: usize) -> bool {
    decimal_shift != 0
        && digits
            .get(decimal_shift - 1)
            .is_some_and(|digit| *digit >= 5)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::equipment_policy::BaseEquipmentAppraisalSource;

    #[test]
    fn frozen_table_rates_round_to_nearest_hundred_exactly() {
        let expected = [
            500, 600, 800, 1_000, 1_300, 1_700, 2_200, 2_800, 3_600, 4_500, 6_000, 8_000, 11_000,
            16_000, 24_000, 36_000, 55_000, 85_000, 130_000, 200_000,
        ];
        for (target, expected_money) in (1_u64..=20).zip(expected) {
            assert_eq!(
                upgrade_money_cost(100_000, target).unwrap(),
                expected_money,
                "target +{target}"
            );
        }
    }

    #[test]
    fn table_money_rounding_is_half_up_at_hundred_boundary() {
        assert_eq!(upgrade_money_cost(10_000, 1).unwrap(), 100);
        assert_eq!(upgrade_money_cost(9_999, 1).unwrap(), 0);
        assert_eq!(upgrade_money_cost(10_001, 1).unwrap(), 100);
    }

    #[test]
    fn continuation_is_exact_decimal_rational_without_intermediate_rounding() {
        assert_eq!(upgrade_money_cost(100_000, 20).unwrap(), 200_000);
        assert_eq!(upgrade_money_cost(100_000, 21).unwrap(), 270_000);
        assert_eq!(upgrade_money_cost(100_000, 22).unwrap(), 364_500);
        assert_eq!(upgrade_money_cost(100_000, 23).unwrap(), 492_100);
        assert_eq!(upgrade_money_cost(100_000, 25).unwrap(), 896_800);
    }

    #[test]
    fn continuation_half_up_uses_only_the_final_rounding_step() {
        assert_eq!(continuation_upgrade_money_cost(1, 1).unwrap(), 0);
        assert_eq!(continuation_upgrade_money_cost(19, 1).unwrap(), 100);
    }

    #[test]
    fn continuation_runtime_is_bounded_for_untrusted_target_levels() {
        assert!(continuation_upgrade_money_cost(1, 143).is_ok());
        assert_eq!(
            continuation_upgrade_money_cost(1, 144),
            Err(UpgradeAttemptCostError::ArithmeticOverflow)
        );
        assert_eq!(
            continuation_upgrade_money_cost(1, u64::MAX),
            Err(UpgradeAttemptCostError::ArithmeticOverflow)
        );
        assert_eq!(continuation_upgrade_money_cost(0, u64::MAX), Ok(0));
    }

    #[test]
    fn reinforcement_units_match_every_frozen_boundary() {
        let cases = [
            (1, 1),
            (10, 1),
            (11, 2),
            (15, 2),
            (16, 3),
            (20, 3),
            (21, 3),
            (24, 3),
            (25, 4),
            (29, 4),
            (30, 5),
            (u64::MAX, 3 + (u64::MAX - 20) / 5),
        ];
        for (target, expected) in cases {
            assert_eq!(reinforcement_units(target), expected, "target +{target}");
        }
    }

    #[test]
    fn preview_uses_pre_attempt_base_appraisal_and_tier_reinforcement_material() {
        let standard = preview_upgrade_attempt_resource_cost(
            EquipmentTier::Graphite,
            EquipmentSlot::Chestplate,
            None,
            1,
        )
        .unwrap();
        assert_eq!(standard.base_appraisal.value, 10_867_500);
        assert_eq!(
            standard.base_appraisal.source,
            BaseEquipmentAppraisalSource::StandardTable
        );
        assert_eq!(standard.money_cost, 54_300);
        assert_eq!(
            standard.reinforcement_material,
            EquipmentMaterial::GraphiteLayer
        );
        assert_eq!(standard.reinforcement_units, 1);
        assert!(standard.committed_failure_consumes_money_and_material);
        assert!(standard.downgrade_modifiers_do_not_change_base_attempt_cost);

        let special = preview_upgrade_attempt_resource_cost(
            EquipmentTier::Diamond,
            EquipmentSlot::Sword,
            Some(123_456),
            10,
        )
        .unwrap();
        assert_eq!(special.base_appraisal.value, 123_456);
        assert_eq!(
            special.base_appraisal.source,
            BaseEquipmentAppraisalSource::DefinitionOverride
        );
        assert_eq!(special.money_cost, 5_600);
        assert_eq!(special.reinforcement_material, EquipmentMaterial::Diamond);
    }

    #[test]
    fn non_upgradeable_or_nonexistent_targets_fail_closed() {
        assert_eq!(
            preview_upgrade_attempt_resource_cost(
                EquipmentTier::StarterLeather,
                EquipmentSlot::Helmet,
                Some(5_000),
                1,
            ),
            Err(UpgradeAttemptCostError::StarterEquipmentNotUpgradeable)
        );
        assert_eq!(
            preview_upgrade_attempt_resource_cost(
                EquipmentTier::Gold,
                EquipmentSlot::Chestplate,
                None,
                1,
            ),
            Err(UpgradeAttemptCostError::UnsupportedGoldSlot(
                EquipmentSlot::Chestplate
            ))
        );
        assert_eq!(
            preview_upgrade_attempt_resource_cost(
                EquipmentTier::Wood,
                EquipmentSlot::Sword,
                None,
                0,
            ),
            Err(UpgradeAttemptCostError::TargetLevelZero)
        );
    }
}
