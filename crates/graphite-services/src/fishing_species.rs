use serde::Serialize;

use crate::{
    fishing_area::FishingArea,
    fishing_bait::{FishingBait, FishingBaitEffect, FishingRarity, fishing_bait_policy},
};

pub const CANONICAL_FISH_SPECIES_COUNT: usize = 22;
pub const CANONICAL_FISH_AREA_ROWS: usize = 31;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FishingSpecies {
    Bluegill,
    Carp,
    Catfish,
    Koi,
    Trout,
    Salmon,
    Pike,
    Sturgeon,
    Bass,
    Mackerel,
    Snapper,
    Tuna,
    Pufferfish,
    Swordfish,
    Marlin,
    GiantGrouper,
    Shark,
    Coelacanth,
    Anglerfish,
    AbyssEel,
    Moonfish,
    LeviathanFry,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FishingSpeciesPolicy {
    pub species: FishingSpecies,
    pub rarity: FishingRarity,
    pub reference_weight_grams: u32,
    pub base_npc_value_money: i64,
    pub reference_length_millimeters: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FishingAreaSpeciesPolicy {
    pub area: FishingArea,
    pub species: FishingSpecies,
    pub pool_weight: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct RareBaitAreaSpeciesWeightPreview {
    pub area: FishingArea,
    pub species: FishingSpecies,
    pub rarity: FishingRarity,
    pub base_pool_weight: u16,
    pub rare_bait_applied: bool,
    relative_weight_factor_numerator: u16,
    relative_weight_factor_denominator: u16,
    adjusted_pool_weight_numerator: u32,
    adjusted_pool_weight_denominator: u16,
}

impl RareBaitAreaSpeciesWeightPreview {
    #[must_use]
    pub const fn relative_weight_factor_numerator(self) -> u16 {
        self.relative_weight_factor_numerator
    }

    #[must_use]
    pub const fn relative_weight_factor_denominator(self) -> u16 {
        self.relative_weight_factor_denominator
    }

    #[must_use]
    pub const fn adjusted_pool_weight_numerator(self) -> u32 {
        self.adjusted_pool_weight_numerator
    }

    #[must_use]
    pub const fn adjusted_pool_weight_denominator(self) -> u16 {
        self.adjusted_pool_weight_denominator
    }
}

const STARTER_POOL: [FishingAreaSpeciesPolicy; 4] = [
    area_row(FishingArea::StarterPool, FishingSpecies::Bluegill, 45),
    area_row(FishingArea::StarterPool, FishingSpecies::Carp, 35),
    area_row(FishingArea::StarterPool, FishingSpecies::Catfish, 15),
    area_row(FishingArea::StarterPool, FishingSpecies::Koi, 5),
];

const RIVER: [FishingAreaSpeciesPolicy; 5] = [
    area_row(FishingArea::River, FishingSpecies::Trout, 30),
    area_row(FishingArea::River, FishingSpecies::Salmon, 25),
    area_row(FishingArea::River, FishingSpecies::Catfish, 20),
    area_row(FishingArea::River, FishingSpecies::Pike, 15),
    area_row(FishingArea::River, FishingSpecies::Sturgeon, 10),
];

const LAKE: [FishingAreaSpeciesPolicy; 5] = [
    area_row(FishingArea::Lake, FishingSpecies::Carp, 25),
    area_row(FishingArea::Lake, FishingSpecies::Bass, 25),
    area_row(FishingArea::Lake, FishingSpecies::Catfish, 20),
    area_row(FishingArea::Lake, FishingSpecies::Koi, 15),
    area_row(FishingArea::Lake, FishingSpecies::Sturgeon, 15),
];

const COAST: [FishingAreaSpeciesPolicy; 5] = [
    area_row(FishingArea::Coast, FishingSpecies::Mackerel, 30),
    area_row(FishingArea::Coast, FishingSpecies::Snapper, 25),
    area_row(FishingArea::Coast, FishingSpecies::Tuna, 20),
    area_row(FishingArea::Coast, FishingSpecies::Pufferfish, 15),
    area_row(FishingArea::Coast, FishingSpecies::Swordfish, 10),
];

const DEEP_SEA: [FishingAreaSpeciesPolicy; 6] = [
    area_row(FishingArea::DeepSea, FishingSpecies::Tuna, 25),
    area_row(FishingArea::DeepSea, FishingSpecies::Swordfish, 20),
    area_row(FishingArea::DeepSea, FishingSpecies::Marlin, 15),
    area_row(FishingArea::DeepSea, FishingSpecies::GiantGrouper, 15),
    area_row(FishingArea::DeepSea, FishingSpecies::Shark, 15),
    area_row(FishingArea::DeepSea, FishingSpecies::Coelacanth, 10),
];

const ABYSS: [FishingAreaSpeciesPolicy; 6] = [
    area_row(FishingArea::Abyss, FishingSpecies::Anglerfish, 25),
    area_row(FishingArea::Abyss, FishingSpecies::AbyssEel, 25),
    area_row(FishingArea::Abyss, FishingSpecies::Coelacanth, 20),
    area_row(FishingArea::Abyss, FishingSpecies::Moonfish, 15),
    area_row(FishingArea::Abyss, FishingSpecies::LeviathanFry, 5),
    area_row(FishingArea::Abyss, FishingSpecies::GiantGrouper, 10),
];

/// Resolves immutable finite species metadata from the canonical Fishing catalog.
///
/// Decimal specification values are represented exactly in integral physical units: reference
/// weight in grams and ReferenceLength in millimeters. This avoids introducing floating-point
/// persistence or arithmetic semantics. It does not choose the future FishInstance storage format.
///
/// `base_npc_value_money` is the species base Money value only. The latest canonical specification
/// already prices rarity into that base value, so rarity must not be multiplied into Money value a
/// second time. This policy deliberately does not evaluate the unresolved `(Weight/Wref)^0.85`
/// valuation term, sample the truncated log-normal weight distribution, or evaluate the cube-root
/// length formula.
#[must_use]
pub const fn fishing_species_policy(species: FishingSpecies) -> FishingSpeciesPolicy {
    use FishingRarity as R;
    use FishingSpecies as S;

    let (rarity, reference_weight_grams, base_npc_value_money, reference_length_millimeters) =
        match species {
            S::Bluegill => (R::Common, 400, 50, 200),
            S::Carp => (R::Common, 2_000, 90, 450),
            S::Catfish => (R::Uncommon, 3_000, 150, 550),
            S::Koi => (R::Rare, 1_500, 400, 400),
            S::Trout => (R::Common, 1_000, 100, 350),
            S::Salmon => (R::Uncommon, 3_000, 180, 650),
            S::Pike => (R::Rare, 4_000, 350, 700),
            S::Sturgeon => (R::Epic, 8_000, 900, 1_100),
            S::Bass => (R::Common, 1_200, 130, 400),
            S::Mackerel => (R::Common, 1_000, 160, 350),
            S::Snapper => (R::Uncommon, 2_000, 250, 450),
            S::Tuna => (R::Rare, 15_000, 700, 1_200),
            S::Pufferfish => (R::Rare, 1_000, 350, 250),
            S::Swordfish => (R::Epic, 30_000, 1_200, 2_000),
            S::Marlin => (R::Epic, 40_000, 1_600, 2_300),
            S::GiantGrouper => (R::Epic, 50_000, 1_800, 1_500),
            S::Shark => (R::Legendary, 70_000, 2_500, 2_200),
            S::Coelacanth => (R::Legendary, 20_000, 5_000, 1_400),
            S::Anglerfish => (R::Rare, 5_000, 1_200, 500),
            S::AbyssEel => (R::Epic, 8_000, 1_800, 1_200),
            S::Moonfish => (R::Legendary, 10_000, 3_500, 600),
            S::LeviathanFry => (R::Mythic, 100_000, 10_000, 2_500),
        };

    FishingSpeciesPolicy {
        species,
        rarity,
        reference_weight_grams,
        base_npc_value_money,
        reference_length_millimeters,
    }
}

/// Returns the canonical relative species-weight rows eligible for one Fishing area.
///
/// Pool weights are kept as the specification's finite relative weights. The current canonical rows
/// happen to total 100 in every area, but callers must not reinterpret the field as a separately
/// frozen percentage representation; future selection logic still owns normalization after applying
/// eligible relative-weight modifiers such as Rare Bait or Luck effects.
#[must_use]
pub const fn fishing_area_species_pool(area: FishingArea) -> &'static [FishingAreaSpeciesPolicy] {
    match area {
        FishingArea::StarterPool => &STARTER_POOL,
        FishingArea::River => &RIVER,
        FishingArea::Lake => &LAKE,
        FishingArea::Coast => &COAST,
        FishingArea::DeepSea => &DEEP_SEA,
        FishingArea::Abyss => &ABYSS,
    }
}

/// Applies Rare Bait to one authoritative species-pool row without normalizing the area pool.
///
/// Rare Bait multiplies Rare/Epic/Legendary/Mythic species pool weights by `1.12` (`28/25`) before
/// normalization. Common and Uncommon rows remain unchanged. Both the affected-rarity set and the
/// boost factor are read from the existing Rare Bait catalog row so this species preview does not
/// create a second source of truth for bait semantics.
///
/// The caller supplies only `area` and `species`; the canonical base `pool_weight` and rarity are
/// re-derived from the existing species owners. A species not present in the requested area returns
/// `None` rather than allowing a fabricated area/species weight pair into the preview.
///
/// This policy is intentionally Rare-Bait-only. It does not compose Gold Rod, Luck of the Sea,
/// shared Fishing modifier caps, final normalization, RNG selection, FishInstance creation, bait
/// consumption, AEXP, or settlement.
#[must_use]
pub fn preview_rare_bait_area_species_weight(
    area: FishingArea,
    species: FishingSpecies,
) -> Option<RareBaitAreaSpeciesWeightPreview> {
    let base_row = fishing_area_species_pool(area)
        .iter()
        .find(|row| row.species == species)?;
    let rarity = fishing_species_policy(species).rarity;

    let FishingBaitEffect::Rare {
        affected_species_rarities,
        eligible_species_relative_weight_factor,
    } = fishing_bait_policy(FishingBait::Rare).effect
    else {
        unreachable!("Rare Bait catalog row returned a non-Rare effect")
    };

    let rare_bait_applied = affected_species_rarities.contains(&rarity);
    let (relative_weight_factor_numerator, relative_weight_factor_denominator) =
        if rare_bait_applied {
            (
                eligible_species_relative_weight_factor.numerator(),
                eligible_species_relative_weight_factor.denominator(),
            )
        } else {
            (1, 1)
        };

    Some(RareBaitAreaSpeciesWeightPreview {
        area,
        species,
        rarity,
        base_pool_weight: base_row.pool_weight,
        rare_bait_applied,
        relative_weight_factor_numerator,
        relative_weight_factor_denominator,
        adjusted_pool_weight_numerator: u32::from(base_row.pool_weight)
            * u32::from(relative_weight_factor_numerator),
        adjusted_pool_weight_denominator: relative_weight_factor_denominator,
    })
}

const fn area_row(
    area: FishingArea,
    species: FishingSpecies,
    pool_weight: u16,
) -> FishingAreaSpeciesPolicy {
    FishingAreaSpeciesPolicy {
        area,
        species,
        pool_weight,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CURRENT_AREA_POOL_WEIGHT_SUM: u16 = 100;
    const ALL_SPECIES: [FishingSpecies; CANONICAL_FISH_SPECIES_COUNT] = [
        FishingSpecies::Bluegill,
        FishingSpecies::Carp,
        FishingSpecies::Catfish,
        FishingSpecies::Koi,
        FishingSpecies::Trout,
        FishingSpecies::Salmon,
        FishingSpecies::Pike,
        FishingSpecies::Sturgeon,
        FishingSpecies::Bass,
        FishingSpecies::Mackerel,
        FishingSpecies::Snapper,
        FishingSpecies::Tuna,
        FishingSpecies::Pufferfish,
        FishingSpecies::Swordfish,
        FishingSpecies::Marlin,
        FishingSpecies::GiantGrouper,
        FishingSpecies::Shark,
        FishingSpecies::Coelacanth,
        FishingSpecies::Anglerfish,
        FishingSpecies::AbyssEel,
        FishingSpecies::Moonfish,
        FishingSpecies::LeviathanFry,
    ];
    const ALL_AREAS: [FishingArea; 6] = [
        FishingArea::StarterPool,
        FishingArea::River,
        FishingArea::Lake,
        FishingArea::Coast,
        FishingArea::DeepSea,
        FishingArea::Abyss,
    ];

    #[test]
    fn species_catalog_has_exact_positive_finite_metadata() {
        assert_eq!(ALL_SPECIES.len(), CANONICAL_FISH_SPECIES_COUNT);
        for species in ALL_SPECIES {
            let policy = fishing_species_policy(species);
            assert_eq!(policy.species, species);
            assert!(policy.reference_weight_grams > 0);
            assert!(policy.base_npc_value_money > 0);
            assert!(policy.reference_length_millimeters > 0);
        }
    }

    #[test]
    fn area_pools_preserve_expected_cardinality_and_current_weight_sums() {
        let expected_lengths = [4, 5, 5, 5, 6, 6];
        let mut total_rows = 0;

        for (area, expected_len) in ALL_AREAS.into_iter().zip(expected_lengths) {
            let rows = fishing_area_species_pool(area);
            total_rows += rows.len();
            assert_eq!(rows.len(), expected_len);
            assert!(
                rows.iter()
                    .all(|row| row.area == area && row.pool_weight > 0)
            );
            assert_eq!(
                rows.iter().map(|row| row.pool_weight).sum::<u16>(),
                CURRENT_AREA_POOL_WEIGHT_SUM
            );
        }

        assert_eq!(total_rows, CANONICAL_FISH_AREA_ROWS);
    }

    #[test]
    fn repeated_species_reuse_one_species_policy_across_areas() {
        assert!(
            fishing_area_species_pool(FishingArea::StarterPool)
                .iter()
                .any(|row| row.species == FishingSpecies::Catfish)
        );
        assert!(
            fishing_area_species_pool(FishingArea::River)
                .iter()
                .any(|row| row.species == FishingSpecies::Catfish)
        );
        assert!(
            fishing_area_species_pool(FishingArea::Lake)
                .iter()
                .any(|row| row.species == FishingSpecies::Catfish)
        );
        assert!(
            fishing_area_species_pool(FishingArea::DeepSea)
                .iter()
                .any(|row| row.species == FishingSpecies::Coelacanth)
        );
        assert!(
            fishing_area_species_pool(FishingArea::Abyss)
                .iter()
                .any(|row| row.species == FishingSpecies::Coelacanth)
        );
    }

    #[test]
    fn highest_rarity_and_reference_values_match_the_frozen_catalog() {
        assert_eq!(
            fishing_species_policy(FishingSpecies::LeviathanFry),
            FishingSpeciesPolicy {
                species: FishingSpecies::LeviathanFry,
                rarity: FishingRarity::Mythic,
                reference_weight_grams: 100_000,
                base_npc_value_money: 10_000,
                reference_length_millimeters: 2_500,
            }
        );
        assert_eq!(
            fishing_species_policy(FishingSpecies::Coelacanth).base_npc_value_money,
            5_000
        );
    }

    #[test]
    fn rare_bait_applies_catalog_factor_to_every_eligible_area_row_only() {
        let mut rows_seen = 0;
        for area in ALL_AREAS {
            for row in fishing_area_species_pool(area) {
                rows_seen += 1;
                let preview = preview_rare_bait_area_species_weight(area, row.species).unwrap();
                let rarity = fishing_species_policy(row.species).rarity;
                let eligible = matches!(
                    rarity,
                    FishingRarity::Rare
                        | FishingRarity::Epic
                        | FishingRarity::Legendary
                        | FishingRarity::Mythic
                );

                assert_eq!(preview.area, area);
                assert_eq!(preview.species, row.species);
                assert_eq!(preview.rarity, rarity);
                assert_eq!(preview.base_pool_weight, row.pool_weight);
                assert_eq!(preview.rare_bait_applied, eligible);

                let expected_factor = if eligible { (28, 25) } else { (1, 1) };
                assert_eq!(
                    (
                        preview.relative_weight_factor_numerator(),
                        preview.relative_weight_factor_denominator(),
                    ),
                    expected_factor
                );
                assert_eq!(
                    preview.adjusted_pool_weight_numerator(),
                    u32::from(row.pool_weight) * u32::from(expected_factor.0)
                );
                assert_eq!(
                    preview.adjusted_pool_weight_denominator(),
                    expected_factor.1
                );
            }
        }

        assert_eq!(rows_seen, CANONICAL_FISH_AREA_ROWS);
    }

    #[test]
    fn rare_bait_preview_rejects_noncanonical_area_species_pairs() {
        assert_eq!(
            preview_rare_bait_area_species_weight(
                FishingArea::StarterPool,
                FishingSpecies::LeviathanFry,
            ),
            None
        );
        assert_eq!(
            preview_rare_bait_area_species_weight(FishingArea::River, FishingSpecies::Bluegill),
            None
        );
    }
}
