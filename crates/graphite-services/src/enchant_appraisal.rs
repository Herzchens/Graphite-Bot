use serde::Serialize;
use thiserror::Error;

const BOOK_APPRAISAL_BASE: i128 = 60_000;
const MULTIPLIER_DENOMINATOR: i128 = 100;
const EMBEDDED_VALUE_NUMERATOR: i128 = 70;
const EMBEDDED_VALUE_DENOMINATOR: i128 = 100;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EnchantAppraisalClass {
    ShopCommon,
    FishingChestMidHigh,
    FishingChestRare,
    Mending,
    Mythic,
    SpecialCommon,
    SpecialMid,
    SpecialRare,
}

impl EnchantAppraisalClass {
    const fn weight(self) -> i128 {
        match self {
            Self::ShopCommon => 1,
            Self::FishingChestMidHigh => 3,
            Self::FishingChestRare | Self::Mending => 8,
            Self::Mythic => 20,
            Self::SpecialCommon => 2,
            Self::SpecialMid => 5,
            Self::SpecialRare => 12,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct EmbeddedEnchantAppraisalInput {
    pub class: EnchantAppraisalClass,
    pub level: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct CanonicalBookAppraisal {
    pub class: EnchantAppraisalClass,
    pub level: u8,
    pub value: i64,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum EnchantAppraisalError {
    #[error("canonical enchant appraisal supports resulting levels I through X; got level {0}")]
    InvalidLevel(u8),
    #[error("enchant appraisal arithmetic exceeded supported integer bounds")]
    ArithmeticOverflow,
}

/// Resolves the frozen canonical book appraisal for an already-classified embedded enchant.
///
/// This is the low-level numeric kernel. Stateful or identity-aware callers should prefer the
/// canonical-enchant bridge owned by `enchant_catalog`, which derives the appraisal class from the
/// canonical enchant identity instead of accepting caller-supplied classification. Shadow Walker
/// uses [`EnchantAppraisalClass::FishingChestMidHigh`] at its resulting level. SoulBind is not an
/// enchant and must not be passed to this policy kernel.
pub fn canonical_book_appraisal(
    class: EnchantAppraisalClass,
    level: u8,
) -> Result<CanonicalBookAppraisal, EnchantAppraisalError> {
    let level_multiplier_hundredths = level_multiplier_hundredths(level)?;
    let numerator = BOOK_APPRAISAL_BASE
        .checked_mul(class.weight())
        .and_then(|value| value.checked_mul(level_multiplier_hundredths))
        .ok_or(EnchantAppraisalError::ArithmeticOverflow)?;
    let value = numerator / MULTIPLIER_DENOMINATOR;

    Ok(CanonicalBookAppraisal {
        class,
        level,
        value: i64::try_from(value).map_err(|_| EnchantAppraisalError::ArithmeticOverflow)?,
    })
}

/// Computes the frozen value contributed by all already-classified enchants embedded in one item.
///
/// This remains the low-level class-based API for policy composition and compatibility. Callers
/// that own concrete canonical enchant identities should use the catalog bridge so an appraisal
/// class cannot be supplied independently from the enchant identity.
pub fn embedded_enchant_value(
    enchants: &[EmbeddedEnchantAppraisalInput],
) -> Result<i64, EnchantAppraisalError> {
    embedded_enchant_value_from_book_appraisals(
        enchants
            .iter()
            .map(|enchant| canonical_book_appraisal(enchant.class, enchant.level)),
    )
}

pub(crate) fn embedded_enchant_value_from_book_appraisals<I>(
    appraisals: I,
) -> Result<i64, EnchantAppraisalError>
where
    I: IntoIterator<Item = Result<CanonicalBookAppraisal, EnchantAppraisalError>>,
{
    let mut total = 0_i128;
    for appraisal in appraisals {
        let appraisal = appraisal?;
        total = total
            .checked_add(i128::from(appraisal.value))
            .ok_or(EnchantAppraisalError::ArithmeticOverflow)?;
    }

    let numerator = total
        .checked_mul(EMBEDDED_VALUE_NUMERATOR)
        .ok_or(EnchantAppraisalError::ArithmeticOverflow)?;
    let rounded = round_half_up_nonnegative(numerator, EMBEDDED_VALUE_DENOMINATOR)?;
    i64::try_from(rounded).map_err(|_| EnchantAppraisalError::ArithmeticOverflow)
}

fn level_multiplier_hundredths(level: u8) -> Result<i128, EnchantAppraisalError> {
    match level {
        1 => Ok(100),
        2 => Ok(175),
        3 => Ok(300),
        4 => Ok(525),
        5 => Ok(900),
        6 => Ok(1_500),
        7 => Ok(2_500),
        8 => Ok(4_200),
        9 => Ok(7_000),
        10 => Ok(12_000),
        _ => Err(EnchantAppraisalError::InvalidLevel(level)),
    }
}

fn round_half_up_nonnegative(
    numerator: i128,
    denominator: i128,
) -> Result<i128, EnchantAppraisalError> {
    debug_assert!(numerator >= 0 && denominator > 0);
    numerator
        .checked_add(denominator / 2)
        .map(|value| value / denominator)
        .ok_or(EnchantAppraisalError::ArithmeticOverflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_multiplier_table_matches_frozen_shop_common_values() {
        let expected = [
            60_000, 105_000, 180_000, 315_000, 540_000, 900_000, 1_500_000, 2_520_000, 4_200_000,
            7_200_000,
        ];

        for (level, expected_value) in (1_u8..=10).zip(expected) {
            assert_eq!(
                canonical_book_appraisal(EnchantAppraisalClass::ShopCommon, level)
                    .unwrap()
                    .value,
                expected_value,
                "level {level}"
            );
        }
    }

    #[test]
    fn acquisition_weight_table_matches_frozen_level_one_values() {
        for (class, expected_value) in [
            (EnchantAppraisalClass::ShopCommon, 60_000),
            (EnchantAppraisalClass::FishingChestMidHigh, 180_000),
            (EnchantAppraisalClass::FishingChestRare, 480_000),
            (EnchantAppraisalClass::Mending, 480_000),
            (EnchantAppraisalClass::Mythic, 1_200_000),
            (EnchantAppraisalClass::SpecialCommon, 120_000),
            (EnchantAppraisalClass::SpecialMid, 300_000),
            (EnchantAppraisalClass::SpecialRare, 720_000),
        ] {
            assert_eq!(
                canonical_book_appraisal(class, 1).unwrap().value,
                expected_value,
                "{class:?}"
            );
        }
    }

    #[test]
    fn cross_product_is_exact_and_monotone_by_level() {
        for class in [
            EnchantAppraisalClass::ShopCommon,
            EnchantAppraisalClass::FishingChestMidHigh,
            EnchantAppraisalClass::FishingChestRare,
            EnchantAppraisalClass::Mending,
            EnchantAppraisalClass::Mythic,
            EnchantAppraisalClass::SpecialCommon,
            EnchantAppraisalClass::SpecialMid,
            EnchantAppraisalClass::SpecialRare,
        ] {
            let mut previous = 0_i64;
            for level in 1..=10 {
                let value = canonical_book_appraisal(class, level).unwrap().value;
                assert!(value > previous);
                assert_eq!(value % 1_000, 0);
                previous = value;
            }
        }
    }

    #[test]
    fn embedded_value_is_seventy_percent_of_canonical_book_sum() {
        let inputs = [
            EmbeddedEnchantAppraisalInput {
                class: EnchantAppraisalClass::ShopCommon,
                level: 2,
            },
            EmbeddedEnchantAppraisalInput {
                class: EnchantAppraisalClass::Mending,
                level: 1,
            },
            EmbeddedEnchantAppraisalInput {
                class: EnchantAppraisalClass::SpecialRare,
                level: 3,
            },
        ];
        // 105,000 + 480,000 + 2,160,000 = 2,745,000; 70% = 1,921,500.
        assert_eq!(embedded_enchant_value(&inputs).unwrap(), 1_921_500);
    }

    #[test]
    fn shadow_walker_policy_is_representable_as_mid_high_at_resulting_level() {
        assert_eq!(
            canonical_book_appraisal(EnchantAppraisalClass::FishingChestMidHigh, 4)
                .unwrap()
                .value,
            945_000
        );
    }

    #[test]
    fn empty_embedded_set_has_zero_value() {
        assert_eq!(embedded_enchant_value(&[]).unwrap(), 0);
    }

    #[test]
    fn invalid_levels_fail_closed() {
        assert_eq!(
            canonical_book_appraisal(EnchantAppraisalClass::ShopCommon, 0),
            Err(EnchantAppraisalError::InvalidLevel(0))
        );
        assert_eq!(
            canonical_book_appraisal(EnchantAppraisalClass::ShopCommon, 11),
            Err(EnchantAppraisalError::InvalidLevel(11))
        );
        assert_eq!(
            embedded_enchant_value(&[EmbeddedEnchantAppraisalInput {
                class: EnchantAppraisalClass::Mythic,
                level: 42,
            }]),
            Err(EnchantAppraisalError::InvalidLevel(42))
        );
    }
}
