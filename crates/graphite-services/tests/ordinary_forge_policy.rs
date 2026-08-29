use graphite_services::{
    EquipmentMaterial, EquipmentSlot, EquipmentTier, ForgePostConfirmCancellation,
    FreshForgeOutcomePolicy, FreshForgeOutputLocation, FreshOrdinaryForgePolicyError,
    preview_fresh_ordinary_forge,
};

#[test]
fn public_preview_composes_standard_appraisal_and_frozen_forge_recipe() {
    let preview =
        preview_fresh_ordinary_forge(EquipmentTier::Obsidian, EquipmentSlot::Chestplate).unwrap();

    assert_eq!(preview.base_appraisal.value, 1_181_300);
    assert_eq!(preview.primary_material, EquipmentMaterial::Obsidian);
    assert_eq!(preview.primary_material_units, 8);
    assert_eq!(preview.auxiliary_wood_logs, 0);
    assert_eq!(preview.money_cost, 95_000);
    assert_eq!(preview.activity_xp_cost, 1_800);
    assert_eq!(preview.duration_seconds, 30 * 60);
    assert_eq!(preview.outcome, FreshForgeOutcomePolicy::Guaranteed);
    assert_eq!(
        preview.cancellation_after_confirm,
        ForgePostConfirmCancellation::Unspecified
    );
    assert_eq!(
        preview.output_location,
        FreshForgeOutputLocation::ToolLocker
    );
    assert_eq!(preview.output_upgrade_level, 0);
    assert!(preview.requires_new_positive_creation_roll);
    assert!(!preview.npc_resale_path);
}

#[test]
fn public_preview_keeps_gold_as_tool_only_side_grade() {
    let rod = preview_fresh_ordinary_forge(EquipmentTier::Gold, EquipmentSlot::FishingRod).unwrap();
    assert_eq!(rod.primary_material, EquipmentMaterial::GoldIngot);
    assert_eq!(rod.primary_material_units, 2);
    assert_eq!(rod.auxiliary_wood_logs, 1);
    assert_eq!(rod.money_cost, 9_000);

    assert_eq!(
        preview_fresh_ordinary_forge(EquipmentTier::Gold, EquipmentSlot::Helmet),
        Err(FreshOrdinaryForgePolicyError::UnsupportedGoldSlot(
            EquipmentSlot::Helmet
        ))
    );
}

#[test]
fn public_preview_rejects_promotion_only_tiers() {
    for tier in [EquipmentTier::Netherite, EquipmentTier::Graphite] {
        assert_eq!(
            preview_fresh_ordinary_forge(tier, EquipmentSlot::Sword),
            Err(FreshOrdinaryForgePolicyError::UnsupportedFreshForgeTier(
                tier
            ))
        );
    }
}
