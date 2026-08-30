use serde::Serialize;
use thiserror::Error;

/// Frozen base cooldown for a repeatable manual reward action, in milliseconds.
pub const BASE_REPEATABLE_MANUAL_REWARD_ACTION_COOLDOWN_MILLIS: u32 = 10_000;

/// Frozen minimum cooldown for manual Mine/Fish after speed modifiers, in milliseconds.
pub const MIN_MINE_FISH_COOLDOWN_MILLIS: u32 = 7_500;

/// Exact numerator of the frozen +33.33% shared action-speed bonus cap.
pub const MAX_SHARED_ACTION_SPEED_BONUS_NUMERATOR: u64 = 3_333;

/// Exact denominator of the frozen +33.33% shared action-speed bonus cap.
pub const MAX_SHARED_ACTION_SPEED_BONUS_DENOMINATOR: u64 = 10_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ActionSpeedBonus {
    numerator: u64,
    denominator: u64,
}

impl ActionSpeedBonus {
    /// Creates an exact non-negative action-speed bonus ratio and reduces it to canonical form.
    ///
    /// For example, `10/100` is stored as `1/10`. No fixed decimal/basis-point precision is chosen
    /// by this type; the future Modifier Registry may therefore preserve whatever exact rational
    /// precision its authoritative combination rule requires.
    pub const fn new(numerator: u64, denominator: u64) -> Result<Self, ActionSpeedPolicyError> {
        if denominator == 0 {
            return Err(ActionSpeedPolicyError::ZeroDenominator);
        }
        if numerator == 0 {
            return Ok(Self {
                numerator: 0,
                denominator: 1,
            });
        }

        let divisor = gcd(numerator, denominator);
        Ok(Self {
            numerator: numerator / divisor,
            denominator: denominator / divisor,
        })
    }

    #[must_use]
    pub const fn numerator(self) -> u64 {
        self.numerator
    }

