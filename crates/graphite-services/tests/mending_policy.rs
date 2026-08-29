use graphite_services::{
    EquipmentSlot, MENDING_AUTOMATION_AEXP_PER_DURABILITY, MENDING_MANUAL_AEXP_PER_DURABILITY,
    MendingContext, MendingPolicyError, preview_mending,
};

#[test]
fn public_api_preserves_manual_and_automation_costs() {
    assert_eq!(MENDING_MANUAL_AEXP_PER_DURABILITY, 5);
    assert_eq!(MENDING_AUTOMATION_AEXP_PER_DURABILITY, 8);

    let manual = preview_mending(EquipmentSlot::Sword, MendingContext::Manual, 20, false).unwrap();
    assert_eq!(manual.activity_xp_per_durability, 5);
    assert_eq!(manual.activity_xp_cost, 100);
    assert!(!manual.resolves_before_machine_experience_pool);

    let automation = preview_mending(
        EquipmentSlot::FishingRod,
        MendingContext::Automation,
        20,
        false,
    )
    .unwrap();
    assert_eq!(automation.activity_xp_per_durability, 8);
    assert_eq!(automation.activity_xp_cost, 160);
    assert!(automation.resolves_before_machine_experience_pool);
}

#[test]
fn public_api_rejects_automation_for_sword_and_armor() {
    for slot in [
        EquipmentSlot::Sword,
        EquipmentSlot::Helmet,
        EquipmentSlot::Chestplate,
        EquipmentSlot::Leggings,
        EquipmentSlot::Boots,
    ] {
        assert_eq!(
            preview_mending(slot, MendingContext::Automation, 1, false),
            Err(MendingPolicyError::AutomationUnsupportedForSlot(slot))
        );
    }
}

#[test]
fn public_api_blocks_pickaxe_mending_during_nuke_burnout() {
    for context in [MendingContext::Manual, MendingContext::Automation] {
        assert_eq!(
            preview_mending(EquipmentSlot::Pickaxe, context, 100, true),
            Err(MendingPolicyError::NukeBurnoutBlocksRestoration)
        );
    }
}

#[test]
fn public_api_checks_negative_and_overflowing_cost_inputs() {
    assert_eq!(
        preview_mending(EquipmentSlot::Helmet, MendingContext::Manual, -1, false),
        Err(MendingPolicyError::NegativeDurabilityToRestore)
    );
    assert_eq!(
        preview_mending(
            EquipmentSlot::Pickaxe,
            MendingContext::Automation,
            i64::MAX,
            false,
        ),
        Err(MendingPolicyError::ArithmeticOverflow)
    );
}
