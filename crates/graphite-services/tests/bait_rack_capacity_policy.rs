use graphite_services::{
    BAIT_RACK_MAX_BOOK_LEVEL, BAIT_RACK_MAX_LEVEL, BaitRackPolicyError,
    MAX_ACTIVE_BAIT_CATEGORY_SLOTS, NATIVE_ACTIVE_BAIT_CATEGORY_SLOTS, bait_rack_capacity_policy,
};

#[test]
fn public_api_preserves_native_and_level_scaled_bait_capacity() {
    let absent = bait_rack_capacity_policy(None).unwrap();
    assert_eq!(NATIVE_ACTIVE_BAIT_CATEGORY_SLOTS, 3);
    assert_eq!(MAX_ACTIVE_BAIT_CATEGORY_SLOTS, 6);
    assert_eq!(absent.active_bait_category_slots, 3);
    assert!(!absent.occupies_one_normal_rod_enchant_slot);

    for (level, expected_capacity) in [(1, 4), (2, 5), (3, 6)] {
        let policy = bait_rack_capacity_policy(Some(level)).unwrap();
        assert_eq!(policy.bait_rack_level, Some(level));
        assert_eq!(policy.additional_active_bait_category_slots, level);
        assert_eq!(policy.active_bait_category_slots, expected_capacity);
        assert_eq!(policy.max_active_bait_category_slots, 6);
        assert!(policy.rod_only);
        assert!(policy.occupies_one_normal_rod_enchant_slot);
    }
}

#[test]
fn public_api_uses_one_max_level_authority_for_effect_and_shop_ceiling() {
    assert_eq!(BAIT_RACK_MAX_LEVEL, 3);
    assert_eq!(BAIT_RACK_MAX_BOOK_LEVEL, BAIT_RACK_MAX_LEVEL);
}

#[test]
fn public_api_distinguishes_absence_from_malformed_present_levels() {
    assert!(bait_rack_capacity_policy(None).is_ok());

    for level in [0, 4, u8::MAX] {
        assert_eq!(
            bait_rack_capacity_policy(Some(level)),
            Err(BaitRackPolicyError::LevelOutOfRange(level))
        );
    }
}
