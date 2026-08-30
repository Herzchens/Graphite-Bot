use graphite_services::{
    EquipmentTier, FishingCapabilityError, GOLD_ROD_ACTION_SPEED_RATING_PERCENT,
    GOLD_ROD_RARE_OR_BETTER_RELATIVE_WEIGHT_PERCENT, GOLD_ROD_TREASURE_RELATIVE_WEIGHT_PERCENT,
    GoldFishingRodModifierStage, GoldFishingRodPolicyError, gold_fishing_rod_side_grade_policy,
};

#[test]
fn public_api_exposes_the_frozen_gold_rod_side_grade_values() {
    assert_eq!(GOLD_ROD_ACTION_SPEED_RATING_PERCENT, 10);
    assert_eq!(GOLD_ROD_RARE_OR_BETTER_RELATIVE_WEIGHT_PERCENT, 15);
    assert_eq!(GOLD_ROD_TREASURE_RELATIVE_WEIGHT_PERCENT, 15);

    let policy = gold_fishing_rod_side_grade_policy(EquipmentTier::Gold, true).unwrap();
    assert_eq!(policy.action_speed_rating_percent, 10);
    assert_eq!(
        (
            policy
                .rare_or_better_species_relative_weight_multiplier
                .numerator(),
            policy
                .rare_or_better_species_relative_weight_multiplier
                .denominator(),
        ),
        (23, 20)
    );
    assert_eq!(
        (
            policy.treasure_branch_relative_weight_multiplier.numerator(),
            policy.treasure_branch_relative_weight_multiplier.denominator(),
        ),
        (23, 20)
    );
    assert_eq!(
        policy.modifier_stage,
        GoldFishingRodModifierStage::BeforeSharedFishingCaps
    );
}

#[test]
fn public_api_does_not_convert_gold_speed_rating_into_a_duration_factor() {
    let policy = gold_fishing_rod_side_grade_policy(EquipmentTier::Gold, true).unwrap();

    assert_eq!(policy.action_speed_rating_percent, GOLD_ROD_ACTION_SPEED_RATING_PERCENT);
}

#[test]
fn public_api_rejects_non_gold_or_non_ordinary_definitions() {
    assert_eq!(
        gold_fishing_rod_side_grade_policy(EquipmentTier::Diamond, true),
        Err(GoldFishingRodPolicyError::NotGoldFishingRod)
    );
    assert_eq!(
        gold_fishing_rod_side_grade_policy(EquipmentTier::Gold, false),
        Err(GoldFishingRodPolicyError::InvalidOrdinaryRod(
            FishingCapabilityError::NotOrdinaryFishingRod
        ))
    );
}
