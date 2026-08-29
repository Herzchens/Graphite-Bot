use serde::Serialize;
use thiserror::Error;

pub const BAIT_RACK_MAX_LEVEL: u8 = 3;
pub const NATIVE_ACTIVE_BAIT_CATEGORY_SLOTS: u8 = 3;
pub const BAIT_RACK_ACTIVE_SLOTS_PER_LEVEL: u8 = 1;
pub const MAX_ACTIVE_BAIT_CATEGORY_SLOTS: u8 = 6;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct BaitRackCapacityPolicy {
    pub bait_rack_level: Option<u8>,
    pub native_active_bait_category_slots: u8,
    pub additional_active_bait_category_slots: u8,
    pub active_bait_category_slots: u8,
    pub max_active_bait_category_slots: u8,
    pub rod_only: bool,
    pub occupies_one_normal_rod_enchant_slot: bool,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum BaitRackPolicyError {
    #[error("Bait Rack level must be between I and III when present; got {0}")]
    LevelOutOfRange(u8),
}

/// Resolves the frozen active bait-category capacity for a normal Fishing Rod.
///
/// Normal fishing starts with three native active bait-category slots. Bait Rack is a Rod-only
/// normal enchant that adds exactly one active bait category per level through Level III, producing
/// capacities four, five, and six. The enchant occupies exactly one normal Rod enchant slot
/// regardless of level.
///
/// `None` represents a Rod without Bait Rack. A present level must be canonical I-III; `Some(0)` and
/// levels above III are rejected instead of being treated as absence or silently clamped. This pure
/// policy does not validate authoritative ItemInstance enchant state, select bait categories,
/// consume bait, or activate Fishing gameplay.
pub fn bait_rack_capacity_policy(
    bait_rack_level: Option<u8>,
) -> Result<BaitRackCapacityPolicy, BaitRackPolicyError> {
    let level = match bait_rack_level {
        None => 0,
        Some(level @ 1..=BAIT_RACK_MAX_LEVEL) => level,
        Some(level) => return Err(BaitRackPolicyError::LevelOutOfRange(level)),
    };

    let additional_active_bait_category_slots = level * BAIT_RACK_ACTIVE_SLOTS_PER_LEVEL;
    let active_bait_category_slots =
        NATIVE_ACTIVE_BAIT_CATEGORY_SLOTS + additional_active_bait_category_slots;

    debug_assert!(active_bait_category_slots <= MAX_ACTIVE_BAIT_CATEGORY_SLOTS);

    Ok(BaitRackCapacityPolicy {
        bait_rack_level,
        native_active_bait_category_slots: NATIVE_ACTIVE_BAIT_CATEGORY_SLOTS,
        additional_active_bait_category_slots,
        active_bait_category_slots,
        max_active_bait_category_slots: MAX_ACTIVE_BAIT_CATEGORY_SLOTS,
        rod_only: true,
        occupies_one_normal_rod_enchant_slot: bait_rack_level.is_some(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_bait_rack_preserves_three_native_active_categories() {
        assert_eq!(
            bait_rack_capacity_policy(None).unwrap(),
            BaitRackCapacityPolicy {
                bait_rack_level: None,
                native_active_bait_category_slots: 3,
                additional_active_bait_category_slots: 0,
                active_bait_category_slots: 3,
                max_active_bait_category_slots: 6,
                rod_only: true,
                occupies_one_normal_rod_enchant_slot: false,
            }
        );
    }

    #[test]
    fn levels_one_through_three_expand_capacity_to_four_through_six() {
        for (level, expected_capacity) in [(1, 4), (2, 5), (3, 6)] {
            let policy = bait_rack_capacity_policy(Some(level)).unwrap();
            assert_eq!(policy.bait_rack_level, Some(level));
            assert_eq!(policy.native_active_bait_category_slots, 3);
            assert_eq!(policy.additional_active_bait_category_slots, level);
            assert_eq!(policy.active_bait_category_slots, expected_capacity);
            assert_eq!(policy.max_active_bait_category_slots, 6);
            assert!(policy.rod_only);
            assert!(policy.occupies_one_normal_rod_enchant_slot);
        }
    }

    #[test]
    fn present_noncanonical_levels_fail_closed() {
        for level in [0, 4, u8::MAX] {
            assert_eq!(
                bait_rack_capacity_policy(Some(level)),
                Err(BaitRackPolicyError::LevelOutOfRange(level))
            );
        }
    }
}
