use graphite_services::{EquipmentTier, preview_soulbind_binding, preview_soulbind_top_up};

#[test]
fn monotonic_top_up_paths_match_bind_late_across_rounding_boundaries() {
    for start in 0_i64..=128 {
        let initial =
            preview_soulbind_binding(EquipmentTier::Netherite, true, 1, start).unwrap();
        let mut cumulative_charge = initial.initial_protection_charge;
        let mut previous = start;

        for appraisal in (start + 1)..=512 {
            let top_up = preview_soulbind_top_up(previous, appraisal).unwrap();
            assert_eq!(top_up.positive_appraisal_delta, appraisal - previous);
            assert!(top_up.money_charge >= 0);
            cumulative_charge = cumulative_charge
                .checked_add(top_up.money_charge)
                .expect("bounded regression charge must remain representable");

            let bind_late =
                preview_soulbind_binding(EquipmentTier::Netherite, true, 1, appraisal).unwrap();
            assert_eq!(
                cumulative_charge, bind_late.initial_protection_charge,
                "monotonic path from {start} to {appraisal} diverged from bind-late liability"
            );
            previous = appraisal;
        }
    }
}

#[test]
fn direct_and_incremental_top_ups_agree_near_i64_max() {
    let start = i64::MAX - 32;
    let initial = preview_soulbind_binding(EquipmentTier::Graphite, true, 1, start).unwrap();
    let direct = preview_soulbind_top_up(start, i64::MAX).unwrap();
    let final_binding =
        preview_soulbind_binding(EquipmentTier::Graphite, true, 1, i64::MAX).unwrap();

    assert_eq!(
        initial.initial_protection_charge
            .checked_add(direct.money_charge)
            .unwrap(),
        final_binding.initial_protection_charge
    );

    let mut incremental = initial.initial_protection_charge;
    let mut previous = start;
    for appraisal in (start + 1)..=i64::MAX {
        let top_up = preview_soulbind_top_up(previous, appraisal).unwrap();
        incremental = incremental.checked_add(top_up.money_charge).unwrap();
        previous = appraisal;
    }
    assert_eq!(incremental, final_binding.initial_protection_charge);
}
