use serde::Serialize;

use crate::enchant_appraisal::embedded_enchant_value_from_book_appraisals;
use crate::{
    CanonicalBookAppraisal, EnchantAppraisalClass, EnchantAppraisalError, canonical_book_appraisal,
};

pub const NORMAL_SHOP_MAX_BOOK_LEVEL: u8 = 5;
pub const BAIT_RACK_MAX_BOOK_LEVEL: u8 = crate::fishing_bait::BAIT_RACK_MAX_LEVEL;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CanonicalEnchant {
    Efficiency,
    Fortune,
    Smelt,
    Lure,
    LuckOfTheSea,
    Luck,
    Strengthen,
    SharpHook,
    BaitRack,
    Sharpness,
    Smite,
    BaneOfArthropods,
    SweepingEdge,
    FireAspect,
    Knockback,
    Protection,
    Thorn,
    Cat,
    Dog,
    Dodge,
    Unbreaking,
    Stabilize,
    Sparkling,
    Grinding,
    Mosaic,
    PickaxeTreasure,
    FishingRodTreasure,
    MultiTreasure,
    Multicatch,
    Looting,
    Devour,
    Bleeding,
    Freezing,
    Angel,
    Evil,
    DayWalker,
    NightWalker,
    Reinforce,
    Empowering,
    Carving,
    Trench,
    Execution,
    BloodFrenzy,
    ArmorPiercing,
    Piercing,
    Guardian,
    NineLife,
    SoulGrind,
    Mending,
    Nuke,
    Annihilation,
    Phoenix,
    ShadowWalker,
    Master,
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

/// Concrete canonical enchant identity plus its already-resolved resulting Level I-X.
///
/// This type deliberately carries no caller-supplied appraisal class. The class is derived from
/// [`enchant_catalog_policy`] at appraisal time so identity and canonical value classification
/// cannot drift independently.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct CanonicalEmbeddedEnchantAppraisalInput {
    pub enchant: CanonicalEnchant,
    pub level: u8,
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

/// Resolves canonical book appraisal from a concrete canonical enchant identity.
///
/// The appraisal class is always derived from [`enchant_catalog_policy`]. Callers cannot relabel an
/// ordinary enchant as Mythic/Special or otherwise choose a more valuable class independently from
/// its identity. Resulting-level validation and numeric appraisal math remain owned by
/// [`canonical_book_appraisal`].
///
/// This pure bridge does not prove that the enchant is actually embedded in a particular item, does
/// not resolve Master tier state into a resulting level, and does not persist or mutate enchant
/// slots. Those remain responsibilities of the future authoritative ItemInstance enchant owner.
pub fn canonical_enchant_book_appraisal(
    enchant: CanonicalEnchant,
    level: u8,
) -> Result<CanonicalBookAppraisal, EnchantAppraisalError> {
    let policy = enchant_catalog_policy(enchant);
    canonical_book_appraisal(policy.appraisal_class, level)
}

/// Computes canonical embedded-enchant contribution from concrete canonical enchant identities.
///
/// This is the identity-aware counterpart to the lower-level class-based appraisal API. Every
/// element derives its class from the catalog and then shares the same checked book summation plus
/// frozen 70%-round-half-up accumulator as [`crate::embedded_enchant_value`]. No intermediate
/// allocation is required.
///
/// Compatibility, slot occupancy, provenance, ItemInstance ownership, and persistence are outside
/// this pure bridge and must be supplied by a future authoritative enchant-state owner.
pub fn canonical_embedded_enchant_value(
    enchants: &[CanonicalEmbeddedEnchantAppraisalInput],
) -> Result<i64, EnchantAppraisalError> {
    embedded_enchant_value_from_book_appraisals(enchants.iter().map(|enchant| {
        canonical_enchant_book_appraisal(enchant.enchant, enchant.level)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn identity_aware_book_appraisal_derives_class_from_catalog() {
        for (enchant, level, class, value) in [
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
            assert_eq!(appraisal.class, class, "{enchant:?}");
            assert_eq!(appraisal.value, value, "{enchant:?}");
        }
    }

    #[test]
    fn identity_aware_embedded_value_reuses_the_exact_seventy_percent_accumulator() {
        let inputs = [
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
        assert_eq!(canonical_embedded_enchant_value(&inputs).unwrap(), 1_921_500);
        assert_eq!(canonical_embedded_enchant_value(&[]).unwrap(), 0);
    }

    #[test]
    fn identity_aware_bridge_preserves_resulting_level_validation() {
        assert_eq!(
            canonical_enchant_book_appraisal(CanonicalEnchant::Efficiency, 0),
            Err(EnchantAppraisalError::InvalidLevel(0))
        );
        assert_eq!(
            canonical_embedded_enchant_value(&[CanonicalEmbeddedEnchantAppraisalInput {
                enchant: CanonicalEnchant::Nuke,
                level: 11,
            }]),
            Err(EnchantAppraisalError::InvalidLevel(11))
        );
    }
}
