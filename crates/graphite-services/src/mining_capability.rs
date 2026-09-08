use crate::EquipmentTier;
use serde::Serialize;
use thiserror::Error;

/// Frozen Mining hardness/capability ladder used to gate an already-selected resource candidate.
///
/// Ordering is authoritative: a Pickaxe whose maximum class is `Cx` may mine resources in classes
/// `C0..=Cx` and may not mine a higher class. This is only the resource-hardness gate; it does not
/// authorize a depth/world, validate the expedition loadout, or choose a fallback resource.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MiningCapabilityClass {
    C0,
    C1,
    C2,
    C3,
    C4,
    C5,
    C6,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum MiningCapabilityError {
    #[error("equipment tier {0:?} is not a valid Mining Pickaxe tier")]
    UnsupportedPickaxeTier(EquipmentTier),
    #[error("content key is not in the frozen Mining capability map")]
    UnknownMiningResource,
}

/// Frozen unmodified Pickaxe roll-count row for one material tier.
///
/// This is deliberately crate-private until a stateful Mining owner needs the policy as part of a
/// complete expedition lifecycle. The range is the base material-tier row only: it does not apply
/// Efficiency, +N main-stat scaling, Day/Night Walker, temporary modifiers, or any other throughput
/// effect. It also does not define a random distribution or RNG-to-integer mapping inside the range.
/// Those semantics must be resolved independently before a production roll owner can draw a count.
///
/// `EquipmentTier::Wood` identifies the Wood material row only. This policy does not prove whether
/// a specific Wood ItemInstance is the system-bound Starter Pickaxe or ordinary Wood equipment;
/// authoritative ItemDefinition identity remains the caller's responsibility.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MiningPickaxeBaseRollRange {
    pub(crate) min_rolls: u64,
    pub(crate) max_rolls: u64,
}

/// Returns the frozen unmodified Pickaxe roll-count range for a material tier.
///
/// `StarterLeather` represents armor rather than a Pickaxe material tier and therefore fails closed.
pub(crate) const fn mining_pickaxe_base_roll_range(
    tier: EquipmentTier,
) -> Result<MiningPickaxeBaseRollRange, MiningCapabilityError> {
    let range = match tier {
        EquipmentTier::StarterLeather => {
            return Err(MiningCapabilityError::UnsupportedPickaxeTier(tier));
        }
        EquipmentTier::Wood => MiningPickaxeBaseRollRange {
            min_rolls: 1,
            max_rolls: 5,
        },
        EquipmentTier::Stone => MiningPickaxeBaseRollRange {
            min_rolls: 2,
            max_rolls: 6,
        },
        EquipmentTier::Copper => MiningPickaxeBaseRollRange {
            min_rolls: 4,
            max_rolls: 8,
        },
        EquipmentTier::Gold => MiningPickaxeBaseRollRange {
            min_rolls: 12,
            max_rolls: 18,
        },
        EquipmentTier::Iron => MiningPickaxeBaseRollRange {
            min_rolls: 6,
            max_rolls: 10,
        },
        EquipmentTier::Diamond => MiningPickaxeBaseRollRange {
            min_rolls: 9,
            max_rolls: 14,
        },
        EquipmentTier::Obsidian => MiningPickaxeBaseRollRange {
            min_rolls: 12,
            max_rolls: 18,
        },
        EquipmentTier::Netherite => MiningPickaxeBaseRollRange {
            min_rolls: 15,
            max_rolls: 22,
        },
        EquipmentTier::Graphite => MiningPickaxeBaseRollRange {
            min_rolls: 18,
            max_rolls: 26,
        },
    };
    Ok(range)
}

/// Returns the frozen maximum resource capability for a Pickaxe material tier.
///
/// Starter Wood Pickaxes use [`EquipmentTier::Wood`] and therefore resolve to `C0`; the
/// `StarterLeather` variant represents armor and is rejected instead of being assigned an invented
/// Pickaxe capability.
pub const fn mining_pickaxe_max_capability(
    tier: EquipmentTier,
) -> Result<MiningCapabilityClass, MiningCapabilityError> {
    match tier {
        EquipmentTier::StarterLeather => Err(MiningCapabilityError::UnsupportedPickaxeTier(tier)),
        EquipmentTier::Wood => Ok(MiningCapabilityClass::C0),
        EquipmentTier::Stone | EquipmentTier::Copper => Ok(MiningCapabilityClass::C1),
        EquipmentTier::Gold => Ok(MiningCapabilityClass::C3),
        EquipmentTier::Iron => Ok(MiningCapabilityClass::C4),
        EquipmentTier::Diamond => Ok(MiningCapabilityClass::C5),
        EquipmentTier::Obsidian | EquipmentTier::Netherite | EquipmentTier::Graphite => {
            Ok(MiningCapabilityClass::C6)
        }
    }
}

