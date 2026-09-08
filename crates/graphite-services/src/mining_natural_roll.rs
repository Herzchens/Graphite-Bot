use crate::MiningDepth;

/// Exact weight scale for the frozen zero-depletion natural Mining roll table.
///
/// `100_000` units equal `100%`; one unit is `0.001` percentage point. The current table needs
/// exactly this precision because its smallest frozen natural-roll rate is `0.001%`.
pub const MINING_NATURAL_ROLL_WEIGHT_SCALE: u32 = 100_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MiningNaturalRollDrop {
    pub content_key: &'static str,
    pub weight_units: u32,
}

impl MiningNaturalRollDrop {
    #[must_use]
    pub const fn probability_numerator(self) -> u32 {
        self.weight_units
    }

    #[must_use]
    pub const fn probability_denominator(self) -> u32 {
        MINING_NATURAL_ROLL_WEIGHT_SCALE
    }
}

const fn drop(content_key: &'static str, weight_units: u32) -> MiningNaturalRollDrop {
    MiningNaturalRollDrop {
        content_key,
        weight_units,
    }
}

const SURFACE_DROPS: &[MiningNaturalRollDrop] = &[
    drop("resource.wood.log", 73_000),
    drop("resource.stone", 25_000),
    drop("resource.coal", 1_700),
    drop("resource.ore.copper", 300),
];

const SHALLOW_DROPS: &[MiningNaturalRollDrop] = &[
    drop("resource.cobblestone", 79_908),
    drop("resource.deepslate", 11_492),
    drop("resource.coal", 4_000),
    drop("resource.ore.copper", 2_500),
    drop("resource.ore.tin", 1_500),
    drop("resource.ore.iron", 600),
];

const UPPER_CAVERN_DROPS: &[MiningNaturalRollDrop] = &[
    drop("resource.cobblestone", 68_771),
    drop("resource.deepslate", 15_229),
    drop("resource.coal", 5_000),
    drop("resource.ore.copper", 4_000),
    drop("resource.ore.tin", 2_500),
    drop("resource.ore.zinc", 1_500),
    drop("resource.ore.iron", 1_500),
    drop("resource.bauxite", 1_000),
    drop("resource.lapis", 500),
];

const DRIPSTONE_CAVERN_DROPS: &[MiningNaturalRollDrop] = &[
    drop("resource.cobblestone", 70_706),
    drop("resource.deepslate", 6_914),
    drop("resource.coal", 4_000),
    drop("resource.ore.copper", 8_000),
    drop("resource.ore.tin", 2_000),
    drop("resource.ore.zinc", 2_000),
    drop("resource.ore.iron", 2_000),
    drop("resource.bauxite", 3_000),
    drop("resource.lapis", 800),
    drop("resource.ore.gold", 80),
    drop("resource.redstone", 500),
];

const CAVERN_DROPS: &[MiningNaturalRollDrop] = &[
    drop("resource.cobblestone", 56_668),
    drop("resource.deepslate", 21_952),
    drop("resource.coal", 3_000),
    drop("resource.ore.copper", 3_000),
    drop("resource.ore.tin", 1_500),
    drop("resource.ore.zinc", 1_500),
    drop("resource.ore.iron", 5_000),
    drop("resource.bauxite", 1_500),
    drop("resource.lapis", 2_000),
    drop("resource.redstone", 2_500),
    drop("resource.ore.gold", 600),
    drop("resource.ore.silver", 300),
    drop("resource.gem.diamond", 30),
    drop("resource.gem.amethyst", 400),
    drop("resource.gem.jade", 50),
];

const LOWER_CAVERN_DROPS: &[MiningNaturalRollDrop] = &[
    drop("resource.cobblestone", 50_999),
    drop("resource.deepslate", 27_191),
    drop("resource.coal", 2_000),
    drop("resource.ore.copper", 2_000),
    drop("resource.ore.tin", 1_000),
    drop("resource.ore.zinc", 1_500),
    drop("resource.ore.iron", 5_000),
    drop("resource.bauxite", 1_000),
    drop("resource.lapis", 2_500),
    drop("resource.redstone", 4_000),
    drop("resource.ore.gold", 800),
    drop("resource.ore.silver", 600),
    drop("resource.ore.nickel", 300),
    drop("resource.ore.lead", 400),
    drop("resource.gem.diamond", 80),
    drop("resource.gem.amethyst", 500),
    drop("resource.gem.jade", 100),
    drop("resource.gem.topaz", 30),
];

