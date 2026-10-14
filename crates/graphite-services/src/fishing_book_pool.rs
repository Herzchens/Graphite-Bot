use serde::Serialize;
use thiserror::Error;

use crate::{
    CanonicalEnchant, EnchantAcquisitionSource, canonical_enchant_max_resulting_level,
    enchant_catalog_policy,
};

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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct DirectFishingBookMemberPolicy {
    pub enchant: CanonicalEnchant,
    pub pool: DirectFishingBookPool,
    pub relative_weight: u16,
}

/// Raw level profile attached to a direct-fishing Book pool/family before a specific enchant's
/// own level ceiling is reconciled.
///
/// `ShopCommon`, `MidLoot`, and `Rare` are intentionally raw pool profiles rather than
/// per-enchant validators. Finished-book resolution applies the selected enchant's canonical level
/// ceiling, while singleton contracts such as Carving I override the generic profile entirely.
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
    #[error("{0:?} is not directly fishable")]
    NotDirectFishingEnchant(CanonicalEnchant),
    #[error(
        "level {level} exceeds the direct-fishing resulting-level ceiling {max_level} for {enchant:?}"
    )]
    LevelOutsideEnchantCeiling {
        enchant: CanonicalEnchant,
        level: u8,
        max_level: u8,
    },
}

/// Resolves the latest authoritative pool weight after the Treasure result has already selected an
/// Enchant Book.
///
/// The five weights sum to 100 and therefore also read as percentages at the current design point.
/// They remain expressed as relative weights so a later selection owner does not confuse this pure
/// table with an RNG draw. Member identity is resolved separately by the owner-approved within-pool
/// policy: equal weights for Shop/Common, Mid Loot and Rare; singleton Mending; explicit Mythic split.
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

const SHOP_COMMON_MEMBERS: [CanonicalEnchant; 25] = [
    CanonicalEnchant::Efficiency,
    CanonicalEnchant::Fortune,
    CanonicalEnchant::Smelt,
    CanonicalEnchant::Lure,
    CanonicalEnchant::LuckOfTheSea,
    CanonicalEnchant::Luck,
    CanonicalEnchant::Strengthen,
    CanonicalEnchant::SharpHook,
    CanonicalEnchant::BaitRack,
    CanonicalEnchant::Sharpness,
    CanonicalEnchant::Smite,
    CanonicalEnchant::BaneOfArthropods,
    CanonicalEnchant::SweepingEdge,
    CanonicalEnchant::FireAspect,
    CanonicalEnchant::Knockback,
    CanonicalEnchant::Protection,
    CanonicalEnchant::Thorn,
    CanonicalEnchant::Cat,
    CanonicalEnchant::Dog,
    CanonicalEnchant::Dodge,
    CanonicalEnchant::Unbreaking,
    CanonicalEnchant::Stabilize,
    CanonicalEnchant::Sparkling,
    CanonicalEnchant::Grinding,
    CanonicalEnchant::Mosaic,
];

const MID_LOOT_MEMBERS: [CanonicalEnchant; 15] = [
    CanonicalEnchant::PickaxeTreasure,
    CanonicalEnchant::FishingRodTreasure,
    CanonicalEnchant::MultiTreasure,
    CanonicalEnchant::Multicatch,
    CanonicalEnchant::Looting,
    CanonicalEnchant::Devour,
    CanonicalEnchant::Bleeding,
    CanonicalEnchant::Freezing,
    CanonicalEnchant::Angel,
    CanonicalEnchant::Evil,
    CanonicalEnchant::DayWalker,
    CanonicalEnchant::NightWalker,
    CanonicalEnchant::Reinforce,
    CanonicalEnchant::Empowering,
    CanonicalEnchant::Carving,
];

const RARE_MEMBERS: [CanonicalEnchant; 8] = [
    CanonicalEnchant::Trench,
    CanonicalEnchant::Execution,
    CanonicalEnchant::BloodFrenzy,
    CanonicalEnchant::ArmorPiercing,
    CanonicalEnchant::Piercing,
    CanonicalEnchant::Guardian,
    CanonicalEnchant::NineLife,
    CanonicalEnchant::SoulGrind,
];

