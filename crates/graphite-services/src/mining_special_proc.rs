use graphite_core::CanonicalEnchant;
use thiserror::Error;

use crate::canonical_enchant_max_resulting_level;

/// Exact probability scale for Manual-Mining special proc policies.
///
/// One unit is one part per million of probability, so `1_000_000 = 100%`. This scale represents
/// Nuke's frozen `0.0025% × level` rule exactly without floating-point gameplay authority.
pub const MINING_SPECIAL_PROC_PROBABILITY_SCALE_PPM: u32 = 1_000_000;
pub const MINING_TRENCH_PROC_PPM_PER_LEVEL: u32 = 2_000;
pub const MINING_NUKE_PROC_PPM_PER_LEVEL: u32 = 25;
pub const MINING_TRENCH_MAX_PROCS_PER_MANUAL_MINE: u8 = 1;
pub const MINING_TRENCH_EXTRA_BLOCKS_ON_PROC: u8 = 8;
pub const MINING_NUKE_CHECKS_PER_MANUAL_MINE: u8 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MiningTrenchProcPolicy {
    pub level: u8,
    pub proc_probability_ppm_per_natural_roll: u32,
    pub max_procs_per_manual_mine: u8,
    pub extra_physical_blocks_on_proc: u8,
    pub generated_blocks_can_trigger_trench_or_nuke: bool,
    pub automation_allowed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MiningNukeProcPolicy {
    pub level: u8,
    pub proc_probability_ppm_per_manual_mine: u32,
    pub checks_per_manual_mine: u8,
    pub automation_allowed: bool,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum MiningSpecialProcPolicyError {
    #[error("Mining enchant {enchant:?} level {level} is outside the frozen 1..={maximum} domain")]
    InvalidLevel {
        enchant: CanonicalEnchant,
        level: u8,
        maximum: u8,
    },
}

/// Returns the frozen Manual-Mining Trench proc policy for an already-authoritative embedded level.
///
/// Trench checks once per natural roll at `0.2% × level`, can proc at most once in a manual
/// `/mine`, and adds exactly eight physical blocks on success (the original block plus eight extras
/// forms the 3×3-style result). Generated Trench blocks do not recursively trigger Trench or Nuke.
///
/// Authoritative enchant state must already satisfy the existing Trench↔Nuke same-item conflict;
/// this policy does not duplicate that validation.
///
/// This policy deliberately does not define the natural-roll count or draw order, perform RNG,
/// apply Pickaxe wear, aggregate geological pressure, choose resource identities for the extra
/// blocks, settle inventory/AEXP, or expose `/mine`.
pub fn mining_trench_proc_policy(
    level: u8,
) -> Result<MiningTrenchProcPolicy, MiningSpecialProcPolicyError> {
    validate_level(CanonicalEnchant::Trench, level)?;
    Ok(MiningTrenchProcPolicy {
        level,
        proc_probability_ppm_per_natural_roll: u32::from(level) * MINING_TRENCH_PROC_PPM_PER_LEVEL,
        max_procs_per_manual_mine: MINING_TRENCH_MAX_PROCS_PER_MANUAL_MINE,
        extra_physical_blocks_on_proc: MINING_TRENCH_EXTRA_BLOCKS_ON_PROC,
        generated_blocks_can_trigger_trench_or_nuke: false,
        automation_allowed: false,
    })
}

/// Returns the frozen Manual-Mining Nuke proc probability for an already-authoritative embedded
/// level.
///
/// Nuke receives exactly one proc check per manual `/mine` at `0.0025% × level`; Level X is
/// therefore exactly `0.025%`. Nuke is disabled in Automation.
///
/// Authoritative enchant state must already satisfy the existing Trench↔Nuke same-item conflict;
/// this policy does not duplicate that validation.
///
/// This policy deliberately does not draw RNG or duplicate Nuke consequence owners. Blast size,
/// Pickaxe destruction and Burnout remain owned by the Pickaxe-durability policy; post-survival cave
/// collapse remains owned by the Nuke-collapse policy. This function also does not resolve Nuke
/// versus ordinary one-point wear ordering, fall damage, pressure mutation, settlement, or command
/// activation.
pub fn mining_nuke_proc_policy(
    level: u8,
) -> Result<MiningNukeProcPolicy, MiningSpecialProcPolicyError> {
    validate_level(CanonicalEnchant::Nuke, level)?;
    Ok(MiningNukeProcPolicy {
        level,
        proc_probability_ppm_per_manual_mine: u32::from(level) * MINING_NUKE_PROC_PPM_PER_LEVEL,
        checks_per_manual_mine: MINING_NUKE_CHECKS_PER_MANUAL_MINE,
        automation_allowed: false,
    })
}

fn validate_level(
    enchant: CanonicalEnchant,
    level: u8,
) -> Result<(), MiningSpecialProcPolicyError> {
    let maximum = canonical_enchant_max_resulting_level(enchant);
    if level == 0 || level > maximum {
        return Err(MiningSpecialProcPolicyError::InvalidLevel {
            enchant,
            level,
            maximum,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trench_probability_scales_exactly_through_level_x() {
        for level in 1..=10 {
            let policy = mining_trench_proc_policy(level).unwrap();
            assert_eq!(policy.level, level);
            assert_eq!(
                policy.proc_probability_ppm_per_natural_roll,
                u32::from(level) * 2_000
            );
            assert_eq!(policy.max_procs_per_manual_mine, 1);
            assert_eq!(policy.extra_physical_blocks_on_proc, 8);
            assert!(!policy.generated_blocks_can_trigger_trench_or_nuke);
            assert!(!policy.automation_allowed);
        }
        assert_eq!(
            mining_trench_proc_policy(10)
                .unwrap()
                .proc_probability_ppm_per_natural_roll,
            20_000
        );
    }

    #[test]
    fn nuke_probability_scales_exactly_through_level_x() {
        for level in 1..=10 {
            let policy = mining_nuke_proc_policy(level).unwrap();
            assert_eq!(policy.level, level);
            assert_eq!(
                policy.proc_probability_ppm_per_manual_mine,
                u32::from(level) * 25
            );
            assert_eq!(policy.checks_per_manual_mine, 1);
            assert!(!policy.automation_allowed);
        }
        assert_eq!(
            mining_nuke_proc_policy(10)
                .unwrap()
                .proc_probability_ppm_per_manual_mine,
            250
        );
    }

    #[test]
    fn invalid_levels_fail_closed_against_canonical_enchant_ceiling() {
        for (enchant, policy_zero, policy_above) in [
            (
                CanonicalEnchant::Trench,
                mining_trench_proc_policy(0).map(|_| ()),
                mining_trench_proc_policy(11).map(|_| ()),
            ),
            (
                CanonicalEnchant::Nuke,
                mining_nuke_proc_policy(0).map(|_| ()),
                mining_nuke_proc_policy(11).map(|_| ()),
            ),
        ] {
            let maximum = canonical_enchant_max_resulting_level(enchant);
            assert_eq!(
                policy_zero,
                Err(MiningSpecialProcPolicyError::InvalidLevel {
                    enchant,
                    level: 0,
                    maximum,
                })
            );
            assert_eq!(
                policy_above,
                Err(MiningSpecialProcPolicyError::InvalidLevel {
                    enchant,
                    level: 11,
                    maximum,
                })
            );
        }
    }
}