const DEEP_CAVERN_DROPS: &[MiningNaturalRollDrop] = &[
    drop("resource.cobblestone", 38_881),
    drop("resource.deepslate", 41_539),
    drop("resource.coal", 1_000),
    drop("resource.ore.copper", 1_000),
    drop("resource.ore.tin", 500),
    drop("resource.ore.zinc", 1_000),
    drop("resource.ore.iron", 4_000),
    drop("resource.bauxite", 500),
    drop("resource.lapis", 2_500),
    drop("resource.redstone", 5_000),
    drop("resource.ore.gold", 1_000),
    drop("resource.ore.silver", 800),
    drop("resource.ore.nickel", 500),
    drop("resource.ore.lead", 600),
    drop("resource.gem.diamond", 150),
    drop("resource.gem.amethyst", 800),
    drop("resource.gem.jade", 150),
    drop("resource.gem.topaz", 50),
    drop("resource.gem.ruby", 15),
    drop("resource.gem.sapphire", 15),
];

const CRYSTAL_DEPTHS_DROPS: &[MiningNaturalRollDrop] = &[
    drop("resource.cobblestone", 59_196),
    drop("resource.deepslate", 22_392),
    drop("resource.coal", 500),
    drop("resource.ore.copper", 500),
    drop("resource.ore.zinc", 500),
    drop("resource.ore.iron", 3_000),
    drop("resource.bauxite", 500),
    drop("resource.lapis", 2_500),
    drop("resource.redstone", 5_000),
    drop("resource.ore.gold", 1_000),
    drop("resource.ore.silver", 800),
    drop("resource.ore.nickel", 500),
    drop("resource.ore.lead", 500),
    drop("resource.gem.diamond", 200),
    drop("resource.gem.amethyst", 2_500),
    drop("resource.gem.jade", 250),
    drop("resource.gem.topaz", 100),
    drop("resource.gem.ruby", 30),
    drop("resource.gem.sapphire", 30),
    drop("resource.gem.blood_diamond", 2),
];

const DIAMOND_SHELF_DROPS: &[MiningNaturalRollDrop] = &[
    drop("resource.cobblestone", 36_637),
    drop("resource.deepslate", 47_370),
    drop("resource.coal", 300),
    drop("resource.ore.copper", 300),
    drop("resource.ore.zinc", 400),
    drop("resource.ore.iron", 2_500),
    drop("resource.lapis", 2_000),
    drop("resource.redstone", 5_000),
    drop("resource.ore.gold", 1_200),
    drop("resource.ore.silver", 800),
    drop("resource.ore.nickel", 700),
    drop("resource.ore.lead", 400),
    drop("resource.gem.diamond", 350),
    drop("resource.gem.amethyst", 1_500),
    drop("resource.gem.jade", 300),
    drop("resource.gem.topaz", 120),
    drop("resource.gem.ruby", 50),
    drop("resource.gem.sapphire", 50),
    drop("resource.gem.onyx", 20),
    drop("resource.gem.blood_diamond", 3),
];

const DIAMOND_DEPTHS_DROPS: &[MiningNaturalRollDrop] = &[
    drop("resource.cobblestone", 22_174),
    drop("resource.deepslate", 63_342),
    drop("resource.coal", 200),
    drop("resource.ore.iron", 2_000),
    drop("resource.lapis", 1_500),
    drop("resource.redstone", 5_500),
    drop("resource.ore.gold", 1_200),
    drop("resource.ore.silver", 800),
    drop("resource.ore.nickel", 800),
    drop("resource.ore.lead", 300),
    drop("resource.gem.diamond", 550),
    drop("resource.gem.amethyst", 1_000),
    drop("resource.gem.jade", 300),
    drop("resource.gem.topaz", 150),
    drop("resource.gem.ruby", 70),
    drop("resource.gem.sapphire", 70),
    drop("resource.gem.onyx", 40),
    drop("resource.gem.blood_diamond", 4),
];

