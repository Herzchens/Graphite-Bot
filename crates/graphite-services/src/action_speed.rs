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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ManualMineFishCooldownPolicy {
    pub speed_policy: SharedActionSpeedBonusPolicy,
    pub cooldown_millis: u32,
    pub minimum_clamp_applied: bool,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ActionSpeedPolicyError {
    #[error("action-speed bonus denominator must be positive")]
    ZeroDenominator,
    #[error("action-speed rational composition overflow")]
    ArithmeticOverflow,
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

/// Adds all non-negative action-speed sources in one shared bucket, then applies the shared cap.
///
/// This is the owner-approved combination rule for Gold/Lure/Efficiency/Day/Night/Event/Partner
/// speed sources. Sources do not multiply one another.
pub fn compose_shared_action_speed_bonus(
    sources: &[ActionSpeedBonus],
) -> Result<SharedActionSpeedBonusPolicy, ActionSpeedPolicyError> {
    let mut uncapped = ActionSpeedBonus {
        numerator: 0,
        denominator: 1,
    };
    for source in sources {
        uncapped = add_action_speed_bonus(uncapped, *source)?;
    }
    Ok(cap_shared_action_speed_bonus(uncapped))
}

/// Resolves the canonical manual Mine/Fish cooldown.
///
/// Cooldown is base / (1 + capped_speed_rating), rounded half-up to the nearest millisecond, then
/// clamped to the frozen 7.5-second minimum.
pub fn manual_mine_fish_cooldown_policy(
    sources: &[ActionSpeedBonus],
) -> Result<ManualMineFishCooldownPolicy, ActionSpeedPolicyError> {
    let speed_policy = compose_shared_action_speed_bonus(sources)?;
    let speed = speed_policy.applied_bonus;

    let numerator = u128::from(BASE_REPEATABLE_MANUAL_REWARD_ACTION_COOLDOWN_MILLIS)
        * u128::from(speed.denominator);
    let denominator = u128::from(speed.denominator) + u128::from(speed.numerator);
    let rounded = round_half_up_non_negative(numerator, denominator);
    let rounded = u32::try_from(rounded).map_err(|_| ActionSpeedPolicyError::ArithmeticOverflow)?;
    let minimum_clamp_applied = rounded < MIN_MINE_FISH_COOLDOWN_MILLIS;
    let cooldown_millis = rounded.max(MIN_MINE_FISH_COOLDOWN_MILLIS);

    Ok(ManualMineFishCooldownPolicy {
        speed_policy,
        cooldown_millis,
        minimum_clamp_applied,
    })
}

fn add_action_speed_bonus(
    left: ActionSpeedBonus,
    right: ActionSpeedBonus,
) -> Result<ActionSpeedBonus, ActionSpeedPolicyError> {
    let left_numerator = u128::from(left.numerator);
    let left_denominator = u128::from(left.denominator);
    let right_numerator = u128::from(right.numerator);
    let right_denominator = u128::from(right.denominator);

    let numerator = left_numerator
        .checked_mul(right_denominator)
        .and_then(|value| {
            right_numerator
                .checked_mul(left_denominator)
                .and_then(|right_scaled| value.checked_add(right_scaled))
        })
        .ok_or(ActionSpeedPolicyError::ArithmeticOverflow)?;
    let denominator = left_denominator
        .checked_mul(right_denominator)
        .ok_or(ActionSpeedPolicyError::ArithmeticOverflow)?;

    let divisor = gcd_u128(numerator, denominator);
    let numerator = numerator / divisor;
    let denominator = denominator / divisor;
    let numerator =
        u64::try_from(numerator).map_err(|_| ActionSpeedPolicyError::ArithmeticOverflow)?;
    let denominator =
        u64::try_from(denominator).map_err(|_| ActionSpeedPolicyError::ArithmeticOverflow)?;

    ActionSpeedBonus::new(numerator, denominator)
}

fn round_half_up_non_negative(numerator: u128, denominator: u128) -> u128 {
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    if remainder * 2 >= denominator {
        quotient + 1
    } else {
        quotient
    }
}

const fn ratio_greater_than(left: ActionSpeedBonus, right: ActionSpeedBonus) -> bool {
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

fn gcd_u128(mut left: u128, mut right: u128) -> u128 {
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
    fn additive_bucket_and_half_up_cooldown_are_exact() {
        let policy = manual_mine_fish_cooldown_policy(&[ratio(1, 10), ratio(1, 20)]).unwrap();
        assert_eq!(policy.speed_policy.uncapped_bonus, ratio(3, 20));
        assert_eq!(policy.speed_policy.applied_bonus, ratio(3, 20));
        assert!(!policy.speed_policy.cap_applied);
        assert_eq!(policy.cooldown_millis, 8_696);
        assert!(!policy.minimum_clamp_applied);
    }

    #[test]
    fn shared_cap_yields_the_frozen_minimum_after_half_up_rounding() {
        let policy = manual_mine_fish_cooldown_policy(&[ratio(2, 5)]).unwrap();
        assert!(policy.speed_policy.cap_applied);
        assert_eq!(policy.speed_policy.applied_bonus, ratio(3_333, 10_000));
        assert_eq!(policy.cooldown_millis, 7_500);
    }

    #[test]
    fn empty_bucket_preserves_the_base_cooldown() {
        let policy = manual_mine_fish_cooldown_policy(&[]).unwrap();
        assert_eq!(policy.speed_policy.uncapped_bonus, ratio(0, 1));
        assert_eq!(policy.cooldown_millis, 10_000);
        assert!(!policy.minimum_clamp_applied);
    }

    #[test]
    fn ratios_normalize_and_invalid_denominator_fails_closed() {
        assert_eq!(ActionSpeedBonus::new(10, 100).unwrap(), ratio(1, 10));
        assert_eq!(ActionSpeedBonus::new(0, u64::MAX).unwrap(), ratio(0, 1));
        assert_eq!(
            ActionSpeedBonus::new(1, 0),
            Err(ActionSpeedPolicyError::ZeroDenominator)
        );
    }

    #[test]
    fn precomposed_cap_api_remains_exact() {
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
}
