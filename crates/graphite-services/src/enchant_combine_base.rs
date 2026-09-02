use serde::Serialize;
use thiserror::Error;

/// Lowest target level present in the frozen standard Enchant combine table.
pub const ENCHANT_COMBINE_MIN_TARGET_LEVEL: u8 = 2;
/// Highest target level present in the frozen standard Enchant combine table.
pub const ENCHANT_COMBINE_MAX_TARGET_LEVEL: u8 = 10;
/// The UI may accept at most `8 × base AEXP fee` as Extra AEXP for one committed combine attempt.
pub const ENCHANT_COMBINE_EXTRA_AEXP_UI_CAP_MULTIPLIER: i64 = 8;

/// Written relative Enchant Catalyst multiplier, where 10,000 basis points means 1.00×.
pub const ENCHANT_COMBINE_CATALYST_MULTIPLIER_BPS: u16 = 13_500;
/// Written combined Extra-AEXP/Catalyst multiplier cap, where 10,000 basis points means 1.00×.
pub const ENCHANT_COMBINE_MULTIPLIER_CAP_BPS: u16 = 18_000;
/// Written absolute final-success cap from §77.14.
///
/// This base-table slice deliberately does not apply this cap. The active specification also freezes
/// standard target Level II at 100% base success with "no failure path", so a settlement owner must
/// not silently turn that row into 95% merely by applying the written cap literally.
pub const ENCHANT_COMBINE_ABSOLUTE_SUCCESS_CAP_BPS: u16 = 9_500;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EnchantCombineFailureConsumption {
    /// The frozen target-Level-II row explicitly has no failure path.
    NoFailurePath,
    /// Exactly one of the two input books is destroyed, selected uniformly at random.
    DestroyOneUniform,
    /// On failure, a second draw chooses whether one or both input books are destroyed.
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
    pub failure_consumption: EnchantCombineFailureConsumption,
    /// Base Money, base AEXP, committed Extra AEXP, and a committed Catalyst are all sunk once the
    /// future owning attempt commits, irrespective of success/failure.
    pub committed_attempt_costs_are_sunk: bool,
    /// The authoritative final success probability remains intentionally unresolved in this slice.
    ///
    /// Levels III–X require deterministic fixed-point exponential semantics before Extra AEXP can be
    /// settlement-authoritative. Level II additionally requires the source-level 100%/no-failure vs
    /// universal 95%-cap tension to be resolved rather than silently picking one rule.
    pub final_success_probability_is_unresolved: bool,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum EnchantCombineBasePolicyError {
    #[error("standard Enchant combine target level must be between II and X; got {0}")]
    TargetLevelOutOfRange(u8),
    #[error("Enchant combine base-policy arithmetic exceeded the supported integer range")]
    ArithmeticOverflow,
}

/// Returns one exact row from the frozen standard same-enchant/same-level combine contract.
///
/// The base table, failure-consumption brackets, sunk-cost rule, Catalyst multiplier metadata,
/// multiplier cap, absolute-success cap, and `8F` Extra-AEXP UI ceiling are all frozen. This function
/// intentionally stops before calculating `FinalSuccess`.
///
/// The active specification writes
/// `AEXPBoost = 1 + 0.30 × (1 - exp(-ExtraAEXP / (2 × max(F, 1))))`, but Graphite's computational
/// contract requires canonical probability accounting to use deterministic integer/fixed-point
/// arithmetic. No authoritative fixed-point precision, exponential approximation/table, or rounding
/// rule is currently frozen. In addition, applying the written 95% absolute cap literally would
/// conflict with target Level II's simultaneously frozen 100%/no-failure row. A future settlement
/// owner must resolve those two boundaries before drawing final combine success.
pub fn standard_enchant_combine_base_policy(
    target_level: u8,
) -> Result<StandardEnchantCombineBasePolicy, EnchantCombineBasePolicyError> {
    let (base_success_basis_points, money_fee, activity_exp_fee, failure_consumption): (
        u16,
        i64,
        i64,
        EnchantCombineFailureConsumption,
    ) = match target_level {
        2 => (
            10_000,
            2_000,
            100,
            EnchantCombineFailureConsumption::NoFailurePath,
        ),
        3 => (
            9_500,
            4_000,
            200,
            EnchantCombineFailureConsumption::DestroyOneUniform,
        ),
        4 => (
            9_000,
            8_000,
            400,
            EnchantCombineFailureConsumption::DestroyOneUniform,
        ),
        5 => (
            8_000,
            15_000,
            800,
            EnchantCombineFailureConsumption::DestroyOneUniform,
        ),
        6 => (
            7_000,
            30_000,
            1_500,
            EnchantCombineFailureConsumption::WeightedOneOrBoth {
                destroy_one_basis_points: 7_000,
                destroy_both_basis_points: 3_000,
            },
        ),
        7 => (
            5_500,
            60_000,
            3_000,
            EnchantCombineFailureConsumption::WeightedOneOrBoth {
                destroy_one_basis_points: 7_000,
                destroy_both_basis_points: 3_000,
            },
        ),
        8 => (
            4_000,
            120_000,
            6_000,
            EnchantCombineFailureConsumption::WeightedOneOrBoth {
                destroy_one_basis_points: 7_000,
                destroy_both_basis_points: 3_000,
            },
        ),
        9 => (
            2_500,
            250_000,
            12_000,
            EnchantCombineFailureConsumption::WeightedOneOrBoth {
                destroy_one_basis_points: 4_000,
                destroy_both_basis_points: 6_000,
            },
        ),
        10 => (
            1_200,
            500_000,
            25_000,
            EnchantCombineFailureConsumption::WeightedOneOrBoth {
                destroy_one_basis_points: 4_000,
                destroy_both_basis_points: 6_000,
            },
        ),
        other => return Err(EnchantCombineBasePolicyError::TargetLevelOutOfRange(other)),
    };

    let extra_aexp_ui_cap = activity_exp_fee
        .checked_mul(ENCHANT_COMBINE_EXTRA_AEXP_UI_CAP_MULTIPLIER)
        .ok_or(EnchantCombineBasePolicyError::ArithmeticOverflow)?;

    Ok(StandardEnchantCombineBasePolicy {
        target_level,
        base_success_basis_points,
        money_fee,
        activity_exp_fee,
        extra_aexp_ui_cap,
        failure_consumption,
        committed_attempt_costs_are_sunk: true,
        final_success_probability_is_unresolved: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_frozen_base_row_is_exact() {
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
            assert_eq!(
                row.extra_aexp_ui_cap,
                aexp * ENCHANT_COMBINE_EXTRA_AEXP_UI_CAP_MULTIPLIER
            );
            assert!(row.committed_attempt_costs_are_sunk);
            assert!(row.final_success_probability_is_unresolved);
        }
    }

    #[test]
    fn failure_consumption_brackets_are_exact() {
        assert_eq!(
            standard_enchant_combine_base_policy(2)
                .unwrap()
                .failure_consumption,
            EnchantCombineFailureConsumption::NoFailurePath
        );
        for level in 3..=5 {
            assert_eq!(
                standard_enchant_combine_base_policy(level)
                    .unwrap()
                    .failure_consumption,
                EnchantCombineFailureConsumption::DestroyOneUniform
            );
        }
        for level in 6..=8 {
            assert_eq!(
                standard_enchant_combine_base_policy(level)
                    .unwrap()
                    .failure_consumption,
                EnchantCombineFailureConsumption::WeightedOneOrBoth {
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
                EnchantCombineFailureConsumption::WeightedOneOrBoth {
                    destroy_one_basis_points: 4_000,
                    destroy_both_basis_points: 6_000,
                }
            );
        }
    }

    #[test]
    fn written_boost_metadata_is_exposed_without_claiming_final_probability() {
        assert_eq!(ENCHANT_COMBINE_CATALYST_MULTIPLIER_BPS, 13_500);
        assert_eq!(ENCHANT_COMBINE_MULTIPLIER_CAP_BPS, 18_000);
        assert_eq!(ENCHANT_COMBINE_ABSOLUTE_SUCCESS_CAP_BPS, 9_500);
        assert_eq!(ENCHANT_COMBINE_EXTRA_AEXP_UI_CAP_MULTIPLIER, 8);
    }

    #[test]
    fn levels_outside_the_frozen_table_fail_closed() {
        for level in [0, 1, 11, u8::MAX] {
            assert_eq!(
                standard_enchant_combine_base_policy(level),
                Err(EnchantCombineBasePolicyError::TargetLevelOutOfRange(level))
            );
        }
    }
}
