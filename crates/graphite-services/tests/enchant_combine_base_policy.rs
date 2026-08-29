use graphite_services::{
    CanonicalEnchant, CombineFailureConsumption, ENCHANT_CATALYST_MULTIPLIER_BPS,
    ENCHANT_COMBINE_ABSOLUTE_SUCCESS_CAP_BPS, ENCHANT_COMBINE_MULTIPLIER_CAP_BPS,
    EnchantCombinePolicyError, EXTRA_AEXP_UI_CAP_MULTIPLIER, ShadowMutationFailurePolicy,
    shadow_walker_mutation_base_policy, standard_enchant_combine_base_policy,
};

#[test]
fn public_api_preserves_the_standard_base_table_and_sunk_cost_contract() {
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
        let policy = standard_enchant_combine_base_policy(level).unwrap();
        assert_eq!(policy.target_level, level);
        assert_eq!(policy.base_success_basis_points, success);
        assert_eq!(policy.money_fee, money);
        assert_eq!(policy.activity_exp_fee, aexp);
        assert_eq!(policy.extra_aexp_ui_cap, aexp * EXTRA_AEXP_UI_CAP_MULTIPLIER);
        assert!(policy.committed_money_and_base_aexp_are_always_consumed);
        assert!(policy.committed_extra_aexp_is_always_consumed);
        assert!(policy.committed_catalyst_is_always_consumed);
    }
}

#[test]
fn public_api_preserves_standard_failure_asset_loss_brackets() {
    assert_eq!(
        standard_enchant_combine_base_policy(2).unwrap().failure_consumption,
        CombineFailureConsumption::NoFailurePath
    );
    assert_eq!(
        standard_enchant_combine_base_policy(5).unwrap().failure_consumption,
        CombineFailureConsumption::DestroyOneUniform
    );
    assert_eq!(
        standard_enchant_combine_base_policy(8).unwrap().failure_consumption,
        CombineFailureConsumption::WeightedOneOrBoth {
            destroy_one_basis_points: 7_000,
            destroy_both_basis_points: 3_000,
        }
    );
    assert_eq!(
        standard_enchant_combine_base_policy(10).unwrap().failure_consumption,
        CombineFailureConsumption::WeightedOneOrBoth {
            destroy_one_basis_points: 4_000,
            destroy_both_basis_points: 6_000,
        }
    );
}

#[test]
fn public_api_preserves_shadow_identity_level_and_level_one_fee_floor() {
    let level_one = shadow_walker_mutation_base_policy(1).unwrap();
    assert_eq!(level_one.first_input, CanonicalEnchant::DayWalker);
    assert_eq!(level_one.second_input, CanonicalEnchant::NightWalker);
    assert_eq!(level_one.output, CanonicalEnchant::ShadowWalker);
    assert_eq!(level_one.output_level, 1);
    assert_eq!(level_one.base_success_basis_points, 2_000);
    assert_eq!(level_one.service_fee_target_level, 2);
    assert_eq!(level_one.money_fee, 2_000);
    assert_eq!(level_one.activity_exp_fee, 100);
    assert_eq!(level_one.extra_aexp_ui_cap, 800);
    assert!(level_one.success_consumes_both_inputs);
    assert_eq!(
        level_one.failure_policy,
        ShadowMutationFailurePolicy::Defined(CombineFailureConsumption::DestroyOneUniform)
    );

    let level_ten = shadow_walker_mutation_base_policy(10).unwrap();
    assert_eq!(level_ten.base_success_basis_points, 6_500);
    assert_eq!(level_ten.output_level, 10);
    assert_eq!(level_ten.service_fee_target_level, 10);
}

#[test]
fn public_api_does_not_invent_shadow_level_two_destructive_failure_semantics() {
    let level_two = shadow_walker_mutation_base_policy(2).unwrap();
    assert_eq!(level_two.base_success_basis_points, 2_500);
    assert_eq!(
        level_two.failure_policy,
        ShadowMutationFailurePolicy::UndefinedForLevelII
    );
}

#[test]
fn public_api_exposes_written_boost_caps_without_claiming_final_exp_evaluation() {
    assert_eq!(ENCHANT_CATALYST_MULTIPLIER_BPS, 13_500);
    assert_eq!(ENCHANT_COMBINE_MULTIPLIER_CAP_BPS, 18_000);
    assert_eq!(ENCHANT_COMBINE_ABSOLUTE_SUCCESS_CAP_BPS, 9_500);
}

#[test]
fn public_api_rejects_levels_outside_the_frozen_tables() {
    assert_eq!(
        standard_enchant_combine_base_policy(1),
        Err(EnchantCombinePolicyError::StandardTargetLevelOutOfRange(1))
    );
    assert_eq!(
        standard_enchant_combine_base_policy(11),
        Err(EnchantCombinePolicyError::StandardTargetLevelOutOfRange(11))
    );
    assert_eq!(
        shadow_walker_mutation_base_policy(0),
        Err(EnchantCombinePolicyError::ShadowMutationLevelOutOfRange(0))
    );
    assert_eq!(
        shadow_walker_mutation_base_policy(11),
        Err(EnchantCombinePolicyError::ShadowMutationLevelOutOfRange(11))
    );
}
