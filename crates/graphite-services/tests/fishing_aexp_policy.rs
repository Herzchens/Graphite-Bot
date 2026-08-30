use graphite_services::{
    FishingRarity, MANUAL_FISHING_BASE_JUNK_AEXP, MANUAL_FISHING_BASE_MULTI_TREASURE_AEXP_CAP,
    MANUAL_FISHING_BASE_TREASURE_AEXP, ManualFishingAexpError, ManualFishingAexpOutcome,
    manual_fishing_base_outcome_aexp, manual_fishing_base_treasure_cast_aexp,
};

#[test]
fn public_api_matches_every_frozen_single_result_base_aexp_value() {
    assert_eq!(MANUAL_FISHING_BASE_JUNK_AEXP, 2);
    assert_eq!(MANUAL_FISHING_BASE_TREASURE_AEXP, 5);
    assert_eq!(MANUAL_FISHING_BASE_MULTI_TREASURE_AEXP_CAP, 10);

    assert_eq!(
        manual_fishing_base_outcome_aexp(ManualFishingAexpOutcome::LandedJunk),
        Some(2)
    );
    assert_eq!(
        manual_fishing_base_outcome_aexp(ManualFishingAexpOutcome::LandedTreasure),
        Some(5)
    );

    for (rarity, expected_aexp) in [
        (FishingRarity::Common, 3),
        (FishingRarity::Uncommon, 4),
        (FishingRarity::Rare, 5),
        (FishingRarity::Epic, 7),
        (FishingRarity::Legendary, 10),
        (FishingRarity::Mythic, 14),
    ] {
        assert_eq!(
            manual_fishing_base_outcome_aexp(ManualFishingAexpOutcome::LandedFish(rarity)),
            Some(expected_aexp)
        );
    }
}

#[test]
fn public_api_returns_no_base_aexp_for_failed_fish_outcomes() {
    assert_eq!(
        manual_fishing_base_outcome_aexp(ManualFishingAexpOutcome::FishEscaped),
        None
    );
    assert_eq!(
        manual_fishing_base_outcome_aexp(ManualFishingAexpOutcome::LineBreak),
        None
    );
}

#[test]
fn public_api_caps_base_multi_treasure_and_rejects_noncanonical_counts() {
    assert_eq!(manual_fishing_base_treasure_cast_aexp(1), Ok(5));
    assert_eq!(manual_fishing_base_treasure_cast_aexp(2), Ok(10));
    assert_eq!(manual_fishing_base_treasure_cast_aexp(3), Ok(10));

    for count in [0, 4, u8::MAX] {
        assert_eq!(
            manual_fishing_base_treasure_cast_aexp(count),
            Err(ManualFishingAexpError::LandedTreasureCountOutOfRange(
                count
            ))
        );
    }
}
