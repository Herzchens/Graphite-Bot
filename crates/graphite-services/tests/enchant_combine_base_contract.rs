use graphite_services::{
    ENCHANT_COMBINE_ABSOLUTE_SUCCESS_CAP_BPS, ENCHANT_COMBINE_CATALYST_MULTIPLIER_BPS,
    ENCHANT_COMBINE_EXTRA_AEXP_UI_CAP_MULTIPLIER, ENCHANT_COMBINE_MULTIPLIER_CAP_BPS,
    EnchantCombineBasePolicyError, EnchantCombineFailureConsumption,
    standard_enchant_combine_base_policy,
};

#[test]
fn public_api_preserves_all_standard_combine_base_rows() {
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

    for (target_level, success, money, aexp) in expected {
        let row = standard_enchant_combine_base_policy(target_level).unwrap();
        assert_eq!(row.target_level, target_level);
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
fn public_api_preserves_failure_asset_loss_brackets() {
    assert_eq!(
        standard_enchant_combine_base_policy(2)
            .unwrap()
            .failure_consumption,
        EnchantCombineFailureConsumption::NoFailurePath
    );
    assert_eq!(
        standard_enchant_combine_base_policy(5)
            .unwrap()
            .failure_consumption,
        EnchantCombineFailureConsumption::DestroyOneUniform
    );
    assert_eq!(
        standard_enchant_combine_base_policy(8)
            .unwrap()
            .failure_consumption,
        EnchantCombineFailureConsumption::WeightedOneOrBoth {
            destroy_one_basis_points: 7_000,
            destroy_both_basis_points: 3_000,
        }
    );
    assert_eq!(
        standard_enchant_combine_base_policy(10)
            .unwrap()
            .failure_consumption,
        EnchantCombineFailureConsumption::WeightedOneOrBoth {
            destroy_one_basis_points: 4_000,
            destroy_both_basis_points: 6_000,
        }
    );
}

#[test]
fn public_api_exposes_written_boost_bounds_without_claiming_final_probability() {
    assert_eq!(ENCHANT_COMBINE_CATALYST_MULTIPLIER_BPS, 13_500);
    assert_eq!(ENCHANT_COMBINE_MULTIPLIER_CAP_BPS, 18_000);
    assert_eq!(ENCHANT_COMBINE_ABSOLUTE_SUCCESS_CAP_BPS, 9_500);
}

#[test]
fn public_api_fails_closed_outside_the_frozen_target_table() {
    for level in [0, 1, 11, u8::MAX] {
        assert_eq!(
            standard_enchant_combine_base_policy(level),
            Err(EnchantCombineBasePolicyError::TargetLevelOutOfRange(level))
        );
    }
}
