use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    CanonicalEnchant, DirectFishingBookPolicyError, DirectFishingBookPool, ManualFishingRngContext,
    ManualFishingRngDrawError, ManualFishingWeightedDrawV1, direct_fishing_book_member_policy,
    direct_fishing_book_pool_members, direct_fishing_book_pool_policy,
    direct_fishing_finished_book_level_weights,
};

const BOOK_POOLS: [DirectFishingBookPool; 5] = [
    DirectFishingBookPool::ShopCommon,
    DirectFishingBookPool::MidLoot,
    DirectFishingBookPool::Rare,
    DirectFishingBookPool::Mending,
    DirectFishingBookPool::Mythic,
];

/// A version-pinned *identity preview*, not a delivered Enchant Book ItemInstance.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FishingDirectBookCandidateV1 {
    pub treasure_slot: u8,
    pub pool: DirectFishingBookPool,
    pub enchant: CanonicalEnchant,
    pub level: u8,
}

#[derive(Debug, Error)]
pub enum FishingDirectBookRollError {
    #[error(transparent)]
    Rng(#[from] ManualFishingRngDrawError),
    #[error(transparent)]
    Policy(#[from] DirectFishingBookPolicyError),
    #[error("canonical direct Fishing Book pool has no eligible members")]
    EmptyBookPool,
    #[error("canonical direct Fishing Book level weights contain no supported level")]
    EmptyBookLevelWeights,
}

/// Resolves the canonical pool -> member -> finished level sequence for one
/// already-selected Enchant Book Treasure slot from the owning operation RNG.
///
/// Pool weights are 58/24/12/4/2. Member weights are supplied exclusively by
/// the frozen per-pool catalog (including the Mythic 45/30/25 split), and level
/// weights are capped by the selected enchant's own finished-book profile.
///
/// Per-slot Pool/Member/Level draws are independent semantic RNG domains. The
/// caller must first establish that the actual Treasure result is Enchant Book;
/// calling this pure resolver alone never selects a catch or issues an asset.
pub fn resolve_manual_fishing_direct_book_candidate_v1(
    rng: ManualFishingRngContext,
    treasure_slot: u8,
) -> Result<FishingDirectBookCandidateV1, FishingDirectBookRollError> {
    let pool_weights =
        BOOK_POOLS.map(|pool| u64::from(direct_fishing_book_pool_policy(pool).relative_weight));
    let pool_index = rng.sample_weighted_v1(
        ManualFishingWeightedDrawV1::EnchantBookPool { treasure_slot },
        &pool_weights,
    )?;
    let pool = BOOK_POOLS[pool_index];
    let members = direct_fishing_book_pool_members(pool);
    if members.is_empty() {
        return Err(FishingDirectBookRollError::EmptyBookPool);
    }
    let member_weights = members
        .iter()
        .map(|member| {
            direct_fishing_book_member_policy(*member)
                .map(|policy| u64::from(policy.relative_weight))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let member_index = rng.sample_weighted_v1(
        ManualFishingWeightedDrawV1::EnchantBookMember { treasure_slot },
        &member_weights,
    )?;
    let enchant = members[member_index];
    let level_weights = direct_fishing_finished_book_level_weights(enchant)?.map(u64::from);
    if level_weights.iter().all(|weight| *weight == 0) {
        return Err(FishingDirectBookRollError::EmptyBookLevelWeights);
    }
    let level_index = rng.sample_weighted_v1(
        ManualFishingWeightedDrawV1::EnchantBookLevel { treasure_slot },
        &level_weights,
    )?;
    let level =
        u8::try_from(level_index + 1).expect("canonical finished-book levels are limited to ten");
    Ok(FishingDirectBookCandidateV1 {
        treasure_slot,
        pool,
        enchant,
        level,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::direct_fishing_book_pool_membership;

    #[test]
    fn canonical_pool_and_member_weights_are_positive_and_complete() {
        let weights = BOOK_POOLS.map(|pool| direct_fishing_book_pool_policy(pool).relative_weight);
        assert_eq!(weights, [58, 24, 12, 4, 2]);
        for pool in BOOK_POOLS {
            let members = direct_fishing_book_pool_members(pool);
            assert!(!members.is_empty());
            for enchant in members {
                let member = direct_fishing_book_member_policy(*enchant).unwrap();
                assert_eq!(member.pool, pool);
                assert_eq!(direct_fishing_book_pool_membership(*enchant), Some(pool));
                assert!(member.relative_weight > 0);
                assert!(
                    direct_fishing_finished_book_level_weights(*enchant)
                        .unwrap()
                        .iter()
                        .any(|weight| *weight > 0)
                );
            }
        }
    }
}
