use serde::Serialize;
use thiserror::Error;

use crate::{
    fishing_bait::FishingRarity, fishing_limits::MAX_FISH_PER_CAST,
    fishing_multi_treasure::MULTI_TREASURE_MAX_ITEMS,
};

pub const MANUAL_FISHING_BASE_JUNK_AEXP: i64 = 2;
pub const MANUAL_FISHING_BASE_TREASURE_AEXP: i64 = 5;
pub const MANUAL_FISHING_BASE_MULTI_TREASURE_AEXP_CAP: i64 = 10;
pub const MANUAL_FISHING_BASE_MULTI_CATCH_AEXP_CAP_MULTIPLIER: i64 = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ManualFishingAexpOutcome {
    LandedJunk,
    LandedTreasure,
    LandedFish(FishingRarity),
    FishEscaped,
    LineBreak,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ManualFishingAexpError {
    #[error("landed treasure count must be between 1 and {MULTI_TREASURE_MAX_ITEMS}; got {0}")]
    LandedTreasureCountOutOfRange(u8),
    #[error("landed fish count must be between 1 and {MAX_FISH_PER_CAST}; got {0}")]
    LandedFishCountOutOfRange(usize),
}

/// Resolves the frozen base Manual Fishing Activity EXP for one terminal single-result outcome.
#[must_use]
pub const fn manual_fishing_base_outcome_aexp(outcome: ManualFishingAexpOutcome) -> Option<i64> {
    match outcome {
        ManualFishingAexpOutcome::LandedJunk => Some(MANUAL_FISHING_BASE_JUNK_AEXP),
        ManualFishingAexpOutcome::LandedTreasure => Some(MANUAL_FISHING_BASE_TREASURE_AEXP),
        ManualFishingAexpOutcome::LandedFish(rarity) => Some(fish_rarity_base_aexp(rarity)),
        ManualFishingAexpOutcome::FishEscaped | ManualFishingAexpOutcome::LineBreak => None,
    }
}

/// Aggregates base AEXP for one landed Multi Catch fish result.
///
/// The first entry is the base fish selected by the cast branch. Every additional fish contributes
/// its own rarity AEXP, but the owner-approved heterogeneous cap is exactly three times the base
/// fish's single-fish AEXP. Additional fish rarity can therefore never raise its own cap basis.
pub fn manual_fishing_base_fish_cast_aexp(
    landed_fish_rarities: &[FishingRarity],
) -> Result<i64, ManualFishingAexpError> {
    if landed_fish_rarities.is_empty()
        || landed_fish_rarities.len() > usize::from(MAX_FISH_PER_CAST)
    {
        return Err(ManualFishingAexpError::LandedFishCountOutOfRange(
            landed_fish_rarities.len(),
        ));
    }

    let base_fish_aexp = fish_rarity_base_aexp(landed_fish_rarities[0]);
    let total = landed_fish_rarities
        .iter()
        .copied()
        .map(fish_rarity_base_aexp)
        .sum::<i64>();
    let cap = base_fish_aexp * MANUAL_FISHING_BASE_MULTI_CATCH_AEXP_CAP_MULTIPLIER;

    Ok(total.min(cap))
}

/// Applies the frozen base Treasure Activity EXP cap for one successfully landed Treasure cast.
pub fn manual_fishing_base_treasure_cast_aexp(
    landed_treasure_count: u8,
) -> Result<i64, ManualFishingAexpError> {
    if !(1..=MULTI_TREASURE_MAX_ITEMS).contains(&landed_treasure_count) {
        return Err(ManualFishingAexpError::LandedTreasureCountOutOfRange(
            landed_treasure_count,
        ));
    }

    Ok(
        (i64::from(landed_treasure_count) * MANUAL_FISHING_BASE_TREASURE_AEXP)
            .min(MANUAL_FISHING_BASE_MULTI_TREASURE_AEXP_CAP),
    )
}

#[must_use]
const fn fish_rarity_base_aexp(rarity: FishingRarity) -> i64 {
    match rarity {
        FishingRarity::Common => 3,
        FishingRarity::Uncommon => 4,
        FishingRarity::Rare => 5,
        FishingRarity::Epic => 7,
        FishingRarity::Legendary => 10,
        FishingRarity::Mythic => 14,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_result_table_matches_every_frozen_manual_fishing_base_value() {
        assert_eq!(
            manual_fishing_base_outcome_aexp(ManualFishingAexpOutcome::LandedJunk),
            Some(2)
        );
        assert_eq!(
            manual_fishing_base_outcome_aexp(ManualFishingAexpOutcome::LandedTreasure),
            Some(5)
        );

        let expected = [
            (FishingRarity::Common, 3),
            (FishingRarity::Uncommon, 4),
            (FishingRarity::Rare, 5),
            (FishingRarity::Epic, 7),
            (FishingRarity::Legendary, 10),
            (FishingRarity::Mythic, 14),
        ];
        for (rarity, aexp) in expected {
            assert_eq!(
                manual_fishing_base_outcome_aexp(ManualFishingAexpOutcome::LandedFish(rarity)),
                Some(aexp)
            );
        }
    }

    #[test]
    fn heterogeneous_multi_catch_uses_the_base_fish_cap_basis() {
        assert_eq!(
            manual_fishing_base_fish_cast_aexp(&[
                FishingRarity::Common,
                FishingRarity::Mythic,
                FishingRarity::Mythic,
            ]),
            Ok(9)
        );
        assert_eq!(
            manual_fishing_base_fish_cast_aexp(&[
                FishingRarity::Mythic,
                FishingRarity::Common,
                FishingRarity::Common,
                FishingRarity::Common,
                FishingRarity::Common,
            ]),
            Ok(26)
        );
    }

    #[test]
    fn failed_fish_outcomes_create_no_aexp_grant() {
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
    fn treasure_cast_base_aexp_caps_at_ten() {
        assert_eq!(manual_fishing_base_treasure_cast_aexp(1), Ok(5));
        assert_eq!(manual_fishing_base_treasure_cast_aexp(2), Ok(10));
        assert_eq!(
            manual_fishing_base_treasure_cast_aexp(MULTI_TREASURE_MAX_ITEMS),
            Ok(10)
        );
    }

    #[test]
    fn noncanonical_counts_fail_closed() {
        for count in [0, MULTI_TREASURE_MAX_ITEMS + 1, u8::MAX] {
            assert_eq!(
                manual_fishing_base_treasure_cast_aexp(count),
                Err(ManualFishingAexpError::LandedTreasureCountOutOfRange(count))
            );
        }
        assert_eq!(
            manual_fishing_base_fish_cast_aexp(&[]),
            Err(ManualFishingAexpError::LandedFishCountOutOfRange(0))
        );
        let six = [FishingRarity::Common; 6];
        assert_eq!(
            manual_fishing_base_fish_cast_aexp(&six),
            Err(ManualFishingAexpError::LandedFishCountOutOfRange(6))
        );
    }
}
