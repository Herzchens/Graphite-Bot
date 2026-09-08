use graphite_services::{ALL_MINING_DEPTHS, MiningDepth, mining_operating_cost_policy};

#[test]
fn public_operating_cost_table_matches_all_sixteen_frozen_depth_rows() {
    let actual = ALL_MINING_DEPTHS.map(|depth| {
        mining_operating_cost_policy(depth)
            .money_per_expedition
            .get()
    });
    assert_eq!(
        actual,
        [
            0, 5, 8, 10, 15, 20, 30, 40, 55, 70, 100, 125, 150, 200, 275, 400,
        ]
    );
}

#[test]
fn public_operating_cost_lifecycle_never_invents_a_discount_or_post_commit_refund() {
    for depth in ALL_MINING_DEPTHS {
        let policy = mining_operating_cost_policy(depth);
        assert_eq!(policy.depth, depth);
        assert_eq!(policy.pre_validation_failure_cost.get(), 0);
        assert!(policy.reserve_at_confirm);
        assert!(policy.charge_when_expedition_commits);
        assert!(!policy.refund_committed_cost_after_death_or_escape);
        assert!(!policy.money_reward_buffs_apply);
        assert!(!policy.shop_discounts_apply);
        assert!(!policy.bank_perks_apply);
    }
}

#[test]
fn surface_is_the_only_depth_without_a_positive_money_reservation() {
    assert!(
        !mining_operating_cost_policy(MiningDepth::Surface).requires_positive_money_reservation()
    );
    for depth in ALL_MINING_DEPTHS.into_iter().skip(1) {
        assert!(mining_operating_cost_policy(depth).requires_positive_money_reservation());
    }
}
