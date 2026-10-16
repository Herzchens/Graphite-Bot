use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{FishingBait, FishingTreasureResult};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FishingPayload {
    JunkDriftwood,
    JunkRustedCan,
    JunkTornNet,
    JunkBrokenHook,
    MaterialBundleCommon,
    MaterialBundleRefined,
    MaterialBundleDeep,
    WeatheredCrate,
    IronboundChest,
    SunkenCache,
    EnchantCatalystFragment,
    NormalSlotOrbFragment,
    SpecialSlotOrbFragment,
    SchoolBait,
    QualityBait,
    RareBait,
    TreasureBait,
    SturdyBait,
    AncientCoin,
    SeaGlass,
    FossilShell,
    AbyssalIdol,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FishingPayloadPolicy {
    pub payload: FishingPayload,
    pub item_definition_key: &'static str,
    pub quantity: u16,
    pub relative_weight: u16,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum FishingPayloadPolicyError {
    #[error("Enchant Book payloads are resolved by the direct-fishing book policy")]
    EnchantBookUsesDedicatedPolicy,
}

const JUNK_PAYLOADS: [FishingPayload; 4] = [
    FishingPayload::JunkDriftwood,
    FishingPayload::JunkRustedCan,
    FishingPayload::JunkTornNet,
    FishingPayload::JunkBrokenHook,
];

const MATERIAL_BUNDLE_PAYLOADS: [FishingPayload; 3] = [
    FishingPayload::MaterialBundleCommon,
    FishingPayload::MaterialBundleRefined,
    FishingPayload::MaterialBundleDeep,
];

const CRATE_OR_CHEST_PAYLOADS: [FishingPayload; 3] = [
    FishingPayload::WeatheredCrate,
    FishingPayload::IronboundChest,
    FishingPayload::SunkenCache,
];

const ORB_OR_CATALYST_PAYLOADS: [FishingPayload; 3] = [
    FishingPayload::EnchantCatalystFragment,
    FishingPayload::NormalSlotOrbFragment,
    FishingPayload::SpecialSlotOrbFragment,
];

const RARE_BAIT_OR_UTILITY_PAYLOADS: [FishingPayload; 5] = [
    FishingPayload::SchoolBait,
    FishingPayload::QualityBait,
    FishingPayload::RareBait,
    FishingPayload::TreasureBait,
    FishingPayload::SturdyBait,
];

const RELIC_OR_COLLECTIBLE_PAYLOADS: [FishingPayload; 4] = [
    FishingPayload::AncientCoin,
    FishingPayload::SeaGlass,
    FishingPayload::FossilShell,
    FishingPayload::AbyssalIdol,
];

#[must_use]
pub const fn fishing_payload_policy(payload: FishingPayload) -> FishingPayloadPolicy {
    use FishingPayload as P;

    let (item_definition_key, quantity, relative_weight) = match payload {
        P::JunkDriftwood => ("fishing.junk.driftwood", 1, 40),
        P::JunkRustedCan => ("fishing.junk.rusted_can", 1, 30),
        P::JunkTornNet => ("fishing.junk.torn_net", 1, 20),
        P::JunkBrokenHook => ("fishing.junk.broken_hook", 1, 10),

        P::MaterialBundleCommon => ("fishing.material_bundle.common", 1, 65),
        P::MaterialBundleRefined => ("fishing.material_bundle.refined", 1, 25),
        P::MaterialBundleDeep => ("fishing.material_bundle.deep", 1, 10),

        P::WeatheredCrate => ("fishing.container.weathered_crate", 1, 70),
        P::IronboundChest => ("fishing.container.ironbound_chest", 1, 25),
        P::SunkenCache => ("fishing.container.sunken_cache", 1, 5),

        P::EnchantCatalystFragment => ("fishing.fragment.enchant_catalyst", 1, 60),
        P::NormalSlotOrbFragment => ("fishing.fragment.normal_slot_orb", 1, 25),
        P::SpecialSlotOrbFragment => ("fishing.fragment.special_slot_orb", 1, 15),

        P::SchoolBait => ("fishing.bait.school", 1, 10),
        P::QualityBait => ("fishing.bait.quality", 1, 15),
        P::RareBait => ("fishing.bait.rare", 1, 30),
        P::TreasureBait => ("fishing.bait.treasure", 1, 25),
        P::SturdyBait => ("fishing.bait.sturdy", 1, 20),

        P::AncientCoin => ("fishing.relic.ancient_coin", 1, 50),
        P::SeaGlass => ("fishing.relic.sea_glass", 1, 30),
        P::FossilShell => ("fishing.relic.fossil_shell", 1, 15),
        P::AbyssalIdol => ("fishing.relic.abyssal_idol", 1, 5),
    };

    FishingPayloadPolicy {
        payload,
        item_definition_key,
        quantity,
        relative_weight,
    }
}

#[must_use]
pub const fn fishing_junk_payloads() -> &'static [FishingPayload] {
    &JUNK_PAYLOADS
}

pub const fn fishing_non_book_treasure_payloads(
    result: FishingTreasureResult,
) -> Result<&'static [FishingPayload], FishingPayloadPolicyError> {
    Ok(match result {
        FishingTreasureResult::MaterialBundle => &MATERIAL_BUNDLE_PAYLOADS,
        FishingTreasureResult::CrateOrChest => &CRATE_OR_CHEST_PAYLOADS,
        FishingTreasureResult::OrbOrCatalystFragmentOrItem => &ORB_OR_CATALYST_PAYLOADS,
        FishingTreasureResult::RareBaitOrUtilityItem => &RARE_BAIT_OR_UTILITY_PAYLOADS,
        FishingTreasureResult::RelicOrCollectible => &RELIC_OR_COLLECTIBLE_PAYLOADS,
        FishingTreasureResult::EnchantBook => {
            return Err(FishingPayloadPolicyError::EnchantBookUsesDedicatedPolicy);
        }
    })
}

#[must_use]
pub const fn fishing_bait_item_definition_key(bait: FishingBait) -> &'static str {
    match bait {
        FishingBait::School => "fishing.bait.school",
        FishingBait::Quality => "fishing.bait.quality",
        FishingBait::Rare => "fishing.bait.rare",
        FishingBait::Treasure => "fishing.bait.treasure",
        FishingBait::Sturdy => "fishing.bait.sturdy",
    }
}
