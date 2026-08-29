use graphite_services::{
    UpgradeOutcomePolicyError, preview_sparkling_upgrade_success,
    preview_stabilize_downgrade_prevention, upgrade_base_outcome_policy,
};

#[test]
fn public_api_preserves_frozen_high_level_upgrade_rows() {
    let ten = upgrade_base_outcome_policy(10).unwrap();
    assert_eq!(
        (ten.success.numerator(), ten.success.denominator()),
        (3, 10)
    );
    assert_eq!(
        (
            ten.downgrade_if_failure.numerator(),
            ten.downgrade_if_failure.denominator()
        ),
        (1, 100)
    );
    assert!(!ten.failure_destroys_equipment);
    assert_eq!(ten.downgrade_levels_on_trigger, 1);
    assert!(ten.success_and_downgrade_parameters_are_independent);
    assert!(ten.protection_orb_resolves_before_stabilize);

    let twenty = upgrade_base_outcome_policy(20).unwrap();
    assert_eq!(
        (twenty.success.numerator(), twenty.success.denominator()),
        (1, 20_000)
    );
    assert_eq!(
        (
            twenty.downgrade_if_failure.numerator(),
            twenty.downgrade_if_failure.denominator()
        ),
        (9, 50)
    );
}

#[test]
fn public_api_applies_relative_sparkling_and_separate_stabilize_components() {
    let sparkling = preview_sparkling_upgrade_success(20, 10).unwrap();
    assert_eq!(sparkling.relative_success_bonus_percent, 50);
    assert_eq!(
        (
            sparkling.adjusted_success.numerator(),
            sparkling.adjusted_success.denominator()
        ),
        (3, 40_000)
    );

    let stabilize = preview_stabilize_downgrade_prevention(10);
    assert_eq!(stabilize.effective_level, 10);
    assert_eq!(
        (
            stabilize.downgrade_prevention.numerator(),
            stabilize.downgrade_prevention.denominator()
        ),
        (7, 10)
    );
    assert!(stabilize.loses_one_level_only_when_prevention_triggers);
}

#[test]
fn public_api_fails_closed_beyond_probability_table_without_claiming_a_level_cap() {
    assert_eq!(
        upgrade_base_outcome_policy(21),
        Err(UpgradeOutcomePolicyError::ProbabilityTableUndefined {
            target_level: 21,
            max_frozen: 20,
        })
    );
    assert_eq!(
        preview_sparkling_upgrade_success(50, 10),
        Err(UpgradeOutcomePolicyError::ProbabilityTableUndefined {
            target_level: 50,
            max_frozen: 20,
        })
    );
}