const MENDING_MEMBERS: [CanonicalEnchant; 1] = [CanonicalEnchant::Mending];

const MYTHIC_MEMBERS: [CanonicalEnchant; 3] = [
    CanonicalEnchant::Nuke,
    CanonicalEnchant::Annihilation,
    CanonicalEnchant::Phoenix,
];

#[must_use]
pub const fn direct_fishing_book_pool_members(
    pool: DirectFishingBookPool,
) -> &'static [CanonicalEnchant] {
    match pool {
        DirectFishingBookPool::ShopCommon => &SHOP_COMMON_MEMBERS,
        DirectFishingBookPool::MidLoot => &MID_LOOT_MEMBERS,
        DirectFishingBookPool::Rare => &RARE_MEMBERS,
        DirectFishingBookPool::Mending => &MENDING_MEMBERS,
        DirectFishingBookPool::Mythic => &MYTHIC_MEMBERS,
    }
}

pub const fn direct_fishing_book_member_policy(
    enchant: CanonicalEnchant,
) -> Result<DirectFishingBookMemberPolicy, DirectFishingBookPolicyError> {
    let Some(pool) = direct_fishing_book_pool_membership(enchant) else {
        return Err(DirectFishingBookPolicyError::NotDirectFishingEnchant(
            enchant,
        ));
    };
    let relative_weight = match pool {
        DirectFishingBookPool::ShopCommon
        | DirectFishingBookPool::MidLoot
        | DirectFishingBookPool::Rare
        | DirectFishingBookPool::Mending => 1,
        DirectFishingBookPool::Mythic => match direct_fishing_mythic_enchant_weight(enchant) {
            Ok(weight) => weight as u16,
            Err(error) => return Err(error),
        },
    };

    Ok(DirectFishingBookMemberPolicy {
        enchant,
        pool,
        relative_weight,
    })
}

pub const fn direct_fishing_book_level_profile(
    enchant: CanonicalEnchant,
) -> Result<DirectFishingBookLevelProfile, DirectFishingBookPolicyError> {
    Ok(match enchant {
        CanonicalEnchant::Mending => DirectFishingBookLevelProfile::Mending,
        CanonicalEnchant::Nuke | CanonicalEnchant::Annihilation => {
            DirectFishingBookLevelProfile::NukeOrAnnihilation
        }
        CanonicalEnchant::Phoenix => DirectFishingBookLevelProfile::Phoenix,
        other => match direct_fishing_book_pool_membership(other) {
            Some(DirectFishingBookPool::ShopCommon) => DirectFishingBookLevelProfile::ShopCommon,
            Some(DirectFishingBookPool::MidLoot) => DirectFishingBookLevelProfile::MidLoot,
            Some(DirectFishingBookPool::Rare) => DirectFishingBookLevelProfile::Rare,
            Some(DirectFishingBookPool::Mending | DirectFishingBookPool::Mythic) | None => {
                return Err(DirectFishingBookPolicyError::NotDirectFishingEnchant(other));
            }
        },
    })
}

pub fn direct_fishing_finished_book_level_weights(
    enchant: CanonicalEnchant,
) -> Result<[u16; 10], DirectFishingBookPolicyError> {
    direct_fishing_book_member_policy(enchant)?;

    if matches!(
        enchant,
        CanonicalEnchant::Mending | CanonicalEnchant::Phoenix | CanonicalEnchant::Carving
    ) {
        let mut weights = [0_u16; 10];
        weights[0] = 100;
        return Ok(weights);
    }

    let profile = direct_fishing_book_level_profile(enchant)?;
    let max_level = canonical_enchant_max_resulting_level(enchant);
    let mut weights = [0_u16; 10];
    let mut level = 1_u8;
    while level <= max_level {
        weights[usize::from(level - 1)] = match direct_fishing_raw_book_level_weight(profile, level)
        {
            Ok(weight) => u16::from(weight),
            Err(DirectFishingBookPolicyError::LevelOutsideProfile { .. }) => 0,
            Err(error) => return Err(error),
        };
        level += 1;
    }
    Ok(weights)
}

