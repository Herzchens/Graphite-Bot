use graphite_services::{
    BASE_REPEATABLE_MANUAL_REWARD_ACTION_COOLDOWN_MILLIS, CanonicalEnchant, DirectFishingBookPool,
    FishingArea, FishingCatchBranch, FishingSpecies, FishingTreasureResult, FishingVariant,
    direct_fishing_book_pool_policy, direct_fishing_mythic_enchant_weight,
    fishing_area_species_pool, fishing_base_catch_branch_policy,
    fishing_base_treasure_result_policy, fishing_variant_catalog, resolve_fish_instance_numeric_v1,
};

const BASE_CASTS_PER_HOUR: u128 = 360;
const VARIANT_COMMON_DENOMINATOR: u128 = 200;
const WEIGHT_QUANTILE_SAMPLES: u32 = 2_048;

#[derive(Clone, Copy)]
struct Probability {
    numerator: u128,
    denominator: u128,
}

impl Probability {
    const fn new(numerator: u128, denominator: u128) -> Self {
        Self {
            numerator,
            denominator,
        }
    }

    const fn multiply(self, other: Self) -> Self {
        Self {
            numerator: self.numerator * other.numerator,
            denominator: self.denominator * other.denominator,
        }
    }
}

#[test]
fn latest_master_fishing_tables_compose_to_the_frozen_base_rates() {
    let fish = fishing_base_catch_branch_policy(FishingCatchBranch::Fish);
    let junk = fishing_base_catch_branch_policy(FishingCatchBranch::Junk);
    let treasure = fishing_base_catch_branch_policy(FishingCatchBranch::Treasure);
    assert_eq!(
        [
            fish.relative_weight,
            junk.relative_weight,
            treasure.relative_weight
        ],
        [176, 17, 7]
    );
    assert_eq!(
        u32::from(fish.relative_weight + junk.relative_weight + treasure.relative_weight),
        200
    );

    let treasure_results = [
        FishingTreasureResult::MaterialBundle,
        FishingTreasureResult::CrateOrChest,
        FishingTreasureResult::EnchantBook,
        FishingTreasureResult::OrbOrCatalystFragmentOrItem,
        FishingTreasureResult::RareBaitOrUtilityItem,
        FishingTreasureResult::RelicOrCollectible,
    ];
    let treasure_weights =
        treasure_results.map(|result| fishing_base_treasure_result_policy(result).relative_weight);
    assert_eq!(treasure_weights, [20, 15, 4, 4, 5, 2]);
    assert_eq!(treasure_weights.iter().sum::<u16>(), 50);

    let overall_basis_points = treasure_weights.map(|result_weight| {
        u32::from(treasure.relative_weight) * u32::from(result_weight) * 10_000 / (200 * 50)
    });
    assert_eq!(overall_basis_points, [140, 105, 28, 28, 35, 14]);

    let book_pools = [
        DirectFishingBookPool::ShopCommon,
        DirectFishingBookPool::MidLoot,
        DirectFishingBookPool::Rare,
        DirectFishingBookPool::Mending,
        DirectFishingBookPool::Mythic,
    ];
    let book_weights = book_pools.map(|pool| direct_fishing_book_pool_policy(pool).relative_weight);
    assert_eq!(book_weights, [58, 24, 12, 4, 2]);
    assert_eq!(
        book_weights
            .iter()
            .map(|weight| u16::from(*weight))
            .sum::<u16>(),
        100
    );

    let mythic_weights = [
        direct_fishing_mythic_enchant_weight(CanonicalEnchant::Nuke).unwrap(),
        direct_fishing_mythic_enchant_weight(CanonicalEnchant::Annihilation).unwrap(),
        direct_fishing_mythic_enchant_weight(CanonicalEnchant::Phoenix).unwrap(),
    ];
    assert_eq!(mythic_weights, [45, 30, 25]);
    assert_eq!(
        mythic_weights
            .iter()
            .map(|weight| u16::from(*weight))
            .sum::<u16>(),
        100
    );
}

#[test]
fn direct_book_pacing_matches_the_latest_master_analytic_table() {
    assert_eq!(BASE_REPEATABLE_MANUAL_REWARD_ACTION_COOLDOWN_MILLIS, 10_000);
    let treasure_branch = Probability::new(7, 200);
    let enchant_book_within_treasure = Probability::new(4, 50);
    let any_book = treasure_branch.multiply(enchant_book_within_treasure);

    let mending = any_book.multiply(Probability::new(4, 100));
    let mythic = any_book.multiply(Probability::new(2, 100));
    let nuke = mythic.multiply(Probability::new(45, 100));
    let annihilation = mythic.multiply(Probability::new(30, 100));
    let phoenix = mythic.multiply(Probability::new(25, 100));

    assert_eq!(round_expected_casts(any_book), 357);
    assert_eq!(round_expected_casts(mending), 8_929);
    assert_eq!(round_expected_casts(nuke), 39_683);
    assert_eq!(round_expected_casts(annihilation), 59_524);
    assert_eq!(round_expected_casts(phoenix), 71_429);

    assert_eq!(round_expected_tenths_minutes(any_book), 595);
    assert_eq!(round_expected_tenths_hours(mending), 248);
    assert_eq!(round_expected_tenths_hours(nuke), 1_102);
    assert_eq!(round_expected_tenths_hours(annihilation), 1_653);
    assert_eq!(round_expected_tenths_hours(phoenix), 1_984);
}

