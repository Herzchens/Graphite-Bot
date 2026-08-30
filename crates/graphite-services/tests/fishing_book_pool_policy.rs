use graphite_services::{
    CanonicalEnchant, DirectFishingBookLevelProfile, DirectFishingBookPolicyError,
    DirectFishingBookPool, direct_fishing_book_pool_membership, direct_fishing_book_pool_policy,
    direct_fishing_mythic_enchant_weight, direct_fishing_raw_book_level_weight,
};

#[test]
fn public_api_exposes_latest_direct_fishing_pool_weights() {
    let pools = [
        DirectFishingBookPool::ShopCommon,
        DirectFishingBookPool::MidLoot,
        DirectFishingBookPool::Rare,
        DirectFishingBookPool::Mending,
        DirectFishingBookPool::Mythic,
    ];
    let policies = pools.map(direct_fishing_book_pool_policy);

    assert_eq!(
        policies.map(|policy| policy.relative_weight),
        [58, 24, 12, 4, 2]
    );
    for (policy, pool) in policies.into_iter().zip(pools) {
        assert_eq!(policy.pool, pool);
    }
}

#[test]
fn public_api_reuses_catalog_membership_without_exposing_uniform_member_weights() {
    for (enchant, expected_pool) in [
        (CanonicalEnchant::Efficiency, Some(DirectFishingBookPool::ShopCommon)),
        (CanonicalEnchant::BaitRack, Some(DirectFishingBookPool::ShopCommon)),
        (CanonicalEnchant::Carving, Some(DirectFishingBookPool::MidLoot)),
        (CanonicalEnchant::Execution, Some(DirectFishingBookPool::Rare)),
        (CanonicalEnchant::Mending, Some(DirectFishingBookPool::Mending)),
        (CanonicalEnchant::Phoenix, Some(DirectFishingBookPool::Mythic)),
        (CanonicalEnchant::ShadowWalker, None),
        (CanonicalEnchant::Master, None),
    ] {
        assert_eq!(direct_fishing_book_pool_membership(enchant), expected_pool);
    }
}

#[test]
fn public_api_exposes_only_the_authoritative_mythic_member_split() {
    assert_eq!(
        direct_fishing_mythic_enchant_weight(CanonicalEnchant::Nuke),
        Ok(45)
    );
    assert_eq!(
        direct_fishing_mythic_enchant_weight(CanonicalEnchant::Annihilation),
        Ok(30)
    );
    assert_eq!(
        direct_fishing_mythic_enchant_weight(CanonicalEnchant::Phoenix),
        Ok(25)
    );
    assert_eq!(
        direct_fishing_mythic_enchant_weight(CanonicalEnchant::SoulGrind),
        Err(DirectFishingBookPolicyError::NotDirectFishingMythic(
            CanonicalEnchant::SoulGrind
        ))
    );
}

#[test]
fn public_api_exposes_latest_raw_pool_level_profiles() {
    let cases: &[(DirectFishingBookLevelProfile, &[(u8, u8)])] = &[
        (
            DirectFishingBookLevelProfile::ShopCommon,
            &[(1, 34), (2, 28), (3, 20), (4, 12), (5, 5), (6, 1)],
        ),
        (
            DirectFishingBookLevelProfile::MidLoot,
            &[(2, 24), (3, 24), (4, 22), (5, 16), (6, 10), (7, 4)],
        ),
        (
            DirectFishingBookLevelProfile::Rare,
            &[(3, 14), (4, 22), (5, 26), (6, 20), (7, 12), (8, 6)],
        ),
        (DirectFishingBookLevelProfile::Mending, &[(1, 100)]),
        (
            DirectFishingBookLevelProfile::NukeOrAnnihilation,
            &[(1, 28), (2, 24), (3, 20), (4, 12), (5, 8), (6, 5), (7, 3)],
        ),
        (DirectFishingBookLevelProfile::Phoenix, &[(1, 100)]),
    ];

    for (profile, levels) in cases {
        let mut sum = 0_u16;
        for (level, expected_weight) in *levels {
            let actual = direct_fishing_raw_book_level_weight(*profile, *level).unwrap();
            assert_eq!(actual, *expected_weight);
            sum += u16::from(actual);
        }
        assert_eq!(sum, 100);
    }
}

#[test]
fn raw_level_profiles_fail_closed_outside_frozen_support() {
    for (profile, level) in [
        (DirectFishingBookLevelProfile::ShopCommon, 7),
        (DirectFishingBookLevelProfile::MidLoot, 1),
        (DirectFishingBookLevelProfile::Rare, 9),
        (DirectFishingBookLevelProfile::Mending, 2),
        (DirectFishingBookLevelProfile::NukeOrAnnihilation, 8),
        (DirectFishingBookLevelProfile::Phoenix, 2),
    ] {
        assert_eq!(
            direct_fishing_raw_book_level_weight(profile, level),
            Err(DirectFishingBookPolicyError::LevelOutsideProfile {
                profile,
                level,
            })
        );
    }
}