/// Classifies one canonical content-registry key under the frozen Mining capability table.
///
/// The mapping includes the reserved Emerald `C3` entry even though Emerald has no active cave
/// source. Unknown keys fail closed; callers must not silently treat a future/unrecognized resource
/// as `C0`.
pub fn mining_resource_capability(
    content_key: &str,
) -> Result<MiningCapabilityClass, MiningCapabilityError> {
    let capability = match content_key {
        // C0 — common terrain/basic blocks.
        "resource.wood.log"
        | "resource.stone"
        | "resource.cobblestone"
        | "resource.deepslate"
        | "resource.netherrack"
        | "resource.blackstone" => MiningCapabilityClass::C0,

        // C1 — upper-world common resources.
        "resource.coal"
        | "resource.ore.copper"
        | "resource.ore.tin"
        | "resource.ore.zinc"
        | "resource.bauxite" => MiningCapabilityClass::C1,

        // C2 — standard metals, utility ores, and common gems.
        "resource.ore.iron"
        | "resource.lapis"
        | "resource.redstone"
        | "resource.ore.lead"
        | "resource.ore.silver"
        | "resource.ore.nickel"
        | "resource.ore.gold"
        | "resource.gem.amethyst"
        | "resource.gem.jade"
        | "resource.quartz.nether" => MiningCapabilityClass::C2,

        // C3 — Gold Pickaxe's highest capability class.
        "resource.gem.emerald"
        | "resource.gem.topaz"
        | "resource.gem.ruby"
        | "resource.gem.sapphire" => MiningCapabilityClass::C3,

        // C4 — Diamond-family/deep-special resources.
        "resource.gem.diamond"
        | "resource.gem.blood_diamond"
        | "resource.gem.onyx"
        | "resource.ore.cobalt" => MiningCapabilityClass::C4,

        // C5 — progression-wall resources.
        "resource.obsidian" | "resource.ancient_debris" => MiningCapabilityClass::C5,

        // C6 — deepest current industrial resources.
        "resource.ore.titanium" | "resource.ore.tungsten" | "resource.ore.platinum" => {
            MiningCapabilityClass::C6
        }

        _ => return Err(MiningCapabilityError::UnknownMiningResource),
    };

    Ok(capability)
}

