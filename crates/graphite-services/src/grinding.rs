use serde::Serialize;
use thiserror::Error;

pub const GRINDING_MAX_LEVEL: u8 = 10;
pub const GRINDING_REDUCTION_BPS_PER_LEVEL: u16 = 300;
pub const GRINDING_MAX_REDUCTION_BPS: u16 = 3_000;
pub const REPAIR_TIME_REDUCTION_BUCKET_CAP_BPS: u16 = 3_500;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct GrindingRepairModifier {
    pub level: u8,
    pub material_reduction_basis_points: u16,
    pub repair_time_reduction_basis_points: u16,
    pub repair_time_bucket_cap_basis_points: u16,
    pub money_fee_unchanged: bool,
    pub applies_after_tier_repair_recipe: bool,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum GrindingPolicyError {
    #[error("Grinding level must be between I and X; got {0}")]
    LevelOutOfRange(u8),
}

/// Returns the frozen Grinding modifier without inventing integer settlement rounding.
///
/// Grinding reduces eligible Repair material requirement and Repair time by 3% per level, up to
/// 30% at Level X. It is applied only after the tier repair recipe has been calculated and never
/// reduces the Money fee. Repair-time reduction shares a 35% total bucket cap with the separately
/// authoritative Rebirth repair-time utility.
///
/// This policy deliberately returns exact basis-point modifiers instead of applying them to integer
/// material units or wall-clock duration. The active specification does not freeze how the resulting
/// fractional material/time values are rounded, so the future Repair settlement owner must supply
/// that rule before producing final integer consumption or duration.
pub fn grinding_repair_modifier(level: u8) -> Result<GrindingRepairModifier, GrindingPolicyError> {
    if !(1..=GRINDING_MAX_LEVEL).contains(&level) {
        return Err(GrindingPolicyError::LevelOutOfRange(level));
    }

    let reduction_basis_points = u16::from(level) * GRINDING_REDUCTION_BPS_PER_LEVEL;
    debug_assert!(reduction_basis_points <= GRINDING_MAX_REDUCTION_BPS);

    Ok(GrindingRepairModifier {
        level,
        material_reduction_basis_points: reduction_basis_points,
        repair_time_reduction_basis_points: reduction_basis_points,
        repair_time_bucket_cap_basis_points: REPAIR_TIME_REDUCTION_BUCKET_CAP_BPS,
        money_fee_unchanged: true,
        applies_after_tier_repair_recipe: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grinding_scales_three_percent_per_level_through_level_ten() {
        for level in 1..=GRINDING_MAX_LEVEL {
            let modifier = grinding_repair_modifier(level).unwrap();
            let expected = u16::from(level) * 300;
            assert_eq!(modifier.level, level);
            assert_eq!(modifier.material_reduction_basis_points, expected);
            assert_eq!(modifier.repair_time_reduction_basis_points, expected);
            assert_eq!(modifier.repair_time_bucket_cap_basis_points, 3_500);
            assert!(modifier.money_fee_unchanged);
            assert!(modifier.applies_after_tier_repair_recipe);
        }
    }

    #[test]
    fn grinding_level_ten_is_exactly_the_thirty_percent_cap() {
        let modifier = grinding_repair_modifier(10).unwrap();
        assert_eq!(modifier.material_reduction_basis_points, 3_000);
        assert_eq!(modifier.repair_time_reduction_basis_points, 3_000);
        assert_eq!(GRINDING_MAX_REDUCTION_BPS, 3_000);
    }

    #[test]
    fn grinding_rejects_noncanonical_levels_instead_of_silently_clamping() {
        assert_eq!(
            grinding_repair_modifier(0),
            Err(GrindingPolicyError::LevelOutOfRange(0))
        );
        assert_eq!(
            grinding_repair_modifier(11),
            Err(GrindingPolicyError::LevelOutOfRange(11))
        );
        assert_eq!(
            grinding_repair_modifier(u8::MAX),
            Err(GrindingPolicyError::LevelOutOfRange(u8::MAX))
        );
    }
}
