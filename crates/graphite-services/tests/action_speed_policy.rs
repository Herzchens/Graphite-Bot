use graphite_services::{
    ActionSpeedBonus, BASE_REPEATABLE_MANUAL_REWARD_ACTION_COOLDOWN_MILLIS, FishingRodEnchant,
    FishingRodLevelXEffect, GOLD_ROD_ACTION_SPEED_RATING_PERCENT,
    MAX_SHARED_ACTION_SPEED_BONUS_DENOMINATOR, MAX_SHARED_ACTION_SPEED_BONUS_NUMERATOR,
    MIN_MINE_FISH_COOLDOWN_MILLIS, SharedActionSpeedBonusPolicy, cap_shared_action_speed_bonus,
    fishing_rod_level_x_policy, max_shared_action_speed_bonus,
};

fn ratio(numerator: u64, denominator: u64) -> ActionSpeedBonus {
    ActionSpeedBonus::new(numerator, denominator).unwrap()
}

#[test]
fn public_api_exposes_the_frozen_shared_timing_boundaries() {
    assert_eq!(BASE_REPEATABLE_MANUAL_REWARD_ACTION_COOLDOWN_MILLIS, 10_000);
    assert_eq!(MIN_MINE_FISH_COOLDOWN_MILLIS, 7_500);
    assert_eq!(MAX_SHARED_ACTION_SPEED_BONUS_NUMERATOR, 3_333);
    assert_eq!(MAX_SHARED_ACTION_SPEED_BONUS_DENOMINATOR, 10_000);
    assert_eq!(max_shared_action_speed_bonus(), ratio(3_333, 10_000));
}

#[test]
fn gold_rod_and_lure_x_expose_exact_inputs_without_composing_them() {
    let FishingRodLevelXEffect::Lure {
        action_speed_rating_percent,
        uses_shared_fishing_speed_bucket,
    } = fishing_rod_level_x_policy(FishingRodEnchant::Lure)
        .unwrap()
        .effect
    else {
        panic!("Lure must expose the Lure scalar effect");
    };

    assert!(uses_shared_fishing_speed_bucket);
    assert_eq!(
        ratio(u64::from(GOLD_ROD_ACTION_SPEED_RATING_PERCENT), 100),
        ratio(1, 10)
    );
    assert_eq!(
        ratio(u64::from(action_speed_rating_percent), 100),
        ratio(3, 20)
    );
}

#[test]
fn public_cap_preserves_sub_basis_point_precision_until_the_exact_cap() {
    let below = ratio(33_329, 100_000);
    assert_eq!(
        cap_shared_action_speed_bonus(below),
        SharedActionSpeedBonusPolicy {
            uncapped_bonus: below,
            applied_bonus: below,
            cap_applied: false,
        }
    );

    let above = ratio(1, 3);
    assert_eq!(
        cap_shared_action_speed_bonus(above),
        SharedActionSpeedBonusPolicy {
            uncapped_bonus: above,
            applied_bonus: max_shared_action_speed_bonus(),
            cap_applied: true,
        }
    );
}
