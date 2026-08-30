use graphite_services::{
    EquipmentTier, FishingArea, FishingCapabilityClassification, FishingCapabilityRatio,
    FishingCapabilityResolutionError, FishingCapabilityRoute, FishingCapabilityTerminalOutcome,
    FishingCatchBranch, FishingOverCapCatchRollResolution, FishingRarity,
    FishingResolvedOverCapSequence, FishingRoutedFishCapabilityStage,
    fishing_capability_routing_policy, fishing_catch_load, fishing_tension,
    manual_fishing_capability_ratio, manual_fishing_line_strength,
    resolve_fishing_over_cap_sequence, resolve_routed_fish_capability_stage,
};

const AREAS: [FishingArea; 6] = [
    FishingArea::StarterPool,
    FishingArea::River,
    FishingArea::Lake,
    FishingArea::Coast,
    FishingArea::DeepSea,
    FishingArea::Abyss,
];

fn manual_wood_ratio(common_weight_grams: u64) -> FishingCapabilityRatio {
    let tensions = [fishing_tension(common_weight_grams, FishingRarity::Common).unwrap()];
    let load = fishing_catch_load(&tensions).unwrap();
    let strength = manual_fishing_line_strength(EquipmentTier::Wood, true, None, false).unwrap();
    manual_fishing_capability_ratio(load, strength).unwrap()
}

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
                    assert_eq!(
                        policy.guaranteed_capability_outcome(),
                        Some(FishingCapabilityTerminalOutcome::Landed)
                    );
                }
                (FishingArea::StarterPool, FishingCatchBranch::Fish) => {
                    assert_eq!(
                        policy.route(),
                        FishingCapabilityRoute::StarterPoolGuaranteedFishLanding
                    );
                    assert!(!policy.requires_line_tension_for_capability());
                    assert!(!policy.line_break_stage_enabled());
                    assert!(!policy.over_cap_escape_stage_enabled());
                    assert_eq!(
                        policy.guaranteed_capability_outcome(),
                        Some(FishingCapabilityTerminalOutcome::Landed)
                    );
                }
                (_, FishingCatchBranch::Fish) => {
                    assert_eq!(
                        policy.route(),
                        FishingCapabilityRoute::ResolveFishCapability
                    );
                    assert!(policy.requires_line_tension_for_capability());
                    assert!(policy.line_break_stage_enabled());
                    assert!(policy.over_cap_escape_stage_enabled());
                    assert_eq!(policy.guaranteed_capability_outcome(), None);
                }
            }
        }
    }
}

#[test]
fn public_api_resolves_within_capability_and_rejects_bypass_misuse() {
    let ratio = manual_wood_ratio(6_000);
    let river_fish =
        fishing_capability_routing_policy(FishingArea::River, FishingCatchBranch::Fish);
    assert_eq!(
        resolve_routed_fish_capability_stage(river_fish, ratio),
        Ok(FishingRoutedFishCapabilityStage::LandedWithinRodCapability)
    );

    for bypass in [
        fishing_capability_routing_policy(FishingArea::River, FishingCatchBranch::Junk),
        fishing_capability_routing_policy(FishingArea::River, FishingCatchBranch::Treasure),
        fishing_capability_routing_policy(FishingArea::StarterPool, FishingCatchBranch::Fish),
    ] {
        assert_eq!(
            resolve_routed_fish_capability_stage(bypass, ratio),
            Err(
                FishingCapabilityResolutionError::RouteDoesNotResolveFishCapability(bypass.route())
            )
        );
    }
}

#[test]
fn public_api_rederives_stage_from_exact_ratio_not_mutable_classification_label() {
    let routing = fishing_capability_routing_policy(FishingArea::River, FishingCatchBranch::Fish);

    let mut within = manual_wood_ratio(6_000);
    within.classification = FishingCapabilityClassification::OverRodCapability;
    assert_eq!(
        resolve_routed_fish_capability_stage(routing, within),
        Ok(FishingRoutedFishCapabilityStage::LandedWithinRodCapability)
    );

    let mut over = manual_wood_ratio(6_600);
    over.classification = FishingCapabilityClassification::WithinRodCapability;
    let FishingRoutedFishCapabilityStage::ResolveOverCap(requirement) =
        resolve_routed_fish_capability_stage(routing, over).unwrap()
    else {
        panic!("numeric over-cap ratio was overridden by a mutable classification label");
    };
    assert_eq!(
        requirement.capability_ratio().classification,
        FishingCapabilityClassification::OverRodCapability
    );
    assert_eq!(requirement.capability_ratio().numerator(), over.numerator());
    assert_eq!(
        requirement.capability_ratio().denominator(),
        over.denominator()
    );
}

#[test]
fn public_api_binds_over_cap_sequence_to_a_valid_over_cap_requirement() {
    let ratio = manual_wood_ratio(6_600);
    let routing = fishing_capability_routing_policy(FishingArea::River, FishingCatchBranch::Fish);
    let FishingRoutedFishCapabilityStage::ResolveOverCap(requirement) =
        resolve_routed_fish_capability_stage(routing, ratio).unwrap()
    else {
        panic!("over-cap Fish did not request the ordered over-cap sequence");
    };

    assert_eq!(requirement.capability_ratio(), ratio);
    assert_eq!(
        resolve_fishing_over_cap_sequence(requirement, FishingResolvedOverCapSequence::LineBreak,),
        FishingCapabilityTerminalOutcome::LineBreak
    );
    assert_eq!(
        resolve_fishing_over_cap_sequence(
            requirement,
            FishingResolvedOverCapSequence::LineSurvived(FishingOverCapCatchRollResolution::Landed,),
        ),
        FishingCapabilityTerminalOutcome::Landed
    );
    assert_eq!(
        resolve_fishing_over_cap_sequence(
            requirement,
            FishingResolvedOverCapSequence::LineSurvived(
                FishingOverCapCatchRollResolution::Escaped,
            ),
        ),
        FishingCapabilityTerminalOutcome::Escaped
    );
}
