use serde::Serialize;

use crate::enchant_catalog::CanonicalEnchant;

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

impl PickaxeEnchant {
    const fn canonical(self) -> CanonicalEnchant {
        match self {
            Self::Efficiency => CanonicalEnchant::Efficiency,
            Self::Fortune => CanonicalEnchant::Fortune,
            Self::Treasure => CanonicalEnchant::PickaxeTreasure,
            Self::Unbreaking => CanonicalEnchant::Unbreaking,
            Self::Mending => CanonicalEnchant::Mending,
            Self::Trench => CanonicalEnchant::Trench,
            Self::Nuke => CanonicalEnchant::Nuke,
            Self::Smelt => CanonicalEnchant::Smelt,
        }
    }
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

impl FishingRodEnchant {
    const fn canonical(self) -> CanonicalEnchant {
        match self {
            Self::Lure => CanonicalEnchant::Lure,
            Self::LuckOfTheSea => CanonicalEnchant::LuckOfTheSea,
            Self::Treasure => CanonicalEnchant::FishingRodTreasure,
            Self::MultiTreasure => CanonicalEnchant::MultiTreasure,
            Self::Luck => CanonicalEnchant::Luck,
            Self::Unbreaking => CanonicalEnchant::Unbreaking,
            Self::Mending => CanonicalEnchant::Mending,
            Self::Multicatch => CanonicalEnchant::Multicatch,
            Self::Strengthen => CanonicalEnchant::Strengthen,
            Self::SharpHook => CanonicalEnchant::SharpHook,
            Self::BaitRack => CanonicalEnchant::BaitRack,
        }
    }
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

impl SwordEnchant {
    const fn canonical(self) -> CanonicalEnchant {
        match self {
            Self::Sharpness => CanonicalEnchant::Sharpness,
            Self::Smite => CanonicalEnchant::Smite,
            Self::BaneOfArthropods => CanonicalEnchant::BaneOfArthropods,
            Self::SweepingEdge => CanonicalEnchant::SweepingEdge,
            Self::FireAspect => CanonicalEnchant::FireAspect,
            Self::Unbreaking => CanonicalEnchant::Unbreaking,
            Self::Mending => CanonicalEnchant::Mending,
            Self::Looting => CanonicalEnchant::Looting,
            Self::Knockback => CanonicalEnchant::Knockback,
            Self::Devour => CanonicalEnchant::Devour,
            Self::Execution => CanonicalEnchant::Execution,
            Self::Bleeding => CanonicalEnchant::Bleeding,
            Self::BloodFrenzy => CanonicalEnchant::BloodFrenzy,
            Self::ArmorPiercing => CanonicalEnchant::ArmorPiercing,
            Self::Piercing => CanonicalEnchant::Piercing,
            Self::Freezing => CanonicalEnchant::Freezing,
            Self::Annihilation => CanonicalEnchant::Annihilation,
        }
    }
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

impl ArmorEnchant {
    const fn canonical(self) -> CanonicalEnchant {
        match self {
            Self::Protection => CanonicalEnchant::Protection,
            Self::Unbreaking => CanonicalEnchant::Unbreaking,
            Self::Thorn => CanonicalEnchant::Thorn,
            Self::Mending => CanonicalEnchant::Mending,
            Self::NineLife => CanonicalEnchant::NineLife,
            Self::Cat => CanonicalEnchant::Cat,
            Self::Dog => CanonicalEnchant::Dog,
            Self::Dodge => CanonicalEnchant::Dodge,
            Self::Guardian => CanonicalEnchant::Guardian,
            Self::ShadowWalker => CanonicalEnchant::ShadowWalker,
            Self::NightWalker => CanonicalEnchant::NightWalker,
            Self::DayWalker => CanonicalEnchant::DayWalker,
            Self::Angel => CanonicalEnchant::Angel,
            Self::Evil => CanonicalEnchant::Evil,
            Self::Reinforce => CanonicalEnchant::Reinforce,
            Self::Phoenix => CanonicalEnchant::Phoenix,
            Self::SoulGrind => CanonicalEnchant::SoulGrind,
        }
    }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EnchantConflictScope {
    SameItem,
    EquippedArmorLoadout,
}

/// Returns the frozen conflict scope for two canonical enchant identities.
///
/// `SameItem` covers all ordinary Pickaxe/Sword conflicts plus the Armor Cat/Dog, Angel/Evil and
/// Thorn/Reinforce pairs. The specification explicitly widens only the Guardian/Nine Life/Phoenix
/// survival-core family to the equipped Armor loadout, so that family returns
/// `EquippedArmorLoadout`. Day Walker, Night Walker and Shadow Walker remain compatible. No conflict
/// is invented for Fishing Rod or Special/universal enchants.
///
/// This function classifies a pair only. A future Enchant mutation owner must separately validate
/// equipment applicability, physical slot capacity/occupancy, authoritative equipped-loadout state,
/// and the target ItemInstance under its owning transaction.
#[must_use]
pub const fn canonical_enchant_conflict_scope(
    left: CanonicalEnchant,
    right: CanonicalEnchant,
) -> Option<EnchantConflictScope> {
    use CanonicalEnchant as E;

    if left as u8 == right as u8 {
        return None;
    }

    if same_distinct_canonical_family(left, right, E::Guardian, E::NineLife, E::Phoenix) {
        return Some(EnchantConflictScope::EquippedArmorLoadout);
    }

    if matches!(
        (left, right),
        (E::Trench, E::Nuke)
            | (E::Nuke, E::Trench)
            | (E::Cat, E::Dog)
            | (E::Dog, E::Cat)
            | (E::Angel, E::Evil)
            | (E::Evil, E::Angel)
            | (E::Thorn, E::Reinforce)
            | (E::Reinforce, E::Thorn)
            | (E::SweepingEdge, E::Piercing)
            | (E::Piercing, E::SweepingEdge)
            | (E::SweepingEdge, E::ArmorPiercing)
            | (E::ArmorPiercing, E::SweepingEdge)
    ) || same_distinct_canonical_family(left, right, E::Sharpness, E::Smite, E::BaneOfArthropods)
        || same_distinct_canonical_family(left, right, E::FireAspect, E::Freezing, E::Bleeding)
        || same_distinct_canonical_family(
            left,
            right,
            E::Annihilation,
            E::BloodFrenzy,
            E::Execution,
        )
    {
        Some(EnchantConflictScope::SameItem)
    } else {
        None
    }
}

/// Returns the canonical pair decision without discarding the scope API for callers that need it.
#[must_use]
pub const fn canonical_enchants_conflict(
    left: CanonicalEnchant,
    right: CanonicalEnchant,
) -> EnchantConflictDecision {
    match canonical_enchant_conflict_scope(left, right) {
        Some(_) => EnchantConflictDecision::Forbidden,
        None => EnchantConflictDecision::Compatible,
    }
}

const fn same_distinct_canonical_family(
    left: CanonicalEnchant,
    right: CanonicalEnchant,
    first: CanonicalEnchant,
    second: CanonicalEnchant,
    third: CanonicalEnchant,
) -> bool {
    if left as u8 == right as u8 {
        return false;
    }

    canonical_is_one_of_three(left, first, second, third)
        && canonical_is_one_of_three(right, first, second, third)
}

const fn canonical_is_one_of_three(
    value: CanonicalEnchant,
    first: CanonicalEnchant,
    second: CanonicalEnchant,
    third: CanonicalEnchant,
) -> bool {
    value as u8 == first as u8 || value as u8 == second as u8 || value as u8 == third as u8
}

/// Returns the frozen same-Pickaxe conflict decision.
///
/// This legacy typed facade delegates to the canonical identity policy so the Trench/Nuke rule has
/// one source of truth. It does not validate book level, slot capacity, acquisition source, or live
/// ItemInstance state.
#[must_use]
pub const fn pickaxe_enchants_conflict(
    left: PickaxeEnchant,
    right: PickaxeEnchant,
) -> EnchantConflictDecision {
    canonical_enchants_conflict(left.canonical(), right.canonical())
}

/// Returns the frozen same-Fishing-Rod conflict decision.
///
/// The active specification defines Multi Treasure and Multicatch as independent branches and does
/// not define any normal/class Fishing Rod conflict pair. This typed facade delegates to the same
/// canonical identity policy used by future lifecycle code.
#[must_use]
pub const fn fishing_rod_enchants_conflict(
    left: FishingRodEnchant,
    right: FishingRodEnchant,
) -> EnchantConflictDecision {
    canonical_enchants_conflict(left.canonical(), right.canonical())
}

/// Returns the frozen same-Sword conflict decision.
///
/// The canonical policy owns the three exclusive families and the Sweeping Edge technique conflicts;
/// this typed facade remains for source compatibility. Proc-chain restrictions are combat-runtime
/// rules and are intentionally outside this static compatibility predicate.
#[must_use]
pub const fn sword_enchants_conflict(
    left: SwordEnchant,
    right: SwordEnchant,
) -> EnchantConflictDecision {
    canonical_enchants_conflict(left.canonical(), right.canonical())
}

/// Returns the frozen Armor pair decision for two normal/class armor enchants.
///
/// The canonical scope API distinguishes same-item conflicts from the Guardian/Nine Life/Phoenix
/// equipped-loadout conflict. This legacy pair facade intentionally collapses either forbidden scope
/// to `Forbidden`; the future owning lifecycle must use [`canonical_enchant_conflict_scope`] when it
/// needs to know which authoritative state surface to inspect.
#[must_use]
pub const fn armor_enchants_conflict(
    left: ArmorEnchant,
    right: ArmorEnchant,
) -> EnchantConflictDecision {
    canonical_enchants_conflict(left.canonical(), right.canonical())
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
    fn canonical_same_item_conflicts_match_the_frozen_pairs() {
        for pair in [
            (CanonicalEnchant::Trench, CanonicalEnchant::Nuke),
            (CanonicalEnchant::Sharpness, CanonicalEnchant::Smite),
            (
                CanonicalEnchant::Sharpness,
                CanonicalEnchant::BaneOfArthropods,
            ),
            (CanonicalEnchant::Smite, CanonicalEnchant::BaneOfArthropods),
            (CanonicalEnchant::FireAspect, CanonicalEnchant::Freezing),
            (CanonicalEnchant::FireAspect, CanonicalEnchant::Bleeding),
            (CanonicalEnchant::Freezing, CanonicalEnchant::Bleeding),
            (
                CanonicalEnchant::Annihilation,
                CanonicalEnchant::BloodFrenzy,
            ),
            (CanonicalEnchant::Annihilation, CanonicalEnchant::Execution),
            (CanonicalEnchant::BloodFrenzy, CanonicalEnchant::Execution),
            (CanonicalEnchant::SweepingEdge, CanonicalEnchant::Piercing),
            (
                CanonicalEnchant::SweepingEdge,
                CanonicalEnchant::ArmorPiercing,
            ),
            (CanonicalEnchant::Cat, CanonicalEnchant::Dog),
            (CanonicalEnchant::Angel, CanonicalEnchant::Evil),
            (CanonicalEnchant::Thorn, CanonicalEnchant::Reinforce),
        ] {
            assert_eq!(
                canonical_enchant_conflict_scope(pair.0, pair.1),
                Some(EnchantConflictScope::SameItem),
                "{pair:?}"
            );
            assert_eq!(
                canonical_enchant_conflict_scope(pair.1, pair.0),
                Some(EnchantConflictScope::SameItem),
                "reverse {pair:?}"
            );
        }
    }

    #[test]
    fn canonical_survival_core_conflict_is_explicitly_loadout_scoped() {
        for pair in [
            (CanonicalEnchant::Guardian, CanonicalEnchant::NineLife),
            (CanonicalEnchant::Guardian, CanonicalEnchant::Phoenix),
            (CanonicalEnchant::NineLife, CanonicalEnchant::Phoenix),
        ] {
            assert_eq!(
                canonical_enchant_conflict_scope(pair.0, pair.1),
                Some(EnchantConflictScope::EquippedArmorLoadout),
                "{pair:?}"
            );
            assert_eq!(
                canonical_enchant_conflict_scope(pair.1, pair.0),
                Some(EnchantConflictScope::EquippedArmorLoadout),
                "reverse {pair:?}"
            );
        }
    }

    #[test]
    fn canonical_explicit_non_conflicts_remain_compatible() {
        for pair in [
            (CanonicalEnchant::Piercing, CanonicalEnchant::ArmorPiercing),
            (CanonicalEnchant::DayWalker, CanonicalEnchant::NightWalker),
            (CanonicalEnchant::DayWalker, CanonicalEnchant::ShadowWalker),
            (
                CanonicalEnchant::NightWalker,
                CanonicalEnchant::ShadowWalker,
            ),
            (
                CanonicalEnchant::MultiTreasure,
                CanonicalEnchant::Multicatch,
            ),
            (CanonicalEnchant::Stabilize, CanonicalEnchant::Sparkling),
        ] {
            assert_eq!(canonical_enchant_conflict_scope(pair.0, pair.1), None);
            assert_eq!(canonical_enchant_conflict_scope(pair.1, pair.0), None);
            assert_eq!(
                canonical_enchants_conflict(pair.0, pair.1),
                EnchantConflictDecision::Compatible
            );
        }
    }

    #[test]
    fn canonical_conflicts_are_irreflexive_for_representative_identities() {
        for enchant in [
            CanonicalEnchant::Trench,
            CanonicalEnchant::Nuke,
            CanonicalEnchant::Sharpness,
            CanonicalEnchant::Guardian,
            CanonicalEnchant::Phoenix,
            CanonicalEnchant::Stabilize,
        ] {
            assert_eq!(canonical_enchant_conflict_scope(enchant, enchant), None);
            assert_eq!(
                canonical_enchants_conflict(enchant, enchant),
                EnchantConflictDecision::Compatible
            );
        }
    }

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
