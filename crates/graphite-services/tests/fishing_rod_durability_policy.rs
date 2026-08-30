use graphite_services::{
    FishingRodDurabilityConsequence, FishingRodDurabilityPolicyError, FishingRodDurabilityPreview,
    FishingRodDurabilityResolution, NORMAL_ROD_DURABILITY_PER_COMPLETED_CAST_ATTEMPT,
    preview_fishing_rod_durability,
};

#[test]
fn public_api_applies_one_ordinary_event_per_completed_cast_not_per_fish() {
    assert_eq!(NORMAL_ROD_DURABILITY_PER_COMPLETED_CAST_ATTEMPT, 1);
    assert_eq!(
        preview_fishing_rod_durability(
            5,
            100,
            true,
            FishingRodDurabilityResolution::CompletedCastAttempt {
                ordinary_event_prevented_by_unbreaking: false,
            },
        ),
        Ok(FishingRodDurabilityPreview {
            current_durability: 5,
            max_durability: 100,
            resulting_durability: 4,
            consequence: FishingRodDurabilityConsequence::OrdinaryWearApplied,
        })
    );
}

#[test]
fn public_api_keeps_unbreaking_as_an_already_resolved_ordinary_event_result() {
    let preview = preview_fishing_rod_durability(
        5,
        100,
        true,
        FishingRodDurabilityResolution::CompletedCastAttempt {
            ordinary_event_prevented_by_unbreaking: true,
        },
    )
    .unwrap();

    assert_eq!(preview.resulting_durability, 5);
    assert_eq!(
        preview.consequence,
        FishingRodDurabilityConsequence::OrdinaryWearPreventedByUnbreaking
    );
}

#[test]
fn public_api_makes_line_break_an_unpreventable_zero_durability_override() {
    for current in [1, 550, 11_000] {
        assert_eq!(
            preview_fishing_rod_durability(
                current,
                11_000,
                true,
                FishingRodDurabilityResolution::LineBreak,
            )
            .unwrap()
            .resulting_durability,
            0
        );
    }
}

#[test]
fn public_api_fails_closed_for_nonordinary_and_malformed_state() {
    assert_eq!(
        preview_fishing_rod_durability(100, 100, false, FishingRodDurabilityResolution::LineBreak,),
        Err(FishingRodDurabilityPolicyError::NotOrdinaryFishingRod)
    );
    assert_eq!(
        preview_fishing_rod_durability(0, 100, true, FishingRodDurabilityResolution::LineBreak,),
        Err(FishingRodDurabilityPolicyError::InvalidCurrentDurability)
    );
    assert_eq!(
        preview_fishing_rod_durability(101, 100, true, FishingRodDurabilityResolution::LineBreak,),
        Err(FishingRodDurabilityPolicyError::InvalidCurrentDurability)
    );
}
