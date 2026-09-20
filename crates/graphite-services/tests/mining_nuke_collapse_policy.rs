use graphite_services::{
    MINING_NUKE_COLLAPSE_PROBABILITY_DENOMINATOR, MINING_NUKE_COLLAPSE_PROBABILITY_NUMERATOR,
    MiningDepth, mining_depth_risk_policy, mining_nuke_collapse_policy,
};

#[test]
fn public_policy_keeps_collapse_inside_each_world_track() {
    let transitions = [
        (MiningDepth::Surface, MiningDepth::Shallow),
        (MiningDepth::Shallow, MiningDepth::UpperCavern),
        (MiningDepth::UpperCavern, MiningDepth::DripstoneCavern),
        (MiningDepth::DripstoneCavern, MiningDepth::Cavern),
        (MiningDepth::Cavern, MiningDepth::LowerCavern),
        (MiningDepth::LowerCavern, MiningDepth::DeepCavern),
        (MiningDepth::DeepCavern, MiningDepth::CrystalDepths),
        (MiningDepth::CrystalDepths, MiningDepth::DiamondShelf),
        (MiningDepth::DiamondShelf, MiningDepth::DiamondDepths),
        (MiningDepth::DiamondDepths, MiningDepth::ObsidianChasm),
        (MiningDepth::NetherFringe, MiningDepth::BasaltDepths),
        (MiningDepth::BasaltDepths, MiningDepth::NetherDepths),
        (MiningDepth::NetherDepths, MiningDepth::AncientDepths),
        (MiningDepth::AncientDepths, MiningDepth::AbyssalDepths),
    ];

    for (current, expected_next) in transitions {
        let policy = mining_nuke_collapse_policy(current);
        assert_eq!(policy.current_depth, current);
        assert_eq!(policy.next_depth, Some(expected_next));
        assert!(policy.collapse_enabled());
        assert_eq!(
            mining_depth_risk_policy(current).world,
            mining_depth_risk_policy(expected_next).world
        );
    }
}

#[test]
fn public_policy_disables_shift_at_each_world_terminal() {
    for terminal in [MiningDepth::ObsidianChasm, MiningDepth::AbyssalDepths] {
        let policy = mining_nuke_collapse_policy(terminal);
        assert_eq!(policy.current_depth, terminal);
        assert_eq!(policy.next_depth, None);
        assert!(!policy.collapse_enabled());
    }
}

#[test]
fn enabled_collapse_probability_is_exactly_one_half() {
    assert_eq!(MINING_NUKE_COLLAPSE_PROBABILITY_NUMERATOR, 1);
    assert_eq!(MINING_NUKE_COLLAPSE_PROBABILITY_DENOMINATOR, 2);
}
