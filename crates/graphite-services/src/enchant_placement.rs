use serde::Serialize;

use crate::{CanonicalEnchant, EquipmentSlot};

const SLOT_PICKAXE: u8 = 1 << 0;
const SLOT_SWORD: u8 = 1 << 1;
const SLOT_FISHING_ROD: u8 = 1 << 2;
const SLOT_HELMET: u8 = 1 << 3;
const SLOT_CHESTPLATE: u8 = 1 << 4;
const SLOT_LEGGINGS: u8 = 1 << 5;
const SLOT_BOOTS: u8 = 1 << 6;
const SLOT_ALL_ARMOR: u8 = SLOT_HELMET | SLOT_CHESTPLATE | SLOT_LEGGINGS | SLOT_BOOTS;
const SLOT_ALL_EQUIPMENT: u8 = SLOT_PICKAXE | SLOT_SWORD | SLOT_FISHING_ROD | SLOT_ALL_ARMOR;

pub const NORMAL_CLASS_NATIVE_SLOTS: u8 = 4;
pub const SPECIAL_UNIVERSAL_NATIVE_SLOTS: u8 = 3;
pub const MAX_ENCHANT_SLOTS_PER_FAMILY: u8 = 6;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EnchantSlotFamily {
    NormalClass,
    SpecialUniversal,
}

impl EnchantSlotFamily {
    #[must_use]
    pub const fn native_slot_count(self) -> u8 {
        match self {
            Self::NormalClass => NORMAL_CLASS_NATIVE_SLOTS,
            Self::SpecialUniversal => SPECIAL_UNIVERSAL_NATIVE_SLOTS,
        }
    }

