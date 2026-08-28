use chrono::{DateTime, Utc};

use crate::bank::BankError;

const FEE_DENOMINATOR_PPM: i128 = 1_000_000;
const DAY_SECONDS: i64 = 86_400;

pub const BANK_BASE_INTEREST_PPM_PER_DAY: i64 = 40;
pub const BANK_MAX_INTEREST_PPM_PER_DAY: i64 = 60;
pub const BANK_MIN_WITHDRAWAL: i64 = 500;

pub(crate) struct FeeLot {
    pub amount: i64,
    pub deposited_at: DateTime<Utc>,
}

pub(crate) fn withdrawal_fee(
    amount: i64,
    bank_before: i64,
    prior_24h_gross: i128,
    lots: &[FeeLot],
    now: DateTime<Utc>,
) -> Result<i64, BankError> {
    let mut numerator = 0_i128;
    for lot in lots {
        let age_seconds = now
            .signed_duration_since(lot.deposited_at)
            .num_seconds()
            .max(0);
        let contribution = i128::from(lot.amount)
            .checked_mul(i128::from(age_fee_ppm(age_seconds)))
            .ok_or(BankError::ArithmeticOverflow)?;
        numerator = numerator
            .checked_add(contribution)
            .ok_or(BankError::ArithmeticOverflow)?;
    }

    let balance = i128::from(amount)
        .checked_mul(i128::from(balance_surcharge_ppm(bank_before)))
        .ok_or(BankError::ArithmeticOverflow)?;
    let rolling = rolling_marginal_numerator(prior_24h_gross, i128::from(amount))?;
    numerator = numerator
        .checked_add(balance)
        .and_then(|value| value.checked_add(rolling))
        .ok_or(BankError::ArithmeticOverflow)?;

    ceil_ppm(numerator)
}

fn age_fee_ppm(age_seconds: i64) -> i64 {
    if age_seconds < DAY_SECONDS {
        10_000
    } else if age_seconds < 7 * DAY_SECONDS {
        5_000
    } else if age_seconds < 14 * DAY_SECONDS {
        3_250
    } else if age_seconds < 30 * DAY_SECONDS {
        2_000
    } else {
        1_200
    }
}

fn balance_surcharge_ppm(bank_before: i64) -> i64 {
    if bank_before < 1_000_000 {
        0
    } else if bank_before < 10_000_000 {
        200
    } else if bank_before < 100_000_000 {
        400
    } else {
        600
    }
}

fn rolling_marginal_numerator(prior: i128, amount: i128) -> Result<i128, BankError> {
    let end = prior
        .checked_add(amount)
        .ok_or(BankError::ArithmeticOverflow)?;
    let segments = [
        (0_i128, 100_000_i128, 0_i128),
        (100_000, 1_000_000, 100),
        (1_000_000, 10_000_000, 250),
    ];
    let mut numerator = 0_i128;
    for (start, stop, rate) in segments {
        let overlap_start = prior.max(start);
        let overlap_end = end.min(stop);
        if overlap_end > overlap_start {
            numerator = numerator
                .checked_add(
                    (overlap_end - overlap_start)
                        .checked_mul(rate)
                        .ok_or(BankError::ArithmeticOverflow)?,
                )
                .ok_or(BankError::ArithmeticOverflow)?;
        }
    }

    let tail_start = prior.max(10_000_000);
    if end > tail_start {
        numerator = numerator
            .checked_add(
                (end - tail_start)
                    .checked_mul(500)
                    .ok_or(BankError::ArithmeticOverflow)?,
            )
            .ok_or(BankError::ArithmeticOverflow)?;
    }
    Ok(numerator)
}

fn ceil_ppm(numerator: i128) -> Result<i64, BankError> {
    let rounded = numerator
        .checked_add(FEE_DENOMINATOR_PPM - 1)
        .ok_or(BankError::ArithmeticOverflow)?
        / FEE_DENOMINATOR_PPM;
    i64::try_from(rounded).map_err(|_| BankError::ArithmeticOverflow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn age_fee_boundaries_match_bank_policy() {
        assert_eq!(age_fee_ppm(0), 10_000);
        assert_eq!(age_fee_ppm(DAY_SECONDS - 1), 10_000);
        assert_eq!(age_fee_ppm(DAY_SECONDS), 5_000);
        assert_eq!(age_fee_ppm(7 * DAY_SECONDS), 3_250);
        assert_eq!(age_fee_ppm(14 * DAY_SECONDS), 2_000);
        assert_eq!(age_fee_ppm(30 * DAY_SECONDS), 1_200);
    }

    #[test]
    fn balance_surcharge_boundaries_match_bank_policy() {
        assert_eq!(balance_surcharge_ppm(999_999), 0);
        assert_eq!(balance_surcharge_ppm(1_000_000), 200);
        assert_eq!(balance_surcharge_ppm(10_000_000), 400);
        assert_eq!(balance_surcharge_ppm(100_000_000), 600);
    }

    #[test]
    fn rolling_surcharge_is_marginal_across_thresholds() {
        assert_eq!(rolling_marginal_numerator(0, 100_000).unwrap(), 0);
        assert_eq!(
            rolling_marginal_numerator(90_000, 20_000).unwrap(),
            10_000 * 100
        );
        assert_eq!(
            rolling_marginal_numerator(990_000, 20_000).unwrap(),
            10_000 * 100 + 10_000 * 250
        );
        assert_eq!(
            rolling_marginal_numerator(9_990_000, 20_000).unwrap(),
            10_000 * 250 + 10_000 * 500
        );
    }

    #[test]
    fn immediate_small_withdrawal_charges_one_percent() {
        let now = Utc::now();
        let lots = [FeeLot {
            amount: 10_000,
            deposited_at: now,
        }];
        assert_eq!(withdrawal_fee(10_000, 20_000, 0, &lots, now).unwrap(), 100);
    }

    #[test]
    fn fee_uses_fifo_lot_ages_and_single_deterministic_ceiling() {
        let now = Utc::now();
        let lots = [
            FeeLot {
                amount: 500,
                deposited_at: now - Duration::days(31),
            },
            FeeLot {
                amount: 500,
                deposited_at: now - Duration::hours(2),
            },
        ];
        assert_eq!(withdrawal_fee(1_000, 2_000_000, 0, &lots, now).unwrap(), 6);
    }
}
