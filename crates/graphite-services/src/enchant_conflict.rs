use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PickaxeEnchant {
    Efficiency,
    Fortune,
    Treasure,
    Unbreaking,
    Mending,
    Trench,
    Nuke,
    Smelt,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FishingRodEnchant {
    Lure,
    LuckOfTheSea,
    Treasure,
    MultiTreasure,
    Luck,
    Unbreaking,
    Mending,
    Multicatch,
    Strengthen,
    SharpHook,
    BaitRack,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SwordEnchant {
    Sharpness,
    Smite,
    BaneOfArthropods,
    SweepingEdge,
    FireAspect,
    Unbreaking,
    Mending,
    Looting,
    Knockback,
    Devour,
    Execution,
    Bleeding,
    BloodFrenzy,
    ArmorPiercing,
    Piercing,
    Freezing,
    Annihilation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ArmorEnchant {
    Protection,
    Unbreaking,
    Thorn,
    Mending,
    NineLife,
    Cat,
    Dog,
    Dodge,
    Guardian,
    ShadowWalker,
    NightWalker,
    DayWalker,
    Angel,
    Evil,
    Reinforce,
    Phoenix,
    SoulGrind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EnchantConflictDecision {
    Compatible,
    Forbidden,
}

impl EnchantConflictDecision {
    #[must_use]
    pub const fn is_forbidden(self) -> bool {
        matches!(self, Self::Forbidden)
    }
}

/// Returns the frozen same-Pickaxe conflict decision.
///
/// Current policy has exactly one Pickaxe conflict pair: Trench and Nuke. The enum contains the
/// complete current normal/class Pickaxe enchant vocabulary so callers do not need string matching.
/// This policy does not validate book level, slot capacity, acquisition source, or live ItemInstance
/// state.
#[must_use]
pub const fn pickaxe_enchants_conflict(
    left: PickaxeEnchant,
    right: PickaxeEnchant,
) -> EnchantConflictDecision {
    if matches!(
        (left, right),
        (PickaxeEnchant::Trench, PickaxeEnchant::Nuke)
            | (PickaxeEnchant::Nuke, PickaxeEnchant::Trench)
    ) {
        EnchantConflictDecision::Forbidden
    } else {
        EnchantConflictDecision::Compatible
    }
}

/// Returns the frozen same-Fishing-Rod conflict decision.
///
/// The active specification defines Multi Treasure and Multicatch as independent branches and does
/// not define any normal/class Fishing Rod conflict pair. This function deliberately records that
/// current-v1 state rather than inventing a conflict from effect interaction.
#[must_use]
pub const fn fishing_rod_enchants_conflict(
    _left: FishingRodEnchant,
    _right: FishingRodEnchant,
) -> EnchantConflictDecision {
    EnchantConflictDecision::Compatible
}

/// Returns the frozen same-Sword conflict decision.
///
/// Four independent conflict rules are represented:
/// - Sharpness / Smite / Bane of Arthropods: exactly one;
/// - Fire Aspect / Freezing / Bleeding: exactly one;
/// - Annihilation / Blood Frenzy / Execution: exactly one;
/// - Sweeping Edge conflicts with Piercing and Armor Piercing, while Piercing and Armor Piercing are
///   mutually compatible.
///
/// Proc-chain restrictions are combat-runtime rules and are intentionally outside this static
/// compatibility predicate.
#[must_use]
pub const fn sword_enchants_conflict(
    left: SwordEnchant,
    right: SwordEnchant,
) -> EnchantConflictDecision {
    if same_distinct_sword_family(left, right, 0)
        || same_distinct_sword_family(left, right, 1)
        || same_distinct_sword_family(left, right, 2)
        || matches!(
            (left, right),
            (SwordEnchant::SweepingEdge, SwordEnchant::Piercing)
                | (SwordEnchant::Piercing, SwordEnchant::SweepingEdge)
                | (SwordEnchant::SweepingEdge, SwordEnchant::ArmorPiercing)
                | (SwordEnchant::ArmorPiercing, SwordEnchant::SweepingEdge)
        )
    {
        EnchantConflictDecision::Forbidden
    } else {
        EnchantConflictDecision::Compatible
    }
}

const fn same_distinct_sword_family(left: SwordEnchant, right: SwordEnchant, family: u8) -> bool {
    if left as u8 == right as u8 {
        return false;
    }

    match family {
        0 => {
            matches!(
                left,
                SwordEnchant::Sharpness | SwordEnchant::Smite | SwordEnchant::BaneOfArthropods
            ) && matches!(
                right,
                SwordEnchant::Sharpness | SwordEnchant::Smite | SwordEnchant::BaneOfArthropods
            )
        }
        1 => {
            matches!(
                left,
                SwordEnchant::FireAspect | SwordEnchant::Freezing | SwordEnchant::Bleeding
            ) && matches!(
                right,
                SwordEnchant::FireAspect | SwordEnchant::Freezing | SwordEnchant::Bleeding
            )
        }
        2 => {
            matches!(
                left,
                SwordEnchant::Annihilation | SwordEnchant::BloodFrenzy | SwordEnchant::Execution
            ) && matches!(
                right,
                SwordEnchant::Annihilation | SwordEnchant::BloodFrenzy | SwordEnchant::Execution
            )
        }
        _ => false,
    }
}

/// Returns the frozen Armor conflict decision for two normal/class armor enchants.
///
/// The active specification defines Cat/Dog, Angel/Evil, Guardian/Nine Life/Phoenix, and
/// Thorn/Reinforce as conflicts. Day Walker, Night Walker, and Shadow Walker are explicitly
/// compatible with each other. Only the Guardian/Nine Life/Phoenix family is explicitly stated to
/// conflict across the equipped armor set; this pure pair predicate does not invent a broader
/// cross-item scope for the other conflict pairs. A future application owner must enforce the
/// authoritative item/loadout scope while also validating slot-specific restrictions such as
/// Phoenix-on-chest.
#[must_use]
pub const fn armor_enchants_conflict(
    left: ArmorEnchant,
    right: ArmorEnchant,
) -> EnchantConflictDecision {
    if matches!(
        (left, right),
        (ArmorEnchant::Cat, ArmorEnchant::Dog)
            | (ArmorEnchant::Dog, ArmorEnchant::Cat)
            | (ArmorEnchant::Angel, ArmorEnchant::Evil)
            | (ArmorEnchant::Evil, ArmorEnchant::Angel)
            | (ArmorEnchant::Thorn, ArmorEnchant::Reinforce)
            | (ArmorEnchant::Reinforce, ArmorEnchant::Thorn)
    ) || same_distinct_armor_survival_family(left, right)
    {
        EnchantConflictDecision::Forbidden
    } else {
        EnchantConflictDecision::Compatible
    }
}

const fn same_distinct_armor_survival_family(left: ArmorEnchant, right: ArmorEnchant) -> bool {
    if left as u8 == right as u8 {
        return false;
    }
    matches!(
        left,
        ArmorEnchant::Guardian | ArmorEnchant::NineLife | ArmorEnchant::Phoenix
    ) && matches!(
        right,
        ArmorEnchant::Guardian | ArmorEnchant::NineLife | ArmorEnchant::Phoenix
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const PICKAXE_ENCHANTS: [PickaxeEnchant; 8] = [
        PickaxeEnchant::Efficiency,
        PickaxeEnchant::Fortune,
        PickaxeEnchant::Treasure,
        PickaxeEnchant::Unbreaking,
        PickaxeEnchant::Mending,
        PickaxeEnchant::Trench,
        PickaxeEnchant::Nuke,
        PickaxeEnchant::Smelt,
    ];
    const FISHING_ROD_ENCHANTS: [FishingRodEnchant; 11] = [
        FishingRodEnchant::Lure,
        FishingRodEnchant::LuckOfTheSea,
        FishingRodEnchant::Treasure,
        FishingRodEnchant::MultiTreasure,
        FishingRodEnchant::Luck,
        FishingRodEnchant::Unbreaking,
        FishingRodEnchant::Mending,
        FishingRodEnchant::Multicatch,
        FishingRodEnchant::Strengthen,
        FishingRodEnchant::SharpHook,
        FishingRodEnchant::BaitRack,
    ];
    const SWORD_ENCHANTS: [SwordEnchant; 17] = [
        SwordEnchant::Sharpness,
        SwordEnchant::Smite,
        SwordEnchant::BaneOfArthropods,
        SwordEnchant::SweepingEdge,
        SwordEnchant::FireAspect,
        SwordEnchant::Unbreaking,
        SwordEnchant::Mending,
        SwordEnchant::Looting,
        SwordEnchant::Knockback,
        SwordEnchant::Devour,
        SwordEnchant::Execution,
        SwordEnchant::Bleeding,
        SwordEnchant::BloodFrenzy,
        SwordEnchant::ArmorPiercing,
        SwordEnchant::Piercing,
        SwordEnchant::Freezing,
        SwordEnchant::Annihilation,
    ];
    const ARMOR_ENCHANTS: [ArmorEnchant; 17] = [
        ArmorEnchant::Protection,
        ArmorEnchant::Unbreaking,
        ArmorEnchant::Thorn,
        ArmorEnchant::Mending,
        ArmorEnchant::NineLife,
        ArmorEnchant::Cat,
        ArmorEnchant::Dog,
        ArmorEnchant::Dodge,
        ArmorEnchant::Guardian,
        ArmorEnchant::ShadowWalker,
        ArmorEnchant::NightWalker,
        ArmorEnchant::DayWalker,
        ArmorEnchant::Angel,
        ArmorEnchant::Evil,
        ArmorEnchant::Reinforce,
        ArmorEnchant::Phoenix,
        ArmorEnchant::SoulGrind,
    ];

    #[test]
    fn pickaxe_has_only_trench_nuke_conflict() {
        for left in PICKAXE_ENCHANTS {
            for right in PICKAXE_ENCHANTS {
                let expected = matches!(
                    (left, right),
                    (PickaxeEnchant::Trench, PickaxeEnchant::Nuke)
                        | (PickaxeEnchant::Nuke, PickaxeEnchant::Trench)
                );
                assert_eq!(
                    pickaxe_enchants_conflict(left, right).is_forbidden(),
                    expected
                );
            }
        }
    }

    #[test]
    fn fishing_rod_has_no_frozen_conflict_pairs() {
        for left in FISHING_ROD_ENCHANTS {
            for right in FISHING_ROD_ENCHANTS {
                assert_eq!(
                    fishing_rod_enchants_conflict(left, right),
                    EnchantConflictDecision::Compatible
                );
            }
        }
    }

    #[test]
    fn sword_conflicts_are_symmetric_and_self_pairs_are_compatible() {
        for left in SWORD_ENCHANTS {
            assert_eq!(
                sword_enchants_conflict(left, left),
                EnchantConflictDecision::Compatible
            );
            for right in SWORD_ENCHANTS {
                assert_eq!(
                    sword_enchants_conflict(left, right),
                    sword_enchants_conflict(right, left),
                    "{left:?} vs {right:?} must be symmetric"
                );
            }
        }
    }

    #[test]
    fn sword_frozen_families_and_technique_pairs_match_exactly() {
        for pair in [
            (SwordEnchant::Sharpness, SwordEnchant::Smite),
            (SwordEnchant::Sharpness, SwordEnchant::BaneOfArthropods),
            (SwordEnchant::Smite, SwordEnchant::BaneOfArthropods),
            (SwordEnchant::FireAspect, SwordEnchant::Freezing),
            (SwordEnchant::FireAspect, SwordEnchant::Bleeding),
            (SwordEnchant::Freezing, SwordEnchant::Bleeding),
            (SwordEnchant::Annihilation, SwordEnchant::BloodFrenzy),
            (SwordEnchant::Annihilation, SwordEnchant::Execution),
            (SwordEnchant::BloodFrenzy, SwordEnchant::Execution),
            (SwordEnchant::SweepingEdge, SwordEnchant::Piercing),
            (SwordEnchant::SweepingEdge, SwordEnchant::ArmorPiercing),
        ] {
            assert!(sword_enchants_conflict(pair.0, pair.1).is_forbidden());
        }
        assert_eq!(
            sword_enchants_conflict(SwordEnchant::Piercing, SwordEnchant::ArmorPiercing),
            EnchantConflictDecision::Compatible
        );
    }

    #[test]
    fn armor_conflicts_are_symmetric_and_self_pairs_are_compatible() {
        for left in ARMOR_ENCHANTS {
            assert_eq!(
                armor_enchants_conflict(left, left),
                EnchantConflictDecision::Compatible
            );
            for right in ARMOR_ENCHANTS {
                assert_eq!(
                    armor_enchants_conflict(left, right),
                    armor_enchants_conflict(right, left),
                    "{left:?} vs {right:?} must be symmetric"
                );
            }
        }
    }

    #[test]
    fn armor_frozen_conflicts_and_walker_non_conflicts_match_exactly() {
        for pair in [
            (ArmorEnchant::Cat, ArmorEnchant::Dog),
            (ArmorEnchant::Angel, ArmorEnchant::Evil),
            (ArmorEnchant::Guardian, ArmorEnchant::NineLife),
            (ArmorEnchant::Guardian, ArmorEnchant::Phoenix),
            (ArmorEnchant::NineLife, ArmorEnchant::Phoenix),
            (ArmorEnchant::Thorn, ArmorEnchant::Reinforce),
        ] {
            assert!(armor_enchants_conflict(pair.0, pair.1).is_forbidden());
        }

        for pair in [
            (ArmorEnchant::DayWalker, ArmorEnchant::NightWalker),
            (ArmorEnchant::DayWalker, ArmorEnchant::ShadowWalker),
            (ArmorEnchant::NightWalker, ArmorEnchant::ShadowWalker),
        ] {
            assert_eq!(
                armor_enchants_conflict(pair.0, pair.1),
                EnchantConflictDecision::Compatible
            );
        }
    }
}
