use chrono::{Datelike, NaiveDate};
use graphite_core::{OperationId, RootSeed};
use graphite_store::PgStore;
use serde_json::json;
use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::fees::BANK_BASE_INTEREST_PPM_PER_DAY;

const BANK_INTEREST_POLICY_VERSION: i32 = 1;
pub const BANK_BONUS_INTEREST_PPM_PER_DAY: i64 = 20;
pub const BANK_BONUS_PRINCIPAL_TRANCHE: i64 = 10_000_000;
const Q32_ONE: i128 = 1_i128 << 32;
const REBIRTH_DECAY_Q32: i128 = 4_085_499_269;
const INTEREST_DENOMINATOR_Q32: i128 = 1_000_000_i128 * Q32_ONE;
const MAX_CATCHUP_DAYS_PER_PLAYER: i64 = 366;

#[derive(Clone)]
pub struct BankInterestService {
    store: PgStore,
}

#[derive(Debug, Error)]
pub enum BankInterestError {
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error("Discord snowflake is outside the signed BIGINT persistence range")]
    SnowflakeOutOfRange,
    #[error("no active Graphite account exists")]
    PlayerNotFound,
    #[error("Bank lots do not reconcile with the materialized Bank balance")]
    LotIntegrityMismatch,
    #[error("Bank interest state is invalid")]
    InvalidState,
    #[error("Bank interest arithmetic exceeded the supported persistence range")]
    ArithmeticOverflow,
    #[error("Bank interest operation already exists while accrual state is still due")]
    OperationConflict,
}

impl From<sqlx::Error> for BankInterestError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BankInterestSummary {
    pub player_id: Uuid,
    pub days_processed: u32,
    pub interest_credited: i64,
    pub last_accrual_day: NaiveDate,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BankInterestBatchSummary {
    pub players_processed: u32,
    pub days_processed: u32,
    pub interest_credited: i64,
}

struct InterestPlayer {
    player_id: Uuid,
    status: String,
    bank: i64,
    rebirth_count: i64,
    remainder_q32: i64,
    last_accrual_day: NaiveDate,
}

struct InterestLot {
    id: Uuid,
    principal: i64,
}

impl BankInterestService {
    #[must_use]
    pub fn new(store: PgStore) -> Self {
        Self { store }
    }

    pub async fn accrue_interest(
        &self,
        discord_user_id: u64,
    ) -> Result<BankInterestSummary, BankInterestError> {
        let discord_user_id =
            i64::try_from(discord_user_id).map_err(|_| BankInterestError::SnowflakeOutOfRange)?;
        let mut tx = self.store.pool().begin().await?;
        let today = utc_today(&mut tx).await?;
        let mut player = lock_interest_player_by_discord(&mut tx, discord_user_id).await?;
        let summary = accrue_locked_player(&mut tx, &mut player, today).await?;
        tx.commit().await?;
        Ok(summary)
    }

    pub async fn accrue_due_interest_batch(
        &self,
        limit: u32,
    ) -> Result<BankInterestBatchSummary, BankInterestError> {
        if limit == 0 {
            return Ok(BankInterestBatchSummary::default());
        }
        let limit = i64::from(limit.min(1_000));
        let rows = sqlx::query(
            r#"
            SELECT p.id
              FROM players p
              JOIN bank_interest_state s ON s.player_id = p.id
             WHERE p.status <> 'DELETED'
               AND s.last_accrual_day < (now() AT TIME ZONE 'UTC')::date
             ORDER BY s.last_accrual_day ASC, p.id ASC
             LIMIT $1
            "#,
        )
        .bind(limit)
        .fetch_all(self.store.pool())
        .await?;

        let mut batch = BankInterestBatchSummary::default();
        for row in rows {
            let player_id: Uuid = row.try_get("id")?;
            let summary = self.accrue_interest_by_player_id(player_id).await?;
            batch.players_processed = batch
                .players_processed
                .checked_add(1)
                .ok_or(BankInterestError::ArithmeticOverflow)?;
            batch.days_processed = batch
                .days_processed
                .checked_add(summary.days_processed)
                .ok_or(BankInterestError::ArithmeticOverflow)?;
            batch.interest_credited =
                checked_add(batch.interest_credited, summary.interest_credited)?;
        }
        Ok(batch)
    }