const OBSIDIAN_CHASM_DROPS: &[MiningNaturalRollDrop] = &[
    drop("resource.cobblestone", 21_500),
    drop("resource.deepslate", 66_845),
    drop("resource.ore.iron", 1_500),
    drop("resource.lapis", 1_000),
    drop("resource.redstone", 4_500),
    drop("resource.ore.gold", 1_000),
    drop("resource.ore.silver", 600),
    drop("resource.ore.nickel", 800),
    drop("resource.ore.lead", 200),
    drop("resource.gem.diamond", 700),
    drop("resource.obsidian", 180),
    drop("resource.gem.amethyst", 600),
    drop("resource.gem.jade", 200),
    drop("resource.gem.topaz", 150),
    drop("resource.gem.ruby", 80),
    drop("resource.gem.sapphire", 80),
    drop("resource.gem.onyx", 60),
    drop("resource.gem.blood_diamond", 5),
];

const NETHER_FRINGE_DROPS: &[MiningNaturalRollDrop] = &[
    drop("resource.netherrack", 10_689),
    drop("resource.blackstone", 81_090),
    drop("resource.quartz.nether", 6_000),
    drop("resource.ore.gold", 2_000),
    drop("resource.ore.cobalt", 150),
    drop("resource.ancient_debris", 15),
    drop("resource.gem.onyx", 30),
    drop("resource.gem.ruby", 20),
    drop("resource.ore.tungsten", 3),
    drop("resource.ore.titanium", 2),
    drop("resource.ore.platinum", 1),
];

const BASALT_DEPTHS_DROPS: &[MiningNaturalRollDrop] = &[
    drop("resource.netherrack", 12_862),
    drop("resource.blackstone", 78_786),
    drop("resource.quartz.nether", 6_000),
    drop("resource.ore.gold", 2_000),
    drop("resource.ore.cobalt", 250),
    drop("resource.ancient_debris", 25),
    drop("resource.gem.onyx", 40),
    drop("resource.gem.ruby", 25),
    drop("resource.ore.tungsten", 6),
    drop("resource.ore.titanium", 4),
    drop("resource.ore.platinum", 2),
];

const NETHER_DEPTHS_DROPS: &[MiningNaturalRollDrop] = &[
    drop("resource.netherrack", 19_034),
    drop("resource.blackstone", 72_227),
    drop("resource.quartz.nether", 6_000),
    drop("resource.ore.gold", 2_200),
    drop("resource.ore.cobalt", 400),
    drop("resource.ancient_debris", 40),
    drop("resource.gem.onyx", 50),
    drop("resource.gem.ruby", 30),
    drop("resource.ore.tungsten", 10),
    drop("resource.ore.titanium", 6),
    drop("resource.ore.platinum", 3),
];

const ANCIENT_DEPTHS_DROPS: &[MiningNaturalRollDrop] = &[
    drop("resource.netherrack", 38_753),
    drop("resource.blackstone", 52_789),
    drop("resource.quartz.nether", 5_500),
    drop("resource.ore.gold", 2_200),
    drop("resource.ore.cobalt", 550),
    drop("resource.ancient_debris", 60),
    drop("resource.gem.onyx", 70),
    drop("resource.gem.ruby", 40),
    drop("resource.ore.tungsten", 20),
    drop("resource.ore.titanium", 12),
    drop("resource.ore.platinum", 6),
];

const ABYSSAL_DEPTHS_DROPS: &[MiningNaturalRollDrop] = &[
    drop("resource.netherrack", 52_808),
    drop("resource.blackstone", 38_992),
    drop("resource.quartz.nether", 5_000),
    drop("resource.ore.gold", 2_200),
    drop("resource.ore.cobalt", 700),
    drop("resource.ancient_debris", 90),
    drop("resource.gem.onyx", 100),
    drop("resource.gem.ruby", 50),
    drop("resource.ore.tungsten", 30),
    drop("resource.ore.titanium", 20),
    drop("resource.ore.platinum", 10),
];

