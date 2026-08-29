use serde::Serialize;
use thiserror::Error;

pub const ORDINARY_SMELT_SECONDS_PER_UNIT: i64 = 20;
const HEAT_HALF_UNITS_PER_SMELT: i64 = 2;
const COAL_HEAT_HALF_UNITS: i64 = 16;
const WOOD_LOG_HEAT_HALF_UNITS: i64 = 3;
const SMELT_UNITS_PER_AEXP: i64 = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SmeltFuelKind {
    Coal,
    WoodLog,
}

impl SmeltFuelKind {
    const fn heat_half_units(self) -> i64 {
        match self {
            Self::Coal => COAL_HEAT_HALF_UNITS,
            Self::WoodLog => WOOD_LOG_HEAT_HALF_UNITS,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SmeltPreview {
    pub requested_units: i64,
    pub processable_units: i64,
    pub unprocessed_units: i64,
    pub fuel_kind: SmeltFuelKind,
    pub available_fuel_items: i64,
    pub fuel_items_needed_for_full_request: i64,
    pub fuel_items_to_reserve: i64,
    pub fuel_item_shortfall: i64,
    pub base_duration_seconds: i64,
    pub projected_activity_xp: i64,
    pub residual_heat_half_units_after_completion: i64,
    pub partial_for_fuel: bool,
    pub confirmable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SmeltStopSettlement {
    pub completed_units: i64,
    pub remaining_accepted_units: i64,
    pub opened_fuel_items: i64,
    pub returnable_whole_fuel_items: i64,
    pub lost_residual_heat_half_units: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct SmeltingAexpProgress {
    pub newly_awarded_activity_xp: i64,
    pub total_awarded_activity_xp: i64,
    pub remainder_completed_units: i64,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum SmeltingMathError {
    #[error("requested smelt units must be positive")]
    InvalidRequestedUnits,
    #[error("available input/fuel quantities cannot be negative")]
    NegativeAvailability,
    #[error("requested {requested} input units but only {available} are available")]
    InsufficientInput { available: i64, requested: i64 },
    #[error("completed smelt units cannot be negative or exceed the accepted job quantity")]
    InvalidCompletedUnits,
    #[error("completed smelt progress cannot move backwards")]
    CompletionWentBackwards,
    #[error("smelting arithmetic exceeded the supported BIGINT range")]
    ArithmeticOverflow,
}

/// Read-only ordinary-Smelting preview for one explicitly selected fuel kind.
/// Confirmation must revalidate authoritative inventory under locks before mutation.
pub fn preview_single_fuel_smelting(
    requested_units: i64,
    available_input_units: i64,
    fuel_kind: SmeltFuelKind,
    available_fuel_items: i64,
) -> Result<SmeltPreview, SmeltingMathError> {
    if requested_units <= 0 {
        return Err(SmeltingMathError::InvalidRequestedUnits);
    }
    if available_input_units < 0 || available_fuel_items < 0 {
        return Err(SmeltingMathError::NegativeAvailability);
    }
    if available_input_units < requested_units {
        return Err(SmeltingMathError::InsufficientInput {
            available: available_input_units,
            requested: requested_units,
        });
    }

    let fuel_heat = fuel_kind.heat_half_units();
    let required_heat = checked_mul(requested_units, HEAT_HALF_UNITS_PER_SMELT)?;
    let needed = ceil_div_positive(required_heat, fuel_heat);
    let reserve = available_fuel_items.min(needed);
    let reserved_heat = checked_mul(reserve, fuel_heat)?;
    let processable = (reserved_heat / HEAT_HALF_UNITS_PER_SMELT).min(requested_units);
    let completion = fuel_state_after_units(reserve, fuel_kind, processable)?;

    Ok(SmeltPreview {
        requested_units,
        processable_units: processable,
        unprocessed_units: checked_sub(requested_units, processable)?,
        fuel_kind,
        available_fuel_items,
        fuel_items_needed_for_full_request: needed,
        fuel_items_to_reserve: reserve,
        fuel_item_shortfall: checked_sub(needed, reserve)?,
        base_duration_seconds: checked_mul(processable, ORDINARY_SMELT_SECONDS_PER_UNIT)?,
        projected_activity_xp: processable / SMELT_UNITS_PER_AEXP,
        residual_heat_half_units_after_completion: completion.lost_residual_heat_half_units,
        partial_for_fuel: processable < requested_units,
        confirmable: processable > 0,
    })
}

/// Returns never-opened whole fuel and residual heat lost when a job stops.
pub fn stop_smelting(
    preview: &SmeltPreview,
    completed_units: i64,
) -> Result<SmeltStopSettlement, SmeltingMathError> {
    if completed_units < 0 || completed_units > preview.processable_units {
        return Err(SmeltingMathError::InvalidCompletedUnits);
    }
    let fuel = fuel_state_after_units(
        preview.fuel_items_to_reserve,
        preview.fuel_kind,
        completed_units,
    )?;
    Ok(SmeltStopSettlement {
        completed_units,
        remaining_accepted_units: checked_sub(preview.processable_units, completed_units)?,
        opened_fuel_items: fuel.opened_fuel_items,
        returnable_whole_fuel_items: fuel.returnable_whole_fuel_items,
        lost_residual_heat_half_units: fuel.lost_residual_heat_half_units,
    })
}

/// Computes job-local Activity EXP progression. Wholly bypassed Smelt-enchant output
/// receives zero processing AEXP and must not share a completion counter with actual work.
pub fn smelting_aexp_progress(
    previous_completed_units: i64,
    total_completed_units: i64,
    bypassed: bool,
) -> Result<SmeltingAexpProgress, SmeltingMathError> {
    if previous_completed_units < 0 || total_completed_units < 0 {
        return Err(SmeltingMathError::InvalidCompletedUnits);
    }
    if total_completed_units < previous_completed_units {
        return Err(SmeltingMathError::CompletionWentBackwards);
    }
    if bypassed {
        return Ok(SmeltingAexpProgress {
            newly_awarded_activity_xp: 0,
            total_awarded_activity_xp: 0,
            remainder_completed_units: total_completed_units % SMELT_UNITS_PER_AEXP,
        });
    }

    let previous_total = previous_completed_units / SMELT_UNITS_PER_AEXP;
    let total_awarded = total_completed_units / SMELT_UNITS_PER_AEXP;
    Ok(SmeltingAexpProgress {
        newly_awarded_activity_xp: checked_sub(total_awarded, previous_total)?,
        total_awarded_activity_xp: total_awarded,
        remainder_completed_units: total_completed_units % SMELT_UNITS_PER_AEXP,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FuelState {
    opened_fuel_items: i64,
    returnable_whole_fuel_items: i64,
    lost_residual_heat_half_units: i64,
}

fn fuel_state_after_units(
    reserved_fuel_items: i64,
    fuel_kind: SmeltFuelKind,
    completed_units: i64,
) -> Result<FuelState, SmeltingMathError> {
    if reserved_fuel_items < 0 || completed_units < 0 {
        return Err(SmeltingMathError::InvalidCompletedUnits);
    }
    if completed_units == 0 {
        return Ok(FuelState {
            opened_fuel_items: 0,
            returnable_whole_fuel_items: reserved_fuel_items,
            lost_residual_heat_half_units: 0,
        });
    }
    let consumed_heat = checked_mul(completed_units, HEAT_HALF_UNITS_PER_SMELT)?;
    let fuel_heat = fuel_kind.heat_half_units();
    let opened = ceil_div_positive(consumed_heat, fuel_heat);
    if opened > reserved_fuel_items {
        return Err(SmeltingMathError::InvalidCompletedUnits);
    }
    Ok(FuelState {
        opened_fuel_items: opened,
        returnable_whole_fuel_items: checked_sub(reserved_fuel_items, opened)?,
        lost_residual_heat_half_units: checked_sub(checked_mul(opened, fuel_heat)?, consumed_heat)?,
    })
}

fn ceil_div_positive(numerator: i64, denominator: i64) -> i64 {
    debug_assert!(numerator > 0 && denominator > 0);
    numerator / denominator + i64::from(numerator % denominator != 0)
}

fn checked_mul(left: i64, right: i64) -> Result<i64, SmeltingMathError> {
    left.checked_mul(right)
        .ok_or(SmeltingMathError::ArithmeticOverflow)
}

fn checked_sub(left: i64, right: i64) -> Result<i64, SmeltingMathError> {
    left.checked_sub(right)
        .ok_or(SmeltingMathError::ArithmeticOverflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_fuel_time_and_partial_examples_are_exact() {
        let full = preview_single_fuel_smelting(10, 10, SmeltFuelKind::Coal, 2).unwrap();
        assert_eq!(
            (full.processable_units, full.fuel_items_to_reserve),
            (10, 2)
        );
        assert_eq!(
            (full.base_duration_seconds, full.projected_activity_xp),
            (200, 1)
        );
        assert_eq!(full.residual_heat_half_units_after_completion, 12);
        assert!(!full.partial_for_fuel);

        let partial = preview_single_fuel_smelting(10, 10, SmeltFuelKind::Coal, 1).unwrap();
        assert_eq!(
            (partial.processable_units, partial.unprocessed_units),
            (8, 2)
        );
        assert_eq!(
            (partial.base_duration_seconds, partial.projected_activity_xp),
            (160, 1)
        );
        assert!(partial.partial_for_fuel && partial.confirmable);

        let stack = preview_single_fuel_smelting(64, 64, SmeltFuelKind::Coal, 8).unwrap();
        assert_eq!(
            (stack.base_duration_seconds, stack.projected_activity_xp),
            (1_280, 8)
        );

        for (requested, fuel, residual, aexp) in [(7, 1, 2, 0), (8, 1, 0, 1), (9, 2, 14, 1)] {
            let p =
                preview_single_fuel_smelting(requested, requested, SmeltFuelKind::Coal, 2).unwrap();
            assert_eq!(
                (
                    p.fuel_items_needed_for_full_request,
                    p.residual_heat_half_units_after_completion,
                    p.projected_activity_xp
                ),
                (fuel, residual, aexp)
            );
        }
        for (requested, fuel, residual) in [(1, 1, 1), (2, 2, 2), (3, 2, 0)] {
            let p = preview_single_fuel_smelting(requested, requested, SmeltFuelKind::WoodLog, 2)
                .unwrap();
            assert_eq!(
                (
                    p.fuel_items_needed_for_full_request,
                    p.residual_heat_half_units_after_completion
                ),
                (fuel, residual)
            );
        }
    }

    #[test]
    fn stop_and_aexp_boundaries_match_job_semantics() {
        let preview = preview_single_fuel_smelting(10, 10, SmeltFuelKind::Coal, 2).unwrap();
        for (done, opened, returned, lost, remaining) in [
            (0, 0, 2, 0, 10),
            (4, 1, 1, 8, 6),
            (9, 2, 0, 14, 1),
            (10, 2, 0, 12, 0),
        ] {
            let s = stop_smelting(&preview, done).unwrap();
            assert_eq!(
                (
                    s.opened_fuel_items,
                    s.returnable_whole_fuel_items,
                    s.lost_residual_heat_half_units,
                    s.remaining_accepted_units
                ),
                (opened, returned, lost, remaining)
            );
        }
        assert_eq!(
            smelting_aexp_progress(7, 8, false)
                .unwrap()
                .newly_awarded_activity_xp,
            1
        );
        assert_eq!(
            smelting_aexp_progress(8, 17, false)
                .unwrap()
                .newly_awarded_activity_xp,
            1
        );
        for (previous, total) in [(0, 7), (7, 8), (8, 9), (9, 64)] {
            let p = smelting_aexp_progress(previous, total, true).unwrap();
            assert_eq!(
                (p.newly_awarded_activity_xp, p.total_awarded_activity_xp),
                (0, 0)
            );
            assert_eq!(p.remainder_completed_units, total % SMELT_UNITS_PER_AEXP);
        }
    }

    #[test]
    fn preview_and_stop_invariants_hold_across_small_batches() {
        for fuel_kind in [SmeltFuelKind::Coal, SmeltFuelKind::WoodLog] {
            for requested in 1..=64 {
                let full = preview_single_fuel_smelting(requested, requested, fuel_kind, i64::MAX)
                    .unwrap();
                let needed = full.fuel_items_needed_for_full_request;
                for available_fuel in 0..=needed + 1 {
                    let p = preview_single_fuel_smelting(
                        requested,
                        requested,
                        fuel_kind,
                        available_fuel,
                    )
                    .unwrap();
                    assert!(p.processable_units <= requested);
                    assert_eq!(p.unprocessed_units, requested - p.processable_units);
                    assert!(
                        p.fuel_items_to_reserve <= available_fuel
                            && p.fuel_items_to_reserve <= needed
                    );
                    assert_eq!(p.fuel_item_shortfall, needed - p.fuel_items_to_reserve);
                    assert_eq!(
                        p.base_duration_seconds,
                        p.processable_units * ORDINARY_SMELT_SECONDS_PER_UNIT
                    );
                    assert_eq!(
                        p.projected_activity_xp,
                        p.processable_units / SMELT_UNITS_PER_AEXP
                    );
                    assert_eq!(p.partial_for_fuel, p.processable_units < requested);
                    assert_eq!(p.confirmable, p.processable_units > 0);

                    let before = stop_smelting(&p, 0).unwrap();
                    assert_eq!(
                        (
                            before.opened_fuel_items,
                            before.returnable_whole_fuel_items,
                            before.lost_residual_heat_half_units
                        ),
                        (0, p.fuel_items_to_reserve, 0)
                    );
                    let done = stop_smelting(&p, p.processable_units).unwrap();
                    assert_eq!(done.remaining_accepted_units, 0);
                    if p.processable_units == 0 {
                        assert_eq!(done.returnable_whole_fuel_items, p.fuel_items_to_reserve);
                    } else {
                        assert_eq!(done.returnable_whole_fuel_items, 0);
                        assert!(done.lost_residual_heat_half_units < fuel_kind.heat_half_units());
                    }
                }
            }
        }
    }

    #[test]
    fn invalid_boundaries_and_overflow_are_rejected() {
        for requested in [0, -1] {
            assert_eq!(
                preview_single_fuel_smelting(requested, 0, SmeltFuelKind::Coal, 0),
                Err(SmeltingMathError::InvalidRequestedUnits)
            );
        }
        assert_eq!(
            preview_single_fuel_smelting(1, -1, SmeltFuelKind::Coal, 1),
            Err(SmeltingMathError::NegativeAvailability)
        );
        assert_eq!(
            preview_single_fuel_smelting(1, 1, SmeltFuelKind::Coal, -1),
            Err(SmeltingMathError::NegativeAvailability)
        );
        assert_eq!(
            preview_single_fuel_smelting(2, 1, SmeltFuelKind::Coal, 1),
            Err(SmeltingMathError::InsufficientInput {
                available: 1,
                requested: 2
            })
        );
        assert_eq!(
            preview_single_fuel_smelting(i64::MAX, i64::MAX, SmeltFuelKind::Coal, i64::MAX),
            Err(SmeltingMathError::ArithmeticOverflow)
        );
        let duration_overflow = i64::MAX / ORDINARY_SMELT_SECONDS_PER_UNIT + 1;
        assert_eq!(
            preview_single_fuel_smelting(
                duration_overflow,
                duration_overflow,
                SmeltFuelKind::Coal,
                i64::MAX
            ),
            Err(SmeltingMathError::ArithmeticOverflow)
        );

        let p = preview_single_fuel_smelting(10, 10, SmeltFuelKind::Coal, 2).unwrap();
        assert_eq!(
            stop_smelting(&p, -1),
            Err(SmeltingMathError::InvalidCompletedUnits)
        );
        assert_eq!(
            stop_smelting(&p, 11),
            Err(SmeltingMathError::InvalidCompletedUnits)
        );
        assert_eq!(
            smelting_aexp_progress(-1, 0, false),
            Err(SmeltingMathError::InvalidCompletedUnits)
        );
        assert_eq!(
            smelting_aexp_progress(0, -1, false),
            Err(SmeltingMathError::InvalidCompletedUnits)
        );
        assert_eq!(
            smelting_aexp_progress(9, 8, false),
            Err(SmeltingMathError::CompletionWentBackwards)
        );
    }
}
