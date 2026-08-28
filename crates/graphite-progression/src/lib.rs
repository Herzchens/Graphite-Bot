use blake3::Hasher;
use graphite_core::{OperationId, RootSeed};
use graphite_economy::{BankInterestError, BankInterestService};
use graphite_store::PgStore;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

const PROGRESSION_POLICY_VERSION: i32 = 1;
pub const ACCOUNT_MAX_LEVEL: u16 = 200;
pub const ACCOUNT_XP_CAP: i64 = 172_370;
pub const ACCOUNT_LEVEL_REWARD_TOTAL: i64 = 1_609_400;
pub const REBIRTH_AEXP_BONUS_MAX_PPM: i64 = 300_000;
pub const REBIRTH_REPAIR_REDUCTION_MAX_PPM: i64 = 100_000;
const Q32_ONE: i128 = 1_i128 << 32;
const REBIRTH_AEXP_DECAY_Q32: i128 = 4_085_499_269;
const REBIRTH_REPAIR_DECAY_Q32: i128 = 4_126_559_220;
const ACCOUNT_XP_THRESHOLDS: [i64; 201] = build_account_xp_thresholds();
const ACCOUNT_CUMULATIVE_LEVEL_REWARDS: [i64; 201] = build_account_level_rewards();

#[derive(Clone)]
pub struct ProgressionService {
    store: PgStore,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AccountProgress {
    pub total_xp: i64,
    pub level: u16,
    pub xp_into_level: i64,
    pub xp_to_next: Option<i64>,
    pub at_cap: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ActivityProgress {
    pub points: i64,
    pub level: u64,
    pub points_into_level: i64,
    pub points_for_next_level: i64,
    pub points_remaining: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProgressionSnapshot {
    pub player_id: Uuid,
    pub rebirth_count: u64,
    pub account: AccountProgress,
    pub activity: ActivityProgress,
    pub rebirth_aexp_bonus_ppm: i64,
    pub rebirth_repair_reduction_ppm: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AccountXpGrantReceipt {
    pub operation_id: Uuid,
    pub requested_xp: i64,
    pub granted_xp: i64,
    pub source: String,
    pub account_xp_before: i64,
    pub account_xp_after: i64,
    pub level_before: u16,
    pub level_after: u16,
    pub level_money_reward: i64,
    pub wallet_after: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RebirthReceipt {
    pub operation_id: Uuid,
    pub previous_rebirth_count: u64,
    pub rebirth_count: u64,
    pub activity_xp_points: i64,
    pub activity_level: u64,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ProgressionMathError {
    #[error("Account XP is outside the canonical Rebirth-cycle range")]
    InvalidAccountXp,
    #[error("Activity EXP cannot be negative")]
    InvalidActivityXp,
    #[error("progression arithmetic exceeded the supported integer range")]
    ArithmeticOverflow,
}

#[derive(Debug, Error)]
pub enum ProgressionError {
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error("Bank interest settlement failed before Rebirth: {0}")]
    BankInterest(Box<BankInterestError>),
    #[error(transparent)]
    Math(#[from] ProgressionMathError),
    #[error("Discord snowflake is outside the signed BIGINT persistence range")]
    SnowflakeOutOfRange,
    #[error("no active Graphite account exists")]
    PlayerNotFound,
    #[error("account progression mutation requires an ACTIVE account; current status is {0}")]
    AccountFrozen(String),
    #[error("Account XP grant must be positive")]
    InvalidXpAmount,
    #[error("progression source must not be empty")]
    InvalidSource,
    #[error("idempotency key was reused with a different progression request")]
    IdempotencyConflict,
    #[error("progression operation is already terminal in state {0}")]
    OperationTerminal(String),
    #[error("progression operation disappeared after idempotent insert")]
    OperationMissingAfterInsert,
    #[error("stored progression operation result is invalid: {0}")]
    InvalidOperationResult(Box<serde_json::Error>),
    #[error("authoritative progression state is invalid")]
    InvalidProgressionState,
    #[error("Rebirth requires Account Level 200")]
    RebirthRequiresLevelCap,
    #[error("progression operation could not be committed exactly once")]
    OperationCommitConflict,
}

impl From<sqlx::Error> for ProgressionError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

#[derive(Clone, Copy)]
enum MutationKind {
    AccountXpGrant,
    Rebirth,
}

impl MutationKind {
    const fn operation_kind(self) -> &'static str {
        match self {
            Self::AccountXpGrant => "ACCOUNT_XP_GRANT",
            Self::Rebirth => "REBIRTH",
        }
    }
}

enum OperationResolution {
    Pending(Uuid),
    Committed(Value),
}

struct LockedProgression {
    player_id: Uuid,
    status: String,
    rebirth_count: i64,
    account_xp: i64,
    activity_xp_points: i64,
    wallet: i64,
}

impl ProgressionService {
    #[must_use]
    pub fn new(store: PgStore) -> Self {
        Self { store }
    }

    pub async fn snapshot(
        &self,
        discord_user_id: u64,
    ) -> Result<ProgressionSnapshot, ProgressionError> {
        let discord_user_id = snowflake_to_i64(discord_user_id)?;
        let row = sqlx::query(
            r#"
            SELECT p.id, p.rebirth_count, g.account_xp, g.activity_xp_points
              FROM players p
              JOIN player_progression g ON g.player_id = p.id
             WHERE p.discord_user_id = $1
               AND p.status <> 'DELETED'
            "#,
        )
        .bind(discord_user_id)
        .fetch_optional(self.store.pool())
        .await?
        .ok_or(ProgressionError::PlayerNotFound)?;

        snapshot_from_values(
            row.try_get("id")?,
            row.try_get("rebirth_count")?,
            row.try_get("account_xp")?,
            row.try_get("activity_xp_points")?,
        )
    }

    pub async fn grant_account_xp(
        &self,
        discord_user_id: u64,
        amount: i64,
        source: &str,
        external_request_key: &str,
    ) -> Result<AccountXpGrantReceipt, ProgressionError> {
        if amount <= 0 {
            return Err(ProgressionError::InvalidXpAmount);
        }
        if source.trim().is_empty() {
            return Err(ProgressionError::InvalidSource);
        }

        let discord_user_id = snowflake_to_i64(discord_user_id)?;
        let kind = MutationKind::AccountXpGrant;
        let request_hash = account_xp_request_hash(amount, source);
        let mut tx = self.store.pool().begin().await?;
        let operation_id = match resolve_operation(
            &mut tx,
            discord_user_id,
            external_request_key,
            kind,
            &request_hash,
        )
        .await?
        {
            OperationResolution::Committed(value) => {
                let receipt = serde_json::from_value(value)
                    .map_err(|error| ProgressionError::InvalidOperationResult(Box::new(error)))?;
                tx.commit().await?;
                return Ok(receipt);
            }
            OperationResolution::Pending(operation_id) => operation_id,
        };

        let player = lock_progression(&mut tx, discord_user_id).await?;
        ensure_mutable(&player.status)?;
        let before = account_progress(player.account_xp)?;
        let remaining = ACCOUNT_XP_CAP
            .checked_sub(player.account_xp)
            .ok_or(ProgressionError::InvalidProgressionState)?;
        let granted_xp = amount.min(remaining);
        let account_xp_after =
            player
                .account_xp
                .checked_add(granted_xp)
                .ok_or(ProgressionError::Math(
                    ProgressionMathError::ArithmeticOverflow,
                ))?;
        let after = account_progress(account_xp_after)?;
        let level_money_reward = cumulative_level_reward(after.level)
            .checked_sub(cumulative_level_reward(before.level))
            .ok_or(ProgressionError::InvalidProgressionState)?;
        let wallet_after =
            player
                .wallet
                .checked_add(level_money_reward)
                .ok_or(ProgressionError::Math(
                    ProgressionMathError::ArithmeticOverflow,
                ))?;

        if granted_xp > 0 {
            sqlx::query(
                "UPDATE player_progression SET account_xp = $1, updated_at = now() WHERE player_id = $2",
            )
            .bind(account_xp_after)
            .bind(player.player_id)
            .execute(&mut *tx)
            .await?;
        }
        if level_money_reward > 0 {
            sqlx::query(
                "UPDATE player_balances SET wallet = $1, updated_at = now() WHERE player_id = $2",
            )
            .bind(wallet_after)
            .bind(player.player_id)
            .execute(&mut *tx)
            .await?;
            let ledger = LevelRewardLedger {
                source,
                level_before: before.level,
                level_after: after.level,
                granted_xp,
                reward: level_money_reward,
            };
            insert_level_reward_ledger(&mut tx, operation_id, player.player_id, &ledger).await?;
        }

        let receipt = AccountXpGrantReceipt {
            operation_id,
            requested_xp: amount,
            granted_xp,
            source: source.to_owned(),
            account_xp_before: player.account_xp,
            account_xp_after,
            level_before: before.level,
            level_after: after.level,
            level_money_reward,
            wallet_after,
        };
        insert_progression_event(
            &mut tx,
            operation_id,
            player.player_id,
            "ACCOUNT_XP_GRANTED",
            json!({
                "source": source,
                "requested_xp": amount,
                "granted_xp": granted_xp,
                "account_xp_before": player.account_xp,
                "account_xp_after": account_xp_after,
                "level_before": before.level,
                "level_after": after.level,
                "level_money_reward": level_money_reward,
            }),
        )
        .await?;
        commit_operation(&mut tx, operation_id, player.player_id, &receipt).await?;
        insert_outbox(
            &mut tx,
            operation_id,
            "progression.account_xp_granted",
            &receipt,
        )
        .await?;
        tx.commit().await?;
        Ok(receipt)
    }

    pub async fn rebirth(
        &self,
        discord_user_id: u64,
        external_request_key: &str,
    ) -> Result<RebirthReceipt, ProgressionError> {
        BankInterestService::new(self.store.clone())
            .accrue_interest(discord_user_id)
            .await
            .map_err(|error| ProgressionError::BankInterest(Box::new(error)))?;

        let discord_user_id = snowflake_to_i64(discord_user_id)?;
        let kind = MutationKind::Rebirth;
        let request_hash = rebirth_request_hash();
        let mut tx = self.store.pool().begin().await?;
        let operation_id = match resolve_operation(
            &mut tx,
            discord_user_id,
            external_request_key,
            kind,
            &request_hash,
        )
        .await?
        {
            OperationResolution::Committed(value) => {
                let receipt = serde_json::from_value(value)
                    .map_err(|error| ProgressionError::InvalidOperationResult(Box::new(error)))?;
                tx.commit().await?;
                return Ok(receipt);
            }
            OperationResolution::Pending(operation_id) => operation_id,
        };

        let player = lock_progression(&mut tx, discord_user_id).await?;
        ensure_mutable(&player.status)?;
        if account_level(player.account_xp)? != ACCOUNT_MAX_LEVEL {
            return Err(ProgressionError::RebirthRequiresLevelCap);
        }
        let previous_rebirth_count = u64::try_from(player.rebirth_count)
            .map_err(|_| ProgressionError::InvalidProgressionState)?;
        let rebirth_count = previous_rebirth_count
            .checked_add(1)
            .ok_or(ProgressionError::Math(
                ProgressionMathError::ArithmeticOverflow,
            ))?;
        let rebirth_count_db = i64::try_from(rebirth_count)
            .map_err(|_| ProgressionError::Math(ProgressionMathError::ArithmeticOverflow))?;
        let activity = activity_progress(player.activity_xp_points)?;

        sqlx::query("UPDATE players SET rebirth_count = $1 WHERE id = $2")
            .bind(rebirth_count_db)
            .bind(player.player_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE player_progression SET account_xp = 0, updated_at = now() WHERE player_id = $1",
        )
        .bind(player.player_id)
        .execute(&mut *tx)
        .await?;

        let receipt = RebirthReceipt {
            operation_id,
            previous_rebirth_count,
            rebirth_count,
            activity_xp_points: player.activity_xp_points,
            activity_level: activity.level,
        };
        insert_progression_event(
            &mut tx,
            operation_id,
            player.player_id,
            "REBIRTH",
            json!({
                "previous_rebirth_count": previous_rebirth_count,
                "rebirth_count": rebirth_count,
                "account_xp_before": player.account_xp,
                "account_xp_after": 0,
                "activity_xp_points": player.activity_xp_points,
                "activity_level": activity.level,
            }),
        )
        .await?;
        commit_operation(&mut tx, operation_id, player.player_id, &receipt).await?;
        insert_outbox(&mut tx, operation_id, "progression.rebirth", &receipt).await?;
        tx.commit().await?;
        Ok(receipt)
    }
}

#[must_use]
pub fn account_xp_to_next(level: u16) -> Option<i64> {
    if (1..ACCOUNT_MAX_LEVEL).contains(&level) {
        Some(account_xp_to_next_const(level))
    } else {
        None
    }
}

#[must_use]
pub fn account_total_xp_for_level(level: u16) -> Option<i64> {
    if (1..=ACCOUNT_MAX_LEVEL).contains(&level) {
        Some(ACCOUNT_XP_THRESHOLDS[usize::from(level)])
    } else {
        None
    }
}

#[must_use]
pub fn level_money_reward(level: u16) -> Option<i64> {
    if (2..=ACCOUNT_MAX_LEVEL).contains(&level) {
        Some(level_money_reward_const(level))
    } else {
        None
    }
}

pub fn account_level(account_xp: i64) -> Result<u16, ProgressionMathError> {
    if !(0..=ACCOUNT_XP_CAP).contains(&account_xp) {
        return Err(ProgressionMathError::InvalidAccountXp);
    }
    let count = ACCOUNT_XP_THRESHOLDS[1..].partition_point(|threshold| *threshold <= account_xp);
    u16::try_from(count).map_err(|_| ProgressionMathError::ArithmeticOverflow)
}

pub fn account_progress(account_xp: i64) -> Result<AccountProgress, ProgressionMathError> {
    let level = account_level(account_xp)?;
    let level_start = ACCOUNT_XP_THRESHOLDS[usize::from(level)];
    let at_cap = level == ACCOUNT_MAX_LEVEL;
    Ok(AccountProgress {
        total_xp: account_xp,
        level,
        xp_into_level: account_xp
            .checked_sub(level_start)
            .ok_or(ProgressionMathError::ArithmeticOverflow)?,
        xp_to_next: account_xp_to_next(level),
        at_cap,
    })
}

pub fn activity_progress(points: i64) -> Result<ActivityProgress, ProgressionMathError> {
    if points < 0 {
        return Err(ProgressionMathError::InvalidActivityXp);
    }
    let target = i128::from(points);
    let mut low = 0_u64;
    let mut high = 1_u64;
    while activity_total_i128(high)? <= target {
        low = high;
        high = high
            .checked_mul(2)
            .ok_or(ProgressionMathError::ArithmeticOverflow)?;
    }
    while low + 1 < high {
        let middle = low + (high - low) / 2;
        if activity_total_i128(middle)? <= target {
            low = middle;
        } else {
            high = middle;
        }
    }

    let level_start = i64::try_from(activity_total_i128(low)?)
        .map_err(|_| ProgressionMathError::ArithmeticOverflow)?;
    let points_for_next_level = activity_xp_to_next(low)?;
    let points_into_level = points
        .checked_sub(level_start)
        .ok_or(ProgressionMathError::ArithmeticOverflow)?;
    let points_remaining = points_for_next_level
        .checked_sub(points_into_level)
        .ok_or(ProgressionMathError::ArithmeticOverflow)?;
    Ok(ActivityProgress {
        points,
        level: low,
        points_into_level,
        points_for_next_level,
        points_remaining,
    })
}

pub fn activity_total_xp_for_level(level: u64) -> Result<i64, ProgressionMathError> {
    i64::try_from(activity_total_i128(level)?).map_err(|_| ProgressionMathError::ArithmeticOverflow)
}

pub fn activity_xp_to_next(level: u64) -> Result<i64, ProgressionMathError> {
    let level = i128::from(level);
    let value = if level <= 15 {
        level.checked_mul(2).and_then(|value| value.checked_add(7))
    } else if level <= 30 {
        level.checked_mul(5).and_then(|value| value.checked_sub(38))
    } else {
        level
            .checked_mul(9)
            .and_then(|value| value.checked_sub(158))
    }
    .ok_or(ProgressionMathError::ArithmeticOverflow)?;
    i64::try_from(value).map_err(|_| ProgressionMathError::ArithmeticOverflow)
}

pub fn rebirth_aexp_bonus_ppm(rebirth_count: u64) -> Result<i64, ProgressionMathError> {
    rebirth_effect_ppm(
        rebirth_count,
        REBIRTH_AEXP_BONUS_MAX_PPM,
        REBIRTH_AEXP_DECAY_Q32,
    )
}

pub fn rebirth_repair_reduction_ppm(rebirth_count: u64) -> Result<i64, ProgressionMathError> {
    rebirth_effect_ppm(
        rebirth_count,
        REBIRTH_REPAIR_REDUCTION_MAX_PPM,
        REBIRTH_REPAIR_DECAY_Q32,
    )
}

fn snapshot_from_values(
    player_id: Uuid,
    rebirth_count: i64,
    account_xp: i64,
    activity_xp_points: i64,
) -> Result<ProgressionSnapshot, ProgressionError> {
    let rebirth_count =
        u64::try_from(rebirth_count).map_err(|_| ProgressionError::InvalidProgressionState)?;
    Ok(ProgressionSnapshot {
        player_id,
        rebirth_count,
        account: account_progress(account_xp)?,
        activity: activity_progress(activity_xp_points)?,
        rebirth_aexp_bonus_ppm: rebirth_aexp_bonus_ppm(rebirth_count)?,
        rebirth_repair_reduction_ppm: rebirth_repair_reduction_ppm(rebirth_count)?,
    })
}

async fn lock_progression(
    tx: &mut Transaction<'_, Postgres>,
    discord_user_id: i64,
) -> Result<LockedProgression, ProgressionError> {
    let row = sqlx::query(
        r#"
        SELECT p.id,
               p.status,
               p.rebirth_count,
               g.account_xp,
               g.activity_xp_points,
               b.wallet
          FROM players p
          JOIN player_progression g ON g.player_id = p.id
          JOIN player_balances b ON b.player_id = p.id
         WHERE p.discord_user_id = $1
           AND p.status <> 'DELETED'
         FOR UPDATE OF p, g, b
        "#,
    )
    .bind(discord_user_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(ProgressionError::PlayerNotFound)?;

    Ok(LockedProgression {
        player_id: row.try_get("id")?,
        status: row.try_get("status")?,
        rebirth_count: row.try_get("rebirth_count")?,
        account_xp: row.try_get("account_xp")?,
        activity_xp_points: row.try_get("activity_xp_points")?,
        wallet: row.try_get("wallet")?,
    })
}

fn ensure_mutable(status: &str) -> Result<(), ProgressionError> {
    if status == "ACTIVE" {
        Ok(())
    } else {
        Err(ProgressionError::AccountFrozen(status.to_owned()))
    }
}

async fn resolve_operation(
    tx: &mut Transaction<'_, Postgres>,
    discord_user_id: i64,
    external_request_key: &str,
    kind: MutationKind,
    request_hash: &[u8; 32],
) -> Result<OperationResolution, ProgressionError> {
    if let Some(row) = select_operation(tx, external_request_key).await? {
        return validate_operation_row(row, discord_user_id, kind, request_hash);
    }

    let operation_id = OperationId::new().as_uuid();
    let rng_root = RootSeed::generate();
    sqlx::query(
        r#"
        INSERT INTO operations (
            id, external_request_key, actor_discord_user_id, kind, state,
            policy_version, request_hash, rng_root
        )
        VALUES ($1, $2, $3, $4, 'PENDING', $5, $6, $7)
        ON CONFLICT (external_request_key) DO NOTHING
        "#,
    )
    .bind(operation_id)
    .bind(external_request_key)
    .bind(discord_user_id)
    .bind(kind.operation_kind())
    .bind(PROGRESSION_POLICY_VERSION)
    .bind(request_hash.as_slice())
    .bind(rng_root.as_bytes().as_slice())
    .execute(&mut **tx)
    .await?;

    let row = select_operation(tx, external_request_key)
        .await?
        .ok_or(ProgressionError::OperationMissingAfterInsert)?;
    validate_operation_row(row, discord_user_id, kind, request_hash)
}

async fn select_operation(
    tx: &mut Transaction<'_, Postgres>,
    external_request_key: &str,
) -> Result<Option<sqlx::postgres::PgRow>, ProgressionError> {
    Ok(sqlx::query(
        r#"
        SELECT id, actor_discord_user_id, kind, state, policy_version, request_hash, result
          FROM operations
         WHERE external_request_key = $1
         FOR UPDATE
        "#,
    )
    .bind(external_request_key)
    .fetch_optional(&mut **tx)
    .await?)
}

fn validate_operation_row(
    row: sqlx::postgres::PgRow,
    discord_user_id: i64,
    kind: MutationKind,
    request_hash: &[u8; 32],
) -> Result<OperationResolution, ProgressionError> {
    let stored_actor: Option<i64> = row.try_get("actor_discord_user_id")?;
    let stored_kind: String = row.try_get("kind")?;
    let stored_policy: i32 = row.try_get("policy_version")?;
    let stored_hash: Vec<u8> = row.try_get("request_hash")?;
    if stored_actor != Some(discord_user_id)
        || stored_kind != kind.operation_kind()
        || stored_policy != PROGRESSION_POLICY_VERSION
        || stored_hash.as_slice() != request_hash.as_slice()
    {
        return Err(ProgressionError::IdempotencyConflict);
    }

    let state: String = row.try_get("state")?;
    if state == "COMMITTED" {
        return Ok(OperationResolution::Committed(row.try_get("result")?));
    }
    if state != "PENDING" {
        return Err(ProgressionError::OperationTerminal(state));
    }
    Ok(OperationResolution::Pending(row.try_get("id")?))
}

async fn commit_operation<T: Serialize>(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
    receipt: &T,
) -> Result<(), ProgressionError> {
    let result = serde_json::to_value(receipt)
        .map_err(|error| ProgressionError::InvalidOperationResult(Box::new(error)))?;
    let updated = sqlx::query(
        r#"
        UPDATE operations
           SET player_id = $1,
               state = 'COMMITTED',
               result = $2,
               committed_at = now()
         WHERE id = $3
           AND state = 'PENDING'
        "#,
    )
    .bind(player_id)
    .bind(result)
    .bind(operation_id)
    .execute(&mut **tx)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(ProgressionError::OperationCommitConflict);
    }
    Ok(())
}

async fn insert_progression_event(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
    event_kind: &str,
    payload: Value,
) -> Result<(), ProgressionError> {
    sqlx::query(
        r#"
        INSERT INTO progression_events (id, operation_id, player_id, event_kind, payload)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(OperationId::new().as_uuid())
    .bind(operation_id)
    .bind(player_id)
    .bind(event_kind)
    .bind(payload)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn insert_outbox<T: Serialize>(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    topic: &str,
    receipt: &T,
) -> Result<(), ProgressionError> {
    let payload = serde_json::to_value(receipt)
        .map_err(|error| ProgressionError::InvalidOperationResult(Box::new(error)))?;
    sqlx::query(
        r#"
        INSERT INTO outbox_events (id, operation_id, topic, payload)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(OperationId::new().as_uuid())
    .bind(operation_id)
    .bind(topic)
    .bind(payload)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

struct LevelRewardLedger<'a> {
    source: &'a str,
    level_before: u16,
    level_after: u16,
    granted_xp: i64,
    reward: i64,
}

async fn insert_level_reward_ledger(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
    ledger: &LevelRewardLedger<'_>,
) -> Result<(), ProgressionError> {
    let transaction_id = OperationId::new().as_uuid();
    sqlx::query(
        r#"
        INSERT INTO ledger_transactions (id, operation_id, kind, provenance)
        VALUES ($1, $2, 'LEVEL_REWARD', $3)
        "#,
    )
    .bind(transaction_id)
    .bind(operation_id)
    .bind(json!({
        "source": ledger.source,
        "level_before": ledger.level_before,
        "level_after": ledger.level_after,
        "account_xp_granted": ledger.granted_xp,
        "reward": ledger.reward,
    }))
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO ledger_postings (
            transaction_id, sequence, player_id, account_kind, amount, metadata
        )
        VALUES
            ($1, 0, $2, 'WALLET', $3, '{}'::jsonb),
            ($1, 1, NULL, 'SYSTEM', $4, '{"reason":"account_level_reward"}'::jsonb)
        "#,
    )
    .bind(transaction_id)
    .bind(player_id)
    .bind(ledger.reward)
    .bind(ledger.reward.checked_neg().ok_or(ProgressionError::Math(
        ProgressionMathError::ArithmeticOverflow,
    ))?)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn account_xp_request_hash(amount: i64, source: &str) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(b"graphite/progression/v1/account-xp-grant\0");
    hasher.update(&amount.to_be_bytes());
    hasher.update(&(source.len() as u64).to_be_bytes());
    hasher.update(source.as_bytes());
    *hasher.finalize().as_bytes()
}

fn rebirth_request_hash() -> [u8; 32] {
    *blake3::hash(b"graphite/progression/v1/rebirth").as_bytes()
}

fn snowflake_to_i64(value: u64) -> Result<i64, ProgressionError> {
    i64::try_from(value).map_err(|_| ProgressionError::SnowflakeOutOfRange)
}

const fn account_xp_to_next_const(level: u16) -> i64 {
    let level = level as i64;
    let numerator = 5_000 + 250 * level + level * level;
    ((numerator + 250) / 500) * 10
}

const fn level_money_reward_const(level: u16) -> i64 {
    let value = 500 + 75 * level as i64;
    ((value + 50) / 100) * 100
}

const fn build_account_xp_thresholds() -> [i64; 201] {
    let mut thresholds = [0_i64; 201];
    let mut level = 1_u16;
    let mut total = 0_i64;
    while level < ACCOUNT_MAX_LEVEL {
        total += account_xp_to_next_const(level);
        thresholds[(level + 1) as usize] = total;
        level += 1;
    }
    thresholds
}

const fn build_account_level_rewards() -> [i64; 201] {
    let mut rewards = [0_i64; 201];
    let mut level = 2_u16;
    let mut total = 0_i64;
    while level <= ACCOUNT_MAX_LEVEL {
        total += level_money_reward_const(level);
        rewards[level as usize] = total;
        level += 1;
    }
    rewards
}

fn cumulative_level_reward(level: u16) -> i64 {
    ACCOUNT_CUMULATIVE_LEVEL_REWARDS[usize::from(level)]
}

fn activity_total_i128(level: u64) -> Result<i128, ProgressionMathError> {
    let level = i128::from(level);
    let square = level
        .checked_mul(level)
        .ok_or(ProgressionMathError::ArithmeticOverflow)?;
    let numerator = if level <= 16 {
        square
            .checked_add(
                level
                    .checked_mul(6)
                    .ok_or(ProgressionMathError::ArithmeticOverflow)?,
            )
            .ok_or(ProgressionMathError::ArithmeticOverflow)?
            .checked_mul(2)
            .ok_or(ProgressionMathError::ArithmeticOverflow)?
    } else if level <= 31 {
        square
            .checked_mul(5)
            .and_then(|value| value.checked_sub(level * 81))
            .and_then(|value| value.checked_add(720))
            .ok_or(ProgressionMathError::ArithmeticOverflow)?
    } else {
        square
            .checked_mul(9)
            .and_then(|value| value.checked_sub(level * 325))
            .and_then(|value| value.checked_add(4_440))
            .ok_or(ProgressionMathError::ArithmeticOverflow)?
    };
    Ok(numerator / 2)
}

fn rebirth_effect_ppm(
    rebirth_count: u64,
    maximum_ppm: i64,
    decay_q32: i128,
) -> Result<i64, ProgressionMathError> {
    let decay = q32_pow(decay_q32, rebirth_count)?;
    let growth = Q32_ONE
        .checked_sub(decay)
        .ok_or(ProgressionMathError::ArithmeticOverflow)?;
    let numerator = i128::from(maximum_ppm)
        .checked_mul(growth)
        .and_then(|value| value.checked_add(Q32_ONE / 2))
        .ok_or(ProgressionMathError::ArithmeticOverflow)?;
    i64::try_from(numerator / Q32_ONE).map_err(|_| ProgressionMathError::ArithmeticOverflow)
}

fn q32_pow(mut base: i128, mut exponent: u64) -> Result<i128, ProgressionMathError> {
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

fn q32_mul(left: i128, right: i128) -> Result<i128, ProgressionMathError> {
    left.checked_mul(right)
        .and_then(|value| value.checked_add(Q32_ONE / 2))
        .map(|value| value / Q32_ONE)
        .ok_or(ProgressionMathError::ArithmeticOverflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_curve_matches_frozen_totals() {
        assert_eq!(
            account_total_xp_for_level(ACCOUNT_MAX_LEVEL),
            Some(ACCOUNT_XP_CAP)
        );
        assert_eq!(
            cumulative_level_reward(ACCOUNT_MAX_LEVEL),
            ACCOUNT_LEVEL_REWARD_TOTAL
        );
        assert_eq!(account_xp_to_next(1), Some(110));
        assert_eq!(account_xp_to_next(100), Some(800));
        assert_eq!(account_xp_to_next(199), Some(1_890));
        assert_eq!(level_money_reward(2), Some(700));
        assert_eq!(level_money_reward(200), Some(15_500));
        assert_eq!(account_level(ACCOUNT_XP_CAP).unwrap(), 200);
    }

    #[test]
    fn activity_curve_matches_minecraft_boundaries() {
        for (level, total) in [
            (0, 0),
            (1, 7),
            (16, 352),
            (17, 394),
            (31, 1_507),
            (32, 1_628),
        ] {
            assert_eq!(activity_total_xp_for_level(level).unwrap(), total);
            assert_eq!(activity_progress(total).unwrap().level, level);
        }
        let progress = activity_progress(1_600).unwrap();
        assert_eq!(progress.level, 31);
        assert_eq!(progress.points_into_level, 93);
        assert_eq!(progress.points_for_next_level, 121);
        assert_eq!(progress.points_remaining, 28);
    }

    #[test]
    fn rebirth_utility_is_monotone_and_bounded() {
        let checkpoints = [0_u64, 1, 5, 10, 20, 30, 50, 100, 1_000];
        let mut previous_aexp = -1_i64;
        let mut previous_repair = -1_i64;
        for rebirths in checkpoints {
            let aexp = rebirth_aexp_bonus_ppm(rebirths).unwrap();
            let repair = rebirth_repair_reduction_ppm(rebirths).unwrap();
            assert!(aexp >= previous_aexp);
            assert!(repair >= previous_repair);
            assert!(aexp <= REBIRTH_AEXP_BONUS_MAX_PPM);
            assert!(repair <= REBIRTH_REPAIR_REDUCTION_MAX_PPM);
            previous_aexp = aexp;
            previous_repair = repair;
        }
        assert!((rebirth_aexp_bonus_ppm(1).unwrap() - 14_631).abs() <= 2);
        assert!((rebirth_aexp_bonus_ppm(20).unwrap() - 189_636).abs() <= 3);
        assert!((rebirth_repair_reduction_ppm(20).unwrap() - 55_067).abs() <= 3);
    }
}
