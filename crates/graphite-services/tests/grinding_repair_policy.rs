use graphite_services::{
    GRINDING_MAX_LEVEL, GRINDING_MAX_REDUCTION_BPS, GRINDING_REDUCTION_BPS_PER_LEVEL,
    GrindingPolicyError, REPAIR_TIME_REDUCTION_BUCKET_CAP_BPS, grinding_repair_modifier,
};

#[test]
fn public_api_preserves_grinding_level_scaling_and_caps() {
    assert_eq!(GRINDING_MAX_LEVEL, 10);
    assert_eq!(GRINDING_REDUCTION_BPS_PER_LEVEL, 300);
    assert_eq!(GRINDING_MAX_REDUCTION_BPS, 3_000);
    assert_eq!(REPAIR_TIME_REDUCTION_BUCKET_CAP_BPS, 3_500);

    for (level, expected_bps) in [(1, 300), (5, 1_500), (10, 3_000)] {
        let modifier = grinding_repair_modifier(level).unwrap();
        assert_eq!(modifier.level, level);
        assert_eq!(modifier.material_reduction_basis_points, expected_bps);
        assert_eq!(modifier.repair_time_reduction_basis_points, expected_bps);
        assert_eq!(modifier.repair_time_bucket_cap_basis_points, 3_500);
        assert!(modifier.money_fee_unchanged);
        assert!(modifier.applies_after_tier_repair_recipe);
    }
}

#[test]
fn public_api_rejects_levels_outside_i_through_x() {
    for level in [0, 11, u8::MAX] {
        assert_eq!(
            grinding_repair_modifier(level),
            Err(GrindingPolicyError::LevelOutOfRange(level))
        );
    }
}
