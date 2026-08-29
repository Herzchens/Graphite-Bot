use graphite_core::Money;

/// Adapts whole-percent service policy to the canonical [`Money::ceil_basis_points`] primitive.
///
/// Callers own domain-specific validation/error mapping. Negative values are rejected by `Money`,
/// and the conversion to basis points is checked even though current service percentages are small.
pub(crate) fn checked_ceil_percentage(value: i64, percent: u8) -> Option<i64> {
    let basis_points = u32::from(percent).checked_mul(100)?;
    Money::new(value)
        .ok()?
        .ceil_basis_points(basis_points)
        .ok()
        .map(Money::get)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ceiling_is_exact_at_and_between_percentage_boundaries() {
        assert_eq!(checked_ceil_percentage(0, 60), Some(0));
        assert_eq!(checked_ceil_percentage(1, 60), Some(1));
        assert_eq!(checked_ceil_percentage(5, 60), Some(3));
        assert_eq!(checked_ceil_percentage(6, 60), Some(4));
        assert_eq!(checked_ceil_percentage(50, 2), Some(1));
        assert_eq!(checked_ceil_percentage(51, 2), Some(2));
    }

    #[test]
    fn negative_values_are_rejected() {
        assert_eq!(checked_ceil_percentage(-1, 60), None);
    }

    #[test]
    fn largest_i64_value_remains_safe_for_supported_percentage_fees() {
        assert_eq!(
            checked_ceil_percentage(i64::MAX, 60),
            Some(5_534_023_222_112_865_485)
        );
        assert_eq!(
            checked_ceil_percentage(i64::MAX, 20),
            Some(1_844_674_407_370_955_162)
        );
    }
}
