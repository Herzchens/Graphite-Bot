use graphite_core::CanonicalEnchant;
use serde::Serialize;

use crate::EnchantAppraisalClass;

pub const NORMAL_SHOP_MAX_BOOK_LEVEL: u8 = 5;
pub const BAIT_RACK_MAX_BOOK_LEVEL: u8 = crate::fishing_bait::BAIT_RACK_MAX_LEVEL;
/// Maximum valid resulting level for canonical embedded state.
///
/// Resulting-level policy deliberately remains in Services: Bait Rack III reuses the fishing-bait
/// effect module's `BAIT_RACK_MAX_LEVEL` authority instead of duplicating that gameplay ceiling in
/// the lower-layer canonical identity vocabulary.
#[must_use]
pub const fn canonical_enchant_max_resulting_level(enchant: CanonicalEnchant) -> u8 {
    use CanonicalEnchant as E;
    match enchant {
        E::Mending | E::Phoenix | E::Carving => 1,
        E::Master => 2,
        E::BaitRack => BAIT_RACK_MAX_BOOK_LEVEL,
        E::NineLife => 9,
        _ => 10,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EnchantAcquisitionSource {
    NormalShopFishingChest,
    FishingChestMidHigh,
    FishingChestRare,
    FishingOnly,
    FishingChestMythic,
    CombineMutationOnly,
    MasterProgression,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct EnchantCatalogPolicy {
    pub enchant: CanonicalEnchant,
    pub acquisition_source: EnchantAcquisitionSource,
    pub appraisal_class: EnchantAppraisalClass,
    pub normal_shop_max_book_level: Option<u8>,
}

impl EnchantCatalogPolicy {
    #[must_use]
    pub const fn normal_shop_eligible(self) -> bool {
        self.normal_shop_max_book_level.is_some()
    }
}

/// Resolves the frozen acquisition family and appraisal value class for one canonical enchant.
///
/// Acquisition source and appraisal class are deliberately separate. Several special/universal
/// enchants are eligible for the same acquisition channels as ordinary books but use different
/// canonical appraisal weights. Normal-Shop eligibility means the book may appear in controlled
/// Shop rotation; it does not promise that a particular weekly inventory contains it. The generic
/// Shop ceiling is Level V, while a more-specific enchant rule may lower its own Shop ceiling; Bait
/// Rack is the current frozen exception at Level III.
///
/// `Master` is one enchant identity with an existing [`crate::MasterEnchantTier`] progression. Its
/// exact tier-specific acquisition is owned by the Master policy: Master I is EXP-Shop-only and
/// Master II is upgrade-only from an existing Master I. The catalog therefore records the broader
/// `MasterProgression` source rather than duplicating the Master I/II state machine.
///
/// This policy does not resolve equipment compatibility, slot occupancy, direct fishing book-level
/// distributions, Shop price, combine success, ItemDefinition identity, or live inventory state.
#[must_use]
pub const fn enchant_catalog_policy(enchant: CanonicalEnchant) -> EnchantCatalogPolicy {
    use CanonicalEnchant as E;
    use EnchantAcquisitionSource as S;
    use EnchantAppraisalClass as A;

    let (acquisition_source, appraisal_class, normal_shop_max_book_level) = match enchant {
        E::BaitRack => (
            S::NormalShopFishingChest,
            A::ShopCommon,
            Some(BAIT_RACK_MAX_BOOK_LEVEL),
        ),

        E::Efficiency
        | E::Fortune
        | E::Smelt
        | E::Lure
        | E::LuckOfTheSea
        | E::Luck
        | E::Strengthen
        | E::SharpHook
        | E::Sharpness
        | E::Smite
        | E::BaneOfArthropods
        | E::SweepingEdge
        | E::FireAspect
        | E::Knockback
        | E::Protection
        | E::Thorn
        | E::Cat
        | E::Dog
        | E::Dodge
        | E::Unbreaking => (
            S::NormalShopFishingChest,
            A::ShopCommon,
            Some(NORMAL_SHOP_MAX_BOOK_LEVEL),
        ),

        E::Stabilize | E::Sparkling | E::Grinding | E::Mosaic => (
            S::NormalShopFishingChest,
            A::SpecialCommon,
            Some(NORMAL_SHOP_MAX_BOOK_LEVEL),
        ),

        E::PickaxeTreasure
        | E::FishingRodTreasure
        | E::MultiTreasure
        | E::Multicatch
        | E::Looting
        | E::Devour
        | E::Bleeding
        | E::Freezing
        | E::Angel
        | E::Evil
        | E::DayWalker
        | E::NightWalker
        | E::Reinforce => (S::FishingChestMidHigh, A::FishingChestMidHigh, None),

        E::Empowering | E::Carving => (S::FishingChestMidHigh, A::SpecialMid, None),

        E::Trench
        | E::Execution
        | E::BloodFrenzy
        | E::ArmorPiercing
        | E::Piercing
        | E::Guardian
        | E::NineLife
        | E::SoulGrind => (S::FishingChestRare, A::FishingChestRare, None),

        E::Mending => (S::FishingOnly, A::Mending, None),

        E::Nuke | E::Annihilation | E::Phoenix => (S::FishingChestMythic, A::Mythic, None),

        E::ShadowWalker => (S::CombineMutationOnly, A::FishingChestMidHigh, None),

        E::Master => (S::MasterProgression, A::SpecialRare, None),
    };

    EnchantCatalogPolicy {
        enchant,
        acquisition_source,
        appraisal_class,
        normal_shop_max_book_level,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_ENCHANTS: [CanonicalEnchant; graphite_core::CANONICAL_ENCHANT_COUNT] = [
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
        CanonicalEnchant::Trench,
        CanonicalEnchant::Execution,
        CanonicalEnchant::BloodFrenzy,
        CanonicalEnchant::ArmorPiercing,
        CanonicalEnchant::Piercing,
        CanonicalEnchant::Guardian,
        CanonicalEnchant::NineLife,
        CanonicalEnchant::SoulGrind,
        CanonicalEnchant::Mending,
        CanonicalEnchant::Nuke,
        CanonicalEnchant::Annihilation,
        CanonicalEnchant::Phoenix,
        CanonicalEnchant::ShadowWalker,
        CanonicalEnchant::Master,
    ];

    #[test]
    fn shop_common_and_special_common_share_source_but_not_appraisal_weight_class() {
        let efficiency = enchant_catalog_policy(CanonicalEnchant::Efficiency);
        assert_eq!(
            efficiency.acquisition_source,
            EnchantAcquisitionSource::NormalShopFishingChest
        );
        assert_eq!(
            efficiency.appraisal_class,
            EnchantAppraisalClass::ShopCommon
        );
        assert_eq!(efficiency.normal_shop_max_book_level, Some(5));
        assert!(efficiency.normal_shop_eligible());

        let grinding = enchant_catalog_policy(CanonicalEnchant::Grinding);
        assert_eq!(
            grinding.acquisition_source,
            EnchantAcquisitionSource::NormalShopFishingChest
        );
        assert_eq!(
            grinding.appraisal_class,
            EnchantAppraisalClass::SpecialCommon
        );
        assert_eq!(grinding.normal_shop_max_book_level, Some(5));
    }

    #[test]
    fn bait_rack_preserves_the_specific_level_three_shop_ceiling() {
        let bait_rack = enchant_catalog_policy(CanonicalEnchant::BaitRack);
        assert_eq!(BAIT_RACK_MAX_BOOK_LEVEL, 3);
        assert_eq!(
            bait_rack.acquisition_source,
            EnchantAcquisitionSource::NormalShopFishingChest
        );
        assert_eq!(bait_rack.appraisal_class, EnchantAppraisalClass::ShopCommon);
        assert_eq!(bait_rack.normal_shop_max_book_level, Some(3));
        assert!(bait_rack.normal_shop_eligible());
    }

    #[test]
    fn loot_specials_preserve_special_appraisal_classes() {
        let looting = enchant_catalog_policy(CanonicalEnchant::Looting);
        assert_eq!(
            looting.acquisition_source,
            EnchantAcquisitionSource::FishingChestMidHigh
        );
        assert_eq!(
            looting.appraisal_class,
            EnchantAppraisalClass::FishingChestMidHigh
        );

        let empowering = enchant_catalog_policy(CanonicalEnchant::Empowering);
        assert_eq!(
            empowering.acquisition_source,
            EnchantAcquisitionSource::FishingChestMidHigh
        );
        assert_eq!(
            empowering.appraisal_class,
            EnchantAppraisalClass::SpecialMid
        );

        let soul_grind = enchant_catalog_policy(CanonicalEnchant::SoulGrind);
        assert_eq!(
            soul_grind.acquisition_source,
            EnchantAcquisitionSource::FishingChestRare
        );
        assert_eq!(
            soul_grind.appraisal_class,
            EnchantAppraisalClass::FishingChestRare
        );
    }

    #[test]
    fn dedicated_sources_map_to_their_frozen_appraisal_classes() {
        let mending = enchant_catalog_policy(CanonicalEnchant::Mending);
        assert_eq!(
            mending.acquisition_source,
            EnchantAcquisitionSource::FishingOnly
        );
        assert_eq!(mending.appraisal_class, EnchantAppraisalClass::Mending);

        let nuke = enchant_catalog_policy(CanonicalEnchant::Nuke);
        assert_eq!(
            nuke.acquisition_source,
            EnchantAcquisitionSource::FishingChestMythic
        );
        assert_eq!(nuke.appraisal_class, EnchantAppraisalClass::Mythic);

        let shadow = enchant_catalog_policy(CanonicalEnchant::ShadowWalker);
        assert_eq!(
            shadow.acquisition_source,
            EnchantAcquisitionSource::CombineMutationOnly
        );
        assert_eq!(
            shadow.appraisal_class,
            EnchantAppraisalClass::FishingChestMidHigh
        );
    }

    #[test]
    fn treasure_contexts_are_distinct_but_share_mid_high_policy() {
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

    #[test]
    fn master_identity_reuses_the_existing_tier_progression_authority() {
        let master = enchant_catalog_policy(CanonicalEnchant::Master);
        assert_eq!(
            master.acquisition_source,
            EnchantAcquisitionSource::MasterProgression
        );
        assert_eq!(master.appraisal_class, EnchantAppraisalClass::SpecialRare);
        assert!(!master.normal_shop_eligible());
    }

    #[test]
    fn persistence_keys_round_trip_and_match_the_existing_serialization_contract() {
        for enchant in ALL_ENCHANTS {
            assert_eq!(
                CanonicalEnchant::from_persisted_key(enchant.persisted_key()),
                Some(enchant)
            );
            assert_eq!(
                serde_json::to_value(enchant).unwrap(),
                serde_json::Value::String(enchant.persisted_key().to_owned())
            );
        }
        for invalid in ["", "sharpness", " SHARPNESS", "SHARPNESS ", "TREASURE"] {
            assert_eq!(CanonicalEnchant::from_persisted_key(invalid), None);
        }
    }

    #[test]
    fn resulting_level_ceiling_preserves_frozen_fixed_level_exceptions() {
        assert_eq!(
            canonical_enchant_max_resulting_level(CanonicalEnchant::Mending),
            1
        );
        assert_eq!(
            canonical_enchant_max_resulting_level(CanonicalEnchant::Phoenix),
            1
        );
        assert_eq!(
            canonical_enchant_max_resulting_level(CanonicalEnchant::Carving),
            1
        );
        assert_eq!(
            canonical_enchant_max_resulting_level(CanonicalEnchant::Master),
            2
        );
        assert_eq!(
            canonical_enchant_max_resulting_level(CanonicalEnchant::BaitRack),
            3
        );
        assert_eq!(
            canonical_enchant_max_resulting_level(CanonicalEnchant::NineLife),
            9
        );
        assert_eq!(
            canonical_enchant_max_resulting_level(CanonicalEnchant::ShadowWalker),
            10
        );
        assert_eq!(
            canonical_enchant_max_resulting_level(CanonicalEnchant::Efficiency),
            10
        );
    }
}
