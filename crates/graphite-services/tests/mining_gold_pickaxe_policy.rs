use graphite_services::{
    EquipmentTier, GOLD_PICKAXE_ACTION_SPEED_RATING_PERCENT,
    GOLD_PICKAXE_ELIGIBLE_TREASURE_RARE_RELATIVE_WEIGHT_DENOMINATOR,
    GOLD_PICKAXE_ELIGIBLE_TREASURE_RARE_RELATIVE_WEIGHT_INCREASE_PERCENT,
    GOLD_PICKAXE_ELIGIBLE_TREASURE_RARE_RELATIVE_WEIGHT_NUMERATOR,
    GoldMiningPickaxeModifierStage, GoldMiningPickaxePolicyError, MiningCapabilityClass,
    gold_mining_pickaxe_side_grade_policy,
};

#[test]
fn public_gold_pickaxe_policy_matches_frozen_side_grade_inputs() {
    assert_eq!(GOLD_PICKAXE_ACTION_SPEED_RATING_PERCENT, 10);
    assert_eq!(
        GOLD_PICKAXE_ELIGIBLE_TREASURE_RARE_RELATIVE_WEIGHT_INCREASE_PERCENT,
        20
    );
    assert_eq!(
        (
            GOLD_PICKAXE_ELIGIBLE_TREASURE_RARE_RELATIVE_WEIGHT_NUMERATOR,
            GOLD_PICKAXE_ELIGIBLE_TREASURE_RARE_RELATIVE_WEIGHT_DENOMINATOR,
        ),
        (6, 5)
    );

    let policy = gold_mining_pickaxe_side_grade_policy(EquipmentTier::Gold, true).unwrap();
    assert_eq!(policy.max_capability, MiningCapabilityClass::C3);
    assert_eq!(policy.action_speed_rating_percent, 10);
    assert_eq!(
        (
            policy.eligible_treasure_rare_relative_weight_numerator,
            policy.eligible_treasure_rare_relative_weight_denominator,
        ),
        (6, 5)
    );
    assert_eq!(
        policy.modifier_stage,
        GoldMiningPickaxeModifierStage::SourceLocalBeforeSharedMiningCaps
    );
}

#[test]
fn public_policy_preserves_the_ordinary_gold_trust_boundary() {
    assert_eq!(
        gold_mining_pickaxe_side_grade_policy(EquipmentTier::Gold, false),
        Err(GoldMiningPickaxePolicyError::NotOrdinaryPickaxe)
    );
    assert_eq!(
        gold_mining_pickaxe_side_grade_policy(EquipmentTier::Diamond, true),
        Err(GoldMiningPickaxePolicyError::NotGoldPickaxe)
    );
}
