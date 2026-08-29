use graphite_services::{
    BaseEquipmentAppraisalSource, EquipmentMaterial, EquipmentSlot, EquipmentTier,
    UpgradeAttemptCostError, preview_upgrade_attempt_resource_cost,
};

#[test]
fn public_api_preserves_frozen_table_and_continuation_costs() {
    let plus_one = preview_upgrade_attempt_resource_cost(
        EquipmentTier::Graphite,
        EquipmentSlot::Chestplate,
        None,
        1,
    )
    .unwrap();
    assert_eq!(plus_one.base_appraisal.value, 10_867_500);
    assert_eq!(
        plus_one.base_appraisal.source,
        BaseEquipmentAppraisalSource::StandardTable
    );
    assert_eq!(plus_one.money_cost, 54_300);
    assert_eq!(
        plus_one.reinforcement_material,
        EquipmentMaterial::GraphiteLayer
    );
    assert_eq!(plus_one.reinforcement_units, 1);

    let plus_twenty_three = preview_upgrade_attempt_resource_cost(
        EquipmentTier::Diamond,
        EquipmentSlot::Sword,
        Some(100_000),
        23,
    )
    .unwrap();
    assert_eq!(plus_twenty_three.money_cost, 492_100);
    assert_eq!(
        plus_twenty_three.reinforcement_material,
        EquipmentMaterial::Diamond
    );
    assert_eq!(plus_twenty_three.reinforcement_units, 3);
    assert!(plus_twenty_three.committed_failure_consumes_money_and_material);
    assert!(plus_twenty_three.downgrade_modifiers_do_not_change_base_attempt_cost);
}

#[test]
fn public_api_uses_definition_override_without_recursive_enhanced_appraisal() {
    let preview = preview_upgrade_attempt_resource_cost(
        EquipmentTier::Diamond,
        EquipmentSlot::Sword,
        Some(123_456),
        10,
    )
    .unwrap();

    assert_eq!(preview.base_appraisal.value, 123_456);
    assert_eq!(
        preview.base_appraisal.source,
        BaseEquipmentAppraisalSource::DefinitionOverride
    );
    assert_eq!(preview.money_cost, 5_600);
}

#[test]
fn public_api_fails_closed_for_non_upgradeable_or_nonexistent_equipment() {
    assert_eq!(
        preview_upgrade_attempt_resource_cost(
            EquipmentTier::StarterLeather,
            EquipmentSlot::Helmet,
            Some(5_000),
            1,
        ),
        Err(UpgradeAttemptCostError::StarterEquipmentNotUpgradeable)
    );
    assert_eq!(
        preview_upgrade_attempt_resource_cost(
            EquipmentTier::Gold,
            EquipmentSlot::Chestplate,
            None,
            1,
        ),
        Err(UpgradeAttemptCostError::UnsupportedGoldSlot(
            EquipmentSlot::Chestplate
        ))
    );
    assert_eq!(
        preview_upgrade_attempt_resource_cost(EquipmentTier::Wood, EquipmentSlot::Sword, None, 0,),
        Err(UpgradeAttemptCostError::TargetLevelZero)
    );
}

#[test]
fn public_api_bounds_extreme_continuation_without_changing_unlimited_progression_semantics() {
    assert_eq!(
        preview_upgrade_attempt_resource_cost(
            EquipmentTier::Wood,
            EquipmentSlot::Sword,
            Some(1),
            u64::MAX,
        ),
        Err(UpgradeAttemptCostError::ArithmeticOverflow)
    );
}
