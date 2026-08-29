use graphite_services::{
    EquipmentTier, FishingArea, FishingAreaPolicyError, FishingRodForUnlock,
    fishing_area_first_unlock_policy, preview_first_fishing_area_unlock,
};

#[test]
fn public_api_freezes_area_progression_requirements() {
    let river = fishing_area_first_unlock_policy(FishingArea::River);
    assert_eq!(river.minimum_account_level, Some(10));
    assert_eq!(river.minimum_rebirth, None);
    assert_eq!(river.minimum_ordinary_rod_tier, Some(EquipmentTier::Wood));

    let deep_sea = fishing_area_first_unlock_policy(FishingArea::DeepSea);
    assert_eq!(deep_sea.minimum_account_level, Some(100));
    assert_eq!(
        deep_sea.minimum_ordinary_rod_tier,
        Some(EquipmentTier::Diamond)
    );
    assert!(deep_sea.gold_counts_as_side_grade);

    let abyss = fishing_area_first_unlock_policy(FishingArea::Abyss);
    assert_eq!(abyss.minimum_account_level, None);
    assert_eq!(abyss.minimum_rebirth, Some(1));
    assert_eq!(
        abyss.minimum_ordinary_rod_tier,
        Some(EquipmentTier::Netherite)
    );
    assert!(!abyss.gold_counts_as_side_grade);

    for area in [
        FishingArea::StarterPool,
        FishingArea::River,
        FishingArea::Lake,
        FishingArea::Coast,
        FishingArea::DeepSea,
        FishingArea::Abyss,
    ] {
        let policy = fishing_area_first_unlock_policy(area);
        assert!(policy.permanent_once_unlocked);
        assert!(policy.rebirth_never_relocks);
        assert!(policy.renewable_without_depletion);
    }
}

#[test]
fn public_api_keeps_gold_as_deep_sea_side_grade_but_not_abyss_capable() {
    let deep_sea = preview_first_fishing_area_unlock(
        FishingArea::DeepSea,
        100,
        0,
        FishingRodForUnlock::Ordinary(EquipmentTier::Gold),
    )
    .unwrap();
    assert!(deep_sea.eligible_for_first_unlock);

    let abyss = preview_first_fishing_area_unlock(
        FishingArea::Abyss,
        u32::MAX,
        1,
        FishingRodForUnlock::Ordinary(EquipmentTier::Gold),
    )
    .unwrap();
    assert!(!abyss.rod_requirement_met);
    assert!(!abyss.eligible_for_first_unlock);
}

#[test]
fn public_api_keeps_starter_basic_pool_only() {
    let pool = preview_first_fishing_area_unlock(
        FishingArea::StarterPool,
        0,
        0,
        FishingRodForUnlock::StarterBasic,
    )
    .unwrap();
    assert!(pool.eligible_for_first_unlock);

    let river = preview_first_fishing_area_unlock(
        FishingArea::River,
        u32::MAX,
        u32::MAX,
        FishingRodForUnlock::StarterBasic,
    )
    .unwrap();
    assert!(!river.rod_requirement_met);
    assert!(!river.eligible_for_first_unlock);
}

#[test]
fn public_api_rejects_non_rod_equipment_tier() {
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
