use serde::Serialize;
use thiserror::Error;

use crate::{
    CanonicalEnchant, EnchantConflictScope, EnchantSlotFamily, EquipmentSlot,
    NORMAL_CLASS_NATIVE_SLOTS, SPECIAL_UNIVERSAL_NATIVE_SLOTS, canonical_enchant_conflict_scope,
    enchant_placement_policy,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct EnchantSlotCapacity {
    pub normal_class: u8,
    pub special_universal: u8,
}

impl EnchantSlotCapacity {
    #[must_use]
    pub const fn native() -> Self {
        Self {
            normal_class: NORMAL_CLASS_NATIVE_SLOTS,
            special_universal: SPECIAL_UNIVERSAL_NATIVE_SLOTS,
        }
    }

    pub fn try_new(normal_class: u8, special_universal: u8) -> Result<Self, EnchantApplyError> {
        validate_unlocked_slot_count(EnchantSlotFamily::NormalClass, normal_class)?;
        validate_unlocked_slot_count(EnchantSlotFamily::SpecialUniversal, special_universal)?;
        Ok(Self {
            normal_class,
            special_universal,
        })
    }

    #[must_use]
    pub const fn for_family(self, family: EnchantSlotFamily) -> u8 {
        match family {
            EnchantSlotFamily::NormalClass => self.normal_class,
            EnchantSlotFamily::SpecialUniversal => self.special_universal,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct EnchantSlotOccupancy {
    pub normal_class: u8,
    pub special_universal: u8,
}

impl EnchantSlotOccupancy {
    #[must_use]
    pub const fn for_family(self, family: EnchantSlotFamily) -> u8 {
        match family {
            EnchantSlotFamily::NormalClass => self.normal_class,
            EnchantSlotFamily::SpecialUniversal => self.special_universal,
        }
    }

    const fn increment(self, family: EnchantSlotFamily) -> Option<Self> {
        match family {
            EnchantSlotFamily::NormalClass => match self.normal_class.checked_add(1) {
                Some(normal_class) => Some(Self {
                    normal_class,
                    special_universal: self.special_universal,
                }),
                None => None,
            },
            EnchantSlotFamily::SpecialUniversal => match self.special_universal.checked_add(1) {
                Some(special_universal) => Some(Self {
                    normal_class: self.normal_class,
                    special_universal,
                }),
                None => None,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ExistingAppliedEnchant {
    pub enchant: CanonicalEnchant,
    pub level: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EnchantApplyAction {
    InsertNew,
    UpgradeExisting { previous_level: u8 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct EnchantApplyPreview {
    pub enchant: CanonicalEnchant,
    pub level: u8,
    pub slot_family: EnchantSlotFamily,
    pub action: EnchantApplyAction,
    pub occupancy_before: EnchantSlotOccupancy,
    pub occupancy_after: EnchantSlotOccupancy,
    pub incoming_finished_book_consumed_on_commit: bool,
    pub success_guaranteed_after_authoritative_revalidation: bool,
    pub resulting_item_requires_equipped_armor_loadout_conflict_validation: bool,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum EnchantApplyError {
    #[error(
        "unlocked {family:?} slot count {current} is outside the supported range {minimum}..={maximum}"
    )]
    InvalidUnlockedSlotCount {
        family: EnchantSlotFamily,
        current: u8,
        minimum: u8,
        maximum: u8,
    },
    #[error("{enchant:?} level {level} exceeds its supported resulting level 1..={maximum}")]
    InvalidEnchantLevel {
        enchant: CanonicalEnchant,
        level: u8,
        maximum: u8,
    },
    #[error("existing enchant state contains duplicate identity {0:?}")]
    DuplicateExistingEnchant(CanonicalEnchant),
    #[error("existing enchant {enchant:?} cannot be placed on equipment slot {slot:?}")]
    ExistingEnchantWrongEquipmentSlot {
        enchant: CanonicalEnchant,
        slot: EquipmentSlot,
    },
    #[error("incoming enchant {enchant:?} cannot be placed on equipment slot {slot:?}")]
    IncomingEnchantWrongEquipmentSlot {
        enchant: CanonicalEnchant,
        slot: EquipmentSlot,
    },
    #[error(
        "existing {family:?} enchant occupancy {occupied} exceeds unlocked capacity {unlocked}"
    )]
    ExistingOccupancyExceedsCapacity {
        family: EnchantSlotFamily,
        occupied: u8,
        unlocked: u8,
    },
    #[error(
        "existing item state contains conflicting enchants {left:?} and {right:?} at scope {scope:?}"
    )]
    ExistingItemConflict {
        left: CanonicalEnchant,
        right: CanonicalEnchant,
        scope: EnchantConflictScope,
    },
    #[error(
        "incoming enchant {incoming:?} conflicts with existing enchant {existing:?} at scope {scope:?}"
    )]
    IncomingConflict {
        incoming: CanonicalEnchant,
        existing: CanonicalEnchant,
        scope: EnchantConflictScope,
    },
    #[error(
        "incoming {enchant:?} level {incoming_level} must be greater than existing level {existing_level}"
    )]
    LowerOrEqualReplacement {
        enchant: CanonicalEnchant,
        existing_level: u8,
        incoming_level: u8,
    },
    #[error("no free {family:?} enchant slot: occupied {occupied} of {unlocked} unlocked slots")]
    NoFreeSlot {
        family: EnchantSlotFamily,
        occupied: u8,
        unlocked: u8,
    },
    #[error("enchant occupancy arithmetic exceeded supported integer bounds")]
    OccupancyOverflow,
}

/// Previews standard finished-book application against one already-resolved equipment item.
///
/// The active specification freezes two independent slot families, rejects lower/equal replacement
/// before consuming the incoming book, and makes an otherwise valid standard finished-book
/// application guaranteed. This pure preflight therefore performs no RNG and never consumes a book.
/// A successful owning transaction consumes the incoming finished book exactly once after repeating
/// these checks against authoritative locked state.
///
/// `capacity` is the already-resolved number of currently unlocked slots on this ItemInstance. This
/// API deliberately chooses no persistence representation for Slot-Orb unlock state. `existing`
/// likewise represents already-resolved embedded enchant identities/levels; acquisition provenance
/// for the incoming finished book remains the owning lifecycle's responsibility.
///
/// Conflict pairs are rejected on the same item regardless of whether the canonical conflict scope
/// is `SameItem` or `EquippedArmorLoadout`: putting two survival-core enchants on one armor item would
/// make that item intrinsically invalid whenever equipped. Cross-item Guardian/Nine Life/Phoenix
/// validation still needs authoritative equipped-loadout state and is therefore surfaced through
/// `resulting_item_requires_equipped_armor_loadout_conflict_validation` rather than guessed here.
/// `LOADOUT_UNIQUE` effect semantics such as Reinforce are not reinterpreted as an application ban.
///
/// This is the standard finished-book path only. Any enchant that receives a separately specified
/// mutation-application lifecycle must be routed through that owning policy instead of treating this
/// preview as authorization. `/enchant` remains unavailable until persistence, ownership, atomic
/// book mutation, loadout checks, appraisal/SoulBind top-up interaction, and idempotent settlement
/// are implemented together.
pub fn preview_standard_finished_book_application(
    equipment_slot: EquipmentSlot,
    capacity: EnchantSlotCapacity,
    existing: &[ExistingAppliedEnchant],
    incoming_enchant: CanonicalEnchant,
    incoming_level: u8,
) -> Result<EnchantApplyPreview, EnchantApplyError> {
    validate_capacity(capacity)?;
    validate_enchant_level(incoming_enchant, incoming_level)?;

    let incoming_policy = enchant_placement_policy(incoming_enchant);
    if !incoming_policy.applies_to(equipment_slot) {
        return Err(EnchantApplyError::IncomingEnchantWrongEquipmentSlot {
            enchant: incoming_enchant,
            slot: equipment_slot,
        });
    }

    let occupancy_before = validate_existing_state(equipment_slot, capacity, existing)?;
    let mut existing_same = None;

    for applied in existing {
        if applied.enchant == incoming_enchant {
            existing_same = Some(*applied);
            continue;
        }
        if let Some(scope) = canonical_enchant_conflict_scope(incoming_enchant, applied.enchant) {
            return Err(EnchantApplyError::IncomingConflict {
                incoming: incoming_enchant,
                existing: applied.enchant,
                scope,
            });
        }
    }

    let (action, occupancy_after) = match existing_same {
        Some(applied) => {
            if incoming_level <= applied.level {
                return Err(EnchantApplyError::LowerOrEqualReplacement {
                    enchant: incoming_enchant,
                    existing_level: applied.level,
                    incoming_level,
                });
            }
            (
                EnchantApplyAction::UpgradeExisting {
                    previous_level: applied.level,
                },
                occupancy_before,
            )
        }
        None => {
            let family = incoming_policy.slot_family;
            let occupied = occupancy_before.for_family(family);
            let unlocked = capacity.for_family(family);
            if occupied >= unlocked {
                return Err(EnchantApplyError::NoFreeSlot {
                    family,
                    occupied,
                    unlocked,
                });
            }
            (
                EnchantApplyAction::InsertNew,
                occupancy_before
                    .increment(family)
                    .ok_or(EnchantApplyError::OccupancyOverflow)?,
            )
        }
    };

    Ok(EnchantApplyPreview {
        enchant: incoming_enchant,
        level: incoming_level,
        slot_family: incoming_policy.slot_family,
        action,
        occupancy_before,
        occupancy_after,
        incoming_finished_book_consumed_on_commit: true,
        success_guaranteed_after_authoritative_revalidation: true,
        resulting_item_requires_equipped_armor_loadout_conflict_validation:
            resulting_item_contains_loadout_scoped_enchant(existing, incoming_enchant),
    })
}

fn validate_capacity(capacity: EnchantSlotCapacity) -> Result<(), EnchantApplyError> {
    validate_unlocked_slot_count(EnchantSlotFamily::NormalClass, capacity.normal_class)?;
    validate_unlocked_slot_count(
        EnchantSlotFamily::SpecialUniversal,
        capacity.special_universal,
    )
}

fn validate_unlocked_slot_count(
    family: EnchantSlotFamily,
    current: u8,
) -> Result<(), EnchantApplyError> {
    let minimum = family.native_slot_count();
    let maximum = family.maximum_slot_count();
    if current < minimum || current > maximum {
        Err(EnchantApplyError::InvalidUnlockedSlotCount {
            family,
            current,
            minimum,
            maximum,
        })
    } else {
        Ok(())
    }
}

fn validate_enchant_level(enchant: CanonicalEnchant, level: u8) -> Result<(), EnchantApplyError> {
    let maximum = crate::canonical_enchant_max_resulting_level(enchant);
    if level == 0 || level > maximum {
        Err(EnchantApplyError::InvalidEnchantLevel {
            enchant,
            level,
            maximum,
        })
    } else {
        Ok(())
    }
}

pub(crate) fn validate_existing_state(
    equipment_slot: EquipmentSlot,
    capacity: EnchantSlotCapacity,
    existing: &[ExistingAppliedEnchant],
) -> Result<EnchantSlotOccupancy, EnchantApplyError> {
    let mut occupancy = EnchantSlotOccupancy {
        normal_class: 0,
        special_universal: 0,
    };

    for (index, applied) in existing.iter().enumerate() {
        validate_enchant_level(applied.enchant, applied.level)?;
        let policy = enchant_placement_policy(applied.enchant);
        if !policy.applies_to(equipment_slot) {
            return Err(EnchantApplyError::ExistingEnchantWrongEquipmentSlot {
                enchant: applied.enchant,
                slot: equipment_slot,
            });
        }

        for previous in &existing[..index] {
            if previous.enchant == applied.enchant {
                return Err(EnchantApplyError::DuplicateExistingEnchant(applied.enchant));
            }
            if let Some(scope) = canonical_enchant_conflict_scope(previous.enchant, applied.enchant)
            {
                return Err(EnchantApplyError::ExistingItemConflict {
                    left: previous.enchant,
                    right: applied.enchant,
                    scope,
                });
            }
        }

        occupancy = occupancy
            .increment(policy.slot_family)
            .ok_or(EnchantApplyError::OccupancyOverflow)?;
        let unlocked = capacity.for_family(policy.slot_family);
        let occupied = occupancy.for_family(policy.slot_family);
        if occupied > unlocked {
            return Err(EnchantApplyError::ExistingOccupancyExceedsCapacity {
                family: policy.slot_family,
                occupied,
                unlocked,
            });
        }
    }

    Ok(occupancy)
}

fn resulting_item_contains_loadout_scoped_enchant(
    existing: &[ExistingAppliedEnchant],
    incoming: CanonicalEnchant,
) -> bool {
    is_loadout_scoped_enchant(incoming)
        || existing
            .iter()
            .any(|applied| is_loadout_scoped_enchant(applied.enchant))
}

const fn is_loadout_scoped_enchant(enchant: CanonicalEnchant) -> bool {
    matches!(
        enchant,
        CanonicalEnchant::Guardian | CanonicalEnchant::NineLife | CanonicalEnchant::Phoenix
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn applied(enchant: CanonicalEnchant, level: u8) -> ExistingAppliedEnchant {
        ExistingAppliedEnchant { enchant, level }
    }

    #[test]
    fn native_and_unlocked_capacity_ranges_match_the_two_frozen_families() {
        assert_eq!(
            EnchantSlotCapacity::native(),
            EnchantSlotCapacity {
                normal_class: 4,
                special_universal: 3
            }
        );
        assert_eq!(
            EnchantSlotCapacity::try_new(6, 6).unwrap(),
            EnchantSlotCapacity {
                normal_class: 6,
                special_universal: 6
            }
        );
        assert!(EnchantSlotCapacity::try_new(3, 3).is_err());
        assert!(EnchantSlotCapacity::try_new(4, 2).is_err());
        assert!(EnchantSlotCapacity::try_new(7, 3).is_err());
        assert!(EnchantSlotCapacity::try_new(4, 7).is_err());
    }

    #[test]
    fn new_standard_book_uses_one_slot_in_its_own_family_and_is_guaranteed() {
        let preview = preview_standard_finished_book_application(
            EquipmentSlot::Pickaxe,
            EnchantSlotCapacity::native(),
            &[
                applied(CanonicalEnchant::Efficiency, 1),
                applied(CanonicalEnchant::Fortune, 2),
            ],
            CanonicalEnchant::Unbreaking,
            3,
        )
        .unwrap();

        assert_eq!(preview.slot_family, EnchantSlotFamily::NormalClass);
        assert_eq!(preview.action, EnchantApplyAction::InsertNew);
        assert_eq!(preview.occupancy_before.normal_class, 2);
        assert_eq!(preview.occupancy_after.normal_class, 3);
        assert_eq!(preview.occupancy_after.special_universal, 0);
        assert!(preview.incoming_finished_book_consumed_on_commit);
        assert!(preview.success_guaranteed_after_authoritative_revalidation);
        assert!(!preview.resulting_item_requires_equipped_armor_loadout_conflict_validation);
    }

    #[test]
    fn normal_and_special_capacity_are_independent() {
        let existing = [
            applied(CanonicalEnchant::Efficiency, 1),
            applied(CanonicalEnchant::Fortune, 1),
            applied(CanonicalEnchant::Unbreaking, 1),
            applied(CanonicalEnchant::Mending, 1),
            applied(CanonicalEnchant::Stabilize, 1),
            applied(CanonicalEnchant::Sparkling, 1),
        ];
        let preview = preview_standard_finished_book_application(
            EquipmentSlot::Pickaxe,
            EnchantSlotCapacity::native(),
            &existing,
            CanonicalEnchant::Grinding,
            1,
        )
        .unwrap();
        assert_eq!(preview.occupancy_before.normal_class, 4);
        assert_eq!(preview.occupancy_before.special_universal, 2);
        assert_eq!(preview.occupancy_after.normal_class, 4);
        assert_eq!(preview.occupancy_after.special_universal, 3);

        assert_eq!(
            preview_standard_finished_book_application(
                EquipmentSlot::Pickaxe,
                EnchantSlotCapacity::native(),
                &existing,
                CanonicalEnchant::PickaxeTreasure,
                1,
            ),
            Err(EnchantApplyError::NoFreeSlot {
                family: EnchantSlotFamily::NormalClass,
                occupied: 4,
                unlocked: 4,
            })
        );
    }

    #[test]
    fn higher_replacement_reuses_the_existing_slot() {
        let existing = [
            applied(CanonicalEnchant::Sharpness, 3),
            applied(CanonicalEnchant::Looting, 2),
            applied(CanonicalEnchant::Unbreaking, 1),
            applied(CanonicalEnchant::Mending, 1),
        ];
        let preview = preview_standard_finished_book_application(
            EquipmentSlot::Sword,
            EnchantSlotCapacity::native(),
            &existing,
            CanonicalEnchant::Sharpness,
            4,
        )
        .unwrap();

        assert_eq!(
            preview.action,
            EnchantApplyAction::UpgradeExisting { previous_level: 3 }
        );
        assert_eq!(preview.occupancy_before, preview.occupancy_after);
        assert_eq!(preview.occupancy_after.normal_class, 4);
    }

    #[test]
    fn lower_or_equal_replacement_is_rejected_before_book_consumption() {
        let existing = [applied(CanonicalEnchant::Efficiency, 5)];
        for incoming_level in [4, 5] {
            assert_eq!(
                preview_standard_finished_book_application(
                    EquipmentSlot::Pickaxe,
                    EnchantSlotCapacity::native(),
                    &existing,
                    CanonicalEnchant::Efficiency,
                    incoming_level,
                ),
                Err(EnchantApplyError::LowerOrEqualReplacement {
                    enchant: CanonicalEnchant::Efficiency,
                    existing_level: 5,
                    incoming_level,
                })
            );
        }
    }

    #[test]
    fn placement_restrictions_fail_closed_for_incoming_and_existing_state() {
        assert_eq!(
            preview_standard_finished_book_application(
                EquipmentSlot::Helmet,
                EnchantSlotCapacity::native(),
                &[],
                CanonicalEnchant::Cat,
                1,
            ),
            Err(EnchantApplyError::IncomingEnchantWrongEquipmentSlot {
                enchant: CanonicalEnchant::Cat,
                slot: EquipmentSlot::Helmet,
            })
        );

        assert_eq!(
            preview_standard_finished_book_application(
                EquipmentSlot::Helmet,
                EnchantSlotCapacity::native(),
                &[applied(CanonicalEnchant::Dodge, 1)],
                CanonicalEnchant::Protection,
                1,
            ),
            Err(EnchantApplyError::ExistingEnchantWrongEquipmentSlot {
                enchant: CanonicalEnchant::Dodge,
                slot: EquipmentSlot::Helmet,
            })
        );
    }

    #[test]
    fn canonical_resulting_level_limits_are_enforced_for_incoming_and_existing_state() {
        for (enchant, invalid_level, maximum) in [
            (CanonicalEnchant::Mending, 2, 1),
            (CanonicalEnchant::BaitRack, 4, 3),
            (CanonicalEnchant::NineLife, 10, 9),
            (CanonicalEnchant::Phoenix, 2, 1),
            (CanonicalEnchant::Carving, 2, 1),
            (CanonicalEnchant::Master, 3, 2),
        ] {
            assert_eq!(
                preview_standard_finished_book_application(
                    EquipmentSlot::FishingRod,
                    EnchantSlotCapacity::native(),
                    &[],
                    enchant,
                    invalid_level,
                ),
                Err(EnchantApplyError::InvalidEnchantLevel {
                    enchant,
                    level: invalid_level,
                    maximum,
                })
            );
        }

        assert_eq!(
            preview_standard_finished_book_application(
                EquipmentSlot::Pickaxe,
                EnchantSlotCapacity::native(),
                &[applied(CanonicalEnchant::Mending, 2)],
                CanonicalEnchant::Efficiency,
                1,
            ),
            Err(EnchantApplyError::InvalidEnchantLevel {
                enchant: CanonicalEnchant::Mending,
                level: 2,
                maximum: 1,
            })
        );
    }

    #[test]
    fn duplicate_or_over_capacity_existing_state_is_rejected() {
        assert_eq!(
            preview_standard_finished_book_application(
                EquipmentSlot::Sword,
                EnchantSlotCapacity::native(),
                &[
                    applied(CanonicalEnchant::Looting, 1),
                    applied(CanonicalEnchant::Looting, 2),
                ],
                CanonicalEnchant::Sharpness,
                1,
            ),
            Err(EnchantApplyError::DuplicateExistingEnchant(
                CanonicalEnchant::Looting
            ))
        );

        assert_eq!(
            preview_standard_finished_book_application(
                EquipmentSlot::Sword,
                EnchantSlotCapacity::native(),
                &[
                    applied(CanonicalEnchant::Looting, 1),
                    applied(CanonicalEnchant::Knockback, 1),
                    applied(CanonicalEnchant::Devour, 1),
                    applied(CanonicalEnchant::Unbreaking, 1),
                    applied(CanonicalEnchant::Mending, 1),
                ],
                CanonicalEnchant::Sharpness,
                1,
            ),
            Err(EnchantApplyError::ExistingOccupancyExceedsCapacity {
                family: EnchantSlotFamily::NormalClass,
                occupied: 5,
                unlocked: 4,
            })
        );
    }

    #[test]
    fn same_item_conflicts_are_rejected_for_both_conflict_scopes() {
        assert_eq!(
            preview_standard_finished_book_application(
                EquipmentSlot::Pickaxe,
                EnchantSlotCapacity::native(),
                &[applied(CanonicalEnchant::Trench, 1)],
                CanonicalEnchant::Nuke,
                1,
            ),
            Err(EnchantApplyError::IncomingConflict {
                incoming: CanonicalEnchant::Nuke,
                existing: CanonicalEnchant::Trench,
                scope: EnchantConflictScope::SameItem,
            })
        );

        assert_eq!(
            preview_standard_finished_book_application(
                EquipmentSlot::Chestplate,
                EnchantSlotCapacity::native(),
                &[applied(CanonicalEnchant::Guardian, 1)],
                CanonicalEnchant::NineLife,
                1,
            ),
            Err(EnchantApplyError::IncomingConflict {
                incoming: CanonicalEnchant::NineLife,
                existing: CanonicalEnchant::Guardian,
                scope: EnchantConflictScope::EquippedArmorLoadout,
            })
        );
    }

    #[test]
    fn explicit_non_conflicts_remain_applicable_when_capacity_allows() {
        let preview = preview_standard_finished_book_application(
            EquipmentSlot::Sword,
            EnchantSlotCapacity::try_new(6, 3).unwrap(),
            &[applied(CanonicalEnchant::Piercing, 1)],
            CanonicalEnchant::ArmorPiercing,
            1,
        )
        .unwrap();
        assert_eq!(preview.action, EnchantApplyAction::InsertNew);
        assert_eq!(preview.occupancy_after.normal_class, 2);
    }

    #[test]
    fn survival_core_result_surfaces_the_cross_item_loadout_check() {
        let guardian = preview_standard_finished_book_application(
            EquipmentSlot::Chestplate,
            EnchantSlotCapacity::native(),
            &[],
            CanonicalEnchant::Guardian,
            1,
        )
        .unwrap();
        assert!(guardian.resulting_item_requires_equipped_armor_loadout_conflict_validation);

        let unrelated_insert = preview_standard_finished_book_application(
            EquipmentSlot::Chestplate,
            EnchantSlotCapacity::native(),
            &[applied(CanonicalEnchant::Guardian, 1)],
            CanonicalEnchant::Protection,
            1,
        )
        .unwrap();
        assert!(
            unrelated_insert.resulting_item_requires_equipped_armor_loadout_conflict_validation
        );
    }

    #[test]
    fn invalid_public_capacity_value_is_revalidated_even_without_constructor() {
        let invalid = EnchantSlotCapacity {
            normal_class: 0,
            special_universal: 3,
        };
        assert_eq!(
            preview_standard_finished_book_application(
                EquipmentSlot::Pickaxe,
                invalid,
                &[],
                CanonicalEnchant::Efficiency,
                1,
            ),
            Err(EnchantApplyError::InvalidUnlockedSlotCount {
                family: EnchantSlotFamily::NormalClass,
                current: 0,
                minimum: 4,
                maximum: 6,
            })
        );
    }
}
