use graphite_services::{
    BlankEnchantBookDisposition, CanonicalEnchant, EnchantRecoveryTerminalOutcome,
    EnchantRemovalMode, EnchantRemovalPolicyError, RecoveredEnchantBook,
    removal_terminal_policy_after_removability_check,
};

#[test]
fn public_api_preserves_plain_removal_terminal_consequences() {
    let policy = removal_terminal_policy_after_removability_check(
        CanonicalEnchant::Sharpness,
        7,
        EnchantRemovalMode::RemoveOnly,
    )
    .unwrap();

    assert!(policy.removes_enchant);
    assert_eq!(
        policy.blank_book_disposition,
        BlankEnchantBookDisposition::NotUsed
    );
    assert_eq!(policy.recovered_book, None);
    assert!(policy.service_fee_is_charged);
    assert!(policy.fee_amount_is_unresolved);
    assert!(!policy.recovery_success_probability_is_unresolved);
}

#[test]
fn public_api_preserves_successful_recovery_identity_and_level() {
    let policy = removal_terminal_policy_after_removability_check(
        CanonicalEnchant::Sharpness,
        9,
        EnchantRemovalMode::RecoverWithBlankBook {
            outcome: EnchantRecoveryTerminalOutcome::Success,
        },
    )
    .unwrap();

    assert!(policy.removes_enchant);
    assert_eq!(
        policy.blank_book_disposition,
        BlankEnchantBookDisposition::ConsumedForRecoveredBook
    );
    assert_eq!(
        policy.recovered_book,
        Some(RecoveredEnchantBook {
            enchant: CanonicalEnchant::Sharpness,
            level: 9,
        })
    );
    assert!(policy.service_fee_is_charged);
    assert!(policy.fee_amount_is_unresolved);
    assert!(policy.recovery_success_probability_is_unresolved);
}

#[test]
fn public_api_preserves_destructive_recovery_failure() {
    let policy = removal_terminal_policy_after_removability_check(
        CanonicalEnchant::Protection,
        4,
        EnchantRemovalMode::RecoverWithBlankBook {
            outcome: EnchantRecoveryTerminalOutcome::Failure,
        },
    )
    .unwrap();

    assert!(policy.removes_enchant);
    assert_eq!(
        policy.blank_book_disposition,
        BlankEnchantBookDisposition::ConsumedWithoutRecoveredBook
    );
    assert_eq!(policy.recovered_book, None);
    assert!(policy.service_fee_is_charged);
    assert!(policy.fee_amount_is_unresolved);
    assert!(policy.recovery_success_probability_is_unresolved);
}

#[test]
fn public_api_reuses_canonical_per_enchant_level_bounds_without_claiming_removability() {
    // These assertions cover only the existing resulting-level domain. This API deliberately runs
    // after a future authoritative removability check and does not classify these enchants itself.
    assert!(
        removal_terminal_policy_after_removability_check(
            CanonicalEnchant::Master,
            2,
            EnchantRemovalMode::RemoveOnly,
        )
        .is_ok()
    );

    for (enchant, level, max_level) in [
        (CanonicalEnchant::Mending, 2, 1),
        (CanonicalEnchant::Phoenix, 2, 1),
        (CanonicalEnchant::Master, 3, 2),
        (CanonicalEnchant::Sharpness, 0, 10),
    ] {
        assert_eq!(
            removal_terminal_policy_after_removability_check(
                enchant,
                level,
                EnchantRemovalMode::RemoveOnly,
            ),
            Err(EnchantRemovalPolicyError::InvalidResultingLevel {
                enchant,
                level,
                max_level,
            })
        );
    }
}