pub fn direct_fishing_finished_book_level_weight(
    enchant: CanonicalEnchant,
    level: u8,
) -> Result<u16, DirectFishingBookPolicyError> {
    let max_level = canonical_enchant_max_resulting_level(enchant);
    if level == 0 || level > max_level {
        return Err(DirectFishingBookPolicyError::LevelOutsideEnchantCeiling {
            enchant,
            level,
            max_level,
        });
    }

    Ok(direct_fishing_finished_book_level_weights(enchant)?[usize::from(level - 1)])
}

/// Resolves the explicit split inside the Mythic direct-fishing Book pool.
///
/// Mythic keeps its frozen 45/30/25 identity split. Shop/Common, Mid Loot and Rare use the separate
/// equal-member policy after their outer pool is selected.
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
    fn within_pool_members_are_explicit_and_only_mythic_is_nonuniform() {
        let mut unique_members = std::collections::HashSet::new();

        assert_eq!(
            direct_fishing_book_pool_members(DirectFishingBookPool::ShopCommon).len(),
            25
        );
        assert_eq!(
            direct_fishing_book_pool_members(DirectFishingBookPool::MidLoot).len(),
            15
        );
        assert_eq!(
            direct_fishing_book_pool_members(DirectFishingBookPool::Rare).len(),
            8
        );
        assert_eq!(
            direct_fishing_book_pool_members(DirectFishingBookPool::Mending),
            &[CanonicalEnchant::Mending]
        );
        assert_eq!(
            direct_fishing_book_pool_members(DirectFishingBookPool::Mythic),
            &[
                CanonicalEnchant::Nuke,
                CanonicalEnchant::Annihilation,
                CanonicalEnchant::Phoenix,
            ]
        );

        for pool in [
            DirectFishingBookPool::ShopCommon,
            DirectFishingBookPool::MidLoot,
            DirectFishingBookPool::Rare,
            DirectFishingBookPool::Mending,
            DirectFishingBookPool::Mythic,
        ] {
            for enchant in direct_fishing_book_pool_members(pool) {
                assert!(unique_members.insert(enchant.persisted_key()));
                let policy = direct_fishing_book_member_policy(*enchant).unwrap();
                assert_eq!(policy.pool, pool);
                if pool == DirectFishingBookPool::Mythic {
                    assert_eq!(
                        policy.relative_weight,
                        u16::from(direct_fishing_mythic_enchant_weight(*enchant).unwrap())
                    );
                } else {
                    assert_eq!(policy.relative_weight, 1);
                }
            }
        }
        assert_eq!(unique_members.len(), 52);

        assert_eq!(
            direct_fishing_book_member_policy(CanonicalEnchant::ShadowWalker),
            Err(DirectFishingBookPolicyError::NotDirectFishingEnchant(
                CanonicalEnchant::ShadowWalker
            ))
        );
        assert_eq!(
            direct_fishing_book_member_policy(CanonicalEnchant::Master),
            Err(DirectFishingBookPolicyError::NotDirectFishingEnchant(
                CanonicalEnchant::Master
            ))
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
    fn selected_enchant_ceiling_filters_profile_without_clamping() {
        assert_eq!(
            direct_fishing_finished_book_level_weights(CanonicalEnchant::BaitRack).unwrap(),
            [34, 28, 20, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            direct_fishing_finished_book_level_weights(CanonicalEnchant::Carving).unwrap(),
            [100, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            direct_fishing_finished_book_level_weights(CanonicalEnchant::Mending).unwrap(),
            [100, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            direct_fishing_finished_book_level_weights(CanonicalEnchant::Phoenix).unwrap(),
            [100, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            direct_fishing_finished_book_level_weights(CanonicalEnchant::NineLife).unwrap(),
            [0, 0, 14, 22, 26, 20, 12, 6, 0, 0]
        );
        assert_eq!(
            direct_fishing_finished_book_level_weight(CanonicalEnchant::BaitRack, 4),
            Err(DirectFishingBookPolicyError::LevelOutsideEnchantCeiling {
                enchant: CanonicalEnchant::BaitRack,
                level: 4,
                max_level: 3,
            })
        );
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
