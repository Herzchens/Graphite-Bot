use serde::Serialize;
use thiserror::Error;

use crate::{
    fishing_area::FishingArea,
    fishing_capability::{FishingCapabilityClassification, FishingCapabilityRatio},
    fishing_droptable::FishingCatchBranch,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FishingCapabilityRoute {
    /// Junk and Treasure branches never enter fish tension/capability resolution.
    BypassNonFishBranch,
    /// Starter Pool fish always land and cannot enter line-break or escape resolution.
    StarterPoolGuaranteedFishLanding,
    /// A non-Starter-Pool fish branch must continue through CatchLoad/Rod capability resolution.
    ResolveFishCapability,
}

impl FishingCapabilityRoute {
    #[must_use]
    pub const fn requires_line_tension_for_capability(self) -> bool {
        matches!(self, Self::ResolveFishCapability)
    }

    #[must_use]
    pub const fn line_break_stage_enabled(self) -> bool {
        self.requires_line_tension_for_capability()
    }

    #[must_use]
    pub const fn over_cap_escape_stage_enabled(self) -> bool {
        self.requires_line_tension_for_capability()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FishingCapabilityRoutingPolicy {
    area: FishingArea,
    branch: FishingCatchBranch,
    route: FishingCapabilityRoute,
}

impl FishingCapabilityRoutingPolicy {
    #[must_use]
    pub const fn area(self) -> FishingArea {
        self.area
    }

    #[must_use]
    pub const fn branch(self) -> FishingCatchBranch {
        self.branch
    }

    #[must_use]
    pub const fn route(self) -> FishingCapabilityRoute {
        self.route
    }

    #[must_use]
    pub const fn requires_line_tension_for_capability(self) -> bool {
        self.route.requires_line_tension_for_capability()
    }

    #[must_use]
    pub const fn line_break_stage_enabled(self) -> bool {
        self.route.line_break_stage_enabled()
    }

    #[must_use]
    pub const fn over_cap_escape_stage_enabled(self) -> bool {
        self.route.over_cap_escape_stage_enabled()
    }

    /// Returns the terminal capability-stage outcome when this route bypasses Rod capability math.
    ///
    /// `Landed` here is scoped to the physical capability stage only. It does not imply that later
    /// inventory, progression, bait, durability, or atomic-settlement work has already succeeded.
    #[must_use]
    pub const fn guaranteed_capability_outcome(self) -> Option<FishingCapabilityTerminalOutcome> {
        match self.route {
            FishingCapabilityRoute::BypassNonFishBranch
            | FishingCapabilityRoute::StarterPoolGuaranteedFishLanding => {
                Some(FishingCapabilityTerminalOutcome::Landed)
            }
            FishingCapabilityRoute::ResolveFishCapability => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FishingCapabilityTerminalOutcome {
    Landed,
    Escaped,
    LineBreak,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FishingOverCapCatchRollResolution {
    Landed,
    Escaped,
}

/// Already-authoritative RNG evidence for the ordered `R > 1` capability sequence.
///
/// The nested shape deliberately makes a catch/escape roll representable only after line survival.
/// A line break is terminal and cannot carry a later over-cap catch result.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FishingResolvedOverCapSequence {
    LineBreak,
    LineSurvived(FishingOverCapCatchRollResolution),
}

/// Proof that the routed candidate is a non-Starter-Pool Fish with an exact `R > 1` ratio.
///
/// The field is private so external callers cannot fabricate this token. It is produced only by
/// [`resolve_routed_fish_capability_stage`] after the route and exact ratio classification agree.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FishingOverCapResolutionRequired {
    capability_ratio: FishingCapabilityRatio,
}

impl FishingOverCapResolutionRequired {
    /// Returns the exact over-cap ratio that owns the later line-break and catch rolls.
    #[must_use]
    pub const fn capability_ratio(self) -> FishingCapabilityRatio {
        self.capability_ratio
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FishingRoutedFishCapabilityStage {
    LandedWithinRodCapability,
    ResolveOverCap(FishingOverCapResolutionRequired),
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum FishingCapabilityResolutionError {
    #[error("routing state {0:?} does not enter Fish capability resolution")]
    RouteDoesNotResolveFishCapability(FishingCapabilityRoute),
}

/// Resolves the frozen routing boundary before Rod-capability math.
///
/// The active Fishing specification defines two independent bypasses:
///
/// - Junk and Treasure branches do not use line tension in any area.
/// - Fish in Starter Pool are tutorial-safe: the candidate fish lands without line-break/fail.
///
/// Only a Fish branch outside Starter Pool continues into CatchLoad / EffectiveLineStrength / `R`
/// classification. This function deliberately does not evaluate the unresolved `(R - 1)^1.30`
/// line-break probability, perform RNG, construct FishInstances, consume bait, or settle rewards.
///
/// `requires_line_tension_for_capability` describes only this capability-routing decision. It does
/// not prohibit a future Starter-Pool FishInstance/statistics layer from deriving FishTension for a
/// separate non-capability purpose. `line_break_stage_enabled` means the route is eligible to reach
/// that later stage when `R > 1`; it does not imply that a particular fish has a non-zero line-break
/// chance. Within-capability fish still land with zero line-break chance under the separate policy.
#[must_use]
pub const fn fishing_capability_routing_policy(
    area: FishingArea,
    branch: FishingCatchBranch,
) -> FishingCapabilityRoutingPolicy {
    let route = match branch {
        FishingCatchBranch::Junk | FishingCatchBranch::Treasure => {
            FishingCapabilityRoute::BypassNonFishBranch
        }
        FishingCatchBranch::Fish if matches!(area, FishingArea::StarterPool) => {
            FishingCapabilityRoute::StarterPoolGuaranteedFishLanding
        }
        FishingCatchBranch::Fish => FishingCapabilityRoute::ResolveFishCapability,
    };

    FishingCapabilityRoutingPolicy {
        area,
        branch,
        route,
    }
}

/// Advances a routed non-Starter-Pool Fish through the exact `R <= 1` / `R > 1` boundary.
///
/// The ratio must come from the existing exact capability kernel. `R <= 1` terminates immediately
/// as a landed catch. `R > 1` returns an unforgeable requirement token that owns the later ordered
/// line-break and over-cap catch sequence.
///
/// Calling this function for Junk, Treasure, or Starter Pool Fish is rejected rather than silently
/// re-running capability math that those routes explicitly bypass. The stage is re-derived from the
/// ratio's private exact numerator/denominator rather than trusting its public classification label;
/// the returned over-cap token normalizes that label to the exact numeric relationship.
pub fn resolve_routed_fish_capability_stage(
    routing: FishingCapabilityRoutingPolicy,
    capability_ratio: FishingCapabilityRatio,
) -> Result<FishingRoutedFishCapabilityStage, FishingCapabilityResolutionError> {
    if routing.route() != FishingCapabilityRoute::ResolveFishCapability {
        return Err(
            FishingCapabilityResolutionError::RouteDoesNotResolveFishCapability(routing.route()),
        );
    }

    let exact_classification = if capability_ratio.numerator() <= capability_ratio.denominator() {
        FishingCapabilityClassification::WithinRodCapability
    } else {
        FishingCapabilityClassification::OverRodCapability
    };
    let mut capability_ratio = capability_ratio;
    capability_ratio.classification = exact_classification;

    Ok(match exact_classification {
        FishingCapabilityClassification::WithinRodCapability => {
            FishingRoutedFishCapabilityStage::LandedWithinRodCapability
        }
        FishingCapabilityClassification::OverRodCapability => {
            FishingRoutedFishCapabilityStage::ResolveOverCap(FishingOverCapResolutionRequired {
                capability_ratio,
            })
        }
    })
}

/// Resolves already-authoritative RNG results for a routed `R > 1` Fish candidate.
///
/// The requirement token proves that routing and exact capability classification reached the
/// over-cap stage. The resolution enum encodes the mandatory order: line-break is decided first;
/// only a survived line can carry the later over-cap catch/escape result.
///
/// This function does not calculate the unresolved fractional-power line-break probability, draw
/// RNG, mutate Rod durability, consume bait, grant AEXP, or settle the catch. Those remain separate
/// owners in the future atomic Fishing lifecycle.
#[must_use]
pub const fn resolve_fishing_over_cap_sequence(
    _requirement: FishingOverCapResolutionRequired,
    resolution: FishingResolvedOverCapSequence,
) -> FishingCapabilityTerminalOutcome {
    match resolution {
        FishingResolvedOverCapSequence::LineBreak => FishingCapabilityTerminalOutcome::LineBreak,
        FishingResolvedOverCapSequence::LineSurvived(FishingOverCapCatchRollResolution::Landed) => {
            FishingCapabilityTerminalOutcome::Landed
        }
        FishingResolvedOverCapSequence::LineSurvived(
            FishingOverCapCatchRollResolution::Escaped,
        ) => FishingCapabilityTerminalOutcome::Escaped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        EquipmentTier, FishingRarity, fishing_catch_load, fishing_tension,
        manual_fishing_capability_ratio, manual_fishing_line_strength,
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
        let strength =
            manual_fishing_line_strength(EquipmentTier::Wood, true, None, false).unwrap();
        manual_fishing_capability_ratio(load, strength).unwrap()
    }

    #[test]
    fn every_non_fish_branch_bypasses_capability_in_every_area() {
        for area in AREAS {
            for branch in [FishingCatchBranch::Junk, FishingCatchBranch::Treasure] {
                let policy = fishing_capability_routing_policy(area, branch);
                assert_eq!(policy.area(), area);
                assert_eq!(policy.branch(), branch);
                assert_eq!(policy.route(), FishingCapabilityRoute::BypassNonFishBranch);
                assert!(!policy.requires_line_tension_for_capability());
                assert!(!policy.line_break_stage_enabled());
                assert!(!policy.over_cap_escape_stage_enabled());
                assert_eq!(
                    policy.guaranteed_capability_outcome(),
                    Some(FishingCapabilityTerminalOutcome::Landed)
                );
            }
        }
    }

    #[test]
    fn starter_pool_fish_is_the_only_safe_fish_route() {
        for area in AREAS {
            let policy = fishing_capability_routing_policy(area, FishingCatchBranch::Fish);
            if area == FishingArea::StarterPool {
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
            } else {
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

    #[test]
    fn within_capability_routed_fish_lands_without_over_cap_resolution() {
        let routing =
            fishing_capability_routing_policy(FishingArea::River, FishingCatchBranch::Fish);
        let stage =
            resolve_routed_fish_capability_stage(routing, manual_wood_ratio(6_000)).unwrap();
        assert_eq!(
            stage,
            FishingRoutedFishCapabilityStage::LandedWithinRodCapability
        );
    }

    #[test]
    fn public_classification_label_cannot_override_exact_ratio() {
        let routing =
            fishing_capability_routing_policy(FishingArea::River, FishingCatchBranch::Fish);

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
    fn over_cap_routed_fish_produces_requirement_bound_to_exact_ratio() {
        let routing =
            fishing_capability_routing_policy(FishingArea::River, FishingCatchBranch::Fish);
        let ratio = manual_wood_ratio(6_600);
        let FishingRoutedFishCapabilityStage::ResolveOverCap(requirement) =
            resolve_routed_fish_capability_stage(routing, ratio).unwrap()
        else {
            panic!("over-cap Fish did not request the ordered over-cap sequence");
        };

        assert_eq!(requirement.capability_ratio(), ratio);
    }

    #[test]
    fn bypass_routes_reject_capability_ratio_resolution() {
        let ratio = manual_wood_ratio(6_000);
        for routing in [
            fishing_capability_routing_policy(FishingArea::River, FishingCatchBranch::Junk),
            fishing_capability_routing_policy(FishingArea::River, FishingCatchBranch::Treasure),
            fishing_capability_routing_policy(FishingArea::StarterPool, FishingCatchBranch::Fish),
        ] {
            assert_eq!(
                resolve_routed_fish_capability_stage(routing, ratio),
                Err(
                    FishingCapabilityResolutionError::RouteDoesNotResolveFishCapability(
                        routing.route()
                    )
                )
            );
        }
    }

    #[test]
    fn over_cap_sequence_enforces_line_break_before_catch_or_escape() {
        let routing =
            fishing_capability_routing_policy(FishingArea::River, FishingCatchBranch::Fish);
        let FishingRoutedFishCapabilityStage::ResolveOverCap(requirement) =
            resolve_routed_fish_capability_stage(routing, manual_wood_ratio(6_600)).unwrap()
        else {
            panic!("over-cap Fish did not request the ordered over-cap sequence");
        };

        assert_eq!(
            resolve_fishing_over_cap_sequence(
                requirement,
                FishingResolvedOverCapSequence::LineBreak,
            ),
            FishingCapabilityTerminalOutcome::LineBreak
        );
        assert_eq!(
            resolve_fishing_over_cap_sequence(
                requirement,
                FishingResolvedOverCapSequence::LineSurvived(
                    FishingOverCapCatchRollResolution::Landed,
                ),
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
}
