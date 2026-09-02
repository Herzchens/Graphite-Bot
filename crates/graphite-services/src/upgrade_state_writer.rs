use sqlx::{Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    CanonicalEquipmentAppraisalError, OrdinaryEquipmentEnhancedResolverError,
    compose_canonical_equipment_appraisal, lock_owned_ordinary_equipment_enhanced_appraisal,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolvedUpgradeLevelTransition {
    AdvanceOne,
    DowngradeOne,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AppliedUpgradeLevelTransition {
    pub transition: ResolvedUpgradeLevelTransition,
    pub previous_upgrade_level: u64,
    pub new_upgrade_level: u64,
    pub previous_recraft_appraisal: i64,
    pub new_recraft_appraisal: i64,
    pub previous_enhanced_canonical_appraisal: i64,
    pub new_enhanced_canonical_appraisal: i64,
}

#[derive(Debug, Error)]
pub enum UpgradeLevelStateWriterError {
    #[error(transparent)]
    Enhanced(#[from] OrdinaryEquipmentEnhancedResolverError),
    #[error("starter equipment cannot receive +N structural transitions")]
    StarterEquipment,
    #[error("the ItemInstance is not upgradeable and cannot receive +N structural transitions")]
    ItemNotUpgradeable,
    #[error(
        "resolved +N transition expected current level +{expected_level}, but authoritative level is +{actual_level}"
    )]
    UpgradeLevelChanged {
        expected_level: u64,
        actual_level: u64,
    },
    #[error("a resolved +N downgrade cannot be applied at +0")]
    CannotDowngradeZero,
    #[error("resolved +N advancement exceeds the supported persisted integer range")]
    UpgradeLevelOverflow,
    #[error(transparent)]
    CanonicalAppraisal(#[from] CanonicalEquipmentAppraisalError),
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error(
        "the locked +N structural state changed unexpectedly before the resolved transition write"
    )]
    LockedStateMismatch,
}

impl From<sqlx::Error> for UpgradeLevelStateWriterError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

/// Writes exactly one already-resolved +N structural transition to owned ordinary equipment.
///
/// This low-level primitive is deliberately not an upgrade-attempt owner. Before calling it, a future
/// owning lifecycle must acquire its operation/player locks, prove all required assets/costs, resolve
/// the authoritative success/failure/downgrade outcome and modifier ordering, and pass the exact +N
/// level against which that outcome was resolved. In particular, this function does not draw RNG,
/// evaluate the +1..+20 probability table, extrapolate a >+20 probability, consume Protection Orb,
/// mutate Stabilize, charge Money/AEXP/materials, finalize an operation/outbox event, or expose a
/// command.
///
/// The authoritative enhanced-equipment resolver locks the ItemInstance, structural state, and
/// embedded-enchant rows for the caller transaction. Starter and non-upgradeable ItemInstances fail
/// closed. The expected level is compared with the locked snapshot before an exact checked +1 or -1
/// transition is calculated. +20 is intentionally not a structural hard cap: the probability layer,
/// not this persistence primitive, owns whether any future attempt is authoritatively resolvable.
///
/// The new Recraft and Enhanced Canonical Appraisals are recomputed with the existing canonical
/// composition kernel before mutation. Returning both before/after appraisal values lets a future
/// SoulBind-aware owner settle any required positive-appraisal top-up in the same surrounding
/// transaction. The writer itself does not inspect or mutate SoulBind state.
///
/// Only `upgrade_level` is compare-and-set. Creation Roll, enchant slot capacities, embedded enchants,
/// durability and all other ItemInstance state are preserved. Rolling back the caller transaction
/// rolls back the +N transition.
pub async fn write_resolved_upgrade_level_transition_to_owned_ordinary_equipment(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
    item_id: Uuid,
    expected_current_level: u64,
    transition: ResolvedUpgradeLevelTransition,
) -> Result<AppliedUpgradeLevelTransition, UpgradeLevelStateWriterError> {
    let before = lock_owned_ordinary_equipment_enhanced_appraisal(tx, player_id, item_id).await?;

    if before.recraft.is_starter {
        return Err(UpgradeLevelStateWriterError::StarterEquipment);
    }
    if !before.recraft.is_upgradeable {
        return Err(UpgradeLevelStateWriterError::ItemNotUpgradeable);
    }

    let actual_level = before.recraft.upgrade_level;
    if actual_level != expected_current_level {
        return Err(UpgradeLevelStateWriterError::UpgradeLevelChanged {
            expected_level: expected_current_level,
            actual_level,
        });
    }

    let new_level = match transition {
        ResolvedUpgradeLevelTransition::AdvanceOne => actual_level
            .checked_add(1)
            .ok_or(UpgradeLevelStateWriterError::UpgradeLevelOverflow)?,
        ResolvedUpgradeLevelTransition::DowngradeOne => actual_level
            .checked_sub(1)
            .ok_or(UpgradeLevelStateWriterError::CannotDowngradeZero)?,
    };

    let after = compose_canonical_equipment_appraisal(
        before.recraft.base_appraisal,
        before.recraft.creation_roll,
        new_level,
        before.embedded_enchant_value,
    )?;

    let expected_level = actual_level.to_string();
    let persisted_new_level = new_level.to_string();
    let result = sqlx::query(
        r#"
        UPDATE item_instance_equipment_structural_state
           SET upgrade_level = $3::NUMERIC
         WHERE item_instance_id = $1
           AND upgrade_level = $2::NUMERIC
        "#,
    )
    .bind(item_id)
    .bind(expected_level)
    .bind(persisted_new_level)
    .execute(&mut **tx)
    .await?;

    if result.rows_affected() != 1 {
        return Err(UpgradeLevelStateWriterError::LockedStateMismatch);
    }

    Ok(AppliedUpgradeLevelTransition {
        transition,
        previous_upgrade_level: actual_level,
        new_upgrade_level: new_level,
        previous_recraft_appraisal: before.recraft.recraft_appraisal,
        new_recraft_appraisal: after.recraft_appraisal,
        previous_enhanced_canonical_appraisal: before.enhanced_canonical_appraisal,
        new_enhanced_canonical_appraisal: after.enhanced_canonical_appraisal,
    })
}