    #[must_use]
    pub const fn maximum_slot_count(self) -> u8 {
        MAX_ENCHANT_SLOTS_PER_FAMILY
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct EnchantEquipmentMask(u8);

impl EnchantEquipmentMask {
    #[must_use]
    pub const fn contains(self, slot: EquipmentSlot) -> bool {
        self.0 & slot_bit(slot) != 0
    }

    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct EnchantPlacementPolicy {
    pub enchant: CanonicalEnchant,
    pub slot_family: EnchantSlotFamily,
    pub equipment_mask: EnchantEquipmentMask,
}

impl EnchantPlacementPolicy {
    #[must_use]
    pub const fn applies_to(self, slot: EquipmentSlot) -> bool {
        self.equipment_mask.contains(slot)
    }
}

/// Resolves the frozen slot family and ordinary-equipment placement mask for one canonical enchant.
///
/// Normal/class enchants follow the per-equipment catalogs in the master specification. Armor
/// entries with an explicit body-part tag are restricted to that slot: Cat/Dog/Day Walker/Night
/// Walker/Shadow Walker to Boots, Dodge to Leggings, Guardian/Phoenix to Chestplate, and Angel/Evil
/// to Helmet. Untagged armor entries apply to armor generally. Unbreaking and Mending are listed in
/// every ordinary equipment catalog and therefore span all seven ordinary equipment slots.
///
/// Stabilize, Sparkling, Empowering, Grinding, Mosaic, Carving, and Master are the complete current
/// special/universal family and use the Special/universal slot family across ordinary enchantable
/// equipment. This pure policy does not prove that an ItemInstance is enchantable, that a physical
/// enchant slot is unlocked/free, that conflicts are satisfied, or that the caller owns a valid
/// Enchant Book. Those are lifecycle checks for the future Enchant mutation owner.
#[must_use]
pub const fn enchant_placement_policy(enchant: CanonicalEnchant) -> EnchantPlacementPolicy {
    use CanonicalEnchant as E;
    use EnchantSlotFamily as F;

    let (slot_family, equipment_mask) = match enchant {
        E::Stabilize
        | E::Sparkling
        | E::Empowering
        | E::Grinding
        | E::Mosaic
        | E::Carving
        | E::Master => (F::SpecialUniversal, SLOT_ALL_EQUIPMENT),

        E::Unbreaking | E::Mending => (F::NormalClass, SLOT_ALL_EQUIPMENT),

        E::Efficiency | E::Fortune | E::PickaxeTreasure | E::Trench | E::Nuke | E::Smelt => {
            (F::NormalClass, SLOT_PICKAXE)
        }

        E::Lure
        | E::LuckOfTheSea
        | E::FishingRodTreasure
        | E::MultiTreasure
        | E::Luck
        | E::Multicatch
        | E::Strengthen
        | E::SharpHook
        | E::BaitRack => (F::NormalClass, SLOT_FISHING_ROD),

        E::Sharpness
        | E::Smite
        | E::BaneOfArthropods
        | E::SweepingEdge
        | E::FireAspect
        | E::Looting
        | E::Knockback
        | E::Devour
        | E::Execution
        | E::Bleeding
        | E::BloodFrenzy
        | E::ArmorPiercing
        | E::Piercing
        | E::Freezing
        | E::Annihilation => (F::NormalClass, SLOT_SWORD),

        E::Protection | E::Thorn | E::NineLife | E::Reinforce | E::SoulGrind => {
            (F::NormalClass, SLOT_ALL_ARMOR)
        }
        E::Cat | E::Dog | E::ShadowWalker | E::NightWalker | E::DayWalker => {
            (F::NormalClass, SLOT_BOOTS)
        }
        E::Dodge => (F::NormalClass, SLOT_LEGGINGS),
        E::Guardian | E::Phoenix => (F::NormalClass, SLOT_CHESTPLATE),
        E::Angel | E::Evil => (F::NormalClass, SLOT_HELMET),
    };

    EnchantPlacementPolicy {
        enchant,
        slot_family,
        equipment_mask: EnchantEquipmentMask(equipment_mask),
    }
}

const fn slot_bit(slot: EquipmentSlot) -> u8 {
    match slot {
        EquipmentSlot::Pickaxe => SLOT_PICKAXE,
        EquipmentSlot::Sword => SLOT_SWORD,
        EquipmentSlot::FishingRod => SLOT_FISHING_ROD,
        EquipmentSlot::Helmet => SLOT_HELMET,
        EquipmentSlot::Chestplate => SLOT_CHESTPLATE,
        EquipmentSlot::Leggings => SLOT_LEGGINGS,
        EquipmentSlot::Boots => SLOT_BOOTS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_SLOTS: [EquipmentSlot; 7] = [
        EquipmentSlot::Pickaxe,
        EquipmentSlot::Sword,
        EquipmentSlot::FishingRod,
        EquipmentSlot::Helmet,
        EquipmentSlot::Chestplate,
        EquipmentSlot::Leggings,
        EquipmentSlot::Boots,
    ];

    #[test]
    fn slot_family_capacity_matches_the_frozen_two_family_model() {
        assert_eq!(EnchantSlotFamily::NormalClass.native_slot_count(), 4);
        assert_eq!(EnchantSlotFamily::SpecialUniversal.native_slot_count(), 3);
        assert_eq!(EnchantSlotFamily::NormalClass.maximum_slot_count(), 6);
        assert_eq!(EnchantSlotFamily::SpecialUniversal.maximum_slot_count(), 6);
    }

    #[test]
    fn special_universal_family_is_exact_and_applies_across_ordinary_equipment() {
        let special = [
            CanonicalEnchant::Stabilize,
            CanonicalEnchant::Sparkling,
            CanonicalEnchant::Empowering,
            CanonicalEnchant::Grinding,
            CanonicalEnchant::Mosaic,
            CanonicalEnchant::Carving,
            CanonicalEnchant::Master,
        ];

        for enchant in special {
            let policy = enchant_placement_policy(enchant);
            assert_eq!(policy.slot_family, EnchantSlotFamily::SpecialUniversal);
            for slot in ALL_SLOTS {
                assert!(
                    policy.applies_to(slot),
                    "{enchant:?} must apply to {slot:?}"
                );
            }
        }
    }

    #[test]
    fn shared_normal_enchants_span_all_ordinary_equipment_but_stay_normal_class() {
        for enchant in [CanonicalEnchant::Unbreaking, CanonicalEnchant::Mending] {
            let policy = enchant_placement_policy(enchant);
            assert_eq!(policy.slot_family, EnchantSlotFamily::NormalClass);
            for slot in ALL_SLOTS {
                assert!(
                    policy.applies_to(slot),
                    "{enchant:?} must apply to {slot:?}"
                );
            }
        }
    }

    #[test]
    fn tool_class_enchants_match_the_frozen_catalogs_exactly() {
        for enchant in [
            CanonicalEnchant::Efficiency,
            CanonicalEnchant::Fortune,
            CanonicalEnchant::PickaxeTreasure,
            CanonicalEnchant::Trench,
            CanonicalEnchant::Nuke,
            CanonicalEnchant::Smelt,
        ] {
            assert_only_slots(enchant, &[EquipmentSlot::Pickaxe]);
        }

        for enchant in [
            CanonicalEnchant::Lure,
            CanonicalEnchant::LuckOfTheSea,
            CanonicalEnchant::FishingRodTreasure,
            CanonicalEnchant::MultiTreasure,
            CanonicalEnchant::Luck,
            CanonicalEnchant::Multicatch,
            CanonicalEnchant::Strengthen,
            CanonicalEnchant::SharpHook,
            CanonicalEnchant::BaitRack,
        ] {
            assert_only_slots(enchant, &[EquipmentSlot::FishingRod]);
        }

        for enchant in [
            CanonicalEnchant::Sharpness,
            CanonicalEnchant::Smite,
            CanonicalEnchant::BaneOfArthropods,
            CanonicalEnchant::SweepingEdge,
            CanonicalEnchant::FireAspect,
            CanonicalEnchant::Looting,
            CanonicalEnchant::Knockback,
            CanonicalEnchant::Devour,
            CanonicalEnchant::Execution,
            CanonicalEnchant::Bleeding,
            CanonicalEnchant::BloodFrenzy,
            CanonicalEnchant::ArmorPiercing,
            CanonicalEnchant::Piercing,
            CanonicalEnchant::Freezing,
            CanonicalEnchant::Annihilation,
        ] {
            assert_only_slots(enchant, &[EquipmentSlot::Sword]);
        }
    }

    #[test]
    fn armor_body_part_restrictions_match_the_frozen_effect_table() {
        let all_armor = [
            EquipmentSlot::Helmet,
            EquipmentSlot::Chestplate,
            EquipmentSlot::Leggings,
            EquipmentSlot::Boots,
        ];
        for enchant in [
            CanonicalEnchant::Protection,
            CanonicalEnchant::Thorn,
            CanonicalEnchant::NineLife,
            CanonicalEnchant::Reinforce,
            CanonicalEnchant::SoulGrind,
        ] {
            assert_only_slots(enchant, &all_armor);
        }

        for enchant in [
            CanonicalEnchant::Cat,
            CanonicalEnchant::Dog,
            CanonicalEnchant::ShadowWalker,
            CanonicalEnchant::NightWalker,
            CanonicalEnchant::DayWalker,
        ] {
            assert_only_slots(enchant, &[EquipmentSlot::Boots]);
        }
        assert_only_slots(CanonicalEnchant::Dodge, &[EquipmentSlot::Leggings]);
        for enchant in [CanonicalEnchant::Guardian, CanonicalEnchant::Phoenix] {
            assert_only_slots(enchant, &[EquipmentSlot::Chestplate]);
        }
        for enchant in [CanonicalEnchant::Angel, CanonicalEnchant::Evil] {
            assert_only_slots(enchant, &[EquipmentSlot::Helmet]);
        }
    }

    fn assert_only_slots(enchant: CanonicalEnchant, expected: &[EquipmentSlot]) {
        let policy = enchant_placement_policy(enchant);
        assert_eq!(policy.slot_family, EnchantSlotFamily::NormalClass);
        for slot in ALL_SLOTS {
            assert_eq!(
                policy.applies_to(slot),
                expected.contains(&slot),
                "unexpected placement for {enchant:?} on {slot:?}"
            );
        }
    }
}
