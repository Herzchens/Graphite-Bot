use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FishingCatchBranch {
    Fish,
    Junk,
    Treasure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FishingCatchBranchBasePolicy {
    pub branch: FishingCatchBranch,
    pub relative_weight: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FishingTreasureResult {
    MaterialBundle,
    CrateOrChest,
    EnchantBook,
    OrbOrCatalystFragmentOrItem,
    RareBaitOrUtilityItem,
    RelicOrCollectible,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FishingTreasureResultBasePolicy {
    pub result: FishingTreasureResult,
    pub relative_weight: u16,
}

/// Resolves one row of the zero-temporary-modifier `/fish` catch-branch table.
///
/// The canonical 88.00% Fish / 8.50% Junk / 3.50% Treasure baseline is represented as the reduced
/// common-scale relative weights 176 / 17 / 7. Normalizing the complete three-row table therefore
/// reproduces the exact baseline probabilities without embedding a percentage scale into later
/// modifier arithmetic.
///
/// These are **base relative weights**, not immutable final probabilities. Fishing modifiers such
/// as Treasure Bait and Gold Rod side-grade effects transform eligible branch weights before the
/// future shared Fishing normalization/cap stage. This pure policy deliberately does not compose
/// those modifiers or perform RNG.
///
/// The branch table has no empty outcome. That does not guarantee final catch settlement: a Fish
/// candidate can still escape or be lost to a line break in the later capability stage.
#[must_use]
pub const fn fishing_base_catch_branch_policy(
    branch: FishingCatchBranch,
) -> FishingCatchBranchBasePolicy {
    let relative_weight = match branch {
        FishingCatchBranch::Fish => 176,
        FishingCatchBranch::Junk => 17,
        FishingCatchBranch::Treasure => 7,
    };

    FishingCatchBranchBasePolicy {
        branch,
        relative_weight,
    }
}

/// Resolves one row of the base result table after the Treasure branch has already been selected.
///
/// The canonical within-Treasure chances 38% / 26% / 10% / 8% / 10% / 8% are represented as the
/// reduced common-scale relative weights 19 / 13 / 5 / 4 / 5 / 4. Treasure-branch modifiers do not
/// get applied a second time to this internal table: for example, Treasure X changes the Treasure
/// branch relatively but does not multiply an Enchant Book's internal rarity. Multi Treasure also
/// resolves quantity only after a Treasure proc/result selection.
///
/// This policy does not resolve the direct Enchant Book pool, item quantities, RNG, or settlement.
#[must_use]
pub const fn fishing_base_treasure_result_policy(
    result: FishingTreasureResult,
) -> FishingTreasureResultBasePolicy {
    let relative_weight = match result {
        FishingTreasureResult::MaterialBundle => 19,
        FishingTreasureResult::CrateOrChest => 13,
        FishingTreasureResult::EnchantBook => 5,
        FishingTreasureResult::OrbOrCatalystFragmentOrItem => 4,
        FishingTreasureResult::RareBaitOrUtilityItem => 5,
        FishingTreasureResult::RelicOrCollectible => 4,
    };

    FishingTreasureResultBasePolicy {
        result,
        relative_weight,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATCH_BRANCHES: [FishingCatchBranch; 3] = [
        FishingCatchBranch::Fish,
        FishingCatchBranch::Junk,
        FishingCatchBranch::Treasure,
    ];
    const TREASURE_RESULTS: [FishingTreasureResult; 6] = [
        FishingTreasureResult::MaterialBundle,
        FishingTreasureResult::CrateOrChest,
        FishingTreasureResult::EnchantBook,
        FishingTreasureResult::OrbOrCatalystFragmentOrItem,
        FishingTreasureResult::RareBaitOrUtilityItem,
        FishingTreasureResult::RelicOrCollectible,
    ];

    #[test]
    fn catch_branch_weights_reproduce_exact_zero_modifier_chances() {
        let weights = CATCH_BRANCHES.map(|branch| {
            fishing_base_catch_branch_policy(branch).relative_weight
        });
        assert_eq!(weights, [176, 17, 7]);

        let total: u16 = weights.iter().sum();
        assert_eq!(total, 200);
        assert_eq!(
            weights.map(|weight| u32::from(weight) * 10_000 / u32::from(total)),
            [8_800, 850, 350]
        );
    }

    #[test]
    fn treasure_result_weights_reproduce_exact_internal_chances() {
        let weights = TREASURE_RESULTS.map(|result| {
            fishing_base_treasure_result_policy(result).relative_weight
        });
        assert_eq!(weights, [19, 13, 5, 4, 5, 4]);

        let total: u16 = weights.iter().sum();
        assert_eq!(total, 50);
        assert_eq!(
            weights.map(|weight| u32::from(weight) * 10_000 / u32::from(total)),
            [3_800, 2_600, 1_000, 800, 1_000, 800]
        );
    }

    #[test]
    fn nested_treasure_weights_reproduce_exact_overall_base_chances() {
        let branch_weights = CATCH_BRANCHES.map(|branch| {
            fishing_base_catch_branch_policy(branch).relative_weight
        });
        let treasure_weights = TREASURE_RESULTS.map(|result| {
            fishing_base_treasure_result_policy(result).relative_weight
        });
        let branch_total: u32 = branch_weights.iter().map(|weight| u32::from(*weight)).sum();
        let treasure_total: u32 = treasure_weights.iter().map(|weight| u32::from(*weight)).sum();
        let treasure_branch_weight =
            fishing_base_catch_branch_policy(FishingCatchBranch::Treasure).relative_weight;

        let overall_basis_points = treasure_weights.map(|result_weight| {
            u32::from(treasure_branch_weight) * u32::from(result_weight) * 10_000
                / (branch_total * treasure_total)
        });
        assert_eq!(overall_basis_points, [133, 91, 35, 28, 35, 28]);
    }
}
