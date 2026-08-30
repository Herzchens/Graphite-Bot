use graphite_services::{
    ActiveFishingBaitInventory, FishingBait, FishingBaitCastConsumptionAction,
    FishingBaitCastConsumptionError, plan_fishing_bait_cast_consumption,
};

#[test]
fn public_api_consumes_one_unit_per_available_active_bait() {
    let plan = plan_fishing_bait_cast_consumption(
        None,
        &[
            ActiveFishingBaitInventory {
                bait: FishingBait::School,
                available_units: 3,
            },
            ActiveFishingBaitInventory {
                bait: FishingBait::Quality,
                available_units: 1,
            },
            ActiveFishingBaitInventory {
                bait: FishingBait::Sturdy,
                available_units: 9,
            },
        ],
    )
    .unwrap();

    assert_eq!(plan.active_category_capacity, 3);
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
                2,
                FishingBaitCastConsumptionAction::ConsumeForCast,
            ),
            (
                FishingBait::Quality,
                1,
                0,
                FishingBaitCastConsumptionAction::ConsumeForCast,
            ),
            (
                FishingBait::Sturdy,
                1,
                8,
                FishingBaitCastConsumptionAction::ConsumeForCast,
            ),
        ]
    );
}

#[test]
fn public_api_auto_detaches_only_missing_active_baits() {
    let plan = plan_fishing_bait_cast_consumption(
        None,
        &[
            ActiveFishingBaitInventory {
                bait: FishingBait::Rare,
                available_units: 0,
            },
            ActiveFishingBaitInventory {
                bait: FishingBait::Treasure,
                available_units: 2,
            },
        ],
    )
    .unwrap();

    assert_eq!(plan.consumed_bait_categories, 1);
    assert_eq!(plan.auto_detached_bait_categories, 1);
    assert_eq!(
        plan.entries[0].action,
        FishingBaitCastConsumptionAction::AutoDetachMissingUnit
    );
    assert_eq!(plan.entries[0].units_consumed, 0);
    assert_eq!(
        plan.entries[1].action,
        FishingBaitCastConsumptionAction::ConsumeForCast
    );
}

#[test]
fn public_api_reuses_bait_rack_capacity_and_rejects_bad_state() {
    let four = [
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
        plan_fishing_bait_cast_consumption(None, &four),
        Err(
            FishingBaitCastConsumptionError::ActiveCategoryCapacityExceeded {
                selected: 4,
                capacity: 3,
            }
        )
    );
    assert!(plan_fishing_bait_cast_consumption(Some(1), &four).is_ok());

    assert_eq!(
        plan_fishing_bait_cast_consumption(
            None,
            &[
                ActiveFishingBaitInventory {
                    bait: FishingBait::School,
                    available_units: 1,
                },
                ActiveFishingBaitInventory {
                    bait: FishingBait::School,
                    available_units: 1,
                },
            ],
        ),
        Err(FishingBaitCastConsumptionError::DuplicateActiveBait(
            FishingBait::School
        ))
    );
    assert_eq!(
        plan_fishing_bait_cast_consumption(
            None,
            &[ActiveFishingBaitInventory {
                bait: FishingBait::Quality,
                available_units: -1,
            }],
        ),
        Err(FishingBaitCastConsumptionError::NegativeAvailableUnits {
            bait: FishingBait::Quality,
            available_units: -1,
        })
    );
}
