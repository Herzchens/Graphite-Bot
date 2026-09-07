use graphite_core::OperationId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

use super::{
    ACCOUNT_XP_CAP, AccountProgress, LevelRewardLedger, ProgressionError, ProgressionMathError,
    account_progress, cumulative_level_reward, insert_level_reward_ledger,
};

const ACCOUNT_XP_SETTLEMENT_MUTATION_KEY: &str = "account_xp";
const ACCOUNT_XP_GRANTED_EVENT_KIND: &str = "ACCOUNT_XP_GRANTED";

#[derive(Clone, Debug, PartialEq)]
pub struct AccountXpSettlementRequest {
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub amount: i64,
    pub source: String,
    pub provenance: Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LockedAccountXpSettlementContext {
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub account_xp: i64,
    pub wallet: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AccountXpSettlementReceipt {
    pub event_id: Uuid,
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub requested_xp: i64,
    pub granted_xp: i64,
    pub source: String,
    pub account_xp_before: i64,
    pub account_xp_after: i64,
    pub level_before: u16,
    pub level_after: u16,
    pub level_money_reward: i64,
    pub wallet_before: i64,
    pub wallet_after: i64,
}

#[derive(Debug, Error)]
pub enum AccountXpSettlementError {
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error(transparent)]
    Math(#[from] ProgressionMathError),
    #[error("Account XP settlement amount must be positive")]
    InvalidAmount,
    #[error("Account XP settlement source must not be empty")]
    InvalidSource,
    #[error("Account XP settlement provenance must be a non-empty JSON object")]
    InvalidProvenance,
    #[error("player does not exist")]
    PlayerNotFound,
    #[error("Account XP settlement requires an ACTIVE account; current status is {0}")]
    AccountFrozen(String),
    #[error("owning operation does not exist")]
    OperationNotFound,
    #[error("owning operation targets a different player")]
    OperationPlayerMismatch,
    #[error("owning operation actor does not match the target player")]
    OperationActorMismatch,
    #[error("owning operation cannot accept a new Account XP settlement in state {0}")]
    OperationTerminal(String),
    #[error("the owning operation already contains a different Account XP settlement")]
    MutationConflict,
    #[error("the owning operation already owns monetary ledger transaction kind {0}")]
    MonetaryLedgerConflict(String),
    #[error("authoritative player balance state is missing")]
    BalanceStateMissing,
    #[error("authoritative player progression state is missing")]
    ProgressionStateMissing,
    #[error("stored Account XP settlement payload is invalid: {0}")]
    InvalidStoredMutation(Box<serde_json::Error>),
    #[error("shared Account Level reward settlement failed: {0}")]
    LevelRewardSettlement(Box<ProgressionError>),
}

impl From<sqlx::Error> for AccountXpSettlementError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct AccountXpSettlementPayload {
    receipt: AccountXpSettlementReceipt,
    provenance: Value,
}

pub(super) struct AccountXpGrantProjection {
    pub(super) before: AccountProgress,
    pub(super) after: AccountProgress,
    pub(super) granted_xp: i64,
    pub(super) account_xp_after: i64,
    pub(super) level_money_reward: i64,
    pub(super) wallet_after: i64,
}

pub(super) enum AccountXpGrantProjectionError {
    Math(ProgressionMathError),
    InvalidState,
}

struct LockedOperation {
    actor_discord_user_id: i64,
    player_id: Option<Uuid>,
    state: String,
}

struct LockedAccountXpState {
    account_xp: i64,
    wallet: i64,
}

/// Projects one canonical Account XP grant from already-authoritative progression and Wallet state.
///
/// Both the dedicated `ProgressionService::grant_account_xp` lifecycle and transaction-composable
/// child settlement reuse this function so cap handling, level transitions, and synchronous Account
/// Level Money reward calculation have one arithmetic owner. Operation creation/replay, row locking,
/// persistence, ledger/outbox ownership, and commit semantics remain lifecycle-specific.
pub(super) fn project_account_xp_grant(
    account_xp_before: i64,
    wallet_before: i64,
    requested_xp: i64,
) -> Result<AccountXpGrantProjection, AccountXpGrantProjectionError> {
    let before =
        account_progress(account_xp_before).map_err(AccountXpGrantProjectionError::Math)?;
    let remaining = ACCOUNT_XP_CAP
        .checked_sub(account_xp_before)
        .ok_or(AccountXpGrantProjectionError::InvalidState)?;
    let granted_xp = requested_xp.min(remaining);
    let account_xp_after =
        account_xp_before
            .checked_add(granted_xp)
            .ok_or(AccountXpGrantProjectionError::Math(
                ProgressionMathError::ArithmeticOverflow,
            ))?;
    let after = account_progress(account_xp_after).map_err(AccountXpGrantProjectionError::Math)?;
    let level_money_reward = cumulative_level_reward(after.level)
        .checked_sub(cumulative_level_reward(before.level))
        .ok_or(AccountXpGrantProjectionError::InvalidState)?;
    let wallet_after = wallet_before.checked_add(level_money_reward).ok_or(
        AccountXpGrantProjectionError::Math(ProgressionMathError::ArithmeticOverflow),
    )?;

    Ok(AccountXpGrantProjection {
        before,
        after,
        granted_xp,
        account_xp_after,
        level_money_reward,
        wallet_after,
    })
}

/// Acquires the canonical locks required before an owning gameplay/service lifecycle enters item
/// state and later settles its single Account XP grant.
///
/// Lock order is `operation -> player -> balance -> progression`. Account XP can cross an Account
/// Level boundary and synchronously mint the frozen Account Level Money reward, so balance ownership
/// must be acquired before a later item/equipment lock even when the caller does not yet know whether
/// this particular grant will level up. The current ledger schema permits one monetary ledger
/// transaction per operation; this prelock therefore fails closed if the operation already owns a
/// monetary ledger row instead of risking a later Account Level reward that cannot be recorded.
///
/// The returned values are snapshots only. PostgreSQL retains the authoritative locks on the caller's
/// transaction until commit/rollback. The higher-level lifecycle must still call
/// [`apply_account_xp_settlement`] in the same transaction after it has resolved whether the action
/// qualifies for Account XP. Committed-operation replay remains the owning lifecycle's responsibility.
pub async fn lock_account_xp_settlement_context(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
) -> Result<LockedAccountXpSettlementContext, AccountXpSettlementError> {
    let operation = lock_operation(tx, operation_id).await?;
    ensure_operation_player(&operation, player_id)?;
    if operation.state != "PENDING" {
        return Err(AccountXpSettlementError::OperationTerminal(operation.state));
    }
    if existing_account_xp_event(tx, operation_id).await?.is_some() {
        return Err(AccountXpSettlementError::MutationConflict);
    }
    if let Some(kind) = existing_monetary_ledger_kind(tx, operation_id).await? {
        return Err(AccountXpSettlementError::MonetaryLedgerConflict(kind));
    }

    let state = lock_account_xp_state(tx, &operation, player_id).await?;
    Ok(LockedAccountXpSettlementContext {
        operation_id,
        player_id,
        account_xp: state.account_xp,
        wallet: state.wallet,
    })
}

/// Settles the single already-authoritative Account XP grant owned by one broader operation.
///
/// This function does not create, finalize, commit, or emit an outbox event for the owning operation.
/// It atomically updates Account XP, pays any crossed Account Level Money reward through the existing
/// canonical `LEVEL_REWARD` ledger helper, and appends one immutable keyed progression event. A stable
/// per-operation mutation key makes exact re-entry return the original receipt without applying XP or
/// Money twice. Different input under that key fails closed.
///
/// The current one-ledger-row-per-operation schema is intentionally enforced here. An owning lifecycle
/// that also needs another Money leg must first provide a composed monetary-ledger authority; this
/// primitive will not silently suppress a mandatory Account Level reward or create a second ledger row.
/// Callers that will enter mutable item/service state before the final Account XP amount is known must
/// invoke [`lock_account_xp_settlement_context`] first to preserve balance-before-item lock ordering.
pub async fn apply_account_xp_settlement(
    tx: &mut Transaction<'_, Postgres>,
    request: &AccountXpSettlementRequest,
) -> Result<AccountXpSettlementReceipt, AccountXpSettlementError> {
    validate_request(request)?;

    let operation = lock_operation(tx, request.operation_id).await?;
    ensure_operation_player(&operation, request.player_id)?;

    if let Some(row) = existing_account_xp_event(tx, request.operation_id).await? {
        return replay_account_xp_settlement(row, request);
    }
    if operation.state != "PENDING" {
        return Err(AccountXpSettlementError::OperationTerminal(operation.state));
    }
    if let Some(kind) = existing_monetary_ledger_kind(tx, request.operation_id).await? {
        return Err(AccountXpSettlementError::MonetaryLedgerConflict(kind));
    }

    let state = lock_account_xp_state(tx, &operation, request.player_id).await?;
    let projection = project_account_xp_grant(state.account_xp, state.wallet, request.amount)
        .map_err(|error| match error {
            AccountXpGrantProjectionError::Math(error) => AccountXpSettlementError::Math(error),
            AccountXpGrantProjectionError::InvalidState => {
                AccountXpSettlementError::Math(ProgressionMathError::ArithmeticOverflow)
            }
        })?;

    if projection.granted_xp > 0 {
        let updated = sqlx::query(
            "UPDATE player_progression SET account_xp = $1, updated_at = now() WHERE player_id = $2",
        )
        .bind(projection.account_xp_after)
        .bind(request.player_id)
        .execute(&mut **tx)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(AccountXpSettlementError::ProgressionStateMissing);
        }
    }

    if projection.level_money_reward > 0 {
        let updated = sqlx::query(
            "UPDATE player_balances SET wallet = $1, updated_at = now() WHERE player_id = $2",
        )
        .bind(projection.wallet_after)
        .bind(request.player_id)
        .execute(&mut **tx)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(AccountXpSettlementError::BalanceStateMissing);
        }

        let ledger = LevelRewardLedger {
            source: &request.source,
            level_before: projection.before.level,
            level_after: projection.after.level,
            granted_xp: projection.granted_xp,
            reward: projection.level_money_reward,
        };
        insert_level_reward_ledger(tx, request.operation_id, request.player_id, &ledger)
            .await
            .map_err(|error| AccountXpSettlementError::LevelRewardSettlement(Box::new(error)))?;
    }

