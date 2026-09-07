/// Number of canonical Mining depth bands in the current specification.
pub const MINING_DEPTH_COUNT: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MiningWorld {
    Overworld,
    Nether,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MiningDepth {
    Surface,
    Shallow,
    UpperCavern,
    DripstoneCavern,
    Cavern,
    LowerCavern,
    DeepCavern,
    CrystalDepths,
    DiamondShelf,
    DiamondDepths,
    ObsidianChasm,
    NetherFringe,
    BasaltDepths,
    NetherDepths,
    AncientDepths,
    AbyssalDepths,
}

impl MiningDepth {
    /// Returns the frozen current-v1 depth index used by formulas that explicitly name
    /// `Surface = 0, Shallow = 1, …, Abyssal = 15`.
    #[must_use]
    pub const fn index(self) -> u8 {
        match self {
            Self::Surface => 0,
            Self::Shallow => 1,
            Self::UpperCavern => 2,
            Self::DripstoneCavern => 3,
            Self::Cavern => 4,
            Self::LowerCavern => 5,
            Self::DeepCavern => 6,
            Self::CrystalDepths => 7,
            Self::DiamondShelf => 8,
            Self::DiamondDepths => 9,
            Self::ObsidianChasm => 10,
            Self::NetherFringe => 11,
            Self::BasaltDepths => 12,
            Self::NetherDepths => 13,
            Self::AncientDepths => 14,
            Self::AbyssalDepths => 15,
        }
    }
}

/// All canonical Mining depth bands in explicit gameplay order.
///
/// Callers must not infer this order from enum discriminants. [`MiningDepth::index`] is the
/// authoritative current-v1 depth index used by policies that explicitly depend on it.
pub const ALL_MINING_DEPTHS: [MiningDepth; MINING_DEPTH_COUNT] = [
    MiningDepth::Surface,
    MiningDepth::Shallow,
    MiningDepth::UpperCavern,
    MiningDepth::DripstoneCavern,
    MiningDepth::Cavern,
    MiningDepth::LowerCavern,
    MiningDepth::DeepCavern,
    MiningDepth::CrystalDepths,
    MiningDepth::DiamondShelf,
    MiningDepth::DiamondDepths,
    MiningDepth::ObsidianChasm,
    MiningDepth::NetherFringe,
    MiningDepth::BasaltDepths,
    MiningDepth::NetherDepths,
    MiningDepth::AncientDepths,
    MiningDepth::AbyssalDepths,
];

const WORLD_BY_DEPTH: [MiningWorld; MINING_DEPTH_COUNT] = [
    MiningWorld::Overworld,
    MiningWorld::Overworld,
    MiningWorld::Overworld,
    MiningWorld::Overworld,
    MiningWorld::Overworld,
    MiningWorld::Overworld,
    MiningWorld::Overworld,
    MiningWorld::Overworld,
    MiningWorld::Overworld,
    MiningWorld::Overworld,
    MiningWorld::Overworld,
    MiningWorld::Nether,
    MiningWorld::Nether,
    MiningWorld::Nether,
    MiningWorld::Nether,
    MiningWorld::Nether,
];
const FIRST_EVENT_CHANCE_BPS_BY_DEPTH: [u16; MINING_DEPTH_COUNT] = [
    0, 50, 100, 130, 180, 230, 280, 400, 480, 550, 700, 850, 920, 1_000, 1_200, 1_400,
];
const SECOND_EVENT_CHANCE_GIVEN_FIRST_BPS_BY_DEPTH: [u16; MINING_DEPTH_COUNT] = [
    0, 0, 100, 150, 200, 300, 400, 600, 750, 900, 1_200, 1_500, 1_650, 1_800, 2_200, 2_500,
];
const PACK_TWO_PLUS_CHANCE_GIVEN_EVENT_BPS_BY_DEPTH: [u16; MINING_DEPTH_COUNT] = [
    0, 300, 500, 600, 800, 1_000, 1_200, 1_600, 1_900, 2_100, 2_700, 3_300, 3_600, 3_900, 4_600,
    5_400,
];
const STANDARD_HP_ANCHOR_BY_DEPTH: [Option<u16>; MINING_DEPTH_COUNT] = [
    None,
    Some(6),
    Some(8),
    Some(9),
    Some(11),
    Some(13),
    Some(15),
    Some(20),
    Some(23),
    Some(27),
    Some(36),
    Some(42),
    Some(50),
    Some(60),
    Some(80),
    Some(105),
];
const BASE_HIT_BY_DEPTH: [Option<u16>; MINING_DEPTH_COUNT] = [
    None,
    Some(1),
    Some(1),
    Some(1),
    Some(2),
    Some(2),
    Some(2),
    Some(3),
    Some(3),
    Some(4),
    Some(5),
    Some(6),
    Some(6),
    Some(7),
    Some(9),
    Some(11),
];
const SEAM_CAPACITY_BY_DEPTH: [u32; MINING_DEPTH_COUNT] = [
    3_800, 5_200, 7_800, 8_800, 10_500, 11_000, 11_500, 16_000, 16_800, 17_500, 22_500, 27_500,
    28_500, 30_000, 35_000, 41_000,
];

/// Exact non-RNG inputs from the canonical Mining depth/risk table.
///
/// Probabilities are basis points (`10_000 = 100%`) so this policy never introduces floating-point
/// probability authority. `second_event_chance_given_first_bps` is conditional on Event #1 having
/// occurred and the player still being in the expedition. `pack_two_plus_chance_given_event_bps` is
/// conditional on an encounter event existing. The policy records no draw order or RNG mapping.
///
/// The specification's `Recommended` column is deliberately not represented as an eligibility gate,
/// and the balance-diagnostic `Ore EV/roll` column is deliberately omitted from this gameplay input
/// policy. Depth access, Pickaxe capability, ore selection, depletion mutation, encounters and `/mine`
/// remain separate owners.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MiningDepthRiskPolicy {
    pub depth: MiningDepth,
    pub depth_index: u8,
    pub world: MiningWorld,
    pub first_event_chance_bps: u16,
    pub second_event_chance_given_first_bps: u16,
    pub pack_two_plus_chance_given_event_bps: u16,
    pub standard_hp_anchor: Option<u16>,
    pub base_hit: Option<u16>,
    pub seam_capacity: u32,
}