/// Returns the exact current-v1 natural Mining roll weights at zero depletion for `depth`.
///
/// These are source weights before Fortune, Treasure, Trench, Nuke, and depletion reweighting.
/// Every returned row sums to [`MINING_NATURAL_ROLL_WEIGHT_SCALE`].
///
/// This function deliberately performs no RNG draw or draw-to-weight mapping. It also does not
/// apply Pickaxe capability filtering, depletion density, modifier reweighting, quantity effects,
/// pressure, durability, settlement, or `/mine` lifecycle behavior.
#[must_use]
pub const fn mining_natural_roll_drops(depth: MiningDepth) -> &'static [MiningNaturalRollDrop] {
    match depth {
        MiningDepth::Surface => SURFACE_DROPS,
        MiningDepth::Shallow => SHALLOW_DROPS,
        MiningDepth::UpperCavern => UPPER_CAVERN_DROPS,
        MiningDepth::DripstoneCavern => DRIPSTONE_CAVERN_DROPS,
        MiningDepth::Cavern => CAVERN_DROPS,
        MiningDepth::LowerCavern => LOWER_CAVERN_DROPS,
        MiningDepth::DeepCavern => DEEP_CAVERN_DROPS,
        MiningDepth::CrystalDepths => CRYSTAL_DEPTHS_DROPS,
        MiningDepth::DiamondShelf => DIAMOND_SHELF_DROPS,
        MiningDepth::DiamondDepths => DIAMOND_DEPTHS_DROPS,
        MiningDepth::ObsidianChasm => OBSIDIAN_CHASM_DROPS,
        MiningDepth::NetherFringe => NETHER_FRINGE_DROPS,
        MiningDepth::BasaltDepths => BASALT_DEPTHS_DROPS,
        MiningDepth::NetherDepths => NETHER_DEPTHS_DROPS,
        MiningDepth::AncientDepths => ANCIENT_DEPTHS_DROPS,
        MiningDepth::AbyssalDepths => ABYSSAL_DEPTHS_DROPS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ALL_MINING_DEPTHS;
    use std::collections::HashSet;

    #[test]
    fn every_depth_row_sums_exactly_to_one_hundred_percent() {
        for depth in ALL_MINING_DEPTHS {
            let drops = mining_natural_roll_drops(depth);
            let sum: u32 = drops.iter().map(|drop| drop.weight_units).sum();
            assert_eq!(sum, MINING_NATURAL_ROLL_WEIGHT_SCALE, "{depth:?}");
            assert!(drops.iter().all(|drop| drop.weight_units > 0), "{depth:?}");
        }
    }

    #[test]
    fn every_depth_row_has_unique_content_keys() {
        for depth in ALL_MINING_DEPTHS {
            let drops = mining_natural_roll_drops(depth);
            let unique: HashSet<&str> = drops.iter().map(|drop| drop.content_key).collect();
            assert_eq!(unique.len(), drops.len(), "{depth:?}");
        }
    }

    #[test]
    fn smallest_frozen_rates_remain_exact_without_floating_point() {
        let nether_fringe = mining_natural_roll_drops(MiningDepth::NetherFringe);
        let platinum = nether_fringe
            .iter()
            .find(|drop| drop.content_key == "resource.ore.platinum")
            .unwrap();
        assert_eq!(platinum.weight_units, 1);
        assert_eq!(platinum.probability_numerator(), 1);
        assert_eq!(platinum.probability_denominator(), 100_000);

        let crystal = mining_natural_roll_drops(MiningDepth::CrystalDepths);
        let blood_diamond = crystal
            .iter()
            .find(|drop| drop.content_key == "resource.gem.blood_diamond")
            .unwrap();
        assert_eq!(blood_diamond.weight_units, 2);
    }
}
