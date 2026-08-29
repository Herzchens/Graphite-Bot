use graphite_services::{
    CreationRoll, EmbeddedEnchantAppraisalInput, EnchantAppraisalClass, EquipmentSlot,
    EquipmentTier, base_equipment_appraisal, compose_canonical_equipment_appraisal,
    embedded_enchant_value,
};

#[test]
fn public_kernels_compose_into_canonical_appraisals() {
    let base = base_equipment_appraisal(EquipmentTier::Iron, EquipmentSlot::Pickaxe, None).unwrap();
    let embedded = embedded_enchant_value(&[
        EmbeddedEnchantAppraisalInput {
            class: EnchantAppraisalClass::ShopCommon,
            level: 2,
        },
        EmbeddedEnchantAppraisalInput {
            class: EnchantAppraisalClass::Mending,
            level: 1,
        },
        EmbeddedEnchantAppraisalInput {
            class: EnchantAppraisalClass::SpecialRare,
            level: 3,
        },
    ])
    .unwrap();
    let appraisal =
        compose_canonical_equipment_appraisal(base, CreationRoll::new(1, 2).unwrap(), 5, embedded)
            .unwrap();

    assert_eq!(appraisal.base.value, 77_500);
    assert_eq!(appraisal.creation_roll.numerator(), 1);
    assert_eq!(appraisal.creation_roll.denominator(), 2);
    assert_eq!(appraisal.upgrade_level, 5);
    assert_eq!(appraisal.embedded_enchant_value, 1_921_500);
    assert_eq!(appraisal.recraft_appraisal, 87_978);
    assert_eq!(appraisal.enhanced_canonical_appraisal, 2_009_478);
}

#[test]
fn override_and_half_up_boundary_remain_exact_through_public_api() {
    let base = base_equipment_appraisal(
        EquipmentTier::StarterLeather,
        EquipmentSlot::Helmet,
        Some(50),
    )
    .unwrap();
    let appraisal =
        compose_canonical_equipment_appraisal(base, CreationRoll::new(50, 100).unwrap(), 0, 0)
            .unwrap();

    assert_eq!(appraisal.creation_roll.numerator(), 1);
    assert_eq!(appraisal.creation_roll.denominator(), 2);
    assert_eq!(appraisal.recraft_appraisal, 52);
    assert_eq!(appraisal.enhanced_canonical_appraisal, 52);
}
