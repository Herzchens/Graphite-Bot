use thiserror::Error;

use crate::{
    FishingCatchBranch, FishingPayload, FishingResolvedNonFishOutcomeV1, FishingTreasureResult,
    ManualFishingPpmDrawV1, ManualFishingRngContext, ManualFishingRngDrawError,
    ManualFishingWeightedDrawV1, MultiTreasureCount, MultiTreasurePolicyError,
    fishing_base_treasure_result_policy, fishing_junk_payloads, fishing_non_book_treasure_payloads,
    fishing_payload_policy, multi_treasure_count_policy,
};

const TREASURE_RESULTS: [FishingTreasureResult; 6] = [
    FishingTreasureResult::MaterialBundle,
    FishingTreasureResult::CrateOrChest,
    FishingTreasureResult::EnchantBook,
    FishingTreasureResult::OrbOrCatalystFragmentOrItem,
    FishingTreasureResult::RareBaitOrUtilityItem,
    FishingTreasureResult::RelicOrCollectible,
];

const MULTI_TREASURE_COUNTS: [MultiTreasureCount; 3] = [
    MultiTreasureCount::Single,
    MultiTreasureCount::Double,
    MultiTreasureCount::Triple,
];

#[derive(Debug, Error)]
pub enum FishingNonFishRollError {
    #[error(transparent)]
    Rng(#[from] ManualFishingRngDrawError),
    #[error(transparent)]
    MultiTreasure(#[from] MultiTreasurePolicyError),
    #[error("Fish outcomes must use the unique FishInstance resolver")]
    FishRequiresUniqueDelivery,
    #[error("direct Enchant Books must use their dedicated identity and level resolver")]
    EnchantBookRequiresDedicatedResolver,
    #[error("canonical non-book Fishing payload pool is empty")]
    EmptyCanonicalPayloadPool,
    #[error("canonical Multi Treasure PPM weights do not span the full probability scale")]
    InvalidMultiTreasureDistribution,
}

/// Resolves an already-authoritative Junk or Treasure *branch* with the operation's pinned RNG.
///
/// The branch must be selected by the owning cast's final normalized/capped branch resolver; this
/// routine deliberately does not choose it. Different semantic RNG domains prevent a change in
/// one Treasure repetition from moving other casts, fish slots, or repeat slots. Within-Treasure
/// category is selected only once, then a Multi Treasure count and each non-book payload independently.
/// Direct Enchant Books fail closed until their dedicated versioned asset settlement is composed.
pub fn resolve_manual_fishing_non_fish_outcome_v1(
    rng: ManualFishingRngContext,
    branch: FishingCatchBranch,
    multi_treasure_level: Option<u8>,
) -> Result<FishingResolvedNonFishOutcomeV1, FishingNonFishRollError> {
    match branch {
        FishingCatchBranch::Fish => Err(FishingNonFishRollError::FishRequiresUniqueDelivery),
        FishingCatchBranch::Junk => Ok(FishingResolvedNonFishOutcomeV1::Junk {
            payload: select_weighted_payload(
                rng,
                ManualFishingWeightedDrawV1::JunkPayload,
                fishing_junk_payloads(),
            )?,
        }),
        FishingCatchBranch::Treasure => {
            let weights = TREASURE_RESULTS.map(|result| {
                u64::from(fishing_base_treasure_result_policy(result).relative_weight)
            });
            let selected =
                rng.sample_weighted_v1(ManualFishingWeightedDrawV1::TreasureResult, &weights)?;
            let result = TREASURE_RESULTS[selected];
            if result == FishingTreasureResult::EnchantBook {
                return Err(FishingNonFishRollError::EnchantBookRequiresDedicatedResolver);
            }
            let count = resolve_treasure_count(rng, multi_treasure_level)?;
            let pool = fishing_non_book_treasure_payloads(result)
                .map_err(|_| FishingNonFishRollError::EnchantBookRequiresDedicatedResolver)?;
            let mut payloads = Vec::with_capacity(usize::from(count));
            for slot in 0..count {
                payloads.push(select_weighted_payload(
                    rng,
                    ManualFishingWeightedDrawV1::NonBookTreasurePayload {
                        treasure_slot: slot,
                    },
                    pool,
                )?);
            }
            Ok(FishingResolvedNonFishOutcomeV1::Treasure { result, payloads })
        }
    }
}

/// Resolves the owner-approved Multi Treasure I-X repetition count from the
/// operation-pinned semantic PPM domain.
///
/// The v1 interval order is Single, Double, Triple, matching the existing
/// non-book payload resolver. Repeated direct Enchant Books may call this same
/// owner so Book and non-book Treasure cannot disagree on quantity or replay.
pub fn resolve_manual_fishing_multi_treasure_count_v1(
    rng: ManualFishingRngContext,
    level: u8,
) -> Result<u8, FishingNonFishRollError> {
    let bucket = rng.sample_ppm_v1(ManualFishingPpmDrawV1::MultiTreasureCount)?;
    let mut threshold = 0_u32;
    for candidate in MULTI_TREASURE_COUNTS {
        let policy = multi_treasure_count_policy(level, candidate)?;
        threshold = threshold
            .checked_add(policy.probability_ppm)
            .ok_or(FishingNonFishRollError::InvalidMultiTreasureDistribution)?;
        if bucket < threshold {
            return Ok(policy.treasure_count);
        }
    }
    Err(FishingNonFishRollError::InvalidMultiTreasureDistribution)
}

fn resolve_treasure_count(
    rng: ManualFishingRngContext,
    level: Option<u8>,
) -> Result<u8, FishingNonFishRollError> {
    match level {
        None => Ok(1),
        Some(level) => resolve_manual_fishing_multi_treasure_count_v1(rng, level),
    }
}

fn select_weighted_payload(
    rng: ManualFishingRngContext,
    draw: ManualFishingWeightedDrawV1,
    pool: &[FishingPayload],
) -> Result<FishingPayload, FishingNonFishRollError> {
    if pool.is_empty() {
        return Err(FishingNonFishRollError::EmptyCanonicalPayloadPool);
    }
    let weights = pool
        .iter()
        .map(|payload| u64::from(fishing_payload_policy(*payload).relative_weight))
        .collect::<Vec<_>>();
    let selected = rng.sample_weighted_v1(draw, &weights)?;
    Ok(pool[selected])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_outcome_tables_have_complete_weight_support() {
        let total: u32 = TREASURE_RESULTS
            .map(|result| u32::from(fishing_base_treasure_result_policy(result).relative_weight))
            .into_iter()
            .sum();
        assert_eq!(total, 50);

        let junk_total: u32 = fishing_junk_payloads()
            .iter()
            .map(|payload| u32::from(fishing_payload_policy(*payload).relative_weight))
            .sum();
        assert_eq!(junk_total, 100);

        for result in TREASURE_RESULTS {
            if result == FishingTreasureResult::EnchantBook {
                continue;
            }
            let pool = fishing_non_book_treasure_payloads(result).unwrap();
            let total: u32 = pool
                .iter()
                .map(|payload| u32::from(fishing_payload_policy(*payload).relative_weight))
                .sum();
            assert_eq!(total, 100);
        }
        assert_eq!(crate::MULTI_TREASURE_MAX_ITEMS, 3);
    }
}
