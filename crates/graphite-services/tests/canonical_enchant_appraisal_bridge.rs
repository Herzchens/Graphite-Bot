use graphite_services::{
    CanonicalEmbeddedEnchantAppraisalInput, CanonicalEnchant, EnchantAppraisalClass,
    EnchantAppraisalError, canonical_embedded_enchant_value, canonical_enchant_book_appraisal,
};

#[test]
fn canonical_identity_owns_appraisal_class_and_book_value() {
    for (enchant, level, expected_class, expected_value) in [
        (
            CanonicalEnchant::Efficiency,
            1,
            EnchantAppraisalClass::ShopCommon,
            60_000,
        ),
        (
            CanonicalEnchant::Stabilize,
            1,
            EnchantAppraisalClass::SpecialCommon,
            120_000,
        ),
        (
            CanonicalEnchant::PickaxeTreasure,
            1,
            EnchantAppraisalClass::FishingChestMidHigh,
            180_000,
        ),
        (
            CanonicalEnchant::Trench,
            1,
            EnchantAppraisalClass::FishingChestRare,
            480_000,
        ),
        (
            CanonicalEnchant::Mending,
            1,
            EnchantAppraisalClass::Mending,
            480_000,
        ),
        (
            CanonicalEnchant::Nuke,
            1,
            EnchantAppraisalClass::Mythic,
            1_200_000,
        ),
        (
            CanonicalEnchant::Empowering,
            1,
            EnchantAppraisalClass::SpecialMid,
            300_000,
        ),
        (
            CanonicalEnchant::Master,
            1,
            EnchantAppraisalClass::SpecialRare,
            720_000,
        ),
        (
            CanonicalEnchant::ShadowWalker,
            4,
            EnchantAppraisalClass::FishingChestMidHigh,
            945_000,
        ),
    ] {
        let appraisal = canonical_enchant_book_appraisal(enchant, level).unwrap();
        assert_eq!(appraisal.class, expected_class, "{enchant:?}");
        assert_eq!(appraisal.value, expected_value, "{enchant:?}");
    }
}

#[test]
fn canonical_identity_aggregate_matches_frozen_embedded_appraisal_math() {
    let enchants = [
        CanonicalEmbeddedEnchantAppraisalInput {
            enchant: CanonicalEnchant::Efficiency,
            level: 2,
        },
        CanonicalEmbeddedEnchantAppraisalInput {
            enchant: CanonicalEnchant::Mending,
            level: 1,
        },
        CanonicalEmbeddedEnchantAppraisalInput {
            enchant: CanonicalEnchant::Master,
            level: 3,
        },
    ];

    // 105,000 + 480,000 + 2,160,000 = 2,745,000; 70% = 1,921,500.
    assert_eq!(canonical_embedded_enchant_value(&enchants).unwrap(), 1_921_500);
    assert_eq!(canonical_embedded_enchant_value(&[]).unwrap(), 0);
}

#[test]
fn canonical_identity_bridge_fails_closed_on_invalid_resulting_levels() {
    assert_eq!(
        canonical_enchant_book_appraisal(CanonicalEnchant::Efficiency, 0),
        Err(EnchantAppraisalError::InvalidLevel(0))
    );
    assert_eq!(
        canonical_enchant_book_appraisal(CanonicalEnchant::Master, 11),
        Err(EnchantAppraisalError::InvalidLevel(11))
    );
    assert_eq!(
        canonical_embedded_enchant_value(&[CanonicalEmbeddedEnchantAppraisalInput {
            enchant: CanonicalEnchant::Nuke,
            level: 42,
        }]),
        Err(EnchantAppraisalError::InvalidLevel(42))
    );
}
