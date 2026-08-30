use graphite_services::{
    FishingArea, FishingCapabilityRoute, FishingCatchBranch, fishing_capability_routing_policy,
};

const AREAS: [FishingArea; 6] = [
    FishingArea::StarterPool,
    FishingArea::River,
    FishingArea::Lake,
    FishingArea::Coast,
    FishingArea::DeepSea,
    FishingArea::Abyss,
];

#[test]
fn public_api_routes_all_eighteen_area_branch_pairs_exactly() {
    for area in AREAS {
        for branch in [
            FishingCatchBranch::Fish,
            FishingCatchBranch::Junk,
            FishingCatchBranch::Treasure,
        ] {
            let policy = fishing_capability_routing_policy(area, branch);
            assert_eq!(policy.area(), area);
            assert_eq!(policy.branch(), branch);

            match (area, branch) {
                (_, FishingCatchBranch::Junk | FishingCatchBranch::Treasure) => {
                    assert_eq!(policy.route(), FishingCapabilityRoute::BypassNonFishBranch);
                    assert!(!policy.requires_line_tension_for_capability());
                    assert!(!policy.line_break_stage_enabled());
                    assert!(!policy.over_cap_escape_stage_enabled());
                }
                (FishingArea::StarterPool, FishingCatchBranch::Fish) => {
                    assert_eq!(
                        policy.route(),
                        FishingCapabilityRoute::StarterPoolGuaranteedFishLanding
                    );
                    assert!(!policy.requires_line_tension_for_capability());
                    assert!(!policy.line_break_stage_enabled());
                    assert!(!policy.over_cap_escape_stage_enabled());
                }
                (_, FishingCatchBranch::Fish) => {
                    assert_eq!(
                        policy.route(),
                        FishingCapabilityRoute::ResolveFishCapability
                    );
                    assert!(policy.requires_line_tension_for_capability());
                    assert!(policy.line_break_stage_enabled());
                    assert!(policy.over_cap_escape_stage_enabled());
                }
            }
        }
    }
}
