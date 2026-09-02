use graphite_services::{
    CanonicalEnchant, EnchantCombineFailureConsumption, ShadowWalkerMutationFailurePolicy,
    ShadowWalkerMutationPolicyError, shadow_walker_mutation_base_policy,
};

#[test]
fn public_api_preserves_identity_level_and_exact_base_success() {
    for level in 1..=10 {
        let policy = shadow_walker_mutation_base_policy(level).unwrap();
        assert_eq!(policy.input_level, level);
        assert_eq!(policy.first_input, CanonicalEnchant::DayWalker);
        assert_eq!(policy.second_input, CanonicalEnchant::NightWalker);
        assert_eq!(policy.output, CanonicalEnchant::ShadowWalker);
        assert_eq!(policy.output_level, level);
        assert_eq!(
            policy.base_success_basis_points,
            1_500 + u16::from(level) * 500
        );
        assert!(policy.success_consumes_both_inputs);
        assert!(policy.committed_attempt_costs_are_sunk);
        assert!(policy.final_success_probability_is_unresolved);
    }
}

#[test]
fn public_api_preserves_level_one_service_floor_and_failure_override() {
    let policy = shadow_walker_mutation_base_policy(1).unwrap();
    assert_eq!(policy.service_fee_target_level, 2);
    assert_eq!(policy.money_fee, 2_000);
    assert_eq!(policy.activity_exp_fee, 100);
    assert_eq!(policy.extra_aexp_ui_cap, 800);
    assert_eq!(policy.base_success_basis_points, 2_000);
    assert_eq!(
        policy.failure_policy,
        ShadowWalkerMutationFailurePolicy::Defined(
            EnchantCombineFailureConsumption::DestroyOneUniform
        )
    );
}

#[test]
fn public_api_does_not_invent_level_two_asset_loss_semantics() {
    let policy = shadow_walker_mutation_base_policy(2).unwrap();
    assert_eq!(policy.base_success_basis_points, 2_500);
    assert_eq!(
        policy.failure_policy,
        ShadowWalkerMutationFailurePolicy::UndefinedForLevelII
    );
}

#[test]
fn public_api_reuses_standard_failure_brackets_above_level_two() {
    assert_eq!(
        shadow_walker_mutation_base_policy(5)
            .unwrap()
            .failure_policy,
        ShadowWalkerMutationFailurePolicy::Defined(
            EnchantCombineFailureConsumption::DestroyOneUniform
        )
    );
    assert_eq!(
        shadow_walker_mutation_base_policy(8)
            .unwrap()
            .failure_policy,
        ShadowWalkerMutationFailurePolicy::Defined(
            EnchantCombineFailureConsumption::WeightedOneOrBoth {
                destroy_one_basis_points: 7_000,
                destroy_both_basis_points: 3_000,
            }
        )
    );
    assert_eq!(
        shadow_walker_mutation_base_policy(10)
            .unwrap()
            .failure_policy,
        ShadowWalkerMutationFailurePolicy::Defined(
            EnchantCombineFailureConsumption::WeightedOneOrBoth {
                destroy_one_basis_points: 4_000,
                destroy_both_basis_points: 6_000,
            }
        )
    );
}

#[test]
fn public_api_fails_closed_outside_levels_one_through_ten() {
    for level in [0, 11, u8::MAX] {
        assert_eq!(
            shadow_walker_mutation_base_policy(level),
            Err(ShadowWalkerMutationPolicyError::LevelOutOfRange(level))
        );
    }
}
