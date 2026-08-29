use serde::Serialize;
use thiserror::Error;

pub const SOULGRIND_MAX_LEVEL: u8 = 10;
pub const SOULGRIND_BASE_PROC_BPS_PER_LEVEL: u16 = 5;
pub const SOULGRIND_BASE_PROC_CAP_BPS: u16 = 50;
pub const SOULGRIND_MAX_SUCCESSES_PER_ITEM_PER_EXPEDITION: u8 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct SoulGrindProbability {
    numerator: u128,
    denominator: u128,
}

impl SoulGrindProbability {
    #[must_use]
    pub const fn numerator(self) -> u128 {
        self.numerator
    }

    #[must_use]
    pub const fn denominator(self) -> u128 {
        self.denominator
    }

    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.numerator == 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct SoulGrindRestorationQuantum {
    numerator: u128,
    denominator: u128,
}

impl SoulGrindRestorationQuantum {
    #[must_use]
    pub const fn numerator(self) -> u128 {
        self.numerator
    }

    #[must_use]
    pub const fn denominator(self) -> u128 {
        self.denominator
    }

    #[must_use]
    pub const fn is_integral(self) -> bool {
        self.denominator == 1
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct SoulGrindProcPreview {
    pub level: u8,
    pub current_durability: i64,
    pub max_durability: i64,
    pub missing_durability: i64,
    pub probability: SoulGrindProbability,
    pub restoration_quantum: SoulGrindRestorationQuantum,
    pub per_item: bool,
    pub max_successful_procs_per_item_per_expedition: u8,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum SoulGrindPolicyError {
    #[error("SoulGrind level must be between I and X; got {0}")]
    LevelOutOfRange(u8),
    #[error("SoulGrind max durability must be positive")]
    NonPositiveMaxDurability,
    #[error("SoulGrind current durability cannot be negative")]
    NegativeCurrentDurability,
    #[error("SoulGrind current durability cannot exceed max durability")]
    CurrentDurabilityExceedsMax,
    #[error("SoulGrind already succeeded for this item in the current expedition")]
    AlreadySucceededThisExpedition,
}

/// Previews SoulGrind for one event that the caller has already classified as a qualifying soul.
///
/// The frozen probability is:
///
/// `min(0.5%, 0.05% × level) × missing_durability_fraction`.
///
/// SoulGrind is per-item and can succeed at most once for that item in one expedition. A successful
/// proc has a frozen restoration quantum of exactly 50% of max durability.
///
/// This policy deliberately does not define what creates a "qualifying soul". The active
/// specification names that semantic event but does not freeze its kill/source qualification here;
/// the owning combat/mining state machine must provide that event authoritatively. It also does not
/// round an odd `max_durability / 2` or clamp the resulting restoration against current durability:
/// the exact half-max quantum is returned as a rational so a future settlement rule cannot silently
/// choose floor or ceiling.
///
/// The specification separately requires `NUKE_BURNOUT` to block any SoulGrind path that could
/// restore the affected Pickaxe during its expedition. This kernel models the canonical Armor
/// per-item SoulGrind effect only and therefore cannot be used as authority for a Pickaxe restoration
/// path. RNG ownership, persisted per-expedition success state, durability mutation, and event/outbox
/// settlement remain with the future owning transaction.
pub fn preview_soulgrind_for_qualifying_soul(
    level: u8,
    current_durability: i64,
    max_durability: i64,
    already_succeeded_this_expedition: bool,
) -> Result<SoulGrindProcPreview, SoulGrindPolicyError> {
    if !(1..=SOULGRIND_MAX_LEVEL).contains(&level) {
        return Err(SoulGrindPolicyError::LevelOutOfRange(level));
    }
    if max_durability <= 0 {
        return Err(SoulGrindPolicyError::NonPositiveMaxDurability);
    }
    if current_durability < 0 {
        return Err(SoulGrindPolicyError::NegativeCurrentDurability);
    }
    if current_durability > max_durability {
        return Err(SoulGrindPolicyError::CurrentDurabilityExceedsMax);
    }
    if already_succeeded_this_expedition {
        return Err(SoulGrindPolicyError::AlreadySucceededThisExpedition);
    }

    let missing_durability = max_durability - current_durability;
    let base_proc_bps = (u16::from(level) * SOULGRIND_BASE_PROC_BPS_PER_LEVEL)
        .min(SOULGRIND_BASE_PROC_CAP_BPS);

    let probability = if missing_durability == 0 {
        SoulGrindProbability {
            numerator: 0,
            denominator: 1,
        }
    } else {
        let numerator = u128::from(base_proc_bps) * (missing_durability as u128);
        let denominator = 10_000_u128 * (max_durability as u128);
        let divisor = gcd_u128(numerator, denominator);
        SoulGrindProbability {
            numerator: numerator / divisor,
            denominator: denominator / divisor,
        }
    };

    let quantum_numerator = max_durability as u128;
    let quantum_divisor = gcd_u128(quantum_numerator, 2);
    let restoration_quantum = SoulGrindRestorationQuantum {
        numerator: quantum_numerator / quantum_divisor,
        denominator: 2 / quantum_divisor,
    };

    Ok(SoulGrindProcPreview {
        level,
        current_durability,
        max_durability,
        missing_durability,
        probability,
        restoration_quantum,
        per_item: true,
        max_successful_procs_per_item_per_expedition:
            SOULGRIND_MAX_SUCCESSES_PER_ITEM_PER_EXPEDITION,
    })
}

fn gcd_u128(mut left: u128, mut right: u128) -> u128 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_probability_scales_with_level_and_missing_fraction() {
        let level_one = preview_soulgrind_for_qualifying_soul(1, 0, 1_000, false).unwrap();
        assert_eq!(level_one.probability.numerator(), 1);
        assert_eq!(level_one.probability.denominator(), 2_000);

        let level_ten_half_missing =
            preview_soulgrind_for_qualifying_soul(10, 500, 1_000, false).unwrap();
        assert_eq!(level_ten_half_missing.probability.numerator(), 1);
        assert_eq!(level_ten_half_missing.probability.denominator(), 400);

        let level_ten_tenth_missing =
            preview_soulgrind_for_qualifying_soul(10, 900, 1_000, false).unwrap();
        assert_eq!(level_ten_tenth_missing.probability.numerator(), 1);
        assert_eq!(level_ten_tenth_missing.probability.denominator(), 2_000);
    }

    #[test]
    fn full_durability_has_zero_proc_probability() {
        let preview = preview_soulgrind_for_qualifying_soul(10, 1_000, 1_000, false).unwrap();
        assert!(preview.probability.is_zero());
        assert_eq!(preview.probability.denominator(), 1);
    }

    #[test]
    fn restoration_quantum_preserves_exact_half_without_rounding() {
        let even = preview_soulgrind_for_qualifying_soul(10, 0, 1_000, false).unwrap();
        assert_eq!(even.restoration_quantum.numerator(), 500);
        assert_eq!(even.restoration_quantum.denominator(), 1);
        assert!(even.restoration_quantum.is_integral());

        let odd = preview_soulgrind_for_qualifying_soul(10, 0, 1_001, false).unwrap();
        assert_eq!(odd.restoration_quantum.numerator(), 1_001);
        assert_eq!(odd.restoration_quantum.denominator(), 2);
        assert!(!odd.restoration_quantum.is_integral());
    }

    #[test]
    fn policy_preserves_per_item_once_per_expedition_semantics() {
        let preview = preview_soulgrind_for_qualifying_soul(7, 100, 1_000, false).unwrap();
        assert!(preview.per_item);
        assert_eq!(preview.max_successful_procs_per_item_per_expedition, 1);
        assert_eq!(
            preview_soulgrind_for_qualifying_soul(7, 100, 1_000, true),
            Err(SoulGrindPolicyError::AlreadySucceededThisExpedition)
        );
    }

    #[test]
    fn invalid_level_and_durability_state_fail_closed() {
        assert_eq!(
            preview_soulgrind_for_qualifying_soul(0, 0, 100, false),
            Err(SoulGrindPolicyError::LevelOutOfRange(0))
        );
        assert_eq!(
            preview_soulgrind_for_qualifying_soul(11, 0, 100, false),
            Err(SoulGrindPolicyError::LevelOutOfRange(11))
        );
        assert_eq!(
            preview_soulgrind_for_qualifying_soul(1, 0, 0, false),
            Err(SoulGrindPolicyError::NonPositiveMaxDurability)
        );
        assert_eq!(
            preview_soulgrind_for_qualifying_soul(1, -1, 100, false),
            Err(SoulGrindPolicyError::NegativeCurrentDurability)
        );
        assert_eq!(
            preview_soulgrind_for_qualifying_soul(1, 101, 100, false),
            Err(SoulGrindPolicyError::CurrentDurabilityExceedsMax)
        );
    }
}
