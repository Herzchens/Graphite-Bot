use graphite_core::CanonicalEnchant;
use graphite_services::{
    MINING_NUKE_CHECKS_PER_MANUAL_MINE, MINING_NUKE_PROC_PPM_PER_LEVEL,
    MINING_SPECIAL_PROC_PROBABILITY_SCALE_PPM, MINING_TRENCH_EXTRA_BLOCKS_ON_PROC,
    MINING_TRENCH_MAX_PROCS_PER_MANUAL_MINE, MINING_TRENCH_PROC_PPM_PER_LEVEL,
    MiningSpecialProcPolicyError, canonical_enchant_max_resulting_level, mining_nuke_proc_policy,
    mining_trench_proc_policy,
};

#[test]
fn public_trench_policy_matches_frozen_level_curve_and_consequence_shape() {
    assert_eq!(MINING_SPECIAL_PROC_PROBABILITY_SCALE_PPM, 1_000_000);
    assert_eq!(MINING_TRENCH_PROC_PPM_PER_LEVEL, 2_000);
    assert_eq!(MINING_TRENCH_MAX_PROCS_PER_MANUAL_MINE, 1);
    assert_eq!(MINING_TRENCH_EXTRA_BLOCKS_ON_PROC, 8);

    let level_one = mining_trench_proc_policy(1).unwrap();
    assert_eq!(level_one.proc_probability_ppm_per_natural_roll, 2_000);

    let level_x = mining_trench_proc_policy(10).unwrap();
    assert_eq!(level_x.proc_probability_ppm_per_natural_roll, 20_000);
    assert_eq!(level_x.max_procs_per_manual_mine, 1);
    assert_eq!(level_x.extra_physical_blocks_on_proc, 8);
    assert!(!level_x.generated_blocks_can_trigger_trench_or_nuke);
    assert!(!level_x.automation_allowed);
}

#[test]
fn public_nuke_policy_represents_sub_basis_point_probability_exactly() {
    assert_eq!(MINING_NUKE_PROC_PPM_PER_LEVEL, 25);
    assert_eq!(MINING_NUKE_CHECKS_PER_MANUAL_MINE, 1);

    let level_one = mining_nuke_proc_policy(1).unwrap();
    assert_eq!(level_one.proc_probability_ppm_per_manual_mine, 25);

    let level_x = mining_nuke_proc_policy(10).unwrap();
    assert_eq!(level_x.proc_probability_ppm_per_manual_mine, 250);
    assert_eq!(level_x.checks_per_manual_mine, 1);
    assert!(!level_x.automation_allowed);
}

#[test]
fn public_policy_reuses_canonical_resulting_level_ceiling_and_rejects_zero() {
    for enchant in [CanonicalEnchant::Trench, CanonicalEnchant::Nuke] {
        assert_eq!(canonical_enchant_max_resulting_level(enchant), 10);
    }

    assert_eq!(
        mining_trench_proc_policy(0),
        Err(MiningSpecialProcPolicyError::InvalidLevel {
            enchant: CanonicalEnchant::Trench,
            level: 0,
            maximum: 10,
        })
    );
    assert_eq!(
        mining_nuke_proc_policy(11),
        Err(MiningSpecialProcPolicyError::InvalidLevel {
            enchant: CanonicalEnchant::Nuke,
            level: 11,
            maximum: 10,
        })
    );
}
