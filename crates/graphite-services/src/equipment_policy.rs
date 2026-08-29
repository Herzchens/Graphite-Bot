use serde::Serialize;
use thiserror::Error;

const SLOT_FACTOR_DENOMINATOR: i128 = 100;
const APPRAISAL_ROUNDING_UNIT: i128 = 100;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EquipmentTier {
    StarterLeather,
    Wood,
    Stone,
    Copper,
    Gold,
    Iron,
    Diamond,
    Obsidian,
    Netherite,
    Graphite,
}

impl EquipmentTier {
    const fn tier_anchor(self) -> Option<i64> {
        match self {
            Self::StarterLeather => None,
            Self::Wood => Some(3_600),
            Self::Stone => Some(8_000),
            Self::Copper => Some(27_000),
            Self::Gold => Some(110_000),
            Self::Iron => Some(77_500),
            Self::Diamond => Some(270_000),
            Self::Obsidian => Some(875_000),
            Self::Netherite => Some(2_380_000),
            Self::Graphite => Some(8_050_000),
        }
    }

    pub(crate) const fn repair_ratio_percent(self) -> Option<i128> {
        match self {
            Self::StarterLeather => None,
            Self::Wood => Some(10),
            Self::Stone => Some(11),
            Self::Copper => Some(13),
            Self::Gold => Some(25),
            Self::Iron => Some(14),
            Self::Diamond => Some(16),
            Self::Obsidian => Some(18),
            Self::Netherite => Some(20),
            Self::Graphite => Some(23),
        }
    }

