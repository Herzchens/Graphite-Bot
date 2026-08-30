use serde::Serialize;
use thiserror::Error;

use crate::fishing_bait::{
    BaitRackPolicyError, FishingBait, FishingBaitCategory, bait_rack_capacity_policy,
    fishing_bait_policy,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ActiveFishingBaitInventory {
    pub bait: FishingBait,
    pub available_units: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FishingBaitCastConsumptionAction {
    ConsumeForCast,
    AutoDetachMissingUnit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FishingBaitCastConsumptionEntry {
    pub bait: FishingBait,
    pub category: FishingBaitCategory,
    pub available_units_before: i64,
    pub units_consumed: u8,
    pub available_units_after: i64,
    pub action: FishingBaitCastConsumptionAction,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FishingBaitCastConsumptionPlan {
    pub bait_rack_level: Option<u8>,
    pub active_category_capacity: u8,
    pub active_baits_before_cast: u8,
    pub consumed_bait_categories: u8,
    pub auto_detached_bait_categories: u8,
    pub entries: Vec<FishingBaitCastConsumptionEntry>,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum FishingBaitCastConsumptionError {
    #[error(transparent)]
    BaitRack(#[from] BaitRackPolicyError),
    #[error("active bait selection has {selected} categories but the Rod permits only {capacity}")]
    ActiveCategoryCapacityExceeded { selected: usize, capacity: u8 },
    #[error("Fishing bait {0:?} appears more than once in the active selection")]
    DuplicateActiveBait(FishingBait),
    #[error("available bait units cannot be negative for {bait:?}; got {available_units}")]
    NegativeAvailableUnits {
        bait: FishingBait,
        available_units: i64,
    },
}

/// Plans the frozen per-cast consumption behavior for all currently active Fishing baits.
///
/// The active selection is validated against the Rod's canonical Bait Rack capacity. Each active
/// bait consumes exactly the `units_consumed_per_cast` value owned by [`fishing_bait_policy`], which
/// is currently one unit for every canonical bait category. An active bait with insufficient units
/// is not consumed and is instead marked [`FishingBaitCastConsumptionAction::AutoDetachMissingUnit`]
/// before the cast. This preserves the specification rule that missing bait auto-detaches rather
/// than blocking the entire Fishing action.
///
/// Multi Catch and Multi Treasure do not increase bait consumption, so candidate/result counts are
/// intentionally absent from this API. The plan also does not purchase bait, mutate inventory or
/// Rod configuration, automatically activate replacement categories, or decide whether a bait that
/// reaches zero *after* consuming its last available unit should be detached immediately. Those are
/// stateful lifecycle concerns outside this pure pre-settlement plan.
pub fn plan_fishing_bait_cast_consumption(
    bait_rack_level: Option<u8>,
    active_inventory: &[ActiveFishingBaitInventory],
) -> Result<FishingBaitCastConsumptionPlan, FishingBaitCastConsumptionError> {
    let capacity = bait_rack_capacity_policy(bait_rack_level)?;
    if active_inventory.len() > usize::from(capacity.active_bait_category_slots) {
        return Err(
            FishingBaitCastConsumptionError::ActiveCategoryCapacityExceeded {
                selected: active_inventory.len(),
                capacity: capacity.active_bait_category_slots,
            },
        );
    }

    for (index, row) in active_inventory.iter().enumerate() {
        if row.available_units < 0 {
            return Err(FishingBaitCastConsumptionError::NegativeAvailableUnits {
                bait: row.bait,
                available_units: row.available_units,
            });
        }
        if active_inventory[..index]
            .iter()
            .any(|prior| prior.bait == row.bait)
        {
            return Err(FishingBaitCastConsumptionError::DuplicateActiveBait(
                row.bait,
            ));
        }
    }

    let mut entries = Vec::with_capacity(active_inventory.len());
    let mut consumed_bait_categories = 0_u8;
    let mut auto_detached_bait_categories = 0_u8;

    for row in active_inventory {
        let policy = fishing_bait_policy(row.bait);
        let required_units = i64::from(policy.units_consumed_per_cast);
        let has_required_units = row.available_units >= required_units;

        let (units_consumed, available_units_after, action) = if has_required_units {
            consumed_bait_categories += 1;
            (
                policy.units_consumed_per_cast,
                row.available_units - required_units,
                FishingBaitCastConsumptionAction::ConsumeForCast,
            )
        } else {
            auto_detached_bait_categories += 1;
            (
                0,
                row.available_units,
                FishingBaitCastConsumptionAction::AutoDetachMissingUnit,
            )
        };

        entries.push(FishingBaitCastConsumptionEntry {
            bait: row.bait,
            category: policy.category,
            available_units_before: row.available_units,
            units_consumed,
            available_units_after,
            action,
        });
    }

    // The capacity check above proves len <= 6, so this cast is exact and cannot truncate.
    let active_baits_before_cast = active_inventory.len() as u8;

    Ok(FishingBaitCastConsumptionPlan {
        bait_rack_level,
        active_category_capacity: capacity.active_bait_category_slots,
        active_baits_before_cast,
        consumed_bait_categories,
        auto_detached_bait_categories,
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_optional_bait_selection_is_valid() {
        let plan = plan_fishing_bait_cast_consumption(None, &[]).unwrap();
        assert_eq!(plan.active_category_capacity, 3);
        assert_eq!(plan.active_baits_before_cast, 0);
        assert_eq!(plan.consumed_bait_categories, 0);
        assert_eq!(plan.auto_detached_bait_categories, 0);
        assert!(plan.entries.is_empty());
    }

    #[test]
    fn each_available_active_bait_consumes_exactly_one_unit() {
        let input = [
            ActiveFishingBaitInventory {
                bait: FishingBait::School,
                available_units: 7,
            },
            ActiveFishingBaitInventory {
                bait: FishingBait::Quality,
                available_units: 1,
            },
            ActiveFishingBaitInventory {
                bait: FishingBait::Treasure,
                available_units: 2,
            },
        ];

        let plan = plan_fishing_bait_cast_consumption(None, &input).unwrap();
        assert_eq!(plan.active_baits_before_cast, 3);
        assert_eq!(plan.consumed_bait_categories, 3);
        assert_eq!(plan.auto_detached_bait_categories, 0);
        assert_eq!(
            plan.entries
                .iter()
                .map(|entry| (
                    entry.bait,
                    entry.units_consumed,
                    entry.available_units_after,
                    entry.action,
                ))
                .collect::<Vec<_>>(),
            vec![
                (
                    FishingBait::School,
                    1,
                    6,
                    FishingBaitCastConsumptionAction::ConsumeForCast,
                ),
                (
                    FishingBait::Quality,
                    1,
                    0,
                    FishingBaitCastConsumptionAction::ConsumeForCast,
                ),
                (
                    FishingBait::Treasure,
                    1,
                    1,
                    FishingBaitCastConsumptionAction::ConsumeForCast,
                ),
            ]
        );
    }

    #[test]
    fn missing_active_bait_auto_detaches_without_blocking_other_categories() {
        let input = [
            ActiveFishingBaitInventory {
                bait: FishingBait::School,
                available_units: 0,
            },
            ActiveFishingBaitInventory {
                bait: FishingBait::Sturdy,
                available_units: 4,
            },
        ];

        let plan = plan_fishing_bait_cast_consumption(None, &input).unwrap();
        assert_eq!(plan.consumed_bait_categories, 1);
        assert_eq!(plan.auto_detached_bait_categories, 1);
        assert_eq!(
            plan.entries[0],
            FishingBaitCastConsumptionEntry {
                bait: FishingBait::School,
                category: FishingBaitCategory::Quantity,
                available_units_before: 0,
                units_consumed: 0,
                available_units_after: 0,
                action: FishingBaitCastConsumptionAction::AutoDetachMissingUnit,
            }
        );
        assert_eq!(
            plan.entries[1].action,
            FishingBaitCastConsumptionAction::ConsumeForCast
        );
    }

    #[test]
    fn active_selection_must_fit_native_or_bait_rack_capacity() {
        let input = [
            ActiveFishingBaitInventory {
                bait: FishingBait::School,
                available_units: 1,
            },
            ActiveFishingBaitInventory {
                bait: FishingBait::Quality,
                available_units: 1,
            },
            ActiveFishingBaitInventory {
                bait: FishingBait::Rare,
                available_units: 1,
            },
            ActiveFishingBaitInventory {
                bait: FishingBait::Treasure,
                available_units: 1,
            },
        ];

        assert_eq!(
            plan_fishing_bait_cast_consumption(None, &input),
            Err(
                FishingBaitCastConsumptionError::ActiveCategoryCapacityExceeded {
                    selected: 4,
                    capacity: 3,
                }
            )
        );
        assert!(plan_fishing_bait_cast_consumption(Some(1), &input).is_ok());
    }

    #[test]
    fn duplicate_or_negative_authoritative_input_fails_closed() {
        let duplicate = [
            ActiveFishingBaitInventory {
                bait: FishingBait::Rare,
                available_units: 1,
            },
            ActiveFishingBaitInventory {
                bait: FishingBait::Rare,
                available_units: 2,
            },
        ];
        assert_eq!(
            plan_fishing_bait_cast_consumption(None, &duplicate),
            Err(FishingBaitCastConsumptionError::DuplicateActiveBait(
                FishingBait::Rare
            ))
        );

        assert_eq!(
            plan_fishing_bait_cast_consumption(
                None,
                &[ActiveFishingBaitInventory {
                    bait: FishingBait::Sturdy,
                    available_units: -1,
                }],
            ),
            Err(FishingBaitCastConsumptionError::NegativeAvailableUnits {
                bait: FishingBait::Sturdy,
                available_units: -1,
            })
        );
    }
}
