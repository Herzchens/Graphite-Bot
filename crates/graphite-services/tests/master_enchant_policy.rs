use graphite_services::{
    MASTER_I_PURCHASE_AEXP, MASTER_II_UPGRADE_AEXP, MasterAcquisitionSource,
    MasterEnchantPolicyError, MasterEnchantTier, master_i_purchase_policy,
    plan_master_full_repair_charge_use, preview_master_ii_upgrade,
};

#[test]
fn public_api_preserves_master_acquisition_and_upgrade_costs() {
    let purchase = master_i_purchase_policy();
    assert_eq!(MASTER_I_PURCHASE_AEXP, 250_000);
    assert_eq!(purchase.tier, MasterEnchantTier::MasterI);
    assert_eq!(purchase.activity_exp_cost, 250_000);
    assert_eq!(
        purchase.acquisition_source,
        MasterAcquisitionSource::ExpShopOnly
    );
    assert_eq!(purchase.full_repair_charges, 1);

    let upgrade = preview_master_ii_upgrade(MasterEnchantTier::MasterI).unwrap();
    assert_eq!(MASTER_II_UPGRADE_AEXP, 500_000);
    assert_eq!(upgrade.from, MasterEnchantTier::MasterI);
    assert_eq!(upgrade.to, MasterEnchantTier::MasterII);
    assert_eq!(upgrade.additional_activity_exp_cost, 500_000);
    assert_eq!(
        upgrade.acquisition_source,
        MasterAcquisitionSource::UpgradeOnlyFromMasterI
    );
    assert_eq!(upgrade.charges_before, 1);
    assert_eq!(upgrade.charges_after, 2);
    assert_eq!(upgrade.additional_full_repair_charges, 1);
}

#[test]
fn public_api_rejects_independent_master_ii_progression() {
    assert_eq!(
        preview_master_ii_upgrade(MasterEnchantTier::MasterII),
        Err(MasterEnchantPolicyError::MasterIiRequiresMasterI)
    );
}

#[test]
fn public_api_preserves_two_to_one_to_removed_charge_path() {
    let first = plan_master_full_repair_charge_use(MasterEnchantTier::MasterII, false).unwrap();
    assert_eq!(first.before, MasterEnchantTier::MasterII);
    assert_eq!(first.after, Some(MasterEnchantTier::MasterI));
    assert_eq!(first.charges_before, 2);
    assert_eq!(first.charges_after, 1);
    assert!(first.consumes_one_charge);
    assert!(first.restores_full_durability);

    let second = plan_master_full_repair_charge_use(MasterEnchantTier::MasterI, false).unwrap();
    assert_eq!(second.before, MasterEnchantTier::MasterI);
    assert_eq!(second.after, None);
    assert_eq!(second.charges_before, 1);
    assert_eq!(second.charges_after, 0);
    assert!(second.consumes_one_charge);
    assert!(second.restores_full_durability);
}

#[test]
fn public_api_blocks_master_restoration_during_nuke_burnout() {
    for tier in [MasterEnchantTier::MasterI, MasterEnchantTier::MasterII] {
        assert_eq!(
            plan_master_full_repair_charge_use(tier, true),
            Err(MasterEnchantPolicyError::NukeBurnoutBlocksRestoration)
        );
    }
}
