use graphite_services::{
    SOULGRIND_BASE_PROC_BPS_PER_LEVEL, SOULGRIND_BASE_PROC_CAP_BPS, SOULGRIND_MAX_LEVEL,
    SOULGRIND_MAX_SUCCESSES_PER_ITEM_PER_EXPEDITION, SoulGrindPolicyError,
    preview_soulgrind_for_qualifying_soul,
};

#[test]
fn public_api_preserves_exact_level_and_missing_durability_scaling() {
    assert_eq!(SOULGRIND_MAX_LEVEL, 10);
    assert_eq!(SOULGRIND_BASE_PROC_BPS_PER_LEVEL, 5);
    assert_eq!(SOULGRIND_BASE_PROC_CAP_BPS, 50);

    let full_missing = preview_soulgrind_for_qualifying_soul(10, 0, 2_000, false).unwrap();
    assert_eq!(full_missing.probability.numerator(), 1);
    assert_eq!(full_missing.probability.denominator(), 200);

    let quarter_missing =
        preview_soulgrind_for_qualifying_soul(10, 1_500, 2_000, false).unwrap();
    assert_eq!(quarter_missing.probability.numerator(), 1);
    assert_eq!(quarter_missing.probability.denominator(), 800);

    let level_one_full_missing =
        preview_soulgrind_for_qualifying_soul(1, 0, 2_000, false).unwrap();
    assert_eq!(level_one_full_missing.probability.numerator(), 1);
    assert_eq!(level_one_full_missing.probability.denominator(), 2_000);
}

#[test]
fn public_api_preserves_exact_half_max_restoration_without_rounding() {
    let even = preview_soulgrind_for_qualifying_soul(10, 0, 2_000, false).unwrap();
    assert_eq!(even.restoration_quantum.numerator(), 1_000);
    assert_eq!(even.restoration_quantum.denominator(), 1);
    assert!(even.restoration_quantum.is_integral());

    let odd = preview_soulgrind_for_qualifying_soul(10, 0, 2_001, false).unwrap();
    assert_eq!(odd.restoration_quantum.numerator(), 2_001);
    assert_eq!(odd.restoration_quantum.denominator(), 2);
    assert!(!odd.restoration_quantum.is_integral());
}

#[test]
fn public_api_preserves_per_item_one_success_per_expedition_limit() {
    assert_eq!(SOULGRIND_MAX_SUCCESSES_PER_ITEM_PER_EXPEDITION, 1);
    let preview = preview_soulgrind_for_qualifying_soul(6, 500, 2_000, false).unwrap();
    assert!(preview.per_item);
    assert_eq!(preview.max_successful_procs_per_item_per_expedition, 1);
    assert_eq!(
        preview_soulgrind_for_qualifying_soul(6, 500, 2_000, true),
        Err(SoulGrindPolicyError::AlreadySucceededThisExpedition)
    );
}

#[test]
fn public_api_fails_closed_on_invalid_durability_and_level_state() {
    assert_eq!(
        preview_soulgrind_for_qualifying_soul(0, 0, 100, false),
        Err(SoulGrindPolicyError::LevelOutOfRange(0))
    );
    assert_eq!(
        preview_soulgrind_for_qualifying_soul(11, 0, 100, false),
        Err(SoulGrindPolicyError::LevelOutOfRange(11))
    );
    assert_eq!(
        preview_soulgrind_for_qualifying_soul(1, 0, 0, false),
        Err(SoulGrindPolicyError::NonPositiveMaxDurability)
    );
    assert_eq!(
        preview_soulgrind_for_qualifying_soul(1, -1, 100, false),
        Err(SoulGrindPolicyError::NegativeCurrentDurability)
    );
    assert_eq!(
        preview_soulgrind_for_qualifying_soul(1, 101, 100, false),
        Err(SoulGrindPolicyError::CurrentDurabilityExceedsMax)
    );
}
