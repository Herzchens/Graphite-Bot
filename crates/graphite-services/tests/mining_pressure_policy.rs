use graphite_services::{
    MINING_PRESSURE_APPRAISAL_PER_UNIT, MINING_PRESSURE_MAX_NON_PHYSICAL_OUTPUT_UNITS,
    MINING_PRESSURE_QUARTERS_PER_UNIT, MiningDepth, MiningPressureContribution,
    MiningPressurePolicyError, additional_non_physical_output_pressure_contribution,
    fortune_extra_material_pressure_contribution, mining_pressure_increment_ratio,
    nuke_blast_block_pressure_contribution, physical_block_pressure_contribution,
    trench_extra_block_pressure_contribution,
};

#[test]
fn public_pressure_sources_preserve_the_frozen_exact_units() {
    assert_eq!(MINING_PRESSURE_QUARTERS_PER_UNIT, 4);
    assert_eq!(MINING_PRESSURE_APPRAISAL_PER_UNIT, 1_000);
    assert_eq!(MINING_PRESSURE_MAX_NON_PHYSICAL_OUTPUT_UNITS, 25);

    assert_eq!(physical_block_pressure_contribution(7).quarter_units(), 28);
    assert_eq!(
        fortune_extra_material_pressure_contribution(7).quarter_units(),
        7
    );
    assert_eq!(
        trench_extra_block_pressure_contribution(7).quarter_units(),
        28
    );
    assert_eq!(
        nuke_blast_block_pressure_contribution(7).quarter_units(),
        28
    );

    let combined = physical_block_pressure_contribution(2)
        .checked_add(fortune_extra_material_pressure_contribution(3))
        .unwrap();
    assert_eq!(combined.quarter_units(), 11);
    assert_eq!(combined.pressure_unit_denominator(), 4);
}

#[test]
fn public_non_physical_output_charge_keeps_absence_floor_ceiling_and_cap_distinct() {
    let cases = [
        (None, 0),
        (Some(0), 4),
        (Some(1), 4),
        (Some(999), 4),
        (Some(1_000), 4),
        (Some(1_001), 8),
        (Some(24_000), 96),
        (Some(24_001), 100),
        (Some(25_000), 100),
        (Some(25_001), 100),
        (Some(i64::MAX), 100),
    ];

    for (appraisal, expected_quarters) in cases {
        assert_eq!(
            additional_non_physical_output_pressure_contribution(appraisal)
                .unwrap()
                .quarter_units(),
            expected_quarters
        );
    }
    assert_eq!(
        additional_non_physical_output_pressure_contribution(Some(-1)),
        Err(MiningPressurePolicyError::NegativeCanonicalAppraisal)
    );
}

#[test]
fn public_depth_increment_is_an_exact_reduced_ratio_without_state_rounding() {
    let surface = mining_pressure_increment_ratio(
        MiningDepth::Surface,
        physical_block_pressure_contribution(1),
    );
    assert_eq!((surface.numerator(), surface.denominator()), (1, 3_800));

    let shallow = mining_pressure_increment_ratio(
        MiningDepth::Shallow,
        fortune_extra_material_pressure_contribution(1),
    );
    assert_eq!((shallow.numerator(), shallow.denominator()), (1, 20_800));

    let abyssal = mining_pressure_increment_ratio(
        MiningDepth::AbyssalDepths,
        additional_non_physical_output_pressure_contribution(Some(i64::MAX)).unwrap(),
    );
    assert_eq!((abyssal.numerator(), abyssal.denominator()), (1, 1_640));

    let zero = mining_pressure_increment_ratio(
        MiningDepth::AbyssalDepths,
        MiningPressureContribution::ZERO,
    );
    assert_eq!((zero.numerator(), zero.denominator()), (0, 1));
}
