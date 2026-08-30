use serde::Serialize;
use thiserror::Error;

use crate::EquipmentTier;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct OrdinarySwordStats {
    pub tier: EquipmentTier,
    pub base_damage: u32,
    pub max_durability: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct OrdinaryPickaxeStats {
    pub tier: EquipmentTier,
    pub natural_roll_min: u32,
    pub natural_roll_max: u32,
    pub max_durability: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct OrdinaryFishingRodStats {
    pub tier: EquipmentTier,
    pub base_line_strength_kg_tension: u32,
    pub max_durability: i64,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum OrdinaryToolStatsError {
    #[error("Starter Leather is not an ordinary tool tier")]
    StarterLeatherIsNotOrdinaryToolTier,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CanonicalOrdinaryToolRow {
    sword_base_damage: u32,
    sword_max_durability: i64,
    pickaxe_roll_min: u32,
    pickaxe_roll_max: u32,
    pickaxe_max_durability: i64,
    rod_base_line_strength_kg_tension: u32,
    rod_max_durability: i64,
}

/// Returns the frozen ordinary Sword stats for one equipment tier.
///
/// Starter Pickaxe/Sword/Basic Rod definitions are separate system-bound equipment and deliberately
/// do not use this ordinary table. Gold is represented exactly as its fragile side-grade row rather
/// than being reordered into the ordinary progression curve.
pub fn ordinary_sword_stats(
    tier: EquipmentTier,
) -> Result<OrdinarySwordStats, OrdinaryToolStatsError> {
    let row = canonical_ordinary_tool_row(tier)?;
    Ok(OrdinarySwordStats {
        tier,
        base_damage: row.sword_base_damage,
        max_durability: row.sword_max_durability,
    })
}

/// Returns the frozen ordinary Pickaxe natural-roll and durability stats for one equipment tier.
pub fn ordinary_pickaxe_stats(
    tier: EquipmentTier,
) -> Result<OrdinaryPickaxeStats, OrdinaryToolStatsError> {
    let row = canonical_ordinary_tool_row(tier)?;
    Ok(OrdinaryPickaxeStats {
        tier,
        natural_roll_min: row.pickaxe_roll_min,
        natural_roll_max: row.pickaxe_roll_max,
        max_durability: row.pickaxe_max_durability,
    })
}

/// Returns the frozen ordinary Fishing Rod base line-strength and durability stats.
///
/// `base_line_strength_kg_tension` is the integer base stat from the canonical tool table. This
/// function does not choose a FishWeight persistence precision or apply Strengthen/bait/Automation
/// factors.
pub fn ordinary_fishing_rod_stats(
    tier: EquipmentTier,
) -> Result<OrdinaryFishingRodStats, OrdinaryToolStatsError> {
    let row = canonical_ordinary_tool_row(tier)?;
    Ok(OrdinaryFishingRodStats {
        tier,
        base_line_strength_kg_tension: row.rod_base_line_strength_kg_tension,
        max_durability: row.rod_max_durability,
    })
}

fn canonical_ordinary_tool_row(
    tier: EquipmentTier,
) -> Result<CanonicalOrdinaryToolRow, OrdinaryToolStatsError> {
    let row = match tier {
        EquipmentTier::StarterLeather => {
            return Err(OrdinaryToolStatsError::StarterLeatherIsNotOrdinaryToolTier);
        }
        EquipmentTier::Wood => CanonicalOrdinaryToolRow {
            sword_base_damage: 3,
            sword_max_durability: 600,
            pickaxe_roll_min: 1,
            pickaxe_roll_max: 5,
            pickaxe_max_durability: 700,
            rod_base_line_strength_kg_tension: 6,
            rod_max_durability: 600,
        },
        EquipmentTier::Stone => CanonicalOrdinaryToolRow {
            sword_base_damage: 4,
            sword_max_durability: 850,
            pickaxe_roll_min: 2,
            pickaxe_roll_max: 6,
            pickaxe_max_durability: 1_000,
            rod_base_line_strength_kg_tension: 10,
            rod_max_durability: 900,
        },
        EquipmentTier::Copper => CanonicalOrdinaryToolRow {
            sword_base_damage: 5,
            sword_max_durability: 1_250,
            pickaxe_roll_min: 4,
            pickaxe_roll_max: 8,
            pickaxe_max_durability: 1_500,
            rod_base_line_strength_kg_tension: 18,
            rod_max_durability: 1_400,
        },
        EquipmentTier::Gold => CanonicalOrdinaryToolRow {
            sword_base_damage: 8,
            sword_max_durability: 450,
            pickaxe_roll_min: 12,
            pickaxe_roll_max: 18,
            pickaxe_max_durability: 600,
            rod_base_line_strength_kg_tension: 40,
            rod_max_durability: 550,
        },
        EquipmentTier::Iron => CanonicalOrdinaryToolRow {
            sword_base_damage: 7,
            sword_max_durability: 1_900,
            pickaxe_roll_min: 6,
            pickaxe_roll_max: 10,
            pickaxe_max_durability: 2_300,
            rod_base_line_strength_kg_tension: 30,
            rod_max_durability: 2_200,
        },
        EquipmentTier::Diamond => CanonicalOrdinaryToolRow {
            sword_base_damage: 10,
            sword_max_durability: 2_800,
            pickaxe_roll_min: 9,
            pickaxe_roll_max: 14,
            pickaxe_max_durability: 3_400,
            rod_base_line_strength_kg_tension: 55,
            rod_max_durability: 3_300,
        },
        EquipmentTier::Obsidian => CanonicalOrdinaryToolRow {
            sword_base_damage: 14,
            sword_max_durability: 4_200,
            pickaxe_roll_min: 12,
            pickaxe_roll_max: 18,
            pickaxe_max_durability: 5_200,
            rod_base_line_strength_kg_tension: 85,
            rod_max_durability: 5_000,
        },
        EquipmentTier::Netherite => CanonicalOrdinaryToolRow {
            sword_base_damage: 19,
            sword_max_durability: 6_300,
            pickaxe_roll_min: 15,
            pickaxe_roll_max: 22,
            pickaxe_max_durability: 7_800,
            rod_base_line_strength_kg_tension: 120,
            rod_max_durability: 7_600,
        },
        EquipmentTier::Graphite => CanonicalOrdinaryToolRow {
            sword_base_damage: 25,
            sword_max_durability: 9_000,
            pickaxe_roll_min: 18,
            pickaxe_roll_max: 26,
            pickaxe_max_durability: 11_000,
            rod_base_line_strength_kg_tension: 160,
            rod_max_durability: 11_000,
        },
    };

    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_tool_table_is_frozen_across_all_ordinary_tiers() {
        let cases = [
            (EquipmentTier::Wood, 3, 600, 1, 5, 700, 6, 600),
            (EquipmentTier::Stone, 4, 850, 2, 6, 1_000, 10, 900),
            (EquipmentTier::Copper, 5, 1_250, 4, 8, 1_500, 18, 1_400),
            (EquipmentTier::Gold, 8, 450, 12, 18, 600, 40, 550),
            (EquipmentTier::Iron, 7, 1_900, 6, 10, 2_300, 30, 2_200),
            (EquipmentTier::Diamond, 10, 2_800, 9, 14, 3_400, 55, 3_300),
            (EquipmentTier::Obsidian, 14, 4_200, 12, 18, 5_200, 85, 5_000),
            (EquipmentTier::Netherite, 19, 6_300, 15, 22, 7_800, 120, 7_600),
            (EquipmentTier::Graphite, 25, 9_000, 18, 26, 11_000, 160, 11_000),
        ];

        for (tier, damage, sword_dura, roll_min, roll_max, pick_dura, line, rod_dura) in cases {
            let sword = ordinary_sword_stats(tier).unwrap();
            let pickaxe = ordinary_pickaxe_stats(tier).unwrap();
            let rod = ordinary_fishing_rod_stats(tier).unwrap();

            assert_eq!(sword.base_damage, damage);
            assert_eq!(sword.max_durability, sword_dura);
            assert_eq!(pickaxe.natural_roll_min, roll_min);
            assert_eq!(pickaxe.natural_roll_max, roll_max);
            assert_eq!(pickaxe.max_durability, pick_dura);
            assert_eq!(rod.base_line_strength_kg_tension, line);
            assert_eq!(rod.max_durability, rod_dura);
        }
    }

    #[test]
    fn gold_remains_a_fragile_side_grade_instead_of_an_ordinal_progression_row() {
        let gold = ordinary_fishing_rod_stats(EquipmentTier::Gold).unwrap();
        let iron = ordinary_fishing_rod_stats(EquipmentTier::Iron).unwrap();
        assert!(gold.base_line_strength_kg_tension > iron.base_line_strength_kg_tension);
        assert!(gold.max_durability < iron.max_durability);
    }

    #[test]
    fn starter_leather_is_rejected_by_every_tool_view() {
        let expected = Err(OrdinaryToolStatsError::StarterLeatherIsNotOrdinaryToolTier);
        assert_eq!(ordinary_sword_stats(EquipmentTier::StarterLeather), expected);
        assert_eq!(ordinary_pickaxe_stats(EquipmentTier::StarterLeather), expected);
        assert_eq!(
            ordinary_fishing_rod_stats(EquipmentTier::StarterLeather),
            expected
        );
    }
}