#[test]
fn ten_million_cast_analytic_book_counts_match_the_frozen_pool_mix() {
    const CASTS: u128 = 10_000_000;
    let any_book = Probability::new(7, 200).multiply(Probability::new(4, 50));
    let expected_books = round_div(CASTS * any_book.numerator, any_book.denominator);
    assert_eq!(expected_books, 28_000);

    let pools = [
        (DirectFishingBookPool::ShopCommon, 16_240),
        (DirectFishingBookPool::MidLoot, 6_720),
        (DirectFishingBookPool::Rare, 3_360),
        (DirectFishingBookPool::Mending, 1_120),
        (DirectFishingBookPool::Mythic, 560),
    ];
    for (pool, expected_count) in pools {
        let weight = u128::from(direct_fishing_book_pool_policy(pool).relative_weight);
        assert_eq!(round_div(expected_books * weight, 100), expected_count);
    }
}

#[test]
fn baseline_fish_money_regression_stays_near_the_master_red_team_anchors() {
    let deep_sea = baseline_fish_money_per_hour(FishingArea::DeepSea);
    let abyss = baseline_fish_money_per_hour(FishingArea::Abyss);

    // Latest Master Spec Monte Carlo anchors after removing the erroneous second rarity multiplier:
    // Deep Sea ≈607k/h and Abyss ≈995k/h at the 10 s zero-modifier cadence. This deterministic
    // stratified integration uses the production integer FishInstance valuation kernel, so a tight
    // envelope protects the economic fix without pretending the historical Monte Carlo sample is an
    // exact oracle.
    assert!(
        (595_000..=620_000).contains(&deep_sea),
        "Deep Sea baseline fish Money drifted outside the frozen balance envelope: {deep_sea}/h"
    );
    assert!(
        (975_000..=1_015_000).contains(&abyss),
        "Abyss baseline fish Money drifted outside the frozen balance envelope: {abyss}/h"
    );

    // The previously rejected double-rarity implementation was roughly 1.94M/h and 3.96M/h.
    assert!(deep_sea < 1_000_000);
    assert!(abyss < 1_500_000);
}

#[test]
fn every_area_remains_a_stationary_complete_probability_table() {
    for area in [
        FishingArea::StarterPool,
        FishingArea::River,
        FishingArea::Lake,
        FishingArea::Coast,
        FishingArea::DeepSea,
        FishingArea::Abyss,
    ] {
        let rows = fishing_area_species_pool(area);
        assert!(!rows.is_empty());
        assert_eq!(rows.iter().map(|row| row.pool_weight).sum::<u16>(), 100);
        assert!(
            rows.iter()
                .all(|row| row.area == area && row.pool_weight > 0)
        );
    }
}

fn baseline_fish_money_per_hour(area: FishingArea) -> u64 {
    let fish_branch_weight =
        u128::from(fishing_base_catch_branch_policy(FishingCatchBranch::Fish).relative_weight);
    let branch_total = 200_u128;

    let mut area_weighted_value_numerator = 0_u128;
    for row in fishing_area_species_pool(area) {
        let species_expected_value_numerator = species_expected_value_numerator(row.species);
        area_weighted_value_numerator +=
            u128::from(row.pool_weight) * species_expected_value_numerator;
    }

    let numerator = area_weighted_value_numerator * fish_branch_weight * BASE_CASTS_PER_HOUR;
    let denominator =
        100_u128 * u128::from(WEIGHT_QUANTILE_SAMPLES) * VARIANT_COMMON_DENOMINATOR * branch_total;

    u64::try_from(round_div(numerator, denominator)).unwrap()
}

fn species_expected_value_numerator(species: FishingSpecies) -> u128 {
    let variants = fishing_variant_catalog();
    let mut total = 0_u128;

    for sample in 0..WEIGHT_QUANTILE_SAMPLES {
        let draw = stratified_ppm_bucket(sample, WEIGHT_QUANTILE_SAMPLES);
        for variant in variants {
            let variant_weight = variant_weight_units(variant.variant);
            let value =
                resolve_fish_instance_numeric_v1(species, variant.variant, draw, 500_000, false)
                    .unwrap()
                    .npc_value_money;
            total += u128::try_from(value).unwrap() * variant_weight;
        }
    }

    total
}

fn variant_weight_units(variant: FishingVariant) -> u128 {
    let policy = graphite_services::fishing_variant_policy(variant);
    let denominator = u128::from(policy.base_probability.denominator());
    assert_eq!(VARIANT_COMMON_DENOMINATOR % denominator, 0);
    u128::from(policy.base_probability.numerator()) * (VARIANT_COMMON_DENOMINATOR / denominator)
}

fn stratified_ppm_bucket(sample: u32, sample_count: u32) -> u32 {
    let numerator = (u64::from(sample) * 2 + 1) * 1_000_000;
    let denominator = u64::from(sample_count) * 2;
    u32::try_from(numerator / denominator).unwrap()
}

fn round_expected_casts(probability: Probability) -> u128 {
    round_div(probability.denominator, probability.numerator)
}

fn round_expected_tenths_minutes(probability: Probability) -> u128 {
    let numerator = u128::from(BASE_REPEATABLE_MANUAL_REWARD_ACTION_COOLDOWN_MILLIS)
        * probability.denominator
        * 10;
    let denominator = probability.numerator * 60_000;
    round_div(numerator, denominator)
}

fn round_expected_tenths_hours(probability: Probability) -> u128 {
    let numerator = u128::from(BASE_REPEATABLE_MANUAL_REWARD_ACTION_COOLDOWN_MILLIS)
        * probability.denominator
        * 10;
    let denominator = probability.numerator * 3_600_000;
    round_div(numerator, denominator)
}

fn round_div(numerator: u128, denominator: u128) -> u128 {
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    if remainder * 2 >= denominator {
        quotient + 1
    } else {
        quotient
    }
}