    async fn accrue_interest_by_player_id(
        &self,
        player_id: Uuid,
    ) -> Result<BankInterestSummary, BankInterestError> {
        let mut tx = self.store.pool().begin().await?;
        let today = utc_today(&mut tx).await?;
        let mut player = lock_interest_player_by_id(&mut tx, player_id).await?;
        let summary = accrue_locked_player(&mut tx, &mut player, today).await?;
        tx.commit().await?;
        Ok(summary)
    }
}

async fn accrue_locked_player(
    tx: &mut Transaction<'_, Postgres>,
    player: &mut InterestPlayer,
    today: NaiveDate,
) -> Result<BankInterestSummary, BankInterestError> {
    if player.last_accrual_day >= today {
        return Ok(BankInterestSummary {
            player_id: player.player_id,
            days_processed: 0,
            interest_credited: 0,
            last_accrual_day: player.last_accrual_day,
        });
    }

    let due_days = (today - player.last_accrual_day).num_days();
    let days_to_process = due_days.min(MAX_CATCHUP_DAYS_PER_PLAYER);
    if player.status == "HARD_FROZEN" {
        player.last_accrual_day = player
            .last_accrual_day
            .checked_add_days(chrono::Days::new(
                u64::try_from(days_to_process)
                    .map_err(|_| BankInterestError::ArithmeticOverflow)?,
            ))
            .ok_or(BankInterestError::ArithmeticOverflow)?;
        update_interest_state(
            tx,
            player.player_id,
            player.remainder_q32,
            player.last_accrual_day,
        )
        .await?;
        return Ok(BankInterestSummary {
            player_id: player.player_id,
            days_processed: u32::try_from(days_to_process)
                .map_err(|_| BankInterestError::ArithmeticOverflow)?,
            interest_credited: 0,
            last_accrual_day: player.last_accrual_day,
        });
    }

    let mut lots = load_interest_lots(tx, player.player_id).await?;
    reconcile_lots(player.bank, &lots)?;
    let mut total_interest = 0_i64;

    for _ in 0..days_to_process {
        let accrual_day = player
            .last_accrual_day
            .succ_opt()
            .ok_or(BankInterestError::ArithmeticOverflow)?;
        let bank_before = player.bank;
        let remainder_before = player.remainder_q32;
        let (interest, remainder_after) =
            calculate_daily_interest(bank_before, player.rebirth_count, remainder_before)?;

        if interest > 0 {
            allocate_interest_to_lots(tx, &mut lots, interest, bank_before).await?;
            player.bank = checked_add(player.bank, interest)?;
            insert_interest_operation(
                tx,
                player,
                accrual_day,
                bank_before,
                interest,
                remainder_before,
                remainder_after,
            )
            .await?;
            total_interest = checked_add(total_interest, interest)?;
        }

        player.remainder_q32 = remainder_after;
        player.last_accrual_day = accrual_day;
    }

    if total_interest > 0 {
        sqlx::query(
            "UPDATE player_balances SET bank = $1, updated_at = now() WHERE player_id = $2",
        )
        .bind(player.bank)
        .bind(player.player_id)
        .execute(&mut **tx)
        .await?;
    }
    update_interest_state(
        tx,
        player.player_id,
        player.remainder_q32,
        player.last_accrual_day,
    )
    .await?;

    Ok(BankInterestSummary {
        player_id: player.player_id,
        days_processed: u32::try_from(days_to_process)
            .map_err(|_| BankInterestError::ArithmeticOverflow)?,
        interest_credited: total_interest,
        last_accrual_day: player.last_accrual_day,
    })
}

async fn lock_interest_player_by_discord(
    tx: &mut Transaction<'_, Postgres>,
    discord_user_id: i64,
) -> Result<InterestPlayer, BankInterestError> {
    let row = sqlx::query(
        r#"
        SELECT p.id, p.status, p.rebirth_count, b.bank,
               s.remainder_q32, s.last_accrual_day
          FROM players p
          JOIN player_balances b ON b.player_id = p.id
          JOIN bank_interest_state s ON s.player_id = p.id
         WHERE p.discord_user_id = $1
           AND p.status <> 'DELETED'
         FOR UPDATE OF p, b, s
        "#,
    )
    .bind(discord_user_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(BankInterestError::PlayerNotFound)?;
    interest_player_from_row(row)
}

async fn lock_interest_player_by_id(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
) -> Result<InterestPlayer, BankInterestError> {
    let row = sqlx::query(
        r#"
        SELECT p.id, p.status, p.rebirth_count, b.bank,
               s.remainder_q32, s.last_accrual_day
          FROM players p
          JOIN player_balances b ON b.player_id = p.id
          JOIN bank_interest_state s ON s.player_id = p.id
         WHERE p.id = $1
           AND p.status <> 'DELETED'
         FOR UPDATE OF p, b, s
        "#,
    )
    .bind(player_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(BankInterestError::PlayerNotFound)?;
    interest_player_from_row(row)
}

fn interest_player_from_row(
    row: sqlx::postgres::PgRow,
) -> Result<InterestPlayer, BankInterestError> {
    let remainder_q32: i64 = row.try_get("remainder_q32")?;
    let denominator = i64::try_from(INTEREST_DENOMINATOR_Q32)
        .map_err(|_| BankInterestError::ArithmeticOverflow)?;
    if !(0..denominator).contains(&remainder_q32) {
        return Err(BankInterestError::InvalidState);
    }

    Ok(InterestPlayer {
        player_id: row.try_get("id")?,
        status: row.try_get("status")?,
        bank: row.try_get("bank")?,
        rebirth_count: row.try_get("rebirth_count")?,
        remainder_q32,
        last_accrual_day: row.try_get("last_accrual_day")?,
    })
}

async fn load_interest_lots(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
) -> Result<Vec<InterestLot>, BankInterestError> {
    let rows = sqlx::query(
        r#"
        SELECT id, principal_remaining
          FROM bank_lots
         WHERE player_id = $1
           AND principal_remaining > 0
         ORDER BY deposited_at ASC, id ASC
         FOR UPDATE
        "#,
    )
    .bind(player_id)
    .fetch_all(&mut **tx)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(InterestLot {
                id: row.try_get("id")?,
                principal: row.try_get("principal_remaining")?,
            })
        })
        .collect()
}

fn reconcile_lots(bank: i64, lots: &[InterestLot]) -> Result<(), BankInterestError> {
    let total = lots
        .iter()
        .try_fold(0_i64, |sum, lot| checked_add(sum, lot.principal))?;
    if total == bank {
        Ok(())
    } else {
        Err(BankInterestError::LotIntegrityMismatch)
    }
}

async fn allocate_interest_to_lots(
    tx: &mut Transaction<'_, Postgres>,
    lots: &mut [InterestLot],
    interest: i64,
    bank_before: i64,
) -> Result<(), BankInterestError> {
    if interest <= 0 {
        return Ok(());
    }
    if bank_before <= 0 || lots.is_empty() {
        return Err(BankInterestError::LotIntegrityMismatch);
    }

    let denominator = i128::from(bank_before);
    let mut allocations = vec![0_i64; lots.len()];
    let mut fractional = Vec::with_capacity(lots.len());
    let mut allocated = 0_i64;

    for (index, lot) in lots.iter().enumerate() {
        let product = i128::from(interest)
            .checked_mul(i128::from(lot.principal))
            .ok_or(BankInterestError::ArithmeticOverflow)?;
        let share = i64::try_from(product / denominator)
            .map_err(|_| BankInterestError::ArithmeticOverflow)?;
        allocations[index] = share;
        allocated = checked_add(allocated, share)?;
        fractional.push((index, product % denominator));
    }

    fractional.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    let leftover = interest
        .checked_sub(allocated)
        .ok_or(BankInterestError::ArithmeticOverflow)?;
    let leftover = usize::try_from(leftover).map_err(|_| BankInterestError::ArithmeticOverflow)?;
    if leftover > fractional.len() {
        return Err(BankInterestError::ArithmeticOverflow);
    }
    for (index, _) in fractional.into_iter().take(leftover) {
        allocations[index] = allocations[index]
            .checked_add(1)
            .ok_or(BankInterestError::ArithmeticOverflow)?;
    }

    for (lot, amount) in lots.iter_mut().zip(allocations) {
        if amount == 0 {
            continue;
        }
        lot.principal = checked_add(lot.principal, amount)?;
        let updated = sqlx::query("UPDATE bank_lots SET principal_remaining = $1 WHERE id = $2")
            .bind(lot.principal)
            .bind(lot.id)
            .execute(&mut **tx)
            .await?;
        if updated.rows_affected() != 1 {
            return Err(BankInterestError::LotIntegrityMismatch);
        }
    }
    Ok(())
}

fn calculate_daily_interest(
    bank: i64,
    rebirth_count: i64,
    remainder_q32: i64,
) -> Result<(i64, i64), BankInterestError> {
    if bank < 0 || rebirth_count < 0 || remainder_q32 < 0 {
        return Err(BankInterestError::InvalidState);
    }

    let growth_q32 = rebirth_growth_q32(
        u64::try_from(rebirth_count).map_err(|_| BankInterestError::InvalidState)?,
    )?;
    let base = i128::from(bank)
        .checked_mul(i128::from(BANK_BASE_INTEREST_PPM_PER_DAY))
        .and_then(|value| value.checked_mul(Q32_ONE))
        .ok_or(BankInterestError::ArithmeticOverflow)?;
    let bonus_principal = bank.min(BANK_BONUS_PRINCIPAL_TRANCHE);
    let bonus = i128::from(bonus_principal)
        .checked_mul(i128::from(BANK_BONUS_INTEREST_PPM_PER_DAY))
        .and_then(|value| value.checked_mul(growth_q32))
        .ok_or(BankInterestError::ArithmeticOverflow)?;
    let numerator = base
        .checked_add(bonus)
        .and_then(|value| value.checked_add(i128::from(remainder_q32)))
        .ok_or(BankInterestError::ArithmeticOverflow)?;
    let credit = i64::try_from(numerator / INTEREST_DENOMINATOR_Q32)
        .map_err(|_| BankInterestError::ArithmeticOverflow)?;
    let remainder = i64::try_from(numerator % INTEREST_DENOMINATOR_Q32)
        .map_err(|_| BankInterestError::ArithmeticOverflow)?;
    Ok((credit, remainder))
}

fn rebirth_growth_q32(rebirth_count: u64) -> Result<i128, BankInterestError> {
    let decay = q32_pow(REBIRTH_DECAY_Q32, rebirth_count)?;
    Q32_ONE
        .checked_sub(decay)
        .ok_or(BankInterestError::ArithmeticOverflow)
}

fn q32_pow(mut base: i128, mut exponent: u64) -> Result<i128, BankInterestError> {
    let mut result = Q32_ONE;
    while exponent > 0 {
        if exponent & 1 == 1 {
            result = q32_mul(result, base)?;
        }
        exponent >>= 1;
        if exponent > 0 {
            base = q32_mul(base, base)?;
        }
    }
    Ok(result)
}

fn q32_mul(left: i128, right: i128) -> Result<i128, BankInterestError> {
    left.checked_mul(right)
        .and_then(|value| value.checked_add(Q32_ONE / 2))
        .map(|value| value / Q32_ONE)
        .ok_or(BankInterestError::ArithmeticOverflow)
}

async fn insert_interest_operation(
    tx: &mut Transaction<'_, Postgres>,
    player: &InterestPlayer,
    accrual_day: NaiveDate,
    bank_before: i64,
    interest: i64,
    remainder_before: i64,
    remainder_after: i64,
) -> Result<(), BankInterestError> {
    let operation_id = OperationId::new().as_uuid();
    let transaction_id = OperationId::new().as_uuid();
    let external_request_key = format!("system:bank-interest:{}:{accrual_day}", player.player_id);
    let request_hash = interest_request_hash(
        player.player_id,
        accrual_day,
        bank_before,
        player.rebirth_count,
        remainder_before,
    );
    let rng_root = RootSeed::generate();
    let bank_after = checked_add(bank_before, interest)?;
    let result = json!({
        "player_id": player.player_id,
        "accrual_day": accrual_day,
        "interest": interest,
        "bank_before": bank_before,
        "bank_after": bank_after,
        "rebirth_count": player.rebirth_count,
        "remainder_q32": remainder_after,
    });

    let inserted = sqlx::query(
        r#"
        INSERT INTO operations (
            id, external_request_key, actor_discord_user_id, player_id, kind, state,
            policy_version, request_hash, rng_root, result, committed_at
        )
        VALUES ($1, $2, NULL, $3, 'BANK_INTEREST', 'COMMITTED', $4, $5, $6, $7, now())
        ON CONFLICT (external_request_key) DO NOTHING
        "#,
    )
    .bind(operation_id)
    .bind(&external_request_key)
    .bind(player.player_id)
    .bind(BANK_INTEREST_POLICY_VERSION)
    .bind(request_hash.as_slice())
    .bind(rng_root.as_bytes().as_slice())
    .bind(&result)
    .execute(&mut **tx)
    .await?;
    if inserted.rows_affected() != 1 {
        return Err(BankInterestError::OperationConflict);
    }

    sqlx::query(
        "INSERT INTO ledger_transactions (id, operation_id, kind, provenance) VALUES ($1, $2, 'BANK_INTEREST', $3)",
    )
    .bind(transaction_id)
    .bind(operation_id)
    .bind(json!({
        "bank_policy_version": BANK_INTEREST_POLICY_VERSION,
        "accrual_day": accrual_day,
        "base_rate_ppm_per_day": BANK_BASE_INTEREST_PPM_PER_DAY,
        "bonus_rate_max_ppm_per_day": BANK_BONUS_INTEREST_PPM_PER_DAY,
        "bonus_principal_tranche": BANK_BONUS_PRINCIPAL_TRANCHE,
        "rebirth_count": player.rebirth_count,
        "remainder_before_q32": remainder_before,
        "remainder_after_q32": remainder_after,
    }))
    .execute(&mut **tx)
    .await?;

    insert_posting(tx, transaction_id, 0, None, "SYSTEM", -interest).await?;
    insert_posting(
        tx,
        transaction_id,
        1,
        Some(player.player_id),
        "BANK",
        interest,
    )
    .await?;

    sqlx::query(
        "INSERT INTO outbox_events (id, operation_id, topic, payload) VALUES ($1, $2, 'bank.interest_accrued', $3)",
    )
    .bind(OperationId::new().as_uuid())
    .bind(operation_id)
    .bind(result)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn insert_posting(
    tx: &mut Transaction<'_, Postgres>,
    transaction_id: Uuid,
    sequence: i16,
    player_id: Option<Uuid>,
    account_kind: &str,
    amount: i64,
) -> Result<(), BankInterestError> {
    sqlx::query(
        "INSERT INTO ledger_postings (transaction_id, sequence, player_id, account_kind, amount) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(transaction_id)
    .bind(sequence)
    .bind(player_id)
    .bind(account_kind)
    .bind(amount)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn update_interest_state(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
    remainder_q32: i64,
    last_accrual_day: NaiveDate,
) -> Result<(), BankInterestError> {
    let updated = sqlx::query(
        "UPDATE bank_interest_state SET remainder_q32 = $1, last_accrual_day = $2, updated_at = now() WHERE player_id = $3",
    )
    .bind(remainder_q32)
    .bind(last_accrual_day)
    .bind(player_id)
    .execute(&mut **tx)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(BankInterestError::InvalidState);
    }
    Ok(())
}

async fn utc_today(tx: &mut Transaction<'_, Postgres>) -> Result<NaiveDate, BankInterestError> {
    let row = sqlx::query("SELECT (now() AT TIME ZONE 'UTC')::date AS utc_day")
        .fetch_one(&mut **tx)
        .await?;
    Ok(row.try_get("utc_day")?)
}

fn interest_request_hash(
    player_id: Uuid,
    accrual_day: NaiveDate,
    bank_before: i64,
    rebirth_count: i64,
    remainder_before: i64,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"graphite.operation.bank-interest.v1\0");
    hasher.update(player_id.as_bytes());
    hasher.update(&accrual_day.num_days_from_ce().to_be_bytes());
    hasher.update(&bank_before.to_be_bytes());
    hasher.update(&rebirth_count.to_be_bytes());
    hasher.update(&remainder_before.to_be_bytes());
    *hasher.finalize().as_bytes()
}

fn checked_add(left: i64, right: i64) -> Result<i64, BankInterestError> {
    left.checked_add(right)
        .ok_or(BankInterestError::ArithmeticOverflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_balance_remainder_eventually_credits_interest() {
        let mut bank = 500_i64;
        let mut remainder = 0_i64;
        let mut credited = 0_i64;
        for _ in 0..50 {
            let (interest, next_remainder) = calculate_daily_interest(bank, 0, remainder).unwrap();
            bank += interest;
            credited += interest;
            remainder = next_remainder;
        }
        assert_eq!(credited, 1);
        assert_eq!(bank, 501);
    }

    #[test]
    fn rebirth_rate_matches_published_reference_points() {
        let cases = [
            (0_u64, 40.000_f64),
            (1, 40.975),
            (5, 44.424),
            (10, 47.869),
            (20, 52.642),
            (50, 58.358),
            (100, 59.865),
        ];
        for (rebirth, expected_ppm) in cases {
            let growth = rebirth_growth_q32(rebirth).unwrap() as f64 / Q32_ONE as f64;
            let rate = 40.0 + 20.0 * growth;
            assert!((rate - expected_ppm).abs() < 0.002, "R={rebirth}: {rate}");
        }
    }

    #[test]
    fn rebirth_bonus_never_applies_above_ten_million_principal() {
        let (interest, _) = calculate_daily_interest(20_000_000, 10_000, 0).unwrap();
        assert_eq!(interest, 1_000);
    }

    #[test]
    fn allocation_preserves_exact_total_credit() {
        let mut lots = [
            InterestLot {
                id: Uuid::nil(),
                principal: 2,
            },
            InterestLot {
                id: Uuid::from_u128(u128::MAX),
                principal: 1,
            },
        ];
        let interest = 2_i64;
        let denominator = 3_i64;
        let mut allocations = vec![0_i64; lots.len()];
        let mut fractional = Vec::new();
        let mut allocated = 0_i64;
        for (index, lot) in lots.iter().enumerate() {
            let product = i128::from(interest) * i128::from(lot.principal);
            let share = i64::try_from(product / i128::from(denominator)).unwrap();
            allocations[index] = share;
            allocated += share;
            fractional.push((index, product % i128::from(denominator)));
        }
        fractional.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        for (index, _) in fractional
            .into_iter()
            .take(usize::try_from(interest - allocated).unwrap())
        {
            allocations[index] += 1;
        }
        assert_eq!(allocations.iter().sum::<i64>(), interest);
        lots[0].principal += allocations[0];
        lots[1].principal += allocations[1];
        assert_eq!(lots.iter().map(|lot| lot.principal).sum::<i64>(), 5);
    }
}
