use serde::Serialize;

use crate::{fishing_area::FishingArea, fishing_droptable::FishingCatchBranch};

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

#[cfg(test)]
mod tests {
    use super::*;

    const AREAS: [FishingArea; 6] = [
        FishingArea::StarterPool,
        FishingArea::River,
        FishingArea::Lake,
        FishingArea::Coast,
        FishingArea::DeepSea,
        FishingArea::Abyss,
    ];

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
            } else {
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
