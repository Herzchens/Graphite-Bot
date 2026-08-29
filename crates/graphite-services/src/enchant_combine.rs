use serde::Serialize;
use thiserror::Error;

use crate::CanonicalEnchant;

pub const ENCHANT_COMBINE_MIN_TARGET_LEVEL: u8 = 2;
pub const ENCHANT_COMBINE_MAX_TARGET_LEVEL: u8 = 10;
pub const SHADOW_MUTATION_MIN_LEVEL: u8 = 1;
pub const SHADOW_MUTATION_MAX_LEVEL: u8 = 10;
pub const EXTRA_AEXP_UI_CAP_MULTIPLIER: i64 = 8;

/// Exact relative multiplier metadata where 10,000 basis points means 1.00×.
pub const ENCHANT_CATALYST_MULTIPLIER_BPS: u16 = 13_500;
/// Exact relative multiplier cap metadata where 10,000 basis points means 1.00×.
pub const ENCHANT_COMBINE_MULTIPLIER_CAP_BPS: u16 = 18_000;
/// The written absolute success cap in §77.14.
///
/// This slice deliberately does not apply the cap because applying 95% literally to the standard
/// Level-II row would contradict the simultaneously frozen 100% base success / no-failure path.
pub const ENCHANT_COMBINE_ABSOLUTE_SUCCESS_CAP_BPS: u16 = 9_500;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CombineFailureConsumption {
    /// The standard Level-II row explicitly has no failure path at its frozen 100% base success.
    NoFailurePath,
    /// Exactly one input is destroyed, selected uniformly between the two inputs.
    DestroyOneUniform,
    /// A failure first chooses whether one or both inputs are destroyed.
    WeightedOneOrBoth {
        destroy_one_basis_points: u16,
        destroy_both_basis_points: u16,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct StandardEnchantCombineBasePolicy {
    pub target_level: u8,
    pub base_success_basis_points: u16,
    pub money_fee: i64,
    pub activity_exp_fee: i64,
    pub extra_aexp_ui_cap: i64,
    pub failure_consumption: CombineFailureConsumption,
    pub committed_money_and_base_aexp_are_always_consumed: bool,
    pub committed_extra_aexp_is_always_consumed: bool,
    pub committed_catalyst_is_always_consumed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ShadowMutationFailurePolicy {
    Defined(CombineFailureConsumption),
    /// Shadow II has a real failure path because its base mutation chance is 25%, but §77.14 says
    /// to reuse the standard failure table whose Level-II row has "no failure path". The source
    /// special-cases Shadow I, not Shadow II, so guessing one-book or weighted loss would invent a
    /// destructive asset rule.
    UndefinedForLevelII,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ShadowWalkerMutationBasePolicy {
    pub input_level: u8,
    pub first_input: CanonicalEnchant,
    pub second_input: CanonicalEnchant,
    pub output: CanonicalEnchant,
    pub output_level: u8,
    pub base_success_basis_points: u16,
    pub service_fee_target_level: u8,
    pub money_fee: i64,
    pub activity_exp_fee: i64,
    pub extra_aexp_ui_cap: i64,
    pub failure_policy: ShadowMutationFailurePolicy,
    pub success_consumes_both_inputs: bool,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum EnchantCombinePolicyError {
    #[error("standard enchant combine target level must be between II and X; got {0}")]
    StandardTargetLevelOutOfRange(u8),
    #[error("Shadow Walker mutation level must be between I and X; got {0}")]
    ShadowMutationLevelOutOfRange(u8),
    #[error("enchant combine policy arithmetic overflow")]
    ArithmeticOverflow,
}

/// Returns the frozen standard same-enchant/same-level combine base row.
///
/// This policy intentionally stops before Extra-AEXP/Catalyst final-success composition. The source
/// defines that composition using `exp(...)` but does not freeze a deterministic transcendental
/// evaluation/rounding algorithm. It also simultaneously freezes Level II at 100% with no failure
/// path while stating a 95% absolute final-success cap. A future owner must resolve both issues
/// authoritatively before using the boost formula for settlement.
pub fn standard_enchant_combine_base_policy(
    target_level: u8,
) -> Result<StandardEnchantCombineBasePolicy, EnchantCombinePolicyError> {
    let (base_success_basis_points, money_fee, activity_exp_fee, failure_consumption): (
        u16,
        i64,
        i64,
        CombineFailureConsumption,
    ) = match target_level {
        2 => (10_000, 2_000, 100, CombineFailureConsumption::NoFailurePath),
        3 => (9_500, 4_000, 200, CombineFailureConsumption::DestroyOneUniform),
        4 => (9_000, 8_000, 400, CombineFailureConsumption::DestroyOneUniform),
        5 => (8_000, 15_000, 800, CombineFailureConsumption::DestroyOneUniform),
        6 => (
            7_000,
            30_000,
            1_500,
            CombineFailureConsumption::WeightedOneOrBoth {
                destroy_one_basis_points: 7_000,
                destroy_both_basis_points: 3_000,
            },
        ),
        7 => (
            5_500,
            60_000,
            3_000,
            CombineFailureConsumption::WeightedOneOrBoth {
                destroy_one_basis_points: 7_000,
                destroy_both_basis_points: 3_000,
            },
        ),
        8 => (
            4_000,
            120_000,
            6_000,
            CombineFailureConsumption::WeightedOneOrBoth {
                destroy_one_basis_points: 7_000,
                destroy_both_basis_points: 3_000,
            },
        ),
        9 => (
            2_500,
            250_000,
            12_000,
            CombineFailureConsumption::WeightedOneOrBoth {
                destroy_one_basis_points: 4_000,
                destroy_both_basis_points: 6_000,
            },
        ),
        10 => (
            1_200,
            500_000,
            25_000,
            CombineFailureConsumption::WeightedOneOrBoth {
                destroy_one_basis_points: 4_000,
                destroy_both_basis_points: 6_000,
            },
        ),
        other => {
            return Err(EnchantCombinePolicyError::StandardTargetLevelOutOfRange(other));
        }
    };

    let extra_aexp_ui_cap = activity_exp_fee
        .checked_mul(EXTRA_AEXP_UI_CAP_MULTIPLIER)
        .ok_or(EnchantCombinePolicyError::ArithmeticOverflow)?;

    Ok(StandardEnchantCombineBasePolicy {
        target_level,
        base_success_basis_points,
        money_fee,
        activity_exp_fee,
        extra_aexp_ui_cap,
        failure_consumption,
        committed_money_and_base_aexp_are_always_consumed: true,
        committed_extra_aexp_is_always_consumed: true,
        committed_catalyst_is_always_consumed: true,
    })
}

/// Returns the exact unboosted Day Walker + Night Walker -> Shadow Walker mutation policy.
///
/// Input books must be the same level and the output preserves that level. Level I uses the Level-II
/// standard service-fee row as the minimum fee/cap row. The boost formula itself remains deliberately
/// unevaluated for the same deterministic-`exp` reason as standard combining.
pub fn shadow_walker_mutation_base_policy(
    level: u8,
) -> Result<ShadowWalkerMutationBasePolicy, EnchantCombinePolicyError> {
    if !(SHADOW_MUTATION_MIN_LEVEL..=SHADOW_MUTATION_MAX_LEVEL).contains(&level) {
        return Err(EnchantCombinePolicyError::ShadowMutationLevelOutOfRange(level));
    }

    let service_fee_target_level = level.max(ENCHANT_COMBINE_MIN_TARGET_LEVEL);
    let service = standard_enchant_combine_base_policy(service_fee_target_level)?;

    // min(65%, 15% + 5%*L) is exactly 20%, 25%, ..., 65% for L=I..X.
    let base_success_basis_points = 1_500_u16 + u16::from(level) * 500;
    debug_assert!(base_success_basis_points <= 6_500);

    let failure_policy = match level {
        1 => ShadowMutationFailurePolicy::Defined(CombineFailureConsumption::DestroyOneUniform),
        2 => ShadowMutationFailurePolicy::UndefinedForLevelII,
        3..=10 => ShadowMutationFailurePolicy::Defined(service.failure_consumption),
        _ => unreachable!("validated Shadow mutation level"),
    };

    Ok(ShadowWalkerMutationBasePolicy {
        input_level: level,
        first_input: CanonicalEnchant::DayWalker,
        second_input: CanonicalEnchant::NightWalker,
        output: CanonicalEnchant::ShadowWalker,
        output_level: level,
        base_success_basis_points,
        service_fee_target_level,
        money_fee: service.money_fee,
        activity_exp_fee: service.activity_exp_fee,
        extra_aexp_ui_cap: service.extra_aexp_ui_cap,
        failure_policy,
        success_consumes_both_inputs: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_rows_match_the_frozen_table_and_eight_x_aexp_cap() {
        let expected = [
            (2, 10_000, 2_000, 100),
            (3, 9_500, 4_000, 200),
            (4, 9_000, 8_000, 400),
            (5, 8_000, 15_000, 800),
            (6, 7_000, 30_000, 1_500),
            (7, 5_500, 60_000, 3_000),
            (8, 4_000, 120_000, 6_000),
            (9, 2_500, 250_000, 12_000),
            (10, 1_200, 500_000, 25_000),
        ];

        for (level, success, money, aexp) in expected {
            let row = standard_enchant_combine_base_policy(level).unwrap();
            assert_eq!(row.target_level, level);
            assert_eq!(row.base_success_basis_points, success);
            assert_eq!(row.money_fee, money);
            assert_eq!(row.activity_exp_fee, aexp);
            assert_eq!(row.extra_aexp_ui_cap, aexp * 8);
            assert!(row.committed_money_and_base_aexp_are_always_consumed);
            assert!(row.committed_extra_aexp_is_always_consumed);
            assert!(row.committed_catalyst_is_always_consumed);
        }
    }

    #[test]
    fn standard_failure_brackets_match_the_frozen_asset_loss_rules() {
        assert_eq!(
            standard_enchant_combine_base_policy(2).unwrap().failure_consumption,
            CombineFailureConsumption::NoFailurePath
        );
        for level in 3..=5 {
            assert_eq!(
                standard_enchant_combine_base_policy(level)
                    .unwrap()
                    .failure_consumption,
                CombineFailureConsumption::DestroyOneUniform
            );
        }
        for level in 6..=8 {
            assert_eq!(
                standard_enchant_combine_base_policy(level)
                    .unwrap()
                    .failure_consumption,
                CombineFailureConsumption::WeightedOneOrBoth {
                    destroy_one_basis_points: 7_000,
                    destroy_both_basis_points: 3_000,
                }
            );
        }
        for level in 9..=10 {
            assert_eq!(
                standard_enchant_combine_base_policy(level)
                    .unwrap()
                    .failure_consumption,
                CombineFailureConsumption::WeightedOneOrBoth {
                    destroy_one_basis_points: 4_000,
                    destroy_both_basis_points: 6_000,
                }
            );
        }
    }

    #[test]
    fn shadow_base_chance_and_service_fee_floor_are_exact() {
        for level in 1..=10 {
            let policy = shadow_walker_mutation_base_policy(level).unwrap();
            assert_eq!(policy.input_level, level);
            assert_eq!(policy.output_level, level);
            assert_eq!(policy.base_success_basis_points, 1_500 + u16::from(level) * 500);
            assert_eq!(policy.service_fee_target_level, level.max(2));
            assert_eq!(policy.first_input, CanonicalEnchant::DayWalker);
            assert_eq!(policy.second_input, CanonicalEnchant::NightWalker);
            assert_eq!(policy.output, CanonicalEnchant::ShadowWalker);
            assert!(policy.success_consumes_both_inputs);
        }

        let level_one = shadow_walker_mutation_base_policy(1).unwrap();
        assert_eq!(level_one.service_fee_target_level, 2);
        assert_eq!(level_one.money_fee, 2_000);
        assert_eq!(level_one.activity_exp_fee, 100);
        assert_eq!(level_one.extra_aexp_ui_cap, 800);
        assert_eq!(
            level_one.failure_policy,
            ShadowMutationFailurePolicy::Defined(CombineFailureConsumption::DestroyOneUniform)
        );
    }

    #[test]
    fn shadow_level_two_failure_rule_remains_explicitly_unresolved() {
        let policy = shadow_walker_mutation_base_policy(2).unwrap();
        assert_eq!(policy.base_success_basis_points, 2_500);
        assert_eq!(
            policy.failure_policy,
            ShadowMutationFailurePolicy::UndefinedForLevelII
        );
    }

    #[test]
    fn invalid_levels_fail_closed() {
        for level in [0, 1, 11, u8::MAX] {
            assert_eq!(
                standard_enchant_combine_base_policy(level),
                Err(EnchantCombinePolicyError::StandardTargetLevelOutOfRange(level))
            );
        }
        for level in [0, 11, u8::MAX] {
            assert_eq!(
                shadow_walker_mutation_base_policy(level),
                Err(EnchantCombinePolicyError::ShadowMutationLevelOutOfRange(level))
            );
        }
    }

    #[test]
    fn written_boost_metadata_is_exact_but_not_evaluated_here() {
        assert_eq!(ENCHANT_CATALYST_MULTIPLIER_BPS, 13_500);
        assert_eq!(ENCHANT_COMBINE_MULTIPLIER_CAP_BPS, 18_000);
        assert_eq!(ENCHANT_COMBINE_ABSOLUTE_SUCCESS_CAP_BPS, 9_500);
        assert_eq!(EXTRA_AEXP_UI_CAP_MULTIPLIER, 8);
    }
}
