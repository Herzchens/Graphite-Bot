use serde::Serialize;
use thiserror::Error;

use crate::fishing_bait::FishingRarity;

pub const MANUAL_FISHING_BASE_JUNK_AEXP: i64 = 2;
pub const MANUAL_FISHING_BASE_TREASURE_AEXP: i64 = 5;
pub const MANUAL_FISHING_BASE_MULTI_TREASURE_AEXP_CAP: i64 = 10;
pub const MANUAL_FISHING_MULTI_TREASURE_MAX_ITEMS: u8 = 3;

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
    #[error(
        "landed treasure count must be between 1 and {MANUAL_FISHING_MULTI_TREASURE_MAX_ITEMS}; got {0}"
    )]
    LandedTreasureCountOutOfRange(u8),
}

/// Resolves the frozen base Manual Fishing Activity EXP for one terminal single-result outcome.
///
/// Successful Junk/Treasure/Fish outcomes return a positive **base** amount before AEXP gain
/// modifiers. `FishEscaped` and `LineBreak` return `None`, which means the owning transaction must
/// not create an Activity EXP grant mutation for that failed fish result. This shape composes with
/// the progression layer, whose mutation primitive intentionally rejects zero-amount mutations.
///
/// The future stateful Fishing settlement owner must apply the authoritative AEXP gain modifier
/// stack and its global cap after this source-specific base policy, then pass the final integer grant
/// to the progression mutation primitive. This module deliberately does not apply Rebirth or any
/// other global AEXP gain modifier itself.
///
/// For a Treasure branch expanded by Multi Treasure, use
/// `manual_fishing_base_treasure_cast_aexp` so the canonical 10-base-AEXP-per-cast Treasure cap is
/// applied. Aggregate Multi Catch fish AEXP is deliberately not calculated here because the
/// specification does not yet define which fish owns the `single-cast fish AEXP` cap basis when
/// landed fish have different rarities.
#[must_use]
pub const fn manual_fishing_base_outcome_aexp(outcome: ManualFishingAexpOutcome) -> Option<i64> {
    match outcome {
        ManualFishingAexpOutcome::LandedJunk => Some(MANUAL_FISHING_BASE_JUNK_AEXP),
        ManualFishingAexpOutcome::LandedTreasure => Some(MANUAL_FISHING_BASE_TREASURE_AEXP),
        ManualFishingAexpOutcome::LandedFish(rarity) => Some(fish_rarity_base_aexp(rarity)),
        ManualFishingAexpOutcome::FishEscaped | ManualFishingAexpOutcome::LineBreak => None,
    }
}

/// Applies the frozen base Treasure Activity EXP cap for one successfully landed Treasure cast.
///
/// The caller supplies the authoritative number of landed Treasure items after Multi Treasure has
/// already resolved. Canonical Multi Treasure output is single, double, or triple Treasure, so any
/// count outside `1..=3` fails closed instead of being silently hidden by the AEXP cap. This function
/// does not own Multi Treasure RNG or enchant-level count distributions; it only applies
/// `5 base AEXP × landed treasure count`, capped at 10 base AEXP per cast before global AEXP gain
/// modifiers.
pub fn manual_fishing_base_treasure_cast_aexp(
    landed_treasure_count: u8,
) -> Result<i64, ManualFishingAexpError> {
    if !(1..=MANUAL_FISHING_MULTI_TREASURE_MAX_ITEMS).contains(&landed_treasure_count) {
        return Err(ManualFishingAexpError::LandedTreasureCountOutOfRange(
            landed_treasure_count,
        ));
    }

    Ok((i64::from(landed_treasure_count) * MANUAL_FISHING_BASE_TREASURE_AEXP)
        .min(MANUAL_FISHING_BASE_MULTI_TREASURE_AEXP_CAP))
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
        assert_eq!(manual_fishing_base_treasure_cast_aexp(3), Ok(10));
    }

    #[test]
    fn noncanonical_treasure_counts_fail_closed() {
        for count in [0, 4, u8::MAX] {
            assert_eq!(
                manual_fishing_base_treasure_cast_aexp(count),
                Err(ManualFishingAexpError::LandedTreasureCountOutOfRange(
                    count
                ))
            );
        }
    }
}
