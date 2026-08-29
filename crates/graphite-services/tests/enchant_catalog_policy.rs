use graphite_services::{
    CanonicalEnchant, EnchantAcquisitionSource, EnchantAppraisalClass, MasterAcquisitionSource,
    MasterEnchantTier, NORMAL_SHOP_MAX_BOOK_LEVEL, enchant_catalog_policy,
    master_i_purchase_policy, preview_master_ii_upgrade,
};

#[test]
fn public_api_separates_acquisition_source_from_appraisal_class() {
    let common = enchant_catalog_policy(CanonicalEnchant::Efficiency);
    assert_eq!(
        common.acquisition_source,
        EnchantAcquisitionSource::NormalShopFishingChest
    );
    assert_eq!(common.appraisal_class, EnchantAppraisalClass::ShopCommon);

    let special_common = enchant_catalog_policy(CanonicalEnchant::Grinding);
    assert_eq!(
        special_common.acquisition_source,
        EnchantAcquisitionSource::NormalShopFishingChest
    );
    assert_eq!(
        special_common.appraisal_class,
        EnchantAppraisalClass::SpecialCommon
    );

    let special_mid = enchant_catalog_policy(CanonicalEnchant::Empowering);
    assert_eq!(
        special_mid.acquisition_source,
        EnchantAcquisitionSource::FishingChestMidHigh
    );
    assert_eq!(
        special_mid.appraisal_class,
        EnchantAppraisalClass::SpecialMid
    );
}

#[test]
fn public_api_freezes_normal_shop_level_five_ceiling_without_promising_stock() {
    assert_eq!(NORMAL_SHOP_MAX_BOOK_LEVEL, 5);

    for enchant in [
        CanonicalEnchant::Efficiency,
        CanonicalEnchant::Unbreaking,
        CanonicalEnchant::Stabilize,
        CanonicalEnchant::Mosaic,
    ] {
        let policy = enchant_catalog_policy(enchant);
        assert!(policy.normal_shop_eligible());
        assert_eq!(policy.normal_shop_max_book_level, Some(5));
    }

    for enchant in [
        CanonicalEnchant::PickaxeTreasure,
        CanonicalEnchant::Mending,
        CanonicalEnchant::Nuke,
        CanonicalEnchant::ShadowWalker,
        CanonicalEnchant::Master,
    ] {
        let policy = enchant_catalog_policy(enchant);
        assert!(!policy.normal_shop_eligible());
        assert_eq!(policy.normal_shop_max_book_level, None);
    }
}

#[test]
fn public_api_preserves_dedicated_and_mutation_only_sources() {
    assert_eq!(
        enchant_catalog_policy(CanonicalEnchant::Mending).acquisition_source,
        EnchantAcquisitionSource::FishingOnly
    );
    assert_eq!(
        enchant_catalog_policy(CanonicalEnchant::Nuke).acquisition_source,
        EnchantAcquisitionSource::FishingChestMythic
    );
    assert_eq!(
        enchant_catalog_policy(CanonicalEnchant::ShadowWalker).acquisition_source,
        EnchantAcquisitionSource::CombineMutationOnly
    );
    assert_eq!(
        enchant_catalog_policy(CanonicalEnchant::ShadowWalker).appraisal_class,
        EnchantAppraisalClass::FishingChestMidHigh
    );
}

#[test]
fn public_api_delegates_master_tier_progression_to_existing_master_policy() {
    let master = enchant_catalog_policy(CanonicalEnchant::Master);
    assert_eq!(
        master.acquisition_source,
        EnchantAcquisitionSource::MasterProgression
    );
    assert_eq!(master.appraisal_class, EnchantAppraisalClass::SpecialRare);

    let master_i = master_i_purchase_policy();
    assert_eq!(master_i.tier, MasterEnchantTier::MasterI);
    assert_eq!(
        master_i.acquisition_source,
        MasterAcquisitionSource::ExpShopOnly
    );

    let master_ii = preview_master_ii_upgrade(MasterEnchantTier::MasterI).unwrap();
    assert_eq!(master_ii.to, MasterEnchantTier::MasterII);
    assert_eq!(
        master_ii.acquisition_source,
        MasterAcquisitionSource::UpgradeOnlyFromMasterI
    );
}

#[test]
fn public_api_keeps_pickaxe_and_rod_treasure_identities_distinct() {
    let pickaxe = enchant_catalog_policy(CanonicalEnchant::PickaxeTreasure);
    let rod = enchant_catalog_policy(CanonicalEnchant::FishingRodTreasure);

    assert_ne!(pickaxe.enchant, rod.enchant);
    assert_eq!(
        pickaxe.acquisition_source,
        EnchantAcquisitionSource::FishingChestMidHigh
    );
    assert_eq!(
        rod.acquisition_source,
        EnchantAcquisitionSource::FishingChestMidHigh
    );
    assert_eq!(
        pickaxe.appraisal_class,
        EnchantAppraisalClass::FishingChestMidHigh
    );
    assert_eq!(
        rod.appraisal_class,
        EnchantAppraisalClass::FishingChestMidHigh
    );
}
