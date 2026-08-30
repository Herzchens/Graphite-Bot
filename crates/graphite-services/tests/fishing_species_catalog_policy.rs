use graphite_services::{
    CANONICAL_AREA_POOL_WEIGHT_TOTAL, CANONICAL_FISH_AREA_ROWS, CANONICAL_FISH_SPECIES_COUNT,
    FishingArea, FishingRarity, FishingSpecies, FishingSpeciesPolicy, fishing_area_species_pool,
    fishing_species_policy,
};

#[test]
fn public_api_preserves_all_twenty_two_species_rows() {
    let expected = [
        (FishingSpecies::Bluegill, FishingRarity::Common, 400, 50, 200),
        (FishingSpecies::Carp, FishingRarity::Common, 2_000, 90, 450),
        (
            FishingSpecies::Catfish,
            FishingRarity::Uncommon,
            3_000,
            150,
            550,
        ),
        (FishingSpecies::Koi, FishingRarity::Rare, 1_500, 400, 400),
        (FishingSpecies::Trout, FishingRarity::Common, 1_000, 100, 350),
        (
            FishingSpecies::Salmon,
            FishingRarity::Uncommon,
            3_000,
            180,
            650,
        ),
        (FishingSpecies::Pike, FishingRarity::Rare, 4_000, 350, 700),
        (
            FishingSpecies::Sturgeon,
            FishingRarity::Epic,
            8_000,
            900,
            1_100,
        ),
        (FishingSpecies::Bass, FishingRarity::Common, 1_200, 130, 400),
        (
            FishingSpecies::Mackerel,
            FishingRarity::Common,
            1_000,
            160,
            350,
        ),
        (
            FishingSpecies::Snapper,
            FishingRarity::Uncommon,
            2_000,
            250,
            450,
        ),
        (FishingSpecies::Tuna, FishingRarity::Rare, 15_000, 700, 1_200),
        (
            FishingSpecies::Pufferfish,
            FishingRarity::Rare,
            1_000,
            350,
            250,
        ),
        (
            FishingSpecies::Swordfish,
            FishingRarity::Epic,
            30_000,
            1_200,
            2_000,
        ),
        (
            FishingSpecies::Marlin,
            FishingRarity::Epic,
            40_000,
            1_600,
            2_300,
        ),
        (
            FishingSpecies::GiantGrouper,
            FishingRarity::Epic,
            50_000,
            1_800,
            1_500,
        ),
        (
            FishingSpecies::Shark,
            FishingRarity::Legendary,
            70_000,
            2_500,
            2_200,
        ),
        (
            FishingSpecies::Coelacanth,
            FishingRarity::Legendary,
            20_000,
            5_000,
            1_400,
        ),
        (
            FishingSpecies::Anglerfish,
            FishingRarity::Rare,
            5_000,
            1_200,
            500,
        ),
        (
            FishingSpecies::AbyssEel,
            FishingRarity::Epic,
            8_000,
            1_800,
            1_200,
        ),
        (
            FishingSpecies::Moonfish,
            FishingRarity::Legendary,
            10_000,
            3_500,
            600,
        ),
        (
            FishingSpecies::LeviathanFry,
            FishingRarity::Mythic,
            100_000,
            10_000,
            2_500,
        ),
    ];

    assert_eq!(expected.len(), CANONICAL_FISH_SPECIES_COUNT);

    for (species, rarity, weight, value, length) in expected {
        assert_eq!(
            fishing_species_policy(species),
            FishingSpeciesPolicy {
                species,
                rarity,
                reference_weight_grams: weight,
                base_npc_value_money: value,
                reference_length_millimeters: length,
            }
        );
    }
}

#[test]
fn public_api_preserves_every_area_pool_and_weight() {
    let expected = [
        (
            FishingArea::StarterPool,
            &[
                (FishingSpecies::Bluegill, 45),
                (FishingSpecies::Carp, 35),
                (FishingSpecies::Catfish, 15),
                (FishingSpecies::Koi, 5),
            ][..],
        ),
        (
            FishingArea::River,
            &[
                (FishingSpecies::Trout, 30),
                (FishingSpecies::Salmon, 25),
                (FishingSpecies::Catfish, 20),
                (FishingSpecies::Pike, 15),
                (FishingSpecies::Sturgeon, 10),
            ][..],
        ),
        (
            FishingArea::Lake,
            &[
                (FishingSpecies::Carp, 25),
                (FishingSpecies::Bass, 25),
                (FishingSpecies::Catfish, 20),
                (FishingSpecies::Koi, 15),
                (FishingSpecies::Sturgeon, 15),
            ][..],
        ),
        (
            FishingArea::Coast,
            &[
                (FishingSpecies::Mackerel, 30),
                (FishingSpecies::Snapper, 25),
                (FishingSpecies::Tuna, 20),
                (FishingSpecies::Pufferfish, 15),
                (FishingSpecies::Swordfish, 10),
            ][..],
        ),
        (
            FishingArea::DeepSea,
            &[
                (FishingSpecies::Tuna, 25),
                (FishingSpecies::Swordfish, 20),
                (FishingSpecies::Marlin, 15),
                (FishingSpecies::GiantGrouper, 15),
                (FishingSpecies::Shark, 15),
                (FishingSpecies::Coelacanth, 10),
            ][..],
        ),
        (
            FishingArea::Abyss,
            &[
                (FishingSpecies::Anglerfish, 25),
                (FishingSpecies::AbyssEel, 25),
                (FishingSpecies::Coelacanth, 20),
                (FishingSpecies::Moonfish, 15),
                (FishingSpecies::LeviathanFry, 5),
                (FishingSpecies::GiantGrouper, 10),
            ][..],
        ),
    ];

    let mut row_count = 0;
    for (area, expected_rows) in expected {
        let actual = fishing_area_species_pool(area);
        assert_eq!(actual.len(), expected_rows.len());
        assert_eq!(
            actual.iter().map(|row| row.pool_weight).sum::<u16>(),
            CANONICAL_AREA_POOL_WEIGHT_TOTAL
        );

        for (row, &(species, pool_weight)) in actual.iter().zip(expected_rows) {
            assert_eq!(row.area, area);
            assert_eq!(row.species, species);
            assert_eq!(row.pool_weight, pool_weight);
        }
        row_count += actual.len();
    }

    assert_eq!(row_count, CANONICAL_FISH_AREA_ROWS);
}

#[test]
fn public_api_keeps_species_metadata_independent_from_area_weight() {
    let deep_sea = fishing_area_species_pool(FishingArea::DeepSea)
        .iter()
        .find(|row| row.species == FishingSpecies::GiantGrouper)
        .unwrap();
    let abyss = fishing_area_species_pool(FishingArea::Abyss)
        .iter()
        .find(|row| row.species == FishingSpecies::GiantGrouper)
        .unwrap();

    assert_eq!(deep_sea.pool_weight, 15);
    assert_eq!(abyss.pool_weight, 10);
    assert_eq!(
        fishing_species_policy(deep_sea.species),
        fishing_species_policy(abyss.species)
    );
}