/// Applies only the frozen Pickaxe-vs-resource capability comparison.
///
/// `true` means the resource's capability class is no higher than the Pickaxe's maximum class.
/// A `false` result does **not** choose the spec's same-depth fallback resource: the canonical
/// fallback draw/reweight rule is not frozen yet and belongs to the future Mining roll owner.
pub fn mining_pickaxe_allows_resource(
    tier: EquipmentTier,
    content_key: &str,
) -> Result<bool, MiningCapabilityError> {
    let pickaxe_capability = mining_pickaxe_max_capability(tier)?;
    let resource_capability = mining_resource_capability(content_key)?;
    Ok(resource_capability <= pickaxe_capability)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pickaxe_base_roll_range_matches_every_frozen_material_row() {
        let cases = [
            (EquipmentTier::Wood, (1, 5)),
            (EquipmentTier::Stone, (2, 6)),
            (EquipmentTier::Copper, (4, 8)),
            (EquipmentTier::Gold, (12, 18)),
            (EquipmentTier::Iron, (6, 10)),
            (EquipmentTier::Diamond, (9, 14)),
            (EquipmentTier::Obsidian, (12, 18)),
            (EquipmentTier::Netherite, (15, 22)),
            (EquipmentTier::Graphite, (18, 26)),
        ];

        for (tier, (min_rolls, max_rolls)) in cases {
            assert_eq!(
                mining_pickaxe_base_roll_range(tier),
                Ok(MiningPickaxeBaseRollRange {
                    min_rolls,
                    max_rolls,
                })
            );
            assert!(min_rolls <= max_rolls);
        }
        assert_eq!(
            mining_pickaxe_base_roll_range(EquipmentTier::StarterLeather),
            Err(MiningCapabilityError::UnsupportedPickaxeTier(
                EquipmentTier::StarterLeather
            ))
        );
    }

    #[test]
    fn pickaxe_max_capability_matches_every_frozen_tier_row() {
        let cases = [
            (EquipmentTier::Wood, MiningCapabilityClass::C0),
            (EquipmentTier::Stone, MiningCapabilityClass::C1),
            (EquipmentTier::Copper, MiningCapabilityClass::C1),
            (EquipmentTier::Gold, MiningCapabilityClass::C3),
            (EquipmentTier::Iron, MiningCapabilityClass::C4),
            (EquipmentTier::Diamond, MiningCapabilityClass::C5),
            (EquipmentTier::Obsidian, MiningCapabilityClass::C6),
            (EquipmentTier::Netherite, MiningCapabilityClass::C6),
            (EquipmentTier::Graphite, MiningCapabilityClass::C6),
        ];

        for (tier, expected) in cases {
            assert_eq!(mining_pickaxe_max_capability(tier), Ok(expected));
        }
        assert_eq!(
            mining_pickaxe_max_capability(EquipmentTier::StarterLeather),
            Err(MiningCapabilityError::UnsupportedPickaxeTier(
                EquipmentTier::StarterLeather
            ))
        );
    }

    #[test]
    fn resource_capability_matches_every_frozen_family() {
        let cases = [
            ("resource.wood.log", MiningCapabilityClass::C0),
            ("resource.stone", MiningCapabilityClass::C0),
            ("resource.cobblestone", MiningCapabilityClass::C0),
            ("resource.deepslate", MiningCapabilityClass::C0),
            ("resource.netherrack", MiningCapabilityClass::C0),
            ("resource.blackstone", MiningCapabilityClass::C0),
            ("resource.coal", MiningCapabilityClass::C1),
            ("resource.ore.copper", MiningCapabilityClass::C1),
            ("resource.ore.tin", MiningCapabilityClass::C1),
            ("resource.ore.zinc", MiningCapabilityClass::C1),
            ("resource.bauxite", MiningCapabilityClass::C1),
            ("resource.ore.iron", MiningCapabilityClass::C2),
            ("resource.lapis", MiningCapabilityClass::C2),
            ("resource.redstone", MiningCapabilityClass::C2),
            ("resource.ore.lead", MiningCapabilityClass::C2),
            ("resource.ore.silver", MiningCapabilityClass::C2),
            ("resource.ore.nickel", MiningCapabilityClass::C2),
            ("resource.ore.gold", MiningCapabilityClass::C2),
            ("resource.gem.amethyst", MiningCapabilityClass::C2),
            ("resource.gem.jade", MiningCapabilityClass::C2),
            ("resource.quartz.nether", MiningCapabilityClass::C2),
            ("resource.gem.emerald", MiningCapabilityClass::C3),
            ("resource.gem.topaz", MiningCapabilityClass::C3),
            ("resource.gem.ruby", MiningCapabilityClass::C3),
            ("resource.gem.sapphire", MiningCapabilityClass::C3),
            ("resource.gem.diamond", MiningCapabilityClass::C4),
            ("resource.gem.blood_diamond", MiningCapabilityClass::C4),
            ("resource.gem.onyx", MiningCapabilityClass::C4),
            ("resource.ore.cobalt", MiningCapabilityClass::C4),
            ("resource.obsidian", MiningCapabilityClass::C5),
            ("resource.ancient_debris", MiningCapabilityClass::C5),
            ("resource.ore.titanium", MiningCapabilityClass::C6),
            ("resource.ore.tungsten", MiningCapabilityClass::C6),
            ("resource.ore.platinum", MiningCapabilityClass::C6),
        ];

        for (content_key, expected) in cases {
            assert_eq!(mining_resource_capability(content_key), Ok(expected));
        }
        assert_eq!(
            mining_resource_capability("resource.future.unfrozen"),
            Err(MiningCapabilityError::UnknownMiningResource)
        );
    }

    #[test]
    fn eligibility_uses_ordered_capability_without_inventing_fallback() {
        assert_eq!(
            mining_pickaxe_allows_resource(EquipmentTier::Gold, "resource.gem.sapphire"),
            Ok(true)
        );
        assert_eq!(
            mining_pickaxe_allows_resource(EquipmentTier::Gold, "resource.gem.diamond"),
            Ok(false)
        );
        assert_eq!(
            mining_pickaxe_allows_resource(EquipmentTier::Wood, "resource.stone"),
            Ok(true)
        );
        assert_eq!(
            mining_pickaxe_allows_resource(EquipmentTier::Wood, "resource.coal"),
            Ok(false)
        );
        assert_eq!(
            mining_pickaxe_allows_resource(EquipmentTier::StarterLeather, "resource.cobblestone"),
            Err(MiningCapabilityError::UnsupportedPickaxeTier(
                EquipmentTier::StarterLeather
            ))
        );
        assert_eq!(
            mining_pickaxe_allows_resource(EquipmentTier::Graphite, "resource.future.unfrozen"),
            Err(MiningCapabilityError::UnknownMiningResource)
        );
    }
}
