use crate::mining_depth::MiningDepth;

pub const MINING_NUKE_COLLAPSE_PROBABILITY_NUMERATOR: u8 = 1;
pub const MINING_NUKE_COLLAPSE_PROBABILITY_DENOMINATOR: u8 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MiningNukeCollapsePolicy {
    pub current_depth: MiningDepth,
    pub next_depth: Option<MiningDepth>,
}

impl MiningNukeCollapsePolicy {
    #[must_use]
    pub fn collapse_enabled(self) -> bool {
        self.next_depth.is_some()
    }
}

/// Returns the frozen cave-collapse routing policy for an already-successful Manual Mining Nuke.
///
/// The owning expedition may perform the collapse roll only after the Nuke blast and fall-damage
/// resolution have completed and the player survived. At every non-terminal depth, collapse chance
/// is exactly one half and a successful roll moves the expedition exactly one depth deeper in the
/// same world track. The encounter for that expedition then uses the shifted depth immediately.
///
/// Obsidian Chasm and Abyssal Depths are terminal for their respective world tracks, so collapse is
/// disabled there and no next depth is returned. In particular this policy never crosses from
/// Obsidian Chasm into Nether Fringe.
///
/// The probability constants apply only when collapse_enabled is true; terminal depths do not
/// perform a collapse roll.
///
/// This pure policy does not draw RNG, decide whether the Nuke proc succeeded, resolve survival or
/// fall damage, persist the selected depth, mutate geological pressure, settle rewards, or expose
/// the mine command.
#[must_use]
pub fn mining_nuke_collapse_policy(depth: MiningDepth) -> MiningNukeCollapsePolicy {
    let next_depth = match depth {
        MiningDepth::Surface => Some(MiningDepth::Shallow),
        MiningDepth::Shallow => Some(MiningDepth::UpperCavern),
        MiningDepth::UpperCavern => Some(MiningDepth::DripstoneCavern),
        MiningDepth::DripstoneCavern => Some(MiningDepth::Cavern),
        MiningDepth::Cavern => Some(MiningDepth::LowerCavern),
        MiningDepth::LowerCavern => Some(MiningDepth::DeepCavern),
        MiningDepth::DeepCavern => Some(MiningDepth::CrystalDepths),
        MiningDepth::CrystalDepths => Some(MiningDepth::DiamondShelf),
        MiningDepth::DiamondShelf => Some(MiningDepth::DiamondDepths),
        MiningDepth::DiamondDepths => Some(MiningDepth::ObsidianChasm),
        MiningDepth::ObsidianChasm => None,
        MiningDepth::NetherFringe => Some(MiningDepth::BasaltDepths),
        MiningDepth::BasaltDepths => Some(MiningDepth::NetherDepths),
        MiningDepth::NetherDepths => Some(MiningDepth::AncientDepths),
        MiningDepth::AncientDepths => Some(MiningDepth::AbyssalDepths),
        MiningDepth::AbyssalDepths => None,
    };

    MiningNukeCollapsePolicy {
        current_depth: depth,
        next_depth,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mining_depth::mining_depth_risk_policy;

    #[test]
    fn every_non_terminal_depth_moves_exactly_one_step_in_the_same_world() {
        for (current, expected_next) in [
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
        ] {
            let policy = mining_nuke_collapse_policy(current);
            assert_eq!(policy.next_depth, Some(expected_next));
            assert_eq!(
                mining_depth_risk_policy(current).world,
                mining_depth_risk_policy(expected_next).world
            );
            assert!(policy.collapse_enabled());
        }
    }

    #[test]
    fn deepest_depth_of_each_world_disables_collapse_without_crossing_tracks() {
        for depth in [MiningDepth::ObsidianChasm, MiningDepth::AbyssalDepths] {
            let policy = mining_nuke_collapse_policy(depth);
            assert_eq!(policy.next_depth, None);
            assert!(!policy.collapse_enabled());
        }
    }

    #[test]
    fn enabled_collapse_probability_is_exactly_one_half() {
        assert_eq!(MINING_NUKE_COLLAPSE_PROBABILITY_NUMERATOR, 1);
        assert_eq!(MINING_NUKE_COLLAPSE_PROBABILITY_DENOMINATOR, 2);
    }
}
