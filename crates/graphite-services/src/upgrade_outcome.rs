use serde::Serialize;
use thiserror::Error;

const MAX_FROZEN_TARGET_LEVEL: u64 = 20;
const MAX_EFFECTIVE_SPECIAL_ENCHANT_LEVEL: u8 = 10;
const SPARKLING_RELATIVE_SUCCESS_PERCENT_PER_LEVEL: u8 = 5;
const STABILIZE_PREVENTION_PERCENT_PER_LEVEL: u8 = 7;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct UpgradeProbability {
    numerator: u128,
    denominator: u128,
}

impl UpgradeProbability {
    pub const fn numerator(self) -> u128 {
        self.numerator
    }

    pub const fn denominator(self) -> u128 {
        self.denominator
    }

    pub const fn is_zero(self) -> bool {
        self.numerator == 0
    }

    pub const fn is_guaranteed(self) -> bool {
        self.numerator == self.denominator
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct UpgradeBaseOutcomePolicy {
    pub target_level: u64,
    pub success: UpgradeProbability,
    /// Conditional probability of a one-level downgrade after the attempt has already failed.
    pub downgrade_if_failure: UpgradeProbability,
    pub failure_destroys_equipment: bool,
    pub downgrade_levels_on_trigger: u8,
    pub success_and_downgrade_parameters_are_independent: bool,
    pub protection_orb_resolves_before_stabilize: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct UpgradeSparklingPreview {
    pub target_level: u64,
    pub enchant_level: u64,
    pub effective_level: u8,
    pub relative_success_bonus_percent: u8,
    pub base_success: UpgradeProbability,
    pub adjusted_success: UpgradeProbability,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct UpgradeStabilizePreview {
    pub enchant_level: u64,
    pub effective_level: u8,
    pub downgrade_prevention: UpgradeProbability,
    pub loses_one_level_only_when_prevention_triggers: bool,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum UpgradeOutcomePolicyError {
    #[error("+N target level must be at least +1")]
    TargetLevelZero,
    #[error(
        "the frozen +N success/downgrade probability table ends at +{max_frozen}; target +{target_level} remains conceptually valid but has no authoritative probability row"
    )]
    ProbabilityTableUndefined { target_level: u64, max_frozen: u64 },
    #[error("+N outcome probability arithmetic exceeded supported integer bounds")]
    ArithmeticOverflow,
}

/// Returns the frozen base success and conditional downgrade-on-failure policy for target +1..+20.
///
/// The active specification defines unlimited conceptual +N progression but freezes numeric
/// success/downgrade rows only through +20. Therefore +21 and above fail closed here rather than
/// inheriting +20, extrapolating a curve, or becoming an accidental gameplay hard cap.
///
/// `downgrade_if_failure` is conditional on the attempt already failing. The success and downgrade
/// parameters are separately frozen inputs; generic failure never destroys the equipment, and a
/// triggered downgrade removes exactly one +N level. Protection Orb resolves before Stabilize, but
/// its numeric prevention effect is not frozen by the active source and is intentionally absent from
/// this policy.
pub fn upgrade_base_outcome_policy(
    target_level: u64,
) -> Result<UpgradeBaseOutcomePolicy, UpgradeOutcomePolicyError> {
    if target_level == 0 {
        return Err(UpgradeOutcomePolicyError::TargetLevelZero);
    }
    if target_level > MAX_FROZEN_TARGET_LEVEL {
        return Err(UpgradeOutcomePolicyError::ProbabilityTableUndefined {
            target_level,
            max_frozen: MAX_FROZEN_TARGET_LEVEL,
        });
    }

    let (success, downgrade_if_failure) = match target_level {
        1 => (probability(1, 1), probability(0, 1)),
        2 => (probability(19, 20), probability(0, 1)),
        3 => (probability(9, 10), probability(0, 1)),
        4 => (probability(17, 20), probability(0, 1)),
        5 => (probability(4, 5), probability(0, 1)),
        6 => (probability(7, 10), probability(0, 1)),
        7 => (probability(3, 5), probability(0, 1)),
        8 => (probability(1, 2), probability(0, 1)),
        9 => (probability(2, 5), probability(0, 1)),
        10 => (probability(3, 10), probability(1, 100)),
        11 => (probability(1, 5), probability(3, 200)),
        12 => (probability(3, 25), probability(1, 50)),
        13 => (probability(7, 100), probability(3, 100)),
        14 => (probability(7, 200), probability(1, 25)),
        15 => (probability(1, 100), probability(1, 20)),
        16 => (probability(1, 500), probability(7, 100)),
        17 => (probability(1, 2_000), probability(1, 10)),
        18 => (probability(1, 5_000), probability(3, 25)),
        19 => (probability(1, 10_000), probability(3, 20)),
        20 => (probability(1, 20_000), probability(9, 50)),
        _ => unreachable!("target level was range-checked"),
    };

    Ok(UpgradeBaseOutcomePolicy {
        target_level,
        success,
        downgrade_if_failure,
        failure_destroys_equipment: false,
        downgrade_levels_on_trigger: 1,
        success_and_downgrade_parameters_are_independent: true,
        protection_orb_resolves_before_stabilize: true,
    })
}

/// Applies only Sparkling's frozen relative success modifier to a frozen +N target row.
///
/// Sparkling grants +5% **relative** success per effective level, capped at +50% relative. A caller
/// value above X is accepted only to demonstrate effect saturation; this function does not authorize
/// persistence of an enchant above its separate canonical level cap. The result is saturated at 1/1
/// because a probability cannot exceed 100%.
pub fn preview_sparkling_upgrade_success(
    target_level: u64,
    sparkling_level: u64,
) -> Result<UpgradeSparklingPreview, UpgradeOutcomePolicyError> {
    let base = upgrade_base_outcome_policy(target_level)?;
    let effective_level = capped_effective_level(sparkling_level);
    let relative_success_bonus_percent =
        effective_level * SPARKLING_RELATIVE_SUCCESS_PERCENT_PER_LEVEL;
    let adjusted_success = apply_relative_bonus(base.success, relative_success_bonus_percent)?;

    Ok(UpgradeSparklingPreview {
        target_level,
        enchant_level: sparkling_level,
        effective_level,
        relative_success_bonus_percent,
        base_success: base.success,
        adjusted_success,
    })
}

/// Returns Stabilize's frozen downgrade-prevention component independently of Protection Orb.
///
/// Stabilize prevents downgrade with 7% chance per effective level, capped at 70%. It loses one of
/// its own enchant levels only when that prevention actually triggers. Protection Orb resolves first;
/// because its numeric prevention effect is unresolved, this function deliberately does not compose
/// a final post-Orb downgrade probability.
pub fn preview_stabilize_downgrade_prevention(stabilize_level: u64) -> UpgradeStabilizePreview {
    let effective_level = capped_effective_level(stabilize_level);
    let prevention_percent = effective_level * STABILIZE_PREVENTION_PERCENT_PER_LEVEL;

    UpgradeStabilizePreview {
        enchant_level: stabilize_level,
        effective_level,
        downgrade_prevention: probability(u128::from(prevention_percent), 100),
        loses_one_level_only_when_prevention_triggers: true,
    }
}

const fn capped_effective_level(level: u64) -> u8 {
    if level >= MAX_EFFECTIVE_SPECIAL_ENCHANT_LEVEL as u64 {
        MAX_EFFECTIVE_SPECIAL_ENCHANT_LEVEL
    } else {
        level as u8
    }
}

fn apply_relative_bonus(
    base: UpgradeProbability,
    relative_bonus_percent: u8,
) -> Result<UpgradeProbability, UpgradeOutcomePolicyError> {
    let multiplier = 100_u128
        .checked_add(u128::from(relative_bonus_percent))
        .ok_or(UpgradeOutcomePolicyError::ArithmeticOverflow)?;
    let numerator = base
        .numerator
        .checked_mul(multiplier)
        .ok_or(UpgradeOutcomePolicyError::ArithmeticOverflow)?;
    let denominator = base
        .denominator
        .checked_mul(100)
        .ok_or(UpgradeOutcomePolicyError::ArithmeticOverflow)?;

    if numerator >= denominator {
        return Ok(probability(1, 1));
    }
    Ok(probability(numerator, denominator))
}

fn probability(numerator: u128, denominator: u128) -> UpgradeProbability {
    debug_assert!(denominator > 0);
    debug_assert!(numerator <= denominator);
    if numerator == 0 {
        return UpgradeProbability {
            numerator: 0,
            denominator: 1,
        };
    }
    let divisor = gcd(numerator, denominator);
    UpgradeProbability {
        numerator: numerator / divisor,
        denominator: denominator / divisor,
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

    #[test]
    fn frozen_target_table_matches_all_twenty_rows_exactly() {
        let expected = [
            (1, (1, 1), (0, 1)),
            (2, (19, 20), (0, 1)),
            (3, (9, 10), (0, 1)),
            (4, (17, 20), (0, 1)),
            (5, (4, 5), (0, 1)),
            (6, (7, 10), (0, 1)),
            (7, (3, 5), (0, 1)),
            (8, (1, 2), (0, 1)),
            (9, (2, 5), (0, 1)),
            (10, (3, 10), (1, 100)),
            (11, (1, 5), (3, 200)),
            (12, (3, 25), (1, 50)),
            (13, (7, 100), (3, 100)),
            (14, (7, 200), (1, 25)),
            (15, (1, 100), (1, 20)),
            (16, (1, 500), (7, 100)),
            (17, (1, 2_000), (1, 10)),
            (18, (1, 5_000), (3, 25)),
            (19, (1, 10_000), (3, 20)),
            (20, (1, 20_000), (9, 50)),
        ];

        for (target, success, downgrade) in expected {
            let policy = upgrade_base_outcome_policy(target).unwrap();
            assert_eq!(
                (policy.success.numerator(), policy.success.denominator()),
                success,
                "target +{target} success"
            );
            assert_eq!(
                (
                    policy.downgrade_if_failure.numerator(),
                    policy.downgrade_if_failure.denominator()
                ),
                downgrade,
                "target +{target} downgrade"
            );
            assert!(!policy.failure_destroys_equipment);
            assert_eq!(policy.downgrade_levels_on_trigger, 1);
            assert!(policy.success_and_downgrade_parameters_are_independent);
            assert!(policy.protection_orb_resolves_before_stabilize);
        }
    }

    #[test]
    fn probability_table_boundary_fails_closed_without_becoming_a_gameplay_cap() {
        assert_eq!(
            upgrade_base_outcome_policy(0),
            Err(UpgradeOutcomePolicyError::TargetLevelZero)
        );
        for target_level in [21, 50, u64::MAX] {
            assert_eq!(
                upgrade_base_outcome_policy(target_level),
                Err(UpgradeOutcomePolicyError::ProbabilityTableUndefined {
                    target_level,
                    max_frozen: 20,
                })
            );
        }
    }

    #[test]
    fn sparkling_is_relative_exact_and_capped_at_fifty_percent_bonus() {
        let none = preview_sparkling_upgrade_success(2, 0).unwrap();
        assert_eq!(none.adjusted_success, probability(19, 20));
        assert_eq!(none.relative_success_bonus_percent, 0);

        let level_one = preview_sparkling_upgrade_success(2, 1).unwrap();
        assert_eq!(level_one.adjusted_success, probability(399, 400));
        assert_eq!(level_one.relative_success_bonus_percent, 5);

        let level_ten = preview_sparkling_upgrade_success(20, 10).unwrap();
        assert_eq!(level_ten.adjusted_success, probability(3, 40_000));
        assert_eq!(level_ten.relative_success_bonus_percent, 50);

        let above_cap = preview_sparkling_upgrade_success(20, 999).unwrap();
        assert_eq!(above_cap.effective_level, 10);
        assert_eq!(above_cap.adjusted_success, level_ten.adjusted_success);
    }

    #[test]
    fn sparkling_never_turns_a_probability_above_one() {
        let guaranteed = preview_sparkling_upgrade_success(1, 10).unwrap();
        assert!(guaranteed.adjusted_success.is_guaranteed());
        assert_eq!(guaranteed.adjusted_success, probability(1, 1));
    }

    #[test]
    fn stabilize_is_seven_percent_per_level_capped_at_seventy_percent() {
        let none = preview_stabilize_downgrade_prevention(0);
        assert!(none.downgrade_prevention.is_zero());
        assert!(none.loses_one_level_only_when_prevention_triggers);

        let one = preview_stabilize_downgrade_prevention(1);
        assert_eq!(one.downgrade_prevention, probability(7, 100));

        let ten = preview_stabilize_downgrade_prevention(10);
        assert_eq!(ten.downgrade_prevention, probability(7, 10));

        let above_cap = preview_stabilize_downgrade_prevention(u64::MAX);
        assert_eq!(above_cap.effective_level, 10);
        assert_eq!(above_cap.downgrade_prevention, ten.downgrade_prevention);
    }

    #[test]
    fn upgrade_twenty_retains_tiny_success_and_nonzero_downgrade_risk() {
        let policy = upgrade_base_outcome_policy(20).unwrap();
        assert_eq!(policy.success, probability(1, 20_000));
        assert_eq!(policy.downgrade_if_failure, probability(9, 50));
        assert!(!policy.success.is_zero());
        assert!(!policy.downgrade_if_failure.is_zero());
    }
}
