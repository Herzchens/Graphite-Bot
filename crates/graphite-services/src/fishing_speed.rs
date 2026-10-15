use serde::Serialize;
use thiserror::Error;

use crate::{
    ActionSpeedBonus, ActionSpeedPolicyError, CanonicalEnchant, EquipmentTier,
    EquippedFishingRodEnchantState, EquippedFishingRodKind, FishingRodEnchant,
    FishingRodLevelXEffect, FishingRodLevelXPolicyError, GoldFishingRodPolicyError,
    ManualMineFishCooldownPolicy, canonical_enchant_max_resulting_level,
    fishing_rod_level_x_policy, gold_fishing_rod_side_grade_policy,
    manual_mine_fish_cooldown_policy,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ManualFishingEquipmentSpeedSourcesV1 {
    pub gold_rod_bonus: Option<ActionSpeedBonus>,
    pub lure_x_bonus: Option<ActionSpeedBonus>,
}

#[derive(Debug, Error)]
pub enum ManualFishingSpeedCompositionError {
    #[error(transparent)]
    ActionSpeed(#[from] ActionSpeedPolicyError),
    #[error(transparent)]
    GoldRod(#[from] GoldFishingRodPolicyError),
    #[error(transparent)]
    RodLevelX(#[from] FishingRodLevelXPolicyError),
    #[error("multiple Lure enchant rows cannot own one Fishing Rod speed source")]
    DuplicateLureEnchant,
    #[error("Lure speed is frozen only at canonical Level X; got level {0}")]
    UnsupportedLureLevel(u8),
    #[error("Lure X scalar policy is inconsistent with the shared Fishing speed bucket contract")]
    LurePolicyMismatch,
}

/// Derives only Rod-owned manual Fishing speed sources from authoritative locked Rod state.
///
/// Gold Rod contributes +10% and canonical Lure X contributes +15%. Other enchants are ignored
/// because they do not own the action-speed axis. Lower Lure levels fail closed: the current
/// scalar policy freezes only the Level-X value and must not be extrapolated downward.
pub fn manual_fishing_equipment_speed_sources_v1(
    rod: EquippedFishingRodKind,
    embedded_enchants: &[EquippedFishingRodEnchantState],
) -> Result<ManualFishingEquipmentSpeedSourcesV1, ManualFishingSpeedCompositionError> {
    let gold_rod_bonus = match rod {
        EquippedFishingRodKind::Ordinary {
            tier: EquipmentTier::Gold,
        } => {
            let policy = gold_fishing_rod_side_grade_policy(EquipmentTier::Gold, true)?;
            Some(ActionSpeedBonus::new(
                u64::from(policy.action_speed_rating_percent),
                100,
            )?)
        }
        _ => None,
    };

    let mut lure_x_bonus = None;
    for embedded in embedded_enchants {
        if embedded.enchant != CanonicalEnchant::Lure {
            continue;
        }
        if lure_x_bonus.is_some() {
            return Err(ManualFishingSpeedCompositionError::DuplicateLureEnchant);
        }
        let expected = canonical_enchant_max_resulting_level(CanonicalEnchant::Lure);
        if embedded.level != expected {
            return Err(ManualFishingSpeedCompositionError::UnsupportedLureLevel(
                embedded.level,
            ));
        }
        let policy = fishing_rod_level_x_policy(FishingRodEnchant::Lure)?;
        let FishingRodLevelXEffect::Lure {
            action_speed_rating_percent,
            uses_shared_fishing_speed_bucket,
        } = policy.effect
        else {
            return Err(ManualFishingSpeedCompositionError::LurePolicyMismatch);
        };
        if !uses_shared_fishing_speed_bucket {
            return Err(ManualFishingSpeedCompositionError::LurePolicyMismatch);
        }
        lure_x_bonus = Some(ActionSpeedBonus::new(
            u64::from(action_speed_rating_percent),
            100,
        )?);
    }

    Ok(ManualFishingEquipmentSpeedSourcesV1 {
        gold_rod_bonus,
        lure_x_bonus,
    })
}

/// Composes Rod-owned sources and already-resolved external speed sources in one shared bucket.
///
/// External inputs must already be authoritative exact ratios from their own Event/Partner/System
/// owners. They are appended before the shared +33.33% cap is applied, so Gold/Lure/Event/Partner
/// cannot accidentally cap independently or multiply durations.
pub fn manual_fishing_cooldown_with_speed_sources_v1(
    rod: EquippedFishingRodKind,
    embedded_enchants: &[EquippedFishingRodEnchantState],
    additional_sources: &[ActionSpeedBonus],
) -> Result<ManualMineFishCooldownPolicy, ManualFishingSpeedCompositionError> {
    let equipment = manual_fishing_equipment_speed_sources_v1(rod, embedded_enchants)?;
    let mut sources = Vec::with_capacity(2 + additional_sources.len());
    if let Some(source) = equipment.gold_rod_bonus {
        sources.push(source);
    }
    if let Some(source) = equipment.lure_x_bonus {
        sources.push(source);
    }
    sources.extend_from_slice(additional_sources);
    Ok(manual_mine_fish_cooldown_policy(&sources)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enchant(enchant: CanonicalEnchant, level: u8) -> EquippedFishingRodEnchantState {
        EquippedFishingRodEnchantState { enchant, level }
    }

    #[test]
    fn baseline_gold_lure_and_combined_cooldowns_are_exact() {
        let wood = EquippedFishingRodKind::Ordinary {
            tier: EquipmentTier::Wood,
        };
        let gold = EquippedFishingRodKind::Ordinary {
            tier: EquipmentTier::Gold,
        };
        let lure_x = [enchant(
            CanonicalEnchant::Lure,
            canonical_enchant_max_resulting_level(CanonicalEnchant::Lure),
        )];

        assert_eq!(
            manual_fishing_cooldown_with_speed_sources_v1(wood, &[], &[])
                .unwrap()
                .cooldown_millis,
            10_000
        );
        assert_eq!(
            manual_fishing_cooldown_with_speed_sources_v1(gold, &[], &[])
                .unwrap()
                .cooldown_millis,
            9_091
        );
        assert_eq!(
            manual_fishing_cooldown_with_speed_sources_v1(wood, &lure_x, &[])
                .unwrap()
                .cooldown_millis,
            8_696
        );
        assert_eq!(
            manual_fishing_cooldown_with_speed_sources_v1(gold, &lure_x, &[])
                .unwrap()
                .cooldown_millis,
            8_000
        );
    }

    #[test]
    fn external_source_joins_equipment_before_one_shared_cap() {
        let gold = EquippedFishingRodKind::Ordinary {
            tier: EquipmentTier::Gold,
        };
        let lure_x = [enchant(
            CanonicalEnchant::Lure,
            canonical_enchant_max_resulting_level(CanonicalEnchant::Lure),
        )];
        let external = ActionSpeedBonus::new(1, 10).unwrap();

        let preview =
            manual_fishing_cooldown_with_speed_sources_v1(gold, &lure_x, &[external]).unwrap();
        assert_eq!(
            (
                preview.speed_policy.uncapped_bonus.numerator(),
                preview.speed_policy.uncapped_bonus.denominator(),
            ),
            (7, 20)
        );
        assert!(preview.speed_policy.cap_applied);
        assert_eq!(preview.cooldown_millis, 7_500);
        assert!(preview.minimum_clamp_applied);
    }

    #[test]
    fn lower_lure_and_duplicate_lure_fail_closed() {
        let wood = EquippedFishingRodKind::Ordinary {
            tier: EquipmentTier::Wood,
        };
        assert!(matches!(
            manual_fishing_equipment_speed_sources_v1(
                wood,
                &[enchant(CanonicalEnchant::Lure, 9)],
            ),
            Err(ManualFishingSpeedCompositionError::UnsupportedLureLevel(9))
        ));
        let lure_x = enchant(
            CanonicalEnchant::Lure,
            canonical_enchant_max_resulting_level(CanonicalEnchant::Lure),
        );
        assert!(matches!(
            manual_fishing_equipment_speed_sources_v1(wood, &[lure_x, lure_x]),
            Err(ManualFishingSpeedCompositionError::DuplicateLureEnchant)
        ));
    }

    #[test]
    fn unrelated_enchants_do_not_create_speed_sources() {
        let wood = EquippedFishingRodKind::Ordinary {
            tier: EquipmentTier::Wood,
        };
        let sources = manual_fishing_equipment_speed_sources_v1(
            wood,
            &[enchant(CanonicalEnchant::Strengthen, 10)],
        )
        .unwrap();
        assert_eq!(
            sources,
            ManualFishingEquipmentSpeedSourcesV1 {
                gold_rod_bonus: None,
                lure_x_bonus: None,
            }
        );
    }
}
