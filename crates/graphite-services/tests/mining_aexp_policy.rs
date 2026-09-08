use graphite_services::{ALL_MINING_DEPTHS, MiningDepth, manual_mining_base_aexp};

#[test]
fn five_ordinary_rolls_reproduce_the_frozen_depth_bonus_rounding() {
    let expected = [3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 6, 6, 6, 6, 7, 7];

    for (depth, expected_aexp) in ALL_MINING_DEPTHS.into_iter().zip(expected) {
        assert_eq!(manual_mining_base_aexp(depth, 5), expected_aexp);
    }
}

#[test]
fn representative_roll_counts_match_the_exact_formula() {
    assert_eq!(manual_mining_base_aexp(MiningDepth::Surface, 1), 2);
    assert_eq!(manual_mining_base_aexp(MiningDepth::Surface, 5), 3);
    assert_eq!(manual_mining_base_aexp(MiningDepth::UpperCavern, 5), 4);
    assert_eq!(manual_mining_base_aexp(MiningDepth::DiamondDepths, 10), 6);
    assert_eq!(manual_mining_base_aexp(MiningDepth::AbyssalDepths, 26), 11);
}

#[test]
fn every_additional_five_ordinary_rolls_adds_exactly_one_base_aexp() {
    for depth in ALL_MINING_DEPTHS {
        for rolls in 0..=128 {
            let before = manual_mining_base_aexp(depth, rolls);
            let after = manual_mining_base_aexp(depth, rolls + 5);
            assert_eq!(after, before + 1, "depth={depth:?}, rolls={rolls}");
        }
    }
}

#[test]
fn zero_count_remains_formula_defined_without_becoming_lifecycle_authority() {
    let expected = [2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 6, 6];

    for (depth, expected_aexp) in ALL_MINING_DEPTHS.into_iter().zip(expected) {
        assert_eq!(manual_mining_base_aexp(depth, 0), expected_aexp);
    }
}

#[test]
fn full_u64_count_domain_stays_inside_progression_i64_output() {
    let value = manual_mining_base_aexp(MiningDepth::AbyssalDepths, u64::MAX);
    assert_eq!(value, 3_689_348_814_741_910_329);
}
