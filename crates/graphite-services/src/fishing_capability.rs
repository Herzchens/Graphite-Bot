use serde::Serialize;
use thiserror::Error;

use crate::{EquipmentTier, OrdinaryFishingRodStats, OrdinaryToolStatsError, ordinary_fishing_rod_stats};

pub const CAPABILITY_FACTOR_BASIS_POINTS: u16 = 10_000;
pub const STRENGTHEN_MAX_LEVEL: u8 = 10;
pub const STRENGTHEN_LINE_STRENGTH_BONUS_BPS_PER_LEVEL: u16 = 400;
pub const STRENGTHEN_MAX_LINE_STRENGTH_BONUS_BPS: u16 = 4_000;
pub const MANUAL_AUTOMATION_STRENGTH_FACTOR_BPS: u16 = CAPABILITY_FACTOR_BASIS_POINTS;
pub const BASELINE_BAIT_STRENGTH_FACTOR_BPS: u16 = CAPABILITY_FACTOR_BASIS_POINTS;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FishRarity {
    Common,
    Uncommon,
    Rare,
    Epic,
    Legendary,
    Mythic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct RarityTensionMultiplier {
    pub rarity: FishRarity,
    pub multiplier_basis_points: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct StrengthenLineStrengthPolicy {
    pub level: Option<u8>,
    pub bonus_basis_points: u16,
    pub multiplier_basis_points: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FishingLineStrengthFoundation {
    pub rod: OrdinaryFishingRodStats,
    pub strengthen: StrengthenLineStrengthPolicy,
    pub baseline_bait_strength_factor_basis_points: u16,
    pub manual_automation_strength_factor_basis_points: u16,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum FishingCapabilityPolicyError {
    #[error("Strengthen level must be between I and X when present; got {0}")]
    StrengthenLevelOutOfRange(u8),
    #[error(transparent)]
    InvalidRodTier(#[from] OrdinaryToolStatsError),
}

#[must_use]
pub const fn rarity_tension_multiplier(rarity: FishRarity) -> RarityTensionMultiplier {
    let multiplier_basis_points = match rarity {
        FishRarity::Common => 10_000,
        FishRarity::Uncommon => 11_000,
        FishRarity::Rare => 12_500,
        FishRarity::Epic => 14_500,
        FishRarity::Legendary => 17_500,
        FishRarity::Mythic => 22_000,
    };

    RarityTensionMultiplier {
        rarity,
        multiplier_basis_points,
    }
}

/// Resolves the canonical Strengthen contribution to Fishing Rod line strength.
///
/// `None` means the Rod has no Strengthen enchant. A present enchant must be Level I-X; malformed
/// Level 0 or XI+ state fails closed. Each level contributes +4% relative line strength, reaching the
/// frozen +40% cap at X. The policy is dimensionless and therefore does not choose any FishWeight or
/// kg-tension fixed-point precision.
pub fn strengthen_line_strength_policy(
    level: Option<u8>,
) -> Result<StrengthenLineStrengthPolicy, FishingCapabilityPolicyError> {
    let Some(level) = level else {
        return Ok(StrengthenLineStrengthPolicy {
            level: None,
            bonus_basis_points: 0,
            multiplier_basis_points: CAPABILITY_FACTOR_BASIS_POINTS,
        });
    };

    if !(1..=STRENGTHEN_MAX_LEVEL).contains(&level) {
        return Err(FishingCapabilityPolicyError::StrengthenLevelOutOfRange(level));
    }

    let bonus_basis_points = u16::from(level) * STRENGTHEN_LINE_STRENGTH_BONUS_BPS_PER_LEVEL;
    debug_assert!(bonus_basis_points <= STRENGTHEN_MAX_LINE_STRENGTH_BONUS_BPS);

    Ok(StrengthenLineStrengthPolicy {
        level: Some(level),
        bonus_basis_points,
        multiplier_basis_points: CAPABILITY_FACTOR_BASIS_POINTS + bonus_basis_points,
    })
}

/// Returns the exact discrete inputs that are currently safe to compose into effective line strength.
///
/// The future Fishing owner may evaluate
/// `BaseRodLineStrength × StrengthenFactor × BaitStrengthFactor × AutomationStrengthFactor` only
/// after it has authoritative bait/Automation modifiers and a canonical numeric representation for
/// the surrounding tension model. This foundation deliberately leaves the multiplication unrounded.
/// Manual fishing contributes factor 1.0, and the no-Sturdy-bait baseline contributes factor 1.0.
///
/// Starter Basic Rod is intentionally absent: the active specification freezes it as a separate
/// Pool-only system-bound Rod but does not freeze a numeric base line strength for it. Borrowing the
/// ordinary Wood value would create an unsupported capability rule.
pub fn fishing_line_strength_foundation(
    tier: EquipmentTier,
    strengthen_level: Option<u8>,
) -> Result<FishingLineStrengthFoundation, FishingCapabilityPolicyError> {
    let rod = ordinary_fishing_rod_stats(tier)?;
    let strengthen = strengthen_line_strength_policy(strengthen_level)?;

    Ok(FishingLineStrengthFoundation {
        rod,
        strengthen,
        baseline_bait_strength_factor_basis_points: BASELINE_BAIT_STRENGTH_FACTOR_BPS,
        manual_automation_strength_factor_basis_points: MANUAL_AUTOMATION_STRENGTH_FACTOR_BPS,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rarity_tension_multipliers_are_exact() {
        let cases = [
            (FishRarity::Common, 10_000),
            (FishRarity::Uncommon, 11_000),
            (FishRarity::Rare, 12_500),
            (FishRarity::Epic, 14_500),
            (FishRarity::Legendary, 17_500),
            (FishRarity::Mythic, 22_000),
        ];

        for (rarity, expected) in cases {
            assert_eq!(
                rarity_tension_multiplier(rarity),
                RarityTensionMultiplier {
                    rarity,
                    multiplier_basis_points: expected,
                }
            );
        }
    }

    #[test]
    fn strengthen_scales_four_percent_per_level_through_x() {
        let absent = strengthen_line_strength_policy(None).unwrap();
        assert_eq!(absent.bonus_basis_points, 0);
        assert_eq!(absent.multiplier_basis_points, 10_000);

        for level in 1..=STRENGTHEN_MAX_LEVEL {
            let policy = strengthen_line_strength_policy(Some(level)).unwrap();
            assert_eq!(
                policy.bonus_basis_points,
                u16::from(level) * STRENGTHEN_LINE_STRENGTH_BONUS_BPS_PER_LEVEL
            );
            assert_eq!(
                policy.multiplier_basis_points,
                10_000 + u16::from(level) * 400
            );
        }

        let max = strengthen_line_strength_policy(Some(10)).unwrap();
        assert_eq!(max.bonus_basis_points, 4_000);
        assert_eq!(max.multiplier_basis_points, 14_000);
    }

    #[test]
    fn malformed_strengthen_levels_fail_closed() {
        assert_eq!(
            strengthen_line_strength_policy(Some(0)),
            Err(FishingCapabilityPolicyError::StrengthenLevelOutOfRange(0))
        );
        assert_eq!(
            strengthen_line_strength_policy(Some(11)),
            Err(FishingCapabilityPolicyError::StrengthenLevelOutOfRange(11))
        );
        assert_eq!(
            strengthen_line_strength_policy(Some(u8::MAX)),
            Err(FishingCapabilityPolicyError::StrengthenLevelOutOfRange(u8::MAX))
        );
    }

    #[test]
    fn line_strength_foundation_reuses_the_shared_rod_table_without_rounding() {
        let gold = fishing_line_strength_foundation(EquipmentTier::Gold, Some(10)).unwrap();
        assert_eq!(gold.rod.base_line_strength_kg_tension, 40);
        assert_eq!(gold.rod.max_durability, 550);
        assert_eq!(gold.strengthen.multiplier_basis_points, 14_000);
        assert_eq!(gold.baseline_bait_strength_factor_basis_points, 10_000);
        assert_eq!(gold.manual_automation_strength_factor_basis_points, 10_000);
    }

    #[test]
    fn starter_leather_is_not_accepted_as_a_rod_tier() {
        assert_eq!(
            fishing_line_strength_foundation(EquipmentTier::StarterLeather, None),
            Err(FishingCapabilityPolicyError::InvalidRodTier(
                OrdinaryToolStatsError::StarterLeatherIsNotOrdinaryToolTier
            ))
        );
    }
}
