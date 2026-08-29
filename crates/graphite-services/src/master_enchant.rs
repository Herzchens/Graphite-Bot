use serde::Serialize;
use thiserror::Error;

pub const MASTER_I_PURCHASE_AEXP: i64 = 250_000;
pub const MASTER_II_UPGRADE_AEXP: i64 = 500_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MasterEnchantTier {
    MasterI,
    MasterII,
}

impl MasterEnchantTier {
    #[must_use]
    pub const fn full_repair_charges(self) -> u8 {
        match self {
            Self::MasterI => 1,
            Self::MasterII => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MasterAcquisitionSource {
    ExpShopOnly,
    UpgradeOnlyFromMasterI,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct MasterIPurchasePolicy {
    pub tier: MasterEnchantTier,
    pub activity_exp_cost: i64,
    pub acquisition_source: MasterAcquisitionSource,
    pub full_repair_charges: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct MasterIIUpgradePreview {
    pub from: MasterEnchantTier,
    pub to: MasterEnchantTier,
    pub additional_activity_exp_cost: i64,
    pub acquisition_source: MasterAcquisitionSource,
    pub charges_before: u8,
    pub charges_after: u8,
    pub additional_full_repair_charges: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct MasterFullRepairChargePlan {
    pub before: MasterEnchantTier,
    pub after: Option<MasterEnchantTier>,
    pub charges_before: u8,
    pub charges_after: u8,
    pub consumes_one_charge: bool,
    pub restores_full_durability: bool,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum MasterEnchantPolicyError {
    #[error("Master II upgrade requires an existing Master I")]
    MasterIiRequiresMasterI,
    #[error("NUKE_BURNOUT blocks Master restoration until the owning expedition is terminal")]
    NukeBurnoutBlocksRestoration,
}

/// Returns the frozen EXP-Shop acquisition policy for Master I.
///
/// This policy intentionally says nothing about whether the future EXP-Shop transaction mints a
/// consumable book or directly invokes an application flow. The active specification freezes only
/// that Master I is EXP-Shop-only, costs 250,000 Activity EXP, and carries one full-repair charge.
#[must_use]
pub const fn master_i_purchase_policy() -> MasterIPurchasePolicy {
    MasterIPurchasePolicy {
        tier: MasterEnchantTier::MasterI,
        activity_exp_cost: MASTER_I_PURCHASE_AEXP,
        acquisition_source: MasterAcquisitionSource::ExpShopOnly,
        full_repair_charges: 1,
    }
}

/// Previews the only canonical Master II progression step.
///
/// Master II is not independently purchasable. It upgrades an existing Master I for an additional
/// 500,000 Activity EXP and adds exactly one repair charge, producing the two-charge Master II state.
/// The caller still owns authoritative enchant state, Activity EXP reservation/consumption,
/// idempotency, and atomic mutation.
pub fn preview_master_ii_upgrade(
    current: MasterEnchantTier,
) -> Result<MasterIIUpgradePreview, MasterEnchantPolicyError> {
    if current != MasterEnchantTier::MasterI {
        return Err(MasterEnchantPolicyError::MasterIiRequiresMasterI);
    }

    Ok(MasterIIUpgradePreview {
        from: MasterEnchantTier::MasterI,
        to: MasterEnchantTier::MasterII,
        additional_activity_exp_cost: MASTER_II_UPGRADE_AEXP,
        acquisition_source: MasterAcquisitionSource::UpgradeOnlyFromMasterI,
        charges_before: MasterEnchantTier::MasterI.full_repair_charges(),
        charges_after: MasterEnchantTier::MasterII.full_repair_charges(),
        additional_full_repair_charges: 1,
    })
}

/// Projects one authorized Master full-repair charge use.
///
/// The frozen charge path is `Master II -> Master I -> removed`: each successful full repair consumes
/// exactly one charge. When the authoritative Pickaxe carries `NUKE_BURNOUT`, Master restoration is
/// forbidden until that expedition reaches `SETTLED`, `ESCAPED`, or `TRUE_DEATH`; callers must pass
/// `nuke_burnout = true` while that blocker is active.
///
/// This pure projection assumes the caller has already validated every other application condition,
/// including ownership, equipment applicability, actual missing durability, mutable ItemInstance
/// state, and any special-slot rules. It does not mutate durability or enchant state itself.
pub fn plan_master_full_repair_charge_use(
    current: MasterEnchantTier,
    nuke_burnout: bool,
) -> Result<MasterFullRepairChargePlan, MasterEnchantPolicyError> {
    if nuke_burnout {
        return Err(MasterEnchantPolicyError::NukeBurnoutBlocksRestoration);
    }

    let (after, charges_after) = match current {
        MasterEnchantTier::MasterII => (Some(MasterEnchantTier::MasterI), 1),
        MasterEnchantTier::MasterI => (None, 0),
    };

    Ok(MasterFullRepairChargePlan {
        before: current,
        after,
        charges_before: current.full_repair_charges(),
        charges_after,
        consumes_one_charge: true,
        restores_full_durability: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_i_purchase_is_exp_shop_only_and_has_one_charge() {
        assert_eq!(
            master_i_purchase_policy(),
            MasterIPurchasePolicy {
                tier: MasterEnchantTier::MasterI,
                activity_exp_cost: 250_000,
                acquisition_source: MasterAcquisitionSource::ExpShopOnly,
                full_repair_charges: 1,
            }
        );
    }

    #[test]
    fn master_ii_upgrade_requires_master_i_and_adds_second_charge() {
        assert_eq!(
            preview_master_ii_upgrade(MasterEnchantTier::MasterI).unwrap(),
            MasterIIUpgradePreview {
                from: MasterEnchantTier::MasterI,
                to: MasterEnchantTier::MasterII,
                additional_activity_exp_cost: 500_000,
                acquisition_source: MasterAcquisitionSource::UpgradeOnlyFromMasterI,
                charges_before: 1,
                charges_after: 2,
                additional_full_repair_charges: 1,
            }
        );
        assert_eq!(
            preview_master_ii_upgrade(MasterEnchantTier::MasterII),
            Err(MasterEnchantPolicyError::MasterIiRequiresMasterI)
        );
    }

    #[test]
    fn master_charge_path_is_two_to_one_to_removed() {
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
    fn nuke_burnout_blocks_both_master_charge_states_without_consuming_a_charge() {
        for tier in [MasterEnchantTier::MasterI, MasterEnchantTier::MasterII] {
            assert_eq!(
                plan_master_full_repair_charge_use(tier, true),
                Err(MasterEnchantPolicyError::NukeBurnoutBlocksRestoration)
            );
        }
    }
}
