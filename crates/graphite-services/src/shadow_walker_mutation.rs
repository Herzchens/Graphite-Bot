use serde::Serialize;
use thiserror::Error;

use crate::{
    CanonicalEnchant, EnchantCombineBasePolicyError, EnchantCombineFailureConsumption,
    standard_enchant_combine_base_policy,
};

pub const SHADOW_WALKER_MUTATION_MIN_LEVEL: u8 = 1;
pub const SHADOW_WALKER_MUTATION_MAX_LEVEL: u8 = 10;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ShadowWalkerMutationFailurePolicy {
    Defined(EnchantCombineFailureConsumption),
    /// Shadow Walker II has a genuine failure path at 25% base success, but the source says to reuse
    /// the standard failure table whose target-Level-II row simultaneously says "no failure path".
    /// Only Level I receives an explicit one-book-loss override, so destructive Level-II settlement
    /// must remain unavailable until the specification resolves this contradiction.
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
    /// Standard combine target-level row used for Money/AEXP fee and Extra-AEXP UI-cap metadata.
    /// Level I is explicitly floored to the target-Level-II service row.
    pub service_fee_target_level: u8,
    pub money_fee: i64,
    pub activity_exp_fee: i64,
    pub extra_aexp_ui_cap: i64,
    pub failure_policy: ShadowWalkerMutationFailurePolicy,
    pub success_consumes_both_inputs: bool,
    /// Base Money, base AEXP, committed Extra AEXP, and a committed Catalyst are sunk once a future
    /// owning mutation attempt commits, regardless of success/failure.
    pub committed_attempt_costs_are_sunk: bool,
    /// The source applies the standard Extra-AEXP/Catalyst multiplier and 95% cap to this mutation,
    /// but canonical settlement cannot evaluate that final probability until deterministic `exp`
    /// semantics are frozen. Level II additionally has unresolved destructive failure semantics.
    pub final_success_probability_is_unresolved: bool,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ShadowWalkerMutationPolicyError {
    #[error("Shadow Walker mutation level must be between I and X; got {0}")]
    LevelOutOfRange(u8),
    #[error(transparent)]
    StandardServicePolicy(#[from] EnchantCombineBasePolicyError),
}

/// Returns the exact discrete base contract for Day Walker + Night Walker -> Shadow Walker.
///
/// The two inputs and output preserve the same level. Base mutation success is
/// `min(65%, 15% + 5% * L)`. Level I uses the standard target-Level-II Money/AEXP service row and
/// explicitly uses the III-V one-book-loss failure bracket. Levels III-X reuse their corresponding
/// standard failure-consumption row.
///
/// Level II deliberately returns [`ShadowWalkerMutationFailurePolicy::UndefinedForLevelII`]: it has
/// 25% base success and therefore a real failure path, while the referenced standard target-Level-II
/// row says there is no failure path. Guessing one-book or two-book destruction would invent an asset
/// loss rule. This pure policy also does not evaluate the shared transcendental boost formula, draw
/// RNG, reserve/consume books or fees, mint Shadow Walker, or activate `/enchant`.
pub fn shadow_walker_mutation_base_policy(
    level: u8,
) -> Result<ShadowWalkerMutationBasePolicy, ShadowWalkerMutationPolicyError> {
    if !(SHADOW_WALKER_MUTATION_MIN_LEVEL..=SHADOW_WALKER_MUTATION_MAX_LEVEL).contains(&level) {
        return Err(ShadowWalkerMutationPolicyError::LevelOutOfRange(level));
    }

    let service_fee_target_level = level.max(2);
    let service = standard_enchant_combine_base_policy(service_fee_target_level)?;

    // For L=I..X this is exactly 20%, 25%, ..., 65%; the written min(65%, ...) binds only at X.
    let base_success_basis_points = 1_500_u16 + u16::from(level) * 500;
    debug_assert!(base_success_basis_points <= 6_500);

    let failure_policy = match level {
        1 => ShadowWalkerMutationFailurePolicy::Defined(
            EnchantCombineFailureConsumption::DestroyOneUniform,
        ),
        2 => ShadowWalkerMutationFailurePolicy::UndefinedForLevelII,
        3..=10 => ShadowWalkerMutationFailurePolicy::Defined(service.failure_consumption),
        _ => unreachable!("validated Shadow Walker mutation level"),
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
        committed_attempt_costs_are_sunk: service.committed_attempt_costs_are_sunk,
        final_success_probability_is_unresolved: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_level_and_base_success_are_exact_for_every_supported_level() {
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
    fn level_one_uses_level_two_service_fee_and_explicit_one_book_loss() {
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
    fn level_two_destructive_failure_semantics_remain_explicitly_unresolved() {
        let policy = shadow_walker_mutation_base_policy(2).unwrap();
        assert_eq!(policy.base_success_basis_points, 2_500);
        assert_eq!(policy.service_fee_target_level, 2);
        assert_eq!(
            policy.failure_policy,
            ShadowWalkerMutationFailurePolicy::UndefinedForLevelII
        );
    }

    #[test]
    fn levels_three_through_ten_reuse_standard_failure_brackets_and_fees() {
        for level in 3..=10 {
            let standard = standard_enchant_combine_base_policy(level).unwrap();
            let mutation = shadow_walker_mutation_base_policy(level).unwrap();
            assert_eq!(mutation.service_fee_target_level, level);
            assert_eq!(mutation.money_fee, standard.money_fee);
            assert_eq!(mutation.activity_exp_fee, standard.activity_exp_fee);
            assert_eq!(mutation.extra_aexp_ui_cap, standard.extra_aexp_ui_cap);
            assert_eq!(
                mutation.failure_policy,
                ShadowWalkerMutationFailurePolicy::Defined(standard.failure_consumption)
            );
        }
    }

    #[test]
    fn unsupported_levels_fail_closed() {
        for level in [0, 11, u8::MAX] {
            assert_eq!(
                shadow_walker_mutation_base_policy(level),
                Err(ShadowWalkerMutationPolicyError::LevelOutOfRange(level))
            );
        }
    }
}
