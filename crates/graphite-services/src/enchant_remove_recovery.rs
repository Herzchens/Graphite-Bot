use serde::Serialize;
use thiserror::Error;

use crate::CanonicalEnchant;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EnchantRecoveryTerminalOutcome {
    Success,
    Failure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EnchantRemovalMode {
    RemoveOnly,
    RecoverWithBlankBook {
        outcome: EnchantRecoveryTerminalOutcome,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BlankEnchantBookDisposition {
    NotUsed,
    /// The committed Blank Enchant Book is consumed as the recovery input and one recovered
    /// Enchanted Book is returned. This does not claim ItemInstance identity is preserved.
    ConsumedForRecoveredBook,
    ConsumedWithoutRecoveredBook,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct RecoveredEnchantBook {
    pub enchant: CanonicalEnchant,
    pub level: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct EnchantRemovalTerminalPolicy {
    pub enchant: CanonicalEnchant,
    pub level: u8,
    pub mode: EnchantRemovalMode,
    /// Every successful plain removal and every terminal recovery outcome removes the selected
    /// enchant from equipment. A failed recovery is therefore not a rollback of the removal.
    pub removes_enchant: bool,
    pub blank_book_disposition: BlankEnchantBookDisposition,
    pub recovered_book: Option<RecoveredEnchantBook>,
    /// Remove/recover is a paid NPC service. The fee is charged for the terminal operation even
    /// though the active specification does not freeze the numeric amount.
    pub service_fee_is_charged: bool,
    /// The active specification says remove/recover fees scale with enchant type and level, but does
    /// not freeze an amount/table/formula. A future owning service must not invent a numeric fee.
    pub fee_amount_is_unresolved: bool,
    /// Recovery has success/failure terminal semantics, but no authoritative success probability is
    /// frozen. This policy accepts an already-resolved terminal outcome and does not draw RNG.
    pub recovery_success_probability_is_unresolved: bool,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum EnchantRemovalPolicyError {
    #[error(
        "invalid resulting level {level} for {enchant:?}; supported canonical maximum is {max_level}"
    )]
    InvalidResultingLevel {
        enchant: CanonicalEnchant,
        level: u8,
        max_level: u8,
    },
}

/// Resolves the frozen terminal asset consequences of one Enchant remove/recovery selection after
/// the owning service has already established that the selected embedded enchant is removable.
///
/// The specification allows removing one or more *removable* enchants, but does not publish a
/// complete canonical removability classifier. This function therefore does not claim that every
/// [`CanonicalEnchant`] is removable. It only validates the persisted enchant identity/level domain
/// and freezes the consequences that are explicit once removability has been established elsewhere.
/// This kernel is intentionally per selected enchant; it does not invent how a future multi-remove
/// batch allocates Blank Books, fees, or recovery outcomes across multiple selections.
///
/// Plain removal always removes the selected enchant and uses no Blank Enchant Book. A recovery
/// attempt also removes the enchant on both success and failure. Success consumes the committed
/// Blank Enchant Book as recovery input and returns one Enchanted Book with exactly the same enchant
/// and level; failure consumes the Blank Book and returns no book. Remove/recover is a paid service,
/// but the numeric fee schedule and recovery success chance remain unresolved, so this policy neither
/// prices the service nor performs RNG.
pub fn removal_terminal_policy_after_removability_check(
    enchant: CanonicalEnchant,
    level: u8,
    mode: EnchantRemovalMode,
) -> Result<EnchantRemovalTerminalPolicy, EnchantRemovalPolicyError> {
    let max_level = crate::canonical_enchant_max_resulting_level(enchant);
    if level == 0 || level > max_level {
        return Err(EnchantRemovalPolicyError::InvalidResultingLevel {
            enchant,
            level,
            max_level,
        });
    }

    let (blank_book_disposition, recovered_book, recovery_success_probability_is_unresolved) =
        match mode {
            EnchantRemovalMode::RemoveOnly => (BlankEnchantBookDisposition::NotUsed, None, false),
            EnchantRemovalMode::RecoverWithBlankBook {
                outcome: EnchantRecoveryTerminalOutcome::Success,
            } => (
                BlankEnchantBookDisposition::ConsumedForRecoveredBook,
                Some(RecoveredEnchantBook { enchant, level }),
                true,
            ),
            EnchantRemovalMode::RecoverWithBlankBook {
                outcome: EnchantRecoveryTerminalOutcome::Failure,
            } => (
                BlankEnchantBookDisposition::ConsumedWithoutRecoveredBook,
                None,
                true,
            ),
        };

    Ok(EnchantRemovalTerminalPolicy {
        enchant,
        level,
        mode,
        removes_enchant: true,
        blank_book_disposition,
        recovered_book,
        service_fee_is_charged: true,
        fee_amount_is_unresolved: true,
        recovery_success_probability_is_unresolved,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_removal_never_uses_or_returns_a_book() {
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
    fn recovery_success_removes_enchant_and_returns_same_identity_and_level() {
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
    fn recovery_failure_still_removes_enchant_and_loses_blank_book() {
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
    fn canonical_per_enchant_level_ceiling_is_enforced() {
        assert!(
            removal_terminal_policy_after_removability_check(
                CanonicalEnchant::Mending,
                1,
                EnchantRemovalMode::RemoveOnly,
            )
            .is_ok()
        );
        assert_eq!(
            removal_terminal_policy_after_removability_check(
                CanonicalEnchant::Mending,
                2,
                EnchantRemovalMode::RemoveOnly,
            ),
            Err(EnchantRemovalPolicyError::InvalidResultingLevel {
                enchant: CanonicalEnchant::Mending,
                level: 2,
                max_level: 1,
            })
        );
        assert_eq!(
            removal_terminal_policy_after_removability_check(
                CanonicalEnchant::Sharpness,
                0,
                EnchantRemovalMode::RemoveOnly,
            ),
            Err(EnchantRemovalPolicyError::InvalidResultingLevel {
                enchant: CanonicalEnchant::Sharpness,
                level: 0,
                max_level: 10,
            })
        );
    }
}
