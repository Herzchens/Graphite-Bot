use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use thiserror::Error;

use crate::smelting_runtime::project_smelting_runtime_progress as project_unpaused_runtime_progress;
use crate::{
    SmeltingMathError, SmeltingRuntimeError, SmeltingRuntimeProgress, SmeltingRuntimeReceipt,
    preview_single_fuel_smelting, smelting_aexp_progress, stop_smelting,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SmeltingTerminalKind {
    Complete,
    Stop,
    Cancel,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SmeltingTerminalPlan {
    pub kind: SmeltingTerminalKind,
    pub observed_at: DateTime<Utc>,
    pub hard_paused_micros: i64,
    pub completed_processing_units: i64,
    pub returnable_reserved_input_units: i64,
    pub opened_fuel_items: i64,
    pub returnable_whole_fuel_items: i64,
    pub lost_residual_heat_half_units: i64,
    pub discarded_partial_unit_micros: i64,
    pub eligible_activity_xp: i64,
}

#[derive(Debug, Error)]
pub enum SmeltingTerminalError {
    #[error(transparent)]
    Runtime(#[from] SmeltingRuntimeError),
    #[error(transparent)]
    Math(#[from] SmeltingMathError),
    #[error(
        "hard-freeze pause duration must be non-negative and cannot exceed this job's wall-clock age"
    )]
    InvalidHardPause,
    #[error("Smelting completion cannot settle before all accepted processing units finish")]
    CompleteBeforeFinished,
    #[error("stored Smelting runtime fuel coverage no longer matches canonical policy")]
    RuntimeFuelInvariant,
    #[error("Smelting terminal clock arithmetic exceeded the supported timestamp range")]
    TimeOutOfRange,
}

/// Projects Smelting progress on an execution clock that excludes accumulated Hard Freeze time.
///
/// `hard_paused_micros` is the authoritative duration of Hard Freeze that overlaps this job's
/// execution lifetime up to `observed_at`. It is deliberately supplied by the owning freeze/job
/// state machine rather than inferred from the immutable runtime snapshot. Soft Freeze contributes
/// zero pause time. The returned `observed_at` remains the real wall-clock observation while
/// `elapsed_work_micros` is active processing time only.
pub fn project_smelting_runtime_progress(
    runtime: &SmeltingRuntimeReceipt,
    observed_at: DateTime<Utc>,
    hard_paused_micros: i64,
) -> Result<SmeltingRuntimeProgress, SmeltingTerminalError> {
    if hard_paused_micros < 0 {
        return Err(SmeltingTerminalError::InvalidHardPause);
    }

    let wall_age_micros = if observed_at <= runtime.started_at {
        0
    } else {
        observed_at
            .signed_duration_since(runtime.started_at)
            .num_microseconds()
            .ok_or(SmeltingTerminalError::TimeOutOfRange)?
    };
    if hard_paused_micros > wall_age_micros {
        return Err(SmeltingTerminalError::InvalidHardPause);
    }

    let effective_observed_at = observed_at
        .checked_sub_signed(Duration::microseconds(hard_paused_micros))
        .ok_or(SmeltingTerminalError::TimeOutOfRange)?;
    let mut progress = project_unpaused_runtime_progress(runtime, effective_observed_at)?;
    progress.observed_at = observed_at;
    Ok(progress)
}

/// Plans the terminal resource/progression consequences of one ordinary Smelting runtime.
///
/// This is intentionally a pure plan, not settlement. It does not mutate `service_jobs`, mint
/// recipe output, return Item Stacks, or grant Activity EXP. A later atomic settlement owner must
/// bind the job's snapshotted recipe/output identity, perform capacity-safe output delivery, apply
/// this plan's raw/fuel returns and AEXP exactly once, transition the job, and commit provenance in
/// one transaction.
pub fn plan_smelting_terminal(
    runtime: &SmeltingRuntimeReceipt,
    observed_at: DateTime<Utc>,
    hard_paused_micros: i64,
    kind: SmeltingTerminalKind,
) -> Result<SmeltingTerminalPlan, SmeltingTerminalError> {
    let progress = project_smelting_runtime_progress(runtime, observed_at, hard_paused_micros)?;
    if kind == SmeltingTerminalKind::Complete && !progress.finished {
        return Err(SmeltingTerminalError::CompleteBeforeFinished);
    }

    let completed_processing_units = progress.completed_units;
    let preview = preview_single_fuel_smelting(
        runtime.accepted_units,
        runtime.accepted_units,
        runtime.fuel_kind,
        runtime.reserved_fuel_items,
    )?;
    if preview.processable_units != runtime.accepted_units
        || preview.fuel_items_to_reserve != runtime.reserved_fuel_items
    {
        return Err(SmeltingTerminalError::RuntimeFuelInvariant);
    }

    let stop = stop_smelting(&preview, completed_processing_units)?;
    let aexp = smelting_aexp_progress(0, completed_processing_units, false)?;

    Ok(SmeltingTerminalPlan {
        kind,
        observed_at,
        hard_paused_micros,
        completed_processing_units,
        returnable_reserved_input_units: stop.remaining_accepted_units,
        opened_fuel_items: stop.opened_fuel_items,
        returnable_whole_fuel_items: stop.returnable_whole_fuel_items,
        lost_residual_heat_half_units: stop.lost_residual_heat_half_units,
        discarded_partial_unit_micros: if progress.finished {
            0
        } else {
            progress.current_unit_elapsed_micros
        },
        eligible_activity_xp: aexp.total_awarded_activity_xp,
    })
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use serde_json::json;
    use uuid::Uuid;

    use super::*;
    use crate::{ORDINARY_SMELT_MICROS_PER_UNIT, SmeltFuelKind};

    fn runtime() -> SmeltingRuntimeReceipt {
        let started_at = DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap();
        SmeltingRuntimeReceipt {
            job_id: Uuid::nil(),
            requested_units: 10,
            accepted_units: 8,
            fuel_kind: SmeltFuelKind::Coal,
            reserved_fuel_items: 1,
            effective_unit_micros: ORDINARY_SMELT_MICROS_PER_UNIT,
            modifier_snapshot: json!({}),
            started_at,
            completes_at: started_at + TimeDelta::seconds(160),
        }
    }

    #[test]
    fn hard_freeze_time_never_becomes_processing_progress() {
        let runtime = runtime();
        let observed = runtime.started_at + TimeDelta::seconds(160);
        let progress = project_smelting_runtime_progress(&runtime, observed, 40_000_000).unwrap();
        assert_eq!(progress.observed_at, observed);
        assert_eq!(progress.elapsed_work_micros, 120_000_000);
        assert_eq!(progress.completed_units, 6);
        assert_eq!(progress.remaining_units, 2);
        assert!(!progress.finished);

        assert!(matches!(
            plan_smelting_terminal(
                &runtime,
                observed,
                40_000_000,
                SmeltingTerminalKind::Complete,
            ),
            Err(SmeltingTerminalError::CompleteBeforeFinished)
        ));

        let resumed_finish = runtime.started_at + TimeDelta::seconds(200);
        let complete = plan_smelting_terminal(
            &runtime,
            resumed_finish,
            40_000_000,
            SmeltingTerminalKind::Complete,
        )
        .unwrap();
        assert_eq!(complete.completed_processing_units, 8);
        assert_eq!(complete.returnable_reserved_input_units, 0);
        assert_eq!(complete.returnable_whole_fuel_items, 0);
        assert_eq!(complete.lost_residual_heat_half_units, 0);
        assert_eq!(complete.discarded_partial_unit_micros, 0);
        assert_eq!(complete.eligible_activity_xp, 1);
    }

    #[test]
    fn stop_discards_partial_unit_but_preserves_completed_work_and_returns_raw() {
        let runtime = runtime();
        let observed = runtime.started_at + TimeDelta::seconds(51);
        let plan =
            plan_smelting_terminal(&runtime, observed, 0, SmeltingTerminalKind::Stop).unwrap();
        assert_eq!(plan.completed_processing_units, 2);
        assert_eq!(plan.returnable_reserved_input_units, 6);
        assert_eq!(plan.opened_fuel_items, 1);
        assert_eq!(plan.returnable_whole_fuel_items, 0);
        assert_eq!(plan.lost_residual_heat_half_units, 12);
        assert_eq!(plan.discarded_partial_unit_micros, 11_000_000);
        assert_eq!(plan.eligible_activity_xp, 0);
    }

    #[test]
    fn cancel_before_start_returns_every_reserved_asset_without_heat_loss() {
        let runtime = runtime();
        let observed = runtime.started_at - TimeDelta::seconds(1);
        let plan =
            plan_smelting_terminal(&runtime, observed, 0, SmeltingTerminalKind::Cancel).unwrap();
        assert_eq!(plan.completed_processing_units, 0);
        assert_eq!(plan.returnable_reserved_input_units, 8);
        assert_eq!(plan.opened_fuel_items, 0);
        assert_eq!(plan.returnable_whole_fuel_items, 1);
        assert_eq!(plan.lost_residual_heat_half_units, 0);
        assert_eq!(plan.discarded_partial_unit_micros, 0);
        assert_eq!(plan.eligible_activity_xp, 0);
    }

    #[test]
    fn invalid_hard_pause_is_rejected_instead_of_backdating_the_active_clock() {
        let runtime = runtime();
        let observed = runtime.started_at + TimeDelta::seconds(10);
        assert!(matches!(
            project_smelting_runtime_progress(&runtime, observed, -1),
            Err(SmeltingTerminalError::InvalidHardPause)
        ));
        assert!(matches!(
            project_smelting_runtime_progress(&runtime, observed, 10_000_001),
            Err(SmeltingTerminalError::InvalidHardPause)
        ));
    }
}
