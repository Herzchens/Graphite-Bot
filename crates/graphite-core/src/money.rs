use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Money(i64);

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum MoneyError {
    #[error("money cannot be negative")]
    Negative,
    #[error("money arithmetic overflow")]
    Overflow,
    #[error("insufficient money")]
    Insufficient,
}

impl Money {
    pub const ZERO: Self = Self(0);

    pub fn new(value: i64) -> Result<Self, MoneyError> {
        if value < 0 {
            return Err(MoneyError::Negative);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }

    pub fn checked_add(self, other: Self) -> Result<Self, MoneyError> {
        let value = i128::from(self.0) + i128::from(other.0);
        let value = i64::try_from(value).map_err(|_| MoneyError::Overflow)?;
        Self::new(value)
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, MoneyError> {
        if other.0 > self.0 {
            return Err(MoneyError::Insufficient);
        }
        Self::new(self.0 - other.0)
    }

    pub fn ceil_basis_points(self, basis_points: u32) -> Result<Self, MoneyError> {
        let numerator = i128::from(self.0)
            .checked_mul(i128::from(basis_points))
            .ok_or(MoneyError::Overflow)?;
        let rounded = numerator.checked_add(9_999).ok_or(MoneyError::Overflow)? / 10_000;
        let value = i64::try_from(rounded).map_err(|_| MoneyError::Overflow)?;
        Self::new(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_negative_money() {
        assert_eq!(Money::new(-1), Err(MoneyError::Negative));
    }

    #[test]
    fn checked_math_never_wraps() {
        let max = Money::new(i64::MAX).expect("valid non-negative money");
        assert_eq!(
            max.checked_add(Money::new(1).unwrap()),
            Err(MoneyError::Overflow)
        );
        assert_eq!(
            Money::new(4).unwrap().checked_sub(Money::new(5).unwrap()),
            Err(MoneyError::Insufficient)
        );
    }

    #[test]
    fn percentage_fee_rounds_up_deterministically() {
        let money = Money::new(10_001).unwrap();
        assert_eq!(money.ceil_basis_points(100).unwrap().get(), 101);
    }
}
