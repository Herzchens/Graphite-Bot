use graphite_services::{
    ActionSpeedBonus, AdditionalFishInstanceIdentityPolicy, FishingRarity,
    MANUAL_FISHING_BASE_MULTI_CATCH_AEXP_CAP_MULTIPLIER, MULTI_TREASURE_MAX_LEVEL,
    MULTI_TREASURE_PROBABILITY_SCALE_PPM, MULTICATCH_MAX_LEVEL, MULTICATCH_PROBABILITY_SCALE_PPM,
    MultiTreasureCount, MulticatchCount, RepeatedTreasurePayloadResolutionPolicy,
    compose_multicatch_and_school_bait, manual_fishing_base_fish_cast_aexp,
    manual_mine_fish_cooldown_policy, multi_treasure_count_policy, multicatch_count_policy,
    repeated_treasure_payload_resolution_policy,
};

fn ratio(numerator: u64, denominator: u64) -> ActionSpeedBonus {
    ActionSpeedBonus::new(numerator, denominator).unwrap()
}

#[test]
fn public_multicatch_curve_scales_linearly_and_keeps_independent_fish_identity() {
    let outcomes = [
        MulticatchCount::Single,
        MulticatchCount::Double,
        MulticatchCount::Triple,
        MulticatchCount::Quadruple,
        MulticatchCount::Quintuple,
    ];

    for level in 1..=MULTICATCH_MAX_LEVEL {
        let policies = outcomes.map(|outcome| multicatch_count_policy(level, outcome).unwrap());
        assert_eq!(
            policies
                .iter()
                .map(|policy| policy.probability_ppm)
                .sum::<u32>(),
            MULTICATCH_PROBABILITY_SCALE_PPM
        );
    }

    let level_one = outcomes.map(|outcome| multicatch_count_policy(1, outcome).unwrap());
    assert_eq!(
        level_one.map(|policy| policy.probability_ppm),
        [985_750, 10_000, 3_000, 1_000, 250]
    );

    let composed = compose_multicatch_and_school_bait(4, true).unwrap();
    assert_eq!(composed.final_fish_count, 5);
    assert_eq!(
        composed.additional_fish_identity,
        AdditionalFishInstanceIdentityPolicy::IndependentFullRoll
    );
}

#[test]
fn public_multi_treasure_curve_and_repeat_identity_are_exact() {
    let outcomes = [
        MultiTreasureCount::Single,
        MultiTreasureCount::Double,
        MultiTreasureCount::Triple,
    ];

    for level in 1..=MULTI_TREASURE_MAX_LEVEL {
        let policies = outcomes.map(|outcome| multi_treasure_count_policy(level, outcome).unwrap());
        assert_eq!(
            policies
                .iter()
                .map(|policy| policy.probability_ppm)
                .sum::<u32>(),
            MULTI_TREASURE_PROBABILITY_SCALE_PPM
        );
    }

    let level_one = outcomes.map(|outcome| multi_treasure_count_policy(1, outcome).unwrap());
    assert_eq!(
        level_one.map(|policy| policy.probability_ppm),
        [992_500, 6_000, 1_500]
    );
    assert_eq!(
        repeated_treasure_payload_resolution_policy(),
        RepeatedTreasurePayloadResolutionPolicy::IndependentRerollWithinSelectedResult
    );
}

#[test]
fn heterogeneous_fish_aexp_cap_is_owned_by_the_base_fish() {
    assert_eq!(MANUAL_FISHING_BASE_MULTI_CATCH_AEXP_CAP_MULTIPLIER, 3);
    assert_eq!(
        manual_fishing_base_fish_cast_aexp(&[
            FishingRarity::Common,
            FishingRarity::Mythic,
            FishingRarity::Mythic,
        ]),
        Ok(9)
    );
}

#[test]
fn action_speed_sources_add_before_cap_and_cooldown_rounds_half_up_to_milliseconds() {
    let fifteen_percent = manual_mine_fish_cooldown_policy(&[ratio(1, 10), ratio(1, 20)]).unwrap();
    assert_eq!(fifteen_percent.speed_policy.uncapped_bonus, ratio(3, 20));
    assert_eq!(fifteen_percent.cooldown_millis, 8_696);

    let capped = manual_mine_fish_cooldown_policy(&[ratio(1, 4), ratio(1, 4)]).unwrap();
    assert!(capped.speed_policy.cap_applied);
    assert_eq!(capped.speed_policy.applied_bonus, ratio(3_333, 10_000));
    assert_eq!(capped.cooldown_millis, 7_500);
}
