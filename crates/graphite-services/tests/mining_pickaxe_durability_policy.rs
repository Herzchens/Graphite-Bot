use graphite_services::{
    MiningPickaxeDurabilityPolicyError, MiningPickaxeOrdinaryDurabilityConsequence,
    NORMAL_PICKAXE_DURABILITY_PER_ORDINARY_EVENT, NUKE_MAX_BLAST_PHYSICAL_BLOCKS,
    preview_mining_pickaxe_nuke_destruction, preview_mining_pickaxe_ordinary_durability_event,
};

#[test]
fn ordinary_event_is_exactly_one_wear_unless_already_prevented() {
    assert_eq!(NORMAL_PICKAXE_DURABILITY_PER_ORDINARY_EVENT, 1);

    let applied = preview_mining_pickaxe_ordinary_durability_event(700, 700, true, false).unwrap();
    assert_eq!(applied.resulting_durability, 699);
    assert_eq!(
        applied.consequence,
        MiningPickaxeOrdinaryDurabilityConsequence::WearApplied
    );

    let prevented = preview_mining_pickaxe_ordinary_durability_event(700, 700, true, true).unwrap();
    assert_eq!(prevented.resulting_durability, 700);
    assert_eq!(
        prevented.consequence,
        MiningPickaxeOrdinaryDurabilityConsequence::WearPreventedByUnbreaking
    );
}

#[test]
fn ordinary_event_can_break_the_last_durability_point() {
    let preview = preview_mining_pickaxe_ordinary_durability_event(1, 11_000, true, false).unwrap();
    assert_eq!(preview.resulting_durability, 0);
}

#[test]
fn nuke_uses_exact_remaining_durability_cap_and_requires_burnout() {
    assert_eq!(NUKE_MAX_BLAST_PHYSICAL_BLOCKS, 100);

    for (remaining, expected_blocks) in [
        (0, 0),
        (1, 1),
        (99, 99),
        (100, 100),
        (101, 100),
        (600, 100),
        (11_000, 100),
    ] {
        let preview = preview_mining_pickaxe_nuke_destruction(remaining, 11_000, true).unwrap();
        assert_eq!(preview.remaining_durability, remaining);
        assert_eq!(preview.blast_physical_blocks, expected_blocks);
        assert_eq!(preview.resulting_durability, 0);
        assert!(preview.nuke_burnout_required);
    }
}

#[test]
fn starter_identity_must_not_be_smuggled_through_ordinary_policy() {
    assert_eq!(
        preview_mining_pickaxe_ordinary_durability_event(700, 700, false, false),
        Err(MiningPickaxeDurabilityPolicyError::NotOrdinaryPickaxe)
    );
    assert_eq!(
        preview_mining_pickaxe_nuke_destruction(700, 700, false),
        Err(MiningPickaxeDurabilityPolicyError::NotOrdinaryPickaxe)
    );
}

#[test]
fn malformed_ranges_fail_closed_but_zero_nuke_remaining_stays_formula_defined() {
    assert_eq!(
        preview_mining_pickaxe_ordinary_durability_event(1, 0, true, false),
        Err(MiningPickaxeDurabilityPolicyError::InvalidMaxDurability)
    );
    assert_eq!(
        preview_mining_pickaxe_ordinary_durability_event(0, 100, true, false),
        Err(MiningPickaxeDurabilityPolicyError::InvalidCurrentDurability)
    );
    assert_eq!(
        preview_mining_pickaxe_ordinary_durability_event(101, 100, true, false),
        Err(MiningPickaxeDurabilityPolicyError::InvalidCurrentDurability)
    );
    assert_eq!(
        preview_mining_pickaxe_nuke_destruction(1, 0, true),
        Err(MiningPickaxeDurabilityPolicyError::InvalidMaxDurability)
    );
    assert_eq!(
        preview_mining_pickaxe_nuke_destruction(101, 100, true),
        Err(MiningPickaxeDurabilityPolicyError::InvalidNukeRemainingDurability)
    );

    let zero = preview_mining_pickaxe_nuke_destruction(0, 100, true).unwrap();
    assert_eq!(zero.blast_physical_blocks, 0);
    assert_eq!(zero.resulting_durability, 0);
    assert!(zero.nuke_burnout_required);
}