    let receipt = AccountXpSettlementReceipt {
        event_id: OperationId::new().as_uuid(),
        operation_id: request.operation_id,
        player_id: request.player_id,
        requested_xp: request.amount,
        granted_xp: projection.granted_xp,
        source: request.source.clone(),
        account_xp_before: state.account_xp,
        account_xp_after: projection.account_xp_after,
        level_before: projection.before.level,
        level_after: projection.after.level,
        level_money_reward: projection.level_money_reward,
        wallet_before: state.wallet,
        wallet_after: projection.wallet_after,
    };
    let payload = AccountXpSettlementPayload {
        receipt: receipt.clone(),
        provenance: request.provenance.clone(),
    };
    let payload = serde_json::to_value(payload)
        .map_err(|error| AccountXpSettlementError::InvalidStoredMutation(Box::new(error)))?;

    sqlx::query(
        r#"
        INSERT INTO progression_events (
            id, operation_id, mutation_key, player_id, event_kind, payload
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(receipt.event_id)
    .bind(request.operation_id)
    .bind(ACCOUNT_XP_SETTLEMENT_MUTATION_KEY)
    .bind(request.player_id)
    .bind(ACCOUNT_XP_GRANTED_EVENT_KIND)
    .bind(payload)
    .execute(&mut **tx)
    .await?;

    Ok(receipt)
}

fn validate_request(request: &AccountXpSettlementRequest) -> Result<(), AccountXpSettlementError> {
    if request.amount <= 0 {
        return Err(AccountXpSettlementError::InvalidAmount);
    }
    if request.source.trim().is_empty() {
        return Err(AccountXpSettlementError::InvalidSource);
    }
    let Value::Object(fields) = &request.provenance else {
        return Err(AccountXpSettlementError::InvalidProvenance);
    };
    if fields.is_empty() {
        return Err(AccountXpSettlementError::InvalidProvenance);
    }
    Ok(())
}

async fn lock_operation(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
) -> Result<LockedOperation, AccountXpSettlementError> {
    let row = sqlx::query(
        "SELECT actor_discord_user_id, player_id, state FROM operations WHERE id = $1 FOR UPDATE",
    )
    .bind(operation_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(AccountXpSettlementError::OperationNotFound)?;

    Ok(LockedOperation {
        actor_discord_user_id: row.try_get("actor_discord_user_id")?,
        player_id: row.try_get("player_id")?,
        state: row.try_get("state")?,
    })
}

fn ensure_operation_player(
    operation: &LockedOperation,
    player_id: Uuid,
) -> Result<(), AccountXpSettlementError> {
    if operation
        .player_id
        .is_some_and(|stored| stored != player_id)
    {
        return Err(AccountXpSettlementError::OperationPlayerMismatch);
    }
    Ok(())
}

async fn lock_account_xp_state(
    tx: &mut Transaction<'_, Postgres>,
    operation: &LockedOperation,
    player_id: Uuid,
) -> Result<LockedAccountXpState, AccountXpSettlementError> {
    let player = sqlx::query(
        "SELECT discord_user_id, status FROM players WHERE id = $1 AND status <> 'DELETED' FOR UPDATE",
    )
    .bind(player_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(AccountXpSettlementError::PlayerNotFound)?;
    let discord_user_id: i64 = player.try_get("discord_user_id")?;
    if discord_user_id != operation.actor_discord_user_id {
        return Err(AccountXpSettlementError::OperationActorMismatch);
    }
    let status: String = player.try_get("status")?;
    if status != "ACTIVE" {
        return Err(AccountXpSettlementError::AccountFrozen(status));
    }

    let wallet = sqlx::query_scalar::<_, i64>(
        "SELECT wallet FROM player_balances WHERE player_id = $1 FOR UPDATE",
    )
    .bind(player_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(AccountXpSettlementError::BalanceStateMissing)?;

    let account_xp = sqlx::query_scalar::<_, i64>(
        "SELECT account_xp FROM player_progression WHERE player_id = $1 FOR UPDATE",
    )
    .bind(player_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(AccountXpSettlementError::ProgressionStateMissing)?;

    Ok(LockedAccountXpState { account_xp, wallet })
}

async fn existing_account_xp_event(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
) -> Result<Option<sqlx::postgres::PgRow>, AccountXpSettlementError> {
    Ok(sqlx::query(
        r#"
        SELECT id, player_id, event_kind, payload
          FROM progression_events
         WHERE operation_id = $1
           AND mutation_key = $2
        "#,
    )
    .bind(operation_id)
    .bind(ACCOUNT_XP_SETTLEMENT_MUTATION_KEY)
    .fetch_optional(&mut **tx)
    .await?)
}

async fn existing_monetary_ledger_kind(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
) -> Result<Option<String>, AccountXpSettlementError> {
    Ok(sqlx::query_scalar::<_, String>(
        "SELECT kind FROM ledger_transactions WHERE operation_id = $1",
    )
    .bind(operation_id)
    .fetch_optional(&mut **tx)
    .await?)
}

fn replay_account_xp_settlement(
    row: sqlx::postgres::PgRow,
    request: &AccountXpSettlementRequest,
) -> Result<AccountXpSettlementReceipt, AccountXpSettlementError> {
    let stored_event_id: Uuid = row.try_get("id")?;
    let stored_player_id: Uuid = row.try_get("player_id")?;
    let stored_event_kind: String = row.try_get("event_kind")?;
    let payload: Value = row.try_get("payload")?;
    let payload: AccountXpSettlementPayload = serde_json::from_value(payload)
        .map_err(|error| AccountXpSettlementError::InvalidStoredMutation(Box::new(error)))?;

    if stored_event_id != payload.receipt.event_id
        || stored_player_id != request.player_id
        || stored_event_kind != ACCOUNT_XP_GRANTED_EVENT_KIND
        || payload.receipt.operation_id != request.operation_id
        || payload.receipt.player_id != request.player_id
        || payload.receipt.requested_xp != request.amount
        || payload.receipt.source != request.source
        || payload.provenance != request.provenance
    {
        return Err(AccountXpSettlementError::MutationConflict);
    }

    Ok(payload.receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request() -> AccountXpSettlementRequest {
        AccountXpSettlementRequest {
            operation_id: Uuid::nil(),
            player_id: Uuid::nil(),
            amount: 2,
            source: "MANUAL_FISHING".to_owned(),
            provenance: json!({"origin":"unit_test"}),
        }
    }

    #[test]
    fn request_requires_positive_amount_source_and_provenance() {
        let mut input = request();
        input.amount = 0;
        assert!(matches!(
            validate_request(&input),
            Err(AccountXpSettlementError::InvalidAmount)
        ));

        let mut input = request();
        input.source.clear();
        assert!(matches!(
            validate_request(&input),
            Err(AccountXpSettlementError::InvalidSource)
        ));

        let mut input = request();
        input.provenance = json!({});
        assert!(matches!(
            validate_request(&input),
            Err(AccountXpSettlementError::InvalidProvenance)
        ));
    }
}