/// Returns the exact current-v1 depth/risk row without performing RNG, access checks or mutation.
#[must_use]
pub const fn mining_depth_risk_policy(depth: MiningDepth) -> MiningDepthRiskPolicy {
    let depth_index = depth.index();
    let index = depth_index as usize;

    MiningDepthRiskPolicy {
        depth,
        depth_index,
        world: WORLD_BY_DEPTH[index],
        first_event_chance_bps: FIRST_EVENT_CHANCE_BPS_BY_DEPTH[index],
        second_event_chance_given_first_bps: SECOND_EVENT_CHANCE_GIVEN_FIRST_BPS_BY_DEPTH[index],
        pack_two_plus_chance_given_event_bps: PACK_TWO_PLUS_CHANCE_GIVEN_EVENT_BPS_BY_DEPTH[index],
        standard_hp_anchor: STANDARD_HP_ANCHOR_BY_DEPTH[index],
        base_hit: BASE_HIT_BY_DEPTH[index],
        seam_capacity: SEAM_CAPACITY_BY_DEPTH[index],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_indices_are_explicit_and_cover_zero_through_fifteen() {
        for (expected_index, depth) in ALL_MINING_DEPTHS.into_iter().enumerate() {
            assert_eq!(usize::from(depth.index()), expected_index);
            assert_eq!(mining_depth_risk_policy(depth).depth_index, depth.index());
        }
    }

    #[test]
    fn every_depth_risk_axis_matches_the_frozen_table() {
        let policies = ALL_MINING_DEPTHS.map(mining_depth_risk_policy);

        assert_eq!(policies.map(|policy| policy.world), WORLD_BY_DEPTH);
        assert_eq!(
            policies.map(|policy| policy.first_event_chance_bps),
            [
                0, 50, 100, 130, 180, 230, 280, 400, 480, 550, 700, 850, 920, 1_000, 1_200, 1_400,
            ]
        );
        assert_eq!(
            policies.map(|policy| policy.second_event_chance_given_first_bps),
            [
                0, 0, 100, 150, 200, 300, 400, 600, 750, 900, 1_200, 1_500, 1_650, 1_800, 2_200,
                2_500,
            ]
        );
        assert_eq!(
            policies.map(|policy| policy.pack_two_plus_chance_given_event_bps),
            [
                0, 300, 500, 600, 800, 1_000, 1_200, 1_600, 1_900, 2_100, 2_700, 3_300, 3_600,
                3_900, 4_600, 5_400,
            ]
        );
        assert_eq!(
            policies.map(|policy| policy.standard_hp_anchor),
            STANDARD_HP_ANCHOR_BY_DEPTH
        );
        assert_eq!(policies.map(|policy| policy.base_hit), BASE_HIT_BY_DEPTH);
        assert_eq!(
            policies.map(|policy| policy.seam_capacity),
            [
                3_800, 5_200, 7_800, 8_800, 10_500, 11_000, 11_500, 16_000, 16_800, 17_500, 22_500,
                27_500, 28_500, 30_000, 35_000, 41_000,
            ]
        );
    }

    #[test]
    fn surface_is_the_only_no_encounter_depth_and_has_no_combat_anchors() {
        let surface = mining_depth_risk_policy(MiningDepth::Surface);
        assert_eq!(surface.first_event_chance_bps, 0);
        assert_eq!(surface.second_event_chance_given_first_bps, 0);
        assert_eq!(surface.pack_two_plus_chance_given_event_bps, 0);
        assert_eq!(surface.standard_hp_anchor, None);
        assert_eq!(surface.base_hit, None);

        for depth in ALL_MINING_DEPTHS.into_iter().skip(1) {
            let policy = mining_depth_risk_policy(depth);
            assert!(policy.first_event_chance_bps > 0);
            assert!(policy.standard_hp_anchor.is_some());
            assert!(policy.base_hit.is_some());
        }
    }

    #[test]
    fn probability_inputs_stay_inside_basis_point_domain_and_capacity_is_positive() {
        for depth in ALL_MINING_DEPTHS {
            let policy = mining_depth_risk_policy(depth);
            assert!(policy.first_event_chance_bps <= 10_000);
            assert!(policy.second_event_chance_given_first_bps <= 10_000);
            assert!(policy.pack_two_plus_chance_given_event_bps <= 10_000);
            assert!(policy.seam_capacity > 0);
        }
    }
}