    #[must_use]
    pub const fn denominator(self) -> u64 {
        self.denominator
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct SharedActionSpeedBonusPolicy {
    pub uncapped_bonus: ActionSpeedBonus,
    pub applied_bonus: ActionSpeedBonus,
    pub cap_applied: bool,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ActionSpeedPolicyError {
    #[error("action-speed bonus denominator must be positive")]
    ZeroDenominator,
}

/// Returns the exact frozen maximum shared action-speed bonus: +33.33% = 3,333 / 10,000.
#[must_use]
pub const fn max_shared_action_speed_bonus() -> ActionSpeedBonus {
    ActionSpeedBonus {
        numerator: MAX_SHARED_ACTION_SPEED_BONUS_NUMERATOR,
        denominator: MAX_SHARED_ACTION_SPEED_BONUS_DENOMINATOR,
    }
}

/// Applies the canonical shared action-speed cap to an already-resolved exact bucket total.
///
/// The active specification freezes a +33.33% maximum shared action-speed bonus and states that
/// action-speed sources share one capped bucket instead of multiplying separately. The Modifier
/// Registry contract, however, gives modifiers an explicit `Combination Rule`, and the current
/// specification does not freeze the rule that combines Gold/Lure/Efficiency/Day/Night/Event/Partner
/// source values into the uncapped bucket total. Therefore this function deliberately accepts an
/// authoritative already-composed total instead of inventing additive stacking.
///
/// The specification also requires deterministic integer/fixed-point canonical arithmetic but does
/// not freeze a one-basis-point precision for Modifier Registry values. The bucket total is therefore
/// accepted as an exact reduced rational instead of being quantized to basis points. The frozen cap
/// itself remains exactly `3333/10000`.
///
/// This policy does **not** convert the capped rating into a cooldown duration. The active
/// specification freezes the 10.0-second base cooldown and the 7.5-second Mine/Fish floor, but does
/// not freeze a rating-to-duration conversion formula. Callers must not infer one here.
#[must_use]
pub const fn cap_shared_action_speed_bonus(
    uncapped_bonus: ActionSpeedBonus,
) -> SharedActionSpeedBonusPolicy {
    let cap = max_shared_action_speed_bonus();
    let cap_applied = ratio_greater_than(uncapped_bonus, cap);
    let applied_bonus = if cap_applied { cap } else { uncapped_bonus };

    SharedActionSpeedBonusPolicy {
        uncapped_bonus,
        applied_bonus,
        cap_applied,
    }
}

const fn ratio_greater_than(left: ActionSpeedBonus, right: ActionSpeedBonus) -> bool {
    // A u64 × u64 product always fits in u128, so comparison is exact and overflow-free.
    (left.numerator as u128) * (right.denominator as u128)
        > (right.numerator as u128) * (left.denominator as u128)
}

const fn gcd(mut left: u64, mut right: u64) -> u64 {
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

    const fn ratio(numerator: u64, denominator: u64) -> ActionSpeedBonus {
        match ActionSpeedBonus::new(numerator, denominator) {
            Ok(value) => value,
            Err(_) => panic!("test ratio denominator must be positive"),
        }
    }

    #[test]
    fn frozen_timing_and_cap_constants_are_exact() {
        assert_eq!(BASE_REPEATABLE_MANUAL_REWARD_ACTION_COOLDOWN_MILLIS, 10_000);
        assert_eq!(MIN_MINE_FISH_COOLDOWN_MILLIS, 7_500);
        assert_eq!(MAX_SHARED_ACTION_SPEED_BONUS_NUMERATOR, 3_333);
        assert_eq!(MAX_SHARED_ACTION_SPEED_BONUS_DENOMINATOR, 10_000);
        assert_eq!(max_shared_action_speed_bonus(), ratio(3_333, 10_000));
    }

    #[test]
    fn ratios_normalize_without_selecting_decimal_precision() {
        assert_eq!(ActionSpeedBonus::new(10, 100).unwrap(), ratio(1, 10));
        assert_eq!(ActionSpeedBonus::new(15, 100).unwrap(), ratio(3, 20));
        assert_eq!(ActionSpeedBonus::new(0, u64::MAX).unwrap(), ratio(0, 1));
        assert_eq!(
            ActionSpeedBonus::new(1, 0),
            Err(ActionSpeedPolicyError::ZeroDenominator)
        );
    }

    #[test]
    fn bucket_total_below_the_cap_is_preserved_exactly() {
        let uncapped = ratio(33_329, 100_000);
        assert_eq!(
            cap_shared_action_speed_bonus(uncapped),
            SharedActionSpeedBonusPolicy {
                uncapped_bonus: uncapped,
                applied_bonus: uncapped,
                cap_applied: false,
            }
        );
    }

    #[test]
    fn bucket_total_above_the_cap_is_clamped_exactly() {
        let uncapped = ratio(1, 3);
        assert_eq!(
            cap_shared_action_speed_bonus(uncapped),
            SharedActionSpeedBonusPolicy {
                uncapped_bonus: uncapped,
                applied_bonus: ratio(3_333, 10_000),
                cap_applied: true,
            }
        );
    }

    #[test]
    fn exact_cap_and_zero_do_not_report_clamping() {
        let cap = ratio(3_333, 10_000);
        assert_eq!(
            cap_shared_action_speed_bonus(cap),
            SharedActionSpeedBonusPolicy {
                uncapped_bonus: cap,
                applied_bonus: cap,
                cap_applied: false,
            }
        );

        let zero = ratio(0, 1);
        assert_eq!(
            cap_shared_action_speed_bonus(zero),
            SharedActionSpeedBonusPolicy {
                uncapped_bonus: zero,
                applied_bonus: zero,
                cap_applied: false,
            }
        );
    }

    #[test]
    fn arbitrarily_large_resolved_ratios_are_safely_capped() {
        let uncapped = ratio(u64::MAX, 1);
        let policy = cap_shared_action_speed_bonus(uncapped);
        assert_eq!(policy.uncapped_bonus, uncapped);
        assert_eq!(policy.applied_bonus, max_shared_action_speed_bonus());
        assert!(policy.cap_applied);
    }
}
