use graphite_services::{
    ALL_MINING_DEPTHS, MINING_DEPTH_COUNT, MiningDepth, MiningWorld, mining_depth_risk_policy,
};

#[test]
fn public_api_exposes_all_sixteen_canonical_depth_rows_exactly() {
    assert_eq!(ALL_MINING_DEPTHS.len(), MINING_DEPTH_COUNT);
    let policies = ALL_MINING_DEPTHS.map(mining_depth_risk_policy);

    assert_eq!(
        policies.map(|policy| policy.depth_index),
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
    );
    assert_eq!(
        policies.map(|policy| policy.world),
        [
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
        ]
    );
    assert_eq!(
        policies.map(|policy| policy.first_event_chance_bps),
        [
            0, 50, 100, 130, 180, 230, 280, 400, 480, 550, 700, 850, 920, 1_000, 1_200, 1_400,
        ]
    );
    assert_eq!(
        policies.map(|policy| policy.second_event_chance_given_first_bps),
        [
            0, 0, 100, 150, 200, 300, 400, 600, 750, 900, 1_200, 1_500, 1_650, 1_800, 2_200, 2_500,
        ]
    );
    assert_eq!(
        policies.map(|policy| policy.pack_two_plus_chance_given_event_bps),
        [
            0, 300, 500, 600, 800, 1_000, 1_200, 1_600, 1_900, 2_100, 2_700, 3_300, 3_600, 3_900,
            4_600, 5_400,
        ]
    );
    assert_eq!(
        policies.map(|policy| policy.standard_hp_anchor),
        [
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
        ]
    );
    assert_eq!(
        policies.map(|policy| policy.base_hit),
        [
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
        ]
    );
    assert_eq!(
        policies.map(|policy| policy.seam_capacity),
        [
            3_800, 5_200, 7_800, 8_800, 10_500, 11_000, 11_500, 16_000, 16_800, 17_500, 22_500,
            27_500, 28_500, 30_000, 35_000, 41_000,
        ]
    );

    for (depth, policy) in ALL_MINING_DEPTHS.into_iter().zip(policies) {
        assert_eq!(policy.depth, depth);
        assert_eq!(policy.depth_index, depth.index());
    }
}

#[test]
fn public_surface_contract_remains_safe_without_implying_later_access_rules() {
    let surface = mining_depth_risk_policy(MiningDepth::Surface);
    assert_eq!(surface.depth_index, 0);
    assert_eq!(surface.world, MiningWorld::Overworld);
    assert_eq!(surface.first_event_chance_bps, 0);
    assert_eq!(surface.second_event_chance_given_first_bps, 0);
    assert_eq!(surface.pack_two_plus_chance_given_event_bps, 0);
    assert_eq!(surface.standard_hp_anchor, None);
    assert_eq!(surface.base_hit, None);
}
