pub use crate::enchant_placement::EnchantSlotFamily as SlotOrbFamily;
use crate::percentage_fee::checked_ceil_percentage;
use serde::Serialize;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SlotOrbUnlock {
    Normal5,
    Normal6,
    Special4,
    Special5,
    Special6,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct SlotOrbSuccessChance {
    numerator: u32,
    denominator: u32,
}

impl SlotOrbSuccessChance {
    const NORMAL_5: Self = Self {
        numerator: 7,
        denominator: 10,
    };
    const NORMAL_6: Self = Self {
        numerator: 7,
        denominator: 20,
    };
    const SPECIAL_4: Self = Self {
        numerator: 3,
        denominator: 5,
    };
    const SPECIAL_5: Self = Self {
        numerator: 3,
        denominator: 10,
    };
    const SPECIAL_6: Self = Self {
        numerator: 3,
        denominator: 25,
    };

    pub const fn numerator(self) -> u32 {
        self.numerator
    }

    pub const fn denominator(self) -> u32 {
        self.denominator
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SlotOrbFailurePolicy {
    ConsumeOrbAndApplicationFeeKeepItemAndSlotsUnchanged,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct SlotOrbPolicy {
    pub unlock: SlotOrbUnlock,
    pub family: SlotOrbFamily,
    pub target_slot_number: u8,
    pub required_unlocked_slots_before_attempt: u8,
    pub minimum_upgrade_level: u32,
    pub success: SlotOrbSuccessChance,
    pub orb_base_price: i64,
    pub application_fee_percent: u8,
    pub failure_policy: SlotOrbFailurePolicy,
    pub sparkling_affects_success: bool,
    pub mosaic_affects_attempt: bool,
}

pub const fn slot_orb_policy(unlock: SlotOrbUnlock) -> SlotOrbPolicy {
    match unlock {
        SlotOrbUnlock::Normal5 => SlotOrbPolicy {
            unlock,
            family: SlotOrbFamily::NormalClass,
            target_slot_number: 5,
            required_unlocked_slots_before_attempt: 4,
            minimum_upgrade_level: 5,
            success: SlotOrbSuccessChance::NORMAL_5,
            orb_base_price: 100_000,
            application_fee_percent: 2,
            failure_policy:
                SlotOrbFailurePolicy::ConsumeOrbAndApplicationFeeKeepItemAndSlotsUnchanged,
            sparkling_affects_success: false,
            mosaic_affects_attempt: false,
        },
        SlotOrbUnlock::Normal6 => SlotOrbPolicy {
            unlock,
            family: SlotOrbFamily::NormalClass,
            target_slot_number: 6,
            required_unlocked_slots_before_attempt: 5,
            minimum_upgrade_level: 10,
            success: SlotOrbSuccessChance::NORMAL_6,
            orb_base_price: 100_000,
            application_fee_percent: 4,
            failure_policy:
                SlotOrbFailurePolicy::ConsumeOrbAndApplicationFeeKeepItemAndSlotsUnchanged,
            sparkling_affects_success: false,
            mosaic_affects_attempt: false,
        },
        SlotOrbUnlock::Special4 => SlotOrbPolicy {
            unlock,
            family: SlotOrbFamily::SpecialUniversal,
            target_slot_number: 4,
            required_unlocked_slots_before_attempt: 3,
            minimum_upgrade_level: 7,
            success: SlotOrbSuccessChance::SPECIAL_4,
            orb_base_price: 300_000,
            application_fee_percent: 3,
            failure_policy:
                SlotOrbFailurePolicy::ConsumeOrbAndApplicationFeeKeepItemAndSlotsUnchanged,
            sparkling_affects_success: false,
            mosaic_affects_attempt: false,
        },
        SlotOrbUnlock::Special5 => SlotOrbPolicy {
            unlock,
            family: SlotOrbFamily::SpecialUniversal,
            target_slot_number: 5,
            required_unlocked_slots_before_attempt: 4,
            minimum_upgrade_level: 12,
            success: SlotOrbSuccessChance::SPECIAL_5,
            orb_base_price: 300_000,
            application_fee_percent: 6,
            failure_policy:
                SlotOrbFailurePolicy::ConsumeOrbAndApplicationFeeKeepItemAndSlotsUnchanged,
            sparkling_affects_success: false,
            mosaic_affects_attempt: false,
        },
        SlotOrbUnlock::Special6 => SlotOrbPolicy {
            unlock,
            family: SlotOrbFamily::SpecialUniversal,
            target_slot_number: 6,
            required_unlocked_slots_before_attempt: 5,
            minimum_upgrade_level: 15,
            success: SlotOrbSuccessChance::SPECIAL_6,
            orb_base_price: 300_000,
            application_fee_percent: 10,
            failure_policy:
                SlotOrbFailurePolicy::ConsumeOrbAndApplicationFeeKeepItemAndSlotsUnchanged,
            sparkling_affects_success: false,
            mosaic_affects_attempt: false,
        },
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct SlotOrbAttemptPreview {
    pub policy: SlotOrbPolicy,
    pub current_upgrade_level: u32,
    pub current_enhanced_appraisal: i64,
    pub application_fee: i64,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum SlotOrbPolicyError {
    #[error(
        "Slot Orb unlock requires equipment upgrade level +{required} or higher; current level is +{current}"
    )]
    UpgradeLevelTooLow { required: u32, current: u32 },
    #[error("current enhanced canonical appraisal cannot be negative")]
    NegativeEnhancedAppraisal,
    #[error("Slot Orb fee arithmetic exceeded supported integer bounds")]
    ArithmeticOverflow,
}

/// Previews the frozen Slot Orb attempt policy from already-resolved equipment appraisal state.
///
/// +N makes an unlock eligible but never grants the slot for free. The future owning stateful
/// service must still verify that the target slot is currently locked, all predecessor slots in the
/// same family are unlocked, the correct Orb item is owned, and the account/item is mutable before
/// consuming anything. This pure preview does not draw RNG or mutate equipment/assets.
pub fn preview_slot_orb_attempt(
    unlock: SlotOrbUnlock,
    current_upgrade_level: u32,
    current_enhanced_appraisal: i64,
) -> Result<SlotOrbAttemptPreview, SlotOrbPolicyError> {
    let policy = slot_orb_policy(unlock);
    if current_upgrade_level < policy.minimum_upgrade_level {
        return Err(SlotOrbPolicyError::UpgradeLevelTooLow {
            required: policy.minimum_upgrade_level,
            current: current_upgrade_level,
        });
    }
    if current_enhanced_appraisal < 0 {
        return Err(SlotOrbPolicyError::NegativeEnhancedAppraisal);
    }

    let application_fee =
        checked_ceil_percentage(current_enhanced_appraisal, policy.application_fee_percent)
            .ok_or(SlotOrbPolicyError::ArithmeticOverflow)?;

    Ok(SlotOrbAttemptPreview {
        policy,
        current_upgrade_level,
        current_enhanced_appraisal,
        application_fee,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_table_matches_frozen_slot_orb_rows() {
        let cases = [
            (
                SlotOrbUnlock::Normal5,
                SlotOrbFamily::NormalClass,
                5,
                4,
                5,
                7,
                10,
                100_000,
                2,
            ),
            (
                SlotOrbUnlock::Normal6,
                SlotOrbFamily::NormalClass,
                6,
                5,
                10,
                7,
                20,
                100_000,
                4,
            ),
            (
                SlotOrbUnlock::Special4,
                SlotOrbFamily::SpecialUniversal,
                4,
                3,
                7,
                3,
                5,
                300_000,
                3,
            ),
            (
                SlotOrbUnlock::Special5,
                SlotOrbFamily::SpecialUniversal,
                5,
                4,
                12,
                3,
                10,
                300_000,
                6,
            ),
            (
                SlotOrbUnlock::Special6,
                SlotOrbFamily::SpecialUniversal,
                6,
                5,
                15,
                3,
                25,
                300_000,
                10,
            ),
        ];

        for (
            unlock,
            family,
            target_slot,
            prior_slots,
            minimum_upgrade,
            chance_numerator,
            chance_denominator,
            orb_base_price,
            fee_percent,
        ) in cases
        {
            let policy = slot_orb_policy(unlock);
            assert_eq!(policy.family, family);
            assert_eq!(policy.target_slot_number, target_slot);
            assert_eq!(policy.required_unlocked_slots_before_attempt, prior_slots);
            assert_eq!(policy.minimum_upgrade_level, minimum_upgrade);
            assert_eq!(policy.success.numerator(), chance_numerator);
            assert_eq!(policy.success.denominator(), chance_denominator);
            assert_eq!(policy.orb_base_price, orb_base_price);
            assert_eq!(policy.application_fee_percent, fee_percent);
            assert_eq!(
                policy.failure_policy,
                SlotOrbFailurePolicy::ConsumeOrbAndApplicationFeeKeepItemAndSlotsUnchanged
            );
            assert!(!policy.sparkling_affects_success);
            assert!(!policy.mosaic_affects_attempt);
            assert!(policy.target_slot_number <= policy.family.maximum_slot_count());
            assert_eq!(
                policy.required_unlocked_slots_before_attempt,
                policy.target_slot_number - 1
            );
        }
    }

    #[test]
    fn first_orb_unlock_starts_after_each_familys_native_capacity() {
        let normal = slot_orb_policy(SlotOrbUnlock::Normal5);
        assert_eq!(
            normal.required_unlocked_slots_before_attempt,
            normal.family.native_slot_count()
        );

        let special = slot_orb_policy(SlotOrbUnlock::Special4);
        assert_eq!(
            special.required_unlocked_slots_before_attempt,
            special.family.native_slot_count()
        );
    }

    #[test]
    fn upgrade_level_only_unlocks_eligibility_at_or_above_threshold() {
        for unlock in [
            SlotOrbUnlock::Normal5,
            SlotOrbUnlock::Normal6,
            SlotOrbUnlock::Special4,
            SlotOrbUnlock::Special5,
            SlotOrbUnlock::Special6,
        ] {
            let policy = slot_orb_policy(unlock);
            assert_eq!(
                preview_slot_orb_attempt(unlock, policy.minimum_upgrade_level - 1, 1_000),
                Err(SlotOrbPolicyError::UpgradeLevelTooLow {
                    required: policy.minimum_upgrade_level,
                    current: policy.minimum_upgrade_level - 1,
                })
            );
            assert!(preview_slot_orb_attempt(unlock, policy.minimum_upgrade_level, 1_000).is_ok());
            assert!(
                preview_slot_orb_attempt(unlock, policy.minimum_upgrade_level + 100, 1_000).is_ok()
            );
        }
    }

    #[test]
    fn player_paid_application_fee_uses_integer_ceiling() {
        assert_eq!(
            preview_slot_orb_attempt(SlotOrbUnlock::Normal5, 5, 0)
                .unwrap()
                .application_fee,
            0
        );
        assert_eq!(
            preview_slot_orb_attempt(SlotOrbUnlock::Normal5, 5, 1)
                .unwrap()
                .application_fee,
            1
        );
        assert_eq!(
            preview_slot_orb_attempt(SlotOrbUnlock::Normal5, 5, 50)
                .unwrap()
                .application_fee,
            1
        );
        assert_eq!(
            preview_slot_orb_attempt(SlotOrbUnlock::Normal5, 5, 51)
                .unwrap()
                .application_fee,
            2
        );
        assert_eq!(
            preview_slot_orb_attempt(SlotOrbUnlock::Special6, 15, 101)
                .unwrap()
                .application_fee,
            11
        );
    }

    #[test]
    fn fee_math_handles_large_canonical_appraisal_without_float_or_wrap() {
        let preview = preview_slot_orb_attempt(SlotOrbUnlock::Special6, 15, i64::MAX).unwrap();
        assert_eq!(
            preview.application_fee, 922_337_203_685_477_581,
            "ceil(10% of i64::MAX)"
        );
    }

    #[test]
    fn negative_appraisal_fails_closed() {
        assert_eq!(
            preview_slot_orb_attempt(SlotOrbUnlock::Normal5, 5, -1),
            Err(SlotOrbPolicyError::NegativeEnhancedAppraisal)
        );
    }
}
