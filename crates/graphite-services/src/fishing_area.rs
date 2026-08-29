use serde::Serialize;
use thiserror::Error;

use crate::EquipmentTier;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FishingArea {
    StarterPool,
    River,
    Lake,
    Coast,
    DeepSea,
    Abyss,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FishingRodForUnlock {
    StarterBasic,
    Ordinary(EquipmentTier),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FishingAreaFirstUnlockPolicy {
    pub area: FishingArea,
    pub minimum_account_level: Option<u32>,
    pub minimum_rebirth: Option<u32>,
    pub minimum_ordinary_rod_tier: Option<EquipmentTier>,
    pub starter_basic_allowed: bool,
    pub gold_counts_as_side_grade: bool,
    pub permanent_once_unlocked: bool,
    pub rebirth_never_relocks: bool,
    pub renewable_without_depletion: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FishingAreaFirstUnlockPreview {
    pub policy: FishingAreaFirstUnlockPolicy,
    pub account_level_met: bool,
    pub rebirth_met: bool,
    pub rod_requirement_met: bool,
    pub eligible_for_first_unlock: bool,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum FishingAreaPolicyError {
    #[error("Starter Leather is not a Fishing Rod tier")]
    StarterLeatherIsNotRodTier,
}

#[must_use]
pub const fn fishing_area_first_unlock_policy(area: FishingArea) -> FishingAreaFirstUnlockPolicy {
    match area {
        FishingArea::StarterPool => FishingAreaFirstUnlockPolicy {
            area,
            minimum_account_level: None,
            minimum_rebirth: None,
            minimum_ordinary_rod_tier: None,
            starter_basic_allowed: true,
            gold_counts_as_side_grade: false,
            permanent_once_unlocked: true,
            rebirth_never_relocks: true,
            renewable_without_depletion: true,
        },
        FishingArea::River => FishingAreaFirstUnlockPolicy {
            area,
            minimum_account_level: Some(10),
            minimum_rebirth: None,
            minimum_ordinary_rod_tier: Some(EquipmentTier::Wood),
            starter_basic_allowed: false,
            gold_counts_as_side_grade: false,
            permanent_once_unlocked: true,
            rebirth_never_relocks: true,
            renewable_without_depletion: true,
        },
        FishingArea::Lake => FishingAreaFirstUnlockPolicy {
            area,
            minimum_account_level: Some(25),
            minimum_rebirth: None,
            minimum_ordinary_rod_tier: Some(EquipmentTier::Stone),
            starter_basic_allowed: false,
            gold_counts_as_side_grade: false,
            permanent_once_unlocked: true,
            rebirth_never_relocks: true,
            renewable_without_depletion: true,
        },
        FishingArea::Coast => FishingAreaFirstUnlockPolicy {
            area,
            minimum_account_level: Some(50),
            minimum_rebirth: None,
            minimum_ordinary_rod_tier: Some(EquipmentTier::Copper),
            starter_basic_allowed: false,
            gold_counts_as_side_grade: false,
            permanent_once_unlocked: true,
            rebirth_never_relocks: true,
            renewable_without_depletion: true,
        },
        FishingArea::DeepSea => FishingAreaFirstUnlockPolicy {
            area,
            minimum_account_level: Some(100),
            minimum_rebirth: None,
            minimum_ordinary_rod_tier: Some(EquipmentTier::Diamond),
            starter_basic_allowed: false,
            gold_counts_as_side_grade: true,
            permanent_once_unlocked: true,
            rebirth_never_relocks: true,
            renewable_without_depletion: true,
        },
        FishingArea::Abyss => FishingAreaFirstUnlockPolicy {
            area,
            minimum_account_level: None,
            minimum_rebirth: Some(1),
            minimum_ordinary_rod_tier: Some(EquipmentTier::Netherite),
            starter_basic_allowed: false,
            gold_counts_as_side_grade: false,
            permanent_once_unlocked: true,
            rebirth_never_relocks: true,
            renewable_without_depletion: true,
        },
    }
}

/// Previews whether the current progression/equipment snapshot satisfies the frozen requirements
/// for the *first* permanent unlock of one Fishing area.
///
/// This is deliberately not a live-cast authorization function. Once an area has been unlocked,
/// Rebirth never removes that persisted access. The future stateful owner must therefore load the
/// authoritative permanent-unlock state first and invoke this policy only when deciding whether a
/// new area should be unlocked. The Starter Basic Rod remains Pool-only independently of permanent
/// area progression.
///
/// `rod` must be derived from authoritative equipped Fishing-Rod state. This pure policy does not
/// establish equipment ownership or allow Discord/request data to become the source of truth.
///
/// Fishing areas are renewable access progression, not resource-exhaustion state: this policy never
/// creates or consumes Mining-style pressure/depletion capacity.
pub fn preview_first_fishing_area_unlock(
    area: FishingArea,
    account_level: u32,
    rebirth: u32,
    rod: FishingRodForUnlock,
) -> Result<FishingAreaFirstUnlockPreview, FishingAreaPolicyError> {
    let policy = fishing_area_first_unlock_policy(area);

    let account_level_met = policy
        .minimum_account_level
        .is_none_or(|minimum| account_level >= minimum);
    let rebirth_met = policy
        .minimum_rebirth
        .is_none_or(|minimum| rebirth >= minimum);
    let rod_requirement_met = rod_meets_first_unlock_requirement(area, rod)?;

    Ok(FishingAreaFirstUnlockPreview {
        policy,
        account_level_met,
        rebirth_met,
        rod_requirement_met,
        eligible_for_first_unlock: account_level_met && rebirth_met && rod_requirement_met,
    })
}

fn rod_meets_first_unlock_requirement(
    area: FishingArea,
    rod: FishingRodForUnlock,
) -> Result<bool, FishingAreaPolicyError> {
    match rod {
        FishingRodForUnlock::StarterBasic => Ok(area == FishingArea::StarterPool),
        FishingRodForUnlock::Ordinary(EquipmentTier::StarterLeather) => {
            Err(FishingAreaPolicyError::StarterLeatherIsNotRodTier)
        }
        FishingRodForUnlock::Ordinary(tier) => Ok(match tier {
            EquipmentTier::StarterLeather => false,
            EquipmentTier::Wood => matches!(area, FishingArea::StarterPool | FishingArea::River),
            EquipmentTier::Stone => matches!(
                area,
                FishingArea::StarterPool | FishingArea::River | FishingArea::Lake
            ),
            EquipmentTier::Copper | EquipmentTier::Iron => matches!(
                area,
                FishingArea::StarterPool
                    | FishingArea::River
                    | FishingArea::Lake
                    | FishingArea::Coast
            ),
            EquipmentTier::Gold | EquipmentTier::Diamond | EquipmentTier::Obsidian => {
                !matches!(area, FishingArea::Abyss)
            }
            EquipmentTier::Netherite | EquipmentTier::Graphite => true,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_first_unlock_table_is_frozen() {
        let cases = [
            (FishingArea::StarterPool, None, None, None, true, false),
            (
                FishingArea::River,
                Some(10),
                None,
                Some(EquipmentTier::Wood),
                false,
                false,
            ),
            (
                FishingArea::Lake,
                Some(25),
                None,
                Some(EquipmentTier::Stone),
                false,
                false,
            ),
            (
                FishingArea::Coast,
                Some(50),
                None,
                Some(EquipmentTier::Copper),
                false,
                false,
            ),
            (
                FishingArea::DeepSea,
                Some(100),
                None,
                Some(EquipmentTier::Diamond),
                false,
                true,
            ),
            (
                FishingArea::Abyss,
                None,
                Some(1),
                Some(EquipmentTier::Netherite),
                false,
                false,
            ),
        ];

        for (area, level, rebirth, minimum_rod, starter_allowed, gold_side_grade) in cases {
            let policy = fishing_area_first_unlock_policy(area);
            assert_eq!(policy.area, area);
            assert_eq!(policy.minimum_account_level, level);
            assert_eq!(policy.minimum_rebirth, rebirth);
            assert_eq!(policy.minimum_ordinary_rod_tier, minimum_rod);
            assert_eq!(policy.starter_basic_allowed, starter_allowed);
            assert_eq!(policy.gold_counts_as_side_grade, gold_side_grade);
            assert!(policy.permanent_once_unlocked);
            assert!(policy.rebirth_never_relocks);
            assert!(policy.renewable_without_depletion);
        }
    }

    #[test]
    fn progression_thresholds_are_exact() {
        let cases = [
            (FishingArea::River, 9, 0, EquipmentTier::Wood, false),
            (FishingArea::River, 10, 0, EquipmentTier::Wood, true),
            (FishingArea::Lake, 24, 0, EquipmentTier::Stone, false),
            (FishingArea::Lake, 25, 0, EquipmentTier::Stone, true),
            (FishingArea::Coast, 49, 0, EquipmentTier::Copper, false),
            (FishingArea::Coast, 50, 0, EquipmentTier::Copper, true),
            (FishingArea::DeepSea, 99, 0, EquipmentTier::Diamond, false),
            (FishingArea::DeepSea, 100, 0, EquipmentTier::Diamond, true),
            (FishingArea::Abyss, 0, 0, EquipmentTier::Netherite, false),
            (FishingArea::Abyss, 0, 1, EquipmentTier::Netherite, true),
        ];

        for (area, level, rebirth, tier, expected) in cases {
            let preview = preview_first_fishing_area_unlock(
                area,
                level,
                rebirth,
                FishingRodForUnlock::Ordinary(tier),
            )
            .unwrap();
            assert_eq!(preview.eligible_for_first_unlock, expected);
        }
    }

    #[test]
    fn starter_basic_is_pool_only() {
        for area in [
            FishingArea::StarterPool,
            FishingArea::River,
            FishingArea::Lake,
            FishingArea::Coast,
            FishingArea::DeepSea,
            FishingArea::Abyss,
        ] {
            let preview = preview_first_fishing_area_unlock(
                area,
                u32::MAX,
                u32::MAX,
                FishingRodForUnlock::StarterBasic,
            )
            .unwrap();
            assert_eq!(
                preview.rod_requirement_met,
                area == FishingArea::StarterPool
            );
        }
    }

    #[test]
    fn ordinary_rod_matrix_handles_gold_as_explicit_side_grade() {
        let areas = [
            FishingArea::StarterPool,
            FishingArea::River,
            FishingArea::Lake,
            FishingArea::Coast,
            FishingArea::DeepSea,
            FishingArea::Abyss,
        ];
        let cases = [
            (
                EquipmentTier::Wood,
                [true, true, false, false, false, false],
            ),
            (
                EquipmentTier::Stone,
                [true, true, true, false, false, false],
            ),
            (
                EquipmentTier::Copper,
                [true, true, true, true, false, false],
            ),
            (EquipmentTier::Gold, [true, true, true, true, true, false]),
            (EquipmentTier::Iron, [true, true, true, true, false, false]),
            (
                EquipmentTier::Diamond,
                [true, true, true, true, true, false],
            ),
            (
                EquipmentTier::Obsidian,
                [true, true, true, true, true, false],
            ),
            (
                EquipmentTier::Netherite,
                [true, true, true, true, true, true],
            ),
            (
                EquipmentTier::Graphite,
                [true, true, true, true, true, true],
            ),
        ];

        for (tier, expected) in cases {
            for (index, area) in areas.into_iter().enumerate() {
                let preview = preview_first_fishing_area_unlock(
                    area,
                    u32::MAX,
                    u32::MAX,
                    FishingRodForUnlock::Ordinary(tier),
                )
                .unwrap();
                assert_eq!(preview.rod_requirement_met, expected[index]);
            }
        }
    }

    #[test]
    fn starter_leather_fails_closed_as_non_rod_tier() {
        assert_eq!(
            preview_first_fishing_area_unlock(
                FishingArea::StarterPool,
                0,
                0,
                FishingRodForUnlock::Ordinary(EquipmentTier::StarterLeather),
            ),
            Err(FishingAreaPolicyError::StarterLeatherIsNotRodTier)
        );
    }
}
