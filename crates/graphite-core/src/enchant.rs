use serde::Serialize;

pub const CANONICAL_ENCHANT_COUNT: usize = 54;

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

impl CanonicalEnchant {
    /// Stable PostgreSQL identity used by `item_instance_embedded_enchants.enchant_key`.
    ///
    /// Persistence identity belongs to the shared core vocabulary so lower-layer item/equipment
    /// invariants and higher-layer service policy cannot normalize the same stored key differently.
    #[must_use]
    pub const fn persisted_key(self) -> &'static str {
        use CanonicalEnchant as E;
        match self {
            E::Efficiency => "EFFICIENCY",
            E::Fortune => "FORTUNE",
            E::Smelt => "SMELT",
            E::Lure => "LURE",
            E::LuckOfTheSea => "LUCK_OF_THE_SEA",
            E::Luck => "LUCK",
            E::Strengthen => "STRENGTHEN",
            E::SharpHook => "SHARP_HOOK",
            E::BaitRack => "BAIT_RACK",
            E::Sharpness => "SHARPNESS",
            E::Smite => "SMITE",
            E::BaneOfArthropods => "BANE_OF_ARTHROPODS",
            E::SweepingEdge => "SWEEPING_EDGE",
            E::FireAspect => "FIRE_ASPECT",
            E::Knockback => "KNOCKBACK",
            E::Protection => "PROTECTION",
            E::Thorn => "THORN",
            E::Cat => "CAT",
            E::Dog => "DOG",
            E::Dodge => "DODGE",
            E::Unbreaking => "UNBREAKING",
            E::Stabilize => "STABILIZE",
            E::Sparkling => "SPARKLING",
            E::Grinding => "GRINDING",
            E::Mosaic => "MOSAIC",
            E::PickaxeTreasure => "PICKAXE_TREASURE",
            E::FishingRodTreasure => "FISHING_ROD_TREASURE",
            E::MultiTreasure => "MULTI_TREASURE",
            E::Multicatch => "MULTICATCH",
            E::Looting => "LOOTING",
            E::Devour => "DEVOUR",
            E::Bleeding => "BLEEDING",
            E::Freezing => "FREEZING",
            E::Angel => "ANGEL",
            E::Evil => "EVIL",
            E::DayWalker => "DAY_WALKER",
            E::NightWalker => "NIGHT_WALKER",
            E::Reinforce => "REINFORCE",
            E::Empowering => "EMPOWERING",
            E::Carving => "CARVING",
            E::Trench => "TRENCH",
            E::Execution => "EXECUTION",
            E::BloodFrenzy => "BLOOD_FRENZY",
            E::ArmorPiercing => "ARMOR_PIERCING",
            E::Piercing => "PIERCING",
            E::Guardian => "GUARDIAN",
            E::NineLife => "NINE_LIFE",
            E::SoulGrind => "SOUL_GRIND",
            E::Mending => "MENDING",
            E::Nuke => "NUKE",
            E::Annihilation => "ANNIHILATION",
            E::Phoenix => "PHOENIX",
            E::ShadowWalker => "SHADOW_WALKER",
            E::Master => "MASTER",
        }
    }

    /// Resolves only exact canonical persistence identities. Unknown, differently-cased, padded,
    /// or legacy-looking strings fail closed instead of being normalized into a different enchant.
    #[must_use]
    pub fn from_persisted_key(key: &str) -> Option<Self> {
        use CanonicalEnchant as E;
        Some(match key {
            "EFFICIENCY" => E::Efficiency,
            "FORTUNE" => E::Fortune,
            "SMELT" => E::Smelt,
            "LURE" => E::Lure,
            "LUCK_OF_THE_SEA" => E::LuckOfTheSea,
            "LUCK" => E::Luck,
            "STRENGTHEN" => E::Strengthen,
            "SHARP_HOOK" => E::SharpHook,
            "BAIT_RACK" => E::BaitRack,
            "SHARPNESS" => E::Sharpness,
            "SMITE" => E::Smite,
            "BANE_OF_ARTHROPODS" => E::BaneOfArthropods,
            "SWEEPING_EDGE" => E::SweepingEdge,
            "FIRE_ASPECT" => E::FireAspect,
            "KNOCKBACK" => E::Knockback,
            "PROTECTION" => E::Protection,
            "THORN" => E::Thorn,
            "CAT" => E::Cat,
            "DOG" => E::Dog,
            "DODGE" => E::Dodge,
            "UNBREAKING" => E::Unbreaking,
            "STABILIZE" => E::Stabilize,
            "SPARKLING" => E::Sparkling,
            "GRINDING" => E::Grinding,
            "MOSAIC" => E::Mosaic,
            "PICKAXE_TREASURE" => E::PickaxeTreasure,
            "FISHING_ROD_TREASURE" => E::FishingRodTreasure,
            "MULTI_TREASURE" => E::MultiTreasure,
            "MULTICATCH" => E::Multicatch,
            "LOOTING" => E::Looting,
            "DEVOUR" => E::Devour,
            "BLEEDING" => E::Bleeding,
            "FREEZING" => E::Freezing,
            "ANGEL" => E::Angel,
            "EVIL" => E::Evil,
            "DAY_WALKER" => E::DayWalker,
            "NIGHT_WALKER" => E::NightWalker,
            "REINFORCE" => E::Reinforce,
            "EMPOWERING" => E::Empowering,
            "CARVING" => E::Carving,
            "TRENCH" => E::Trench,
            "EXECUTION" => E::Execution,
            "BLOOD_FRENZY" => E::BloodFrenzy,
            "ARMOR_PIERCING" => E::ArmorPiercing,
            "PIERCING" => E::Piercing,
            "GUARDIAN" => E::Guardian,
            "NINE_LIFE" => E::NineLife,
            "SOUL_GRIND" => E::SoulGrind,
            "MENDING" => E::Mending,
            "NUKE" => E::Nuke,
            "ANNIHILATION" => E::Annihilation,
            "PHOENIX" => E::Phoenix,
            "SHADOW_WALKER" => E::ShadowWalker,
            "MASTER" => E::Master,
            _ => return None,
        })
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
/// This shared classification is deliberately below both item mutation and service policy. It does
/// not validate equipment applicability, slot capacity/occupancy, resulting levels, or live loadout
/// membership; callers own those authoritative state checks at their transaction boundary.
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

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_ENCHANTS: [CanonicalEnchant; CANONICAL_ENCHANT_COUNT] = [
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
    fn persistence_keys_are_exact_and_round_trip_all_canonical_identities() {
        for enchant in ALL_ENCHANTS {
            assert_eq!(
                CanonicalEnchant::from_persisted_key(enchant.persisted_key()),
                Some(enchant)
            );
        }
        for invalid in ["", "sharpness", " SHARPNESS", "SHARPNESS ", "TREASURE"] {
            assert_eq!(CanonicalEnchant::from_persisted_key(invalid), None);
        }
    }

    #[test]
    fn survival_core_is_the_only_loadout_scoped_conflict_family() {
        for pair in [
            (CanonicalEnchant::Guardian, CanonicalEnchant::NineLife),
            (CanonicalEnchant::Guardian, CanonicalEnchant::Phoenix),
            (CanonicalEnchant::NineLife, CanonicalEnchant::Phoenix),
        ] {
            assert_eq!(
                canonical_enchant_conflict_scope(pair.0, pair.1),
                Some(EnchantConflictScope::EquippedArmorLoadout)
            );
            assert_eq!(
                canonical_enchant_conflict_scope(pair.1, pair.0),
                Some(EnchantConflictScope::EquippedArmorLoadout)
            );
        }
    }

    #[test]
    fn representative_same_item_and_non_conflicts_preserve_scope() {
        for pair in [
            (CanonicalEnchant::Trench, CanonicalEnchant::Nuke),
            (CanonicalEnchant::Sharpness, CanonicalEnchant::Smite),
            (CanonicalEnchant::FireAspect, CanonicalEnchant::Freezing),
            (CanonicalEnchant::Annihilation, CanonicalEnchant::Execution),
            (CanonicalEnchant::SweepingEdge, CanonicalEnchant::Piercing),
            (CanonicalEnchant::Cat, CanonicalEnchant::Dog),
            (CanonicalEnchant::Angel, CanonicalEnchant::Evil),
            (CanonicalEnchant::Thorn, CanonicalEnchant::Reinforce),
        ] {
            assert_eq!(
                canonical_enchant_conflict_scope(pair.0, pair.1),
                Some(EnchantConflictScope::SameItem)
            );
            assert_eq!(
                canonical_enchant_conflict_scope(pair.1, pair.0),
                Some(EnchantConflictScope::SameItem)
            );
        }

        for pair in [
            (CanonicalEnchant::Piercing, CanonicalEnchant::ArmorPiercing),
            (CanonicalEnchant::DayWalker, CanonicalEnchant::NightWalker),
            (
                CanonicalEnchant::MultiTreasure,
                CanonicalEnchant::Multicatch,
            ),
        ] {
            assert_eq!(canonical_enchant_conflict_scope(pair.0, pair.1), None);
            assert_eq!(canonical_enchant_conflict_scope(pair.1, pair.0), None);
        }
    }
}