    pub const fn material(self) -> EquipmentMaterial {
        match self {
            Self::StarterLeather => EquipmentMaterial::Leather,
            Self::Wood => EquipmentMaterial::WoodLog,
            Self::Stone => EquipmentMaterial::Stone,
            Self::Copper => EquipmentMaterial::CopperIngot,
            Self::Gold => EquipmentMaterial::GoldIngot,
            Self::Iron => EquipmentMaterial::IronIngot,
            Self::Diamond => EquipmentMaterial::Diamond,
            Self::Obsidian => EquipmentMaterial::Obsidian,
            Self::Netherite => EquipmentMaterial::NetheriteScrap,
            Self::Graphite => EquipmentMaterial::GraphiteLayer,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EquipmentSlot {
    Pickaxe,
    Sword,
    FishingRod,
    Helmet,
    Chestplate,
    Leggings,
    Boots,
}

impl EquipmentSlot {
    const fn factor_hundredths(self) -> i128 {
        match self {
            Self::Pickaxe | Self::Sword | Self::FishingRod => 100,
            Self::Helmet => 80,
            Self::Chestplate => 135,
            Self::Leggings => 115,
            Self::Boots => 70,
        }
    }

    pub const fn base_material_units(self) -> i64 {
        match self {
            Self::Pickaxe | Self::Sword | Self::FishingRod | Self::Helmet | Self::Boots => 2,
            Self::Chestplate => 4,
            Self::Leggings => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EquipmentMaterial {
    Leather,
    WoodLog,
    Stone,
    CopperIngot,
    GoldIngot,
    IronIngot,
    Diamond,
    Obsidian,
    NetheriteScrap,
    GraphiteLayer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BaseEquipmentAppraisalSource {
    StandardTable,
    DefinitionOverride,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct BaseEquipmentAppraisal {
    pub tier: EquipmentTier,
    pub slot: EquipmentSlot,
    pub value: i64,
    pub source: BaseEquipmentAppraisalSource,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum EquipmentAppraisalError {
    #[error("definition-specific base appraisal cannot be negative")]
    NegativeDefinitionOverride,
    #[error(
        "the canonical TierAnchor is not defined for tier {0:?}; an explicit ItemDefinition base_appraisal override is required"
    )]
    MissingTierAnchor(EquipmentTier),
    #[error("equipment appraisal arithmetic exceeded supported integer bounds")]
    ArithmeticOverflow,
}

/// Resolves the canonical base appraisal for one already-resolved equipment definition.
///
/// A definition-specific `base_appraisal` is authoritative when present and is returned unchanged.
/// Otherwise ordinary equipment uses `round100(TierAnchor × SlotFactor)` with exact non-negative
/// rational arithmetic and round-half-up semantics. Starter Leather has no frozen TierAnchor, so it
/// requires an explicit definition override instead of borrowing another material's anchor.
pub fn base_equipment_appraisal(
    tier: EquipmentTier,
    slot: EquipmentSlot,
    definition_override: Option<i64>,
) -> Result<BaseEquipmentAppraisal, EquipmentAppraisalError> {
    if let Some(value) = definition_override {
        if value < 0 {
            return Err(EquipmentAppraisalError::NegativeDefinitionOverride);
        }
        return Ok(BaseEquipmentAppraisal {
            tier,
            slot,
            value,
            source: BaseEquipmentAppraisalSource::DefinitionOverride,
        });
    }

    let anchor = tier
        .tier_anchor()
        .ok_or(EquipmentAppraisalError::MissingTierAnchor(tier))?;
    let scaled_hundredths = i128::from(anchor)
        .checked_mul(slot.factor_hundredths())
        .ok_or(EquipmentAppraisalError::ArithmeticOverflow)?;
    let round100_denominator = SLOT_FACTOR_DENOMINATOR
        .checked_mul(APPRAISAL_ROUNDING_UNIT)
        .ok_or(EquipmentAppraisalError::ArithmeticOverflow)?;
    let rounded_units = scaled_hundredths
        .checked_add(round100_denominator / 2)
        .ok_or(EquipmentAppraisalError::ArithmeticOverflow)?
        / round100_denominator;
    let value = rounded_units
        .checked_mul(APPRAISAL_ROUNDING_UNIT)
        .ok_or(EquipmentAppraisalError::ArithmeticOverflow)?;

    Ok(BaseEquipmentAppraisal {
        tier,
        slot,
        value: i64::try_from(value).map_err(|_| EquipmentAppraisalError::ArithmeticOverflow)?,
        source: BaseEquipmentAppraisalSource::StandardTable,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_table_matches_every_frozen_tier_and_slot_combination() {
        let expected = [
            (
                EquipmentTier::Wood,
                [3_600, 3_600, 3_600, 2_900, 4_900, 4_100, 2_500],
            ),
            (
                EquipmentTier::Stone,
                [8_000, 8_000, 8_000, 6_400, 10_800, 9_200, 5_600],
            ),
            (
                EquipmentTier::Copper,
                [27_000, 27_000, 27_000, 21_600, 36_500, 31_100, 18_900],
            ),
            (
                EquipmentTier::Gold,
                [110_000, 110_000, 110_000, 88_000, 148_500, 126_500, 77_000],
            ),
            (
                EquipmentTier::Iron,
                [77_500, 77_500, 77_500, 62_000, 104_600, 89_100, 54_300],
            ),
            (
                EquipmentTier::Diamond,
                [
                    270_000, 270_000, 270_000, 216_000, 364_500, 310_500, 189_000,
                ],
            ),
            (
                EquipmentTier::Obsidian,
                [
                    875_000, 875_000, 875_000, 700_000, 1_181_300, 1_006_300, 612_500,
                ],
            ),
            (
                EquipmentTier::Netherite,
                [
                    2_380_000, 2_380_000, 2_380_000, 1_904_000, 3_213_000, 2_737_000, 1_666_000,
                ],
            ),
            (
                EquipmentTier::Graphite,
                [
                    8_050_000, 8_050_000, 8_050_000, 6_440_000, 10_867_500, 9_257_500, 5_635_000,
                ],
            ),
        ];
        let slots = [
            EquipmentSlot::Pickaxe,
            EquipmentSlot::Sword,
            EquipmentSlot::FishingRod,
            EquipmentSlot::Helmet,
            EquipmentSlot::Chestplate,
            EquipmentSlot::Leggings,
            EquipmentSlot::Boots,
        ];

        for (tier, values) in expected {
            for (slot, expected_value) in slots.into_iter().zip(values) {
                let appraisal = base_equipment_appraisal(tier, slot, None).unwrap();
                assert_eq!(appraisal.value, expected_value, "{tier:?} {slot:?}");
                assert_eq!(
                    appraisal.source,
                    BaseEquipmentAppraisalSource::StandardTable
                );
                assert_eq!(appraisal.value % 100, 0);
            }
        }
    }

    #[test]
    fn round100_uses_exact_half_up_semantics() {
        assert_eq!(
            base_equipment_appraisal(EquipmentTier::Obsidian, EquipmentSlot::Chestplate, None)
                .unwrap()
                .value,
            1_181_300
        );
        assert_eq!(
            base_equipment_appraisal(EquipmentTier::Obsidian, EquipmentSlot::Leggings, None)
                .unwrap()
                .value,
            1_006_300
        );
        assert_eq!(
            base_equipment_appraisal(EquipmentTier::Iron, EquipmentSlot::Chestplate, None)
                .unwrap()
                .value,
            104_600
        );
    }

    #[test]
    fn definition_override_has_exact_precedence_and_is_not_rounded() {
        let appraisal = base_equipment_appraisal(
            EquipmentTier::Graphite,
            EquipmentSlot::Chestplate,
            Some(123_456),
        )
        .unwrap();
        assert_eq!(appraisal.value, 123_456);
        assert_eq!(
            appraisal.source,
            BaseEquipmentAppraisalSource::DefinitionOverride
        );

        let zero =
            base_equipment_appraisal(EquipmentTier::Wood, EquipmentSlot::Pickaxe, Some(0)).unwrap();
        assert_eq!(zero.value, 0);
        assert_eq!(
            zero.source,
            BaseEquipmentAppraisalSource::DefinitionOverride
        );
    }

    #[test]
    fn starter_leather_requires_definition_override_but_accepts_one() {
        assert_eq!(
            base_equipment_appraisal(EquipmentTier::StarterLeather, EquipmentSlot::Helmet, None,),
            Err(EquipmentAppraisalError::MissingTierAnchor(
                EquipmentTier::StarterLeather
            ))
        );

        let appraisal = base_equipment_appraisal(
            EquipmentTier::StarterLeather,
            EquipmentSlot::Helmet,
            Some(2_750),
        )
        .unwrap();
        assert_eq!(appraisal.value, 2_750);
        assert_eq!(
            appraisal.source,
            BaseEquipmentAppraisalSource::DefinitionOverride
        );
    }

    #[test]
    fn negative_definition_override_is_rejected() {
        assert_eq!(
            base_equipment_appraisal(EquipmentTier::Wood, EquipmentSlot::Pickaxe, Some(-1),),
            Err(EquipmentAppraisalError::NegativeDefinitionOverride)
        );
    }
}
