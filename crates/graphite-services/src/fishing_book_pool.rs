use serde::Serialize;
use thiserror::Error;

use crate::{CanonicalEnchant, EnchantAcquisitionSource, enchant_catalog_policy};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DirectFishingBookPool {
    ShopCommon,
    MidLoot,
    Rare,
    Mending,
    Mythic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct DirectFishingBookPoolPolicy {
    pub pool: DirectFishingBookPool,
    pub relative_weight: u8,
}

/// Raw level profile attached to a direct-fishing Book pool/family before a specific enchant's
/// own level ceiling is reconciled.
///
/// `ShopCommon`, `MidLoot`, and `Rare` are intentionally not per-enchant validators. The active
/// specification freezes pool-level distributions but also contains more-specific level contracts
/// such as Bait Rack III and Carving I. Until the specification defines how those narrower ceilings
/// compose with the pool profile, callers must not use a raw profile alone to mint a finished book.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DirectFishingBookLevelProfile {
    ShopCommon,
    MidLoot,
    Rare,
    Mending,
    NukeOrAnnihilation,
    Phoenix,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum DirectFishingBookPolicyError {
    #[error("level {level} is not directly fishable for raw profile {profile:?}")]
    LevelOutsideProfile {
        profile: DirectFishingBookLevelProfile,
        level: u8,
    },
    #[error("{0:?} is not a direct-fishing mythic enchant")]
    NotDirectFishingMythic(CanonicalEnchant),
}

/// Resolves the latest authoritative pool weight after the Treasure result has already selected an
/// Enchant Book.
///
/// The five weights sum to 100 and therefore also read as percentages at the current design point.
/// They remain expressed as relative weights so a later selection owner does not confuse this pure
/// table with an RNG draw. This policy does not choose an enchant inside Shop/Common, Mid Loot, or
/// Rare because the specification lists pool members but does not freeze within-pool member weights.
#[must_use]
pub const fn direct_fishing_book_pool_policy(
    pool: DirectFishingBookPool,
) -> DirectFishingBookPoolPolicy {
    let relative_weight = match pool {
        DirectFishingBookPool::ShopCommon => 58,
        DirectFishingBookPool::MidLoot => 24,
        DirectFishingBookPool::Rare => 12,
        DirectFishingBookPool::Mending => 4,
        DirectFishingBookPool::Mythic => 2,
    };

    DirectFishingBookPoolPolicy {
        pool,
        relative_weight,
    }
}

/// Maps the canonical acquisition catalog into direct-fishing Book-pool eligibility.
///
/// This deliberately reuses [`enchant_catalog_policy`] instead of copying the long member lists.
/// Combine-only Shadow Walker and Master progression are not directly fishable. The current sole
/// `FishingOnly` identity, Mending, maps to its dedicated pool; future `FishingOnly` identities must
/// receive an explicit pool decision rather than being silently treated as Mending.
#[must_use]
pub const fn direct_fishing_book_pool_membership(
    enchant: CanonicalEnchant,
) -> Option<DirectFishingBookPool> {
    if matches!(enchant, CanonicalEnchant::Mending) {
        return Some(DirectFishingBookPool::Mending);
    }

    match enchant_catalog_policy(enchant).acquisition_source {
        EnchantAcquisitionSource::NormalShopFishingChest => Some(DirectFishingBookPool::ShopCommon),
        EnchantAcquisitionSource::FishingChestMidHigh => Some(DirectFishingBookPool::MidLoot),
        EnchantAcquisitionSource::FishingChestRare => Some(DirectFishingBookPool::Rare),
        EnchantAcquisitionSource::FishingChestMythic => Some(DirectFishingBookPool::Mythic),
        EnchantAcquisitionSource::FishingOnly
        | EnchantAcquisitionSource::CombineMutationOnly
        | EnchantAcquisitionSource::MasterProgression => None,
    }
}

/// Resolves the explicit split inside the Mythic direct-fishing Book pool.
///
/// Shop/Common, Mid Loot, and Rare do not have authoritative per-enchant weights and therefore have
/// no equivalent resolver yet.
pub const fn direct_fishing_mythic_enchant_weight(
    enchant: CanonicalEnchant,
) -> Result<u8, DirectFishingBookPolicyError> {
    match enchant {
        CanonicalEnchant::Nuke => Ok(45),
        CanonicalEnchant::Annihilation => Ok(30),
        CanonicalEnchant::Phoenix => Ok(25),
        other => Err(DirectFishingBookPolicyError::NotDirectFishingMythic(other)),
    }
}

/// Returns one weight from the latest raw direct-fishing Book-level profile.
///
/// Every complete profile sums to 100. A level outside the frozen support fails closed rather than
/// being treated as zero-probability valid state. This function does not reconcile a selected
/// enchant's own max-level rule; in particular, callers must not infer that every Shop/Common book
/// may reach VI or every Mid Loot book may reach VII.
pub const fn direct_fishing_raw_book_level_weight(
    profile: DirectFishingBookLevelProfile,
    level: u8,
) -> Result<u8, DirectFishingBookPolicyError> {
    let weight = match (profile, level) {
        (DirectFishingBookLevelProfile::ShopCommon, 1) => 34,
        (DirectFishingBookLevelProfile::ShopCommon, 2) => 28,
        (DirectFishingBookLevelProfile::ShopCommon, 3) => 20,
        (DirectFishingBookLevelProfile::ShopCommon, 4) => 12,
        (DirectFishingBookLevelProfile::ShopCommon, 5) => 5,
        (DirectFishingBookLevelProfile::ShopCommon, 6) => 1,

        (DirectFishingBookLevelProfile::MidLoot, 2) => 24,
        (DirectFishingBookLevelProfile::MidLoot, 3) => 24,
        (DirectFishingBookLevelProfile::MidLoot, 4) => 22,
        (DirectFishingBookLevelProfile::MidLoot, 5) => 16,
        (DirectFishingBookLevelProfile::MidLoot, 6) => 10,
        (DirectFishingBookLevelProfile::MidLoot, 7) => 4,

        (DirectFishingBookLevelProfile::Rare, 3) => 14,
        (DirectFishingBookLevelProfile::Rare, 4) => 22,
        (DirectFishingBookLevelProfile::Rare, 5) => 26,
        (DirectFishingBookLevelProfile::Rare, 6) => 20,
        (DirectFishingBookLevelProfile::Rare, 7) => 12,
        (DirectFishingBookLevelProfile::Rare, 8) => 6,

        (DirectFishingBookLevelProfile::Mending, 1) => 100,

        (DirectFishingBookLevelProfile::NukeOrAnnihilation, 1) => 28,
        (DirectFishingBookLevelProfile::NukeOrAnnihilation, 2) => 24,
        (DirectFishingBookLevelProfile::NukeOrAnnihilation, 3) => 20,
        (DirectFishingBookLevelProfile::NukeOrAnnihilation, 4) => 12,
        (DirectFishingBookLevelProfile::NukeOrAnnihilation, 5) => 8,
        (DirectFishingBookLevelProfile::NukeOrAnnihilation, 6) => 5,
        (DirectFishingBookLevelProfile::NukeOrAnnihilation, 7) => 3,

        (DirectFishingBookLevelProfile::Phoenix, 1) => 100,

        _ => {
            return Err(DirectFishingBookPolicyError::LevelOutsideProfile { profile, level });
        }
    };

    Ok(weight)
}

#[cfg(test)]
mod tests {
    use super::*;

    const POOLS: [DirectFishingBookPool; 5] = [
        DirectFishingBookPool::ShopCommon,
        DirectFishingBookPool::MidLoot,
        DirectFishingBookPool::Rare,
        DirectFishingBookPool::Mending,
        DirectFishingBookPool::Mythic,
    ];

    const LEVEL_PROFILES: [DirectFishingBookLevelProfile; 6] = [
        DirectFishingBookLevelProfile::ShopCommon,
        DirectFishingBookLevelProfile::MidLoot,
        DirectFishingBookLevelProfile::Rare,
        DirectFishingBookLevelProfile::Mending,
        DirectFishingBookLevelProfile::NukeOrAnnihilation,
        DirectFishingBookLevelProfile::Phoenix,
    ];

    #[test]
    fn pool_weights_match_latest_master_and_sum_to_one_hundred() {
        let weights = POOLS.map(|pool| direct_fishing_book_pool_policy(pool).relative_weight);
        assert_eq!(weights, [58, 24, 12, 4, 2]);
        assert_eq!(
            weights.iter().map(|weight| u16::from(*weight)).sum::<u16>(),
            100
        );
    }

    #[test]
    fn membership_reuses_catalog_sources_and_excludes_non_fishing_sources() {
        assert_eq!(
            direct_fishing_book_pool_membership(CanonicalEnchant::Efficiency),
            Some(DirectFishingBookPool::ShopCommon)
        );
        assert_eq!(
            direct_fishing_book_pool_membership(CanonicalEnchant::Carving),
            Some(DirectFishingBookPool::MidLoot)
        );
        assert_eq!(
            direct_fishing_book_pool_membership(CanonicalEnchant::SoulGrind),
            Some(DirectFishingBookPool::Rare)
        );
        assert_eq!(
            direct_fishing_book_pool_membership(CanonicalEnchant::Mending),
            Some(DirectFishingBookPool::Mending)
        );
        assert_eq!(
            direct_fishing_book_pool_membership(CanonicalEnchant::Nuke),
            Some(DirectFishingBookPool::Mythic)
        );
        assert_eq!(
            direct_fishing_book_pool_membership(CanonicalEnchant::ShadowWalker),
            None
        );
        assert_eq!(
            direct_fishing_book_pool_membership(CanonicalEnchant::Master),
            None
        );
    }

    #[test]
    fn mythic_split_is_exact_and_rejects_non_mythic_enchants() {
        let weights = [
            direct_fishing_mythic_enchant_weight(CanonicalEnchant::Nuke).unwrap(),
            direct_fishing_mythic_enchant_weight(CanonicalEnchant::Annihilation).unwrap(),
            direct_fishing_mythic_enchant_weight(CanonicalEnchant::Phoenix).unwrap(),
        ];
        assert_eq!(weights, [45, 30, 25]);
        assert_eq!(
            weights.iter().map(|weight| u16::from(*weight)).sum::<u16>(),
            100
        );
        assert_eq!(
            direct_fishing_mythic_enchant_weight(CanonicalEnchant::Mending),
            Err(DirectFishingBookPolicyError::NotDirectFishingMythic(
                CanonicalEnchant::Mending
            ))
        );
    }

    #[test]
    fn every_raw_level_profile_matches_latest_master_and_sums_to_one_hundred() {
        let expected = [
            [0, 34, 28, 20, 12, 5, 1, 0, 0],
            [0, 0, 24, 24, 22, 16, 10, 4, 0],
            [0, 0, 0, 14, 22, 26, 20, 12, 6],
            [0, 100, 0, 0, 0, 0, 0, 0, 0],
            [0, 28, 24, 20, 12, 8, 5, 3, 0],
            [0, 100, 0, 0, 0, 0, 0, 0, 0],
        ];

        for (profile, expected_weights) in LEVEL_PROFILES.into_iter().zip(expected) {
            let actual = std::array::from_fn::<_, 9, _>(|level| {
                direct_fishing_raw_book_level_weight(profile, level as u8).unwrap_or(0)
            });
            assert_eq!(actual, expected_weights);
            assert_eq!(
                actual.iter().map(|weight| u16::from(*weight)).sum::<u16>(),
                100
            );
        }
    }

    #[test]
    fn levels_outside_each_raw_profile_fail_closed() {
        for (profile, level) in [
            (DirectFishingBookLevelProfile::ShopCommon, 0),
            (DirectFishingBookLevelProfile::ShopCommon, 7),
            (DirectFishingBookLevelProfile::MidLoot, 1),
            (DirectFishingBookLevelProfile::MidLoot, 8),
            (DirectFishingBookLevelProfile::Rare, 2),
            (DirectFishingBookLevelProfile::Rare, 9),
            (DirectFishingBookLevelProfile::Mending, 2),
            (DirectFishingBookLevelProfile::NukeOrAnnihilation, 8),
            (DirectFishingBookLevelProfile::Phoenix, 2),
        ] {
            assert_eq!(
                direct_fishing_raw_book_level_weight(profile, level),
                Err(DirectFishingBookPolicyError::LevelOutsideProfile { profile, level })
            );
        }
    }
}
