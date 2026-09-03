use graphite_core::OperationId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

const WALLET_SPEND_POLICY_VERSION: i32 = 1;
const WALLET_SPEND_LEDGER_KIND: &str = "WALLET_SPEND";

#[derive(Clone, Debug, PartialEq)]
pub struct WalletSpendRequest {
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub amount: i64,
    pub source: String,
    pub provenance: Value,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WalletSpendReceipt {
    pub ledger_transaction_id: Uuid,
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub amount: i64,
    pub source: String,
    pub wallet_before: i64,
    pub wallet_after: i64,
}

#[derive(Debug, Error)]
pub enum WalletSpendError {
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error("Wallet spend amount must be a positive integer")]
    InvalidAmount,
    #[error("Wallet spend source must not be empty")]
    InvalidSource,
    #[error("Wallet spend provenance must be a non-empty JSON object")]
    InvalidProvenance,
    #[error("player does not exist")]
    PlayerNotFound,
    #[error("Wallet spending requires an ACTIVE account; current status is {0}")]
    AccountFrozen(String),
    #[error("owning operation does not exist")]
    OperationNotFound,
    #[error("owning operation targets a different player")]
    OperationPlayerMismatch,
    #[error("owning operation cannot accept a new Wallet spend in state {0}")]
    OperationTerminal(String),
    #[error("Wallet has {available} Money but {requested} is required")]
    InsufficientWallet { available: i64, requested: i64 },
    #[error("the owning operation already has a different monetary ledger mutation")]
    MutationConflict,
    #[error("stored Wallet spend ledger state is invalid: {0}")]
    InvalidStoredSpend(String),
    #[error("Wallet arithmetic exceeded the supported persistence range")]
    ArithmeticOverflow,
}

impl From<sqlx::Error> for WalletSpendError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct WalletSpendLedgerProvenance {
    wallet_spend_policy_version: i32,
    source: String,
    request_provenance: Value,
    receipt: WalletSpendReceipt,
}

/// Atomically settles one already-resolved Wallet-only Money sink inside an owning
/// gameplay/service transaction.
///
/// The caller must resolve and own `request.operation_id` first. Lock order is
/// `operation -> player/balance`; later item/service locks may follow in the same
/// transaction. The operation remains `PENDING` and outbox settlement remains the
/// responsibility of the owning lifecycle.
///
/// This primitive deliberately never auto-pulls from Bank. Graphite only permits a
/// Bank pull when the owning action explicitly authorizes it, and that pull must use
/// the canonical fee-aware withdrawal path. A Wallet-only service cost therefore
/// fails closed with [`WalletSpendError::InsufficientWallet`] instead of silently
/// inventing Bank-spend semantics.
///
/// The current ledger schema permits exactly one `ledger_transactions` row per
/// operation. This function therefore owns that operation's monetary ledger row and
/// is replay-idempotent by `operation_id`. A future lifecycle with multiple monetary
/// legs must compose those legs into one balanced ledger transaction (or first
/// evidence and migrate a different schema) rather than calling this primitive more
/// than once with different input.
pub async fn apply_wallet_spend(
    tx: &mut Transaction<'_, Postgres>,
    request: &WalletSpendRequest,
) -> Result<WalletSpendReceipt, WalletSpendError> {
    validate_request(request)?;

    let operation = sqlx::query("SELECT player_id, state FROM operations WHERE id = $1 FOR UPDATE")
        .bind(request.operation_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(WalletSpendError::OperationNotFound)?;
    let operation_player_id: Option<Uuid> = operation.try_get("player_id")?;
    if operation_player_id.is_some_and(|stored| stored != request.player_id) {
        return Err(WalletSpendError::OperationPlayerMismatch);
    }
    let operation_state: String = operation.try_get("state")?;

    if let Some(row) = sqlx::query(
        r#"
        SELECT id, kind, provenance
          FROM ledger_transactions
         WHERE operation_id = $1
         FOR UPDATE
        "#,
    )
    .bind(request.operation_id)
    .fetch_optional(&mut **tx)
    .await?
    {
        return replay_wallet_spend(tx, row, request).await;
    }

    if operation_state != "PENDING" {
        return Err(WalletSpendError::OperationTerminal(operation_state));
    }

    let player = sqlx::query(
        r#"
        SELECT p.status, b.wallet
          FROM players p
          JOIN player_balances b ON b.player_id = p.id
         WHERE p.id = $1
           AND p.status <> 'DELETED'
         FOR UPDATE OF p, b
        "#,
    )
    .bind(request.player_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(WalletSpendError::PlayerNotFound)?;
    let status: String = player.try_get("status")?;
    if status != "ACTIVE" {
        return Err(WalletSpendError::AccountFrozen(status));
    }

    let wallet_before: i64 = player.try_get("wallet")?;
    if wallet_before < request.amount {
        return Err(WalletSpendError::InsufficientWallet {
            available: wallet_before,
            requested: request.amount,
        });
    }
    let wallet_after = wallet_before
        .checked_sub(request.amount)
        .ok_or(WalletSpendError::ArithmeticOverflow)?;

    let balance_update = sqlx::query(
        "UPDATE player_balances SET wallet = $1, updated_at = now() WHERE player_id = $2",
    )
    .bind(wallet_after)
    .bind(request.player_id)
    .execute(&mut **tx)
    .await?;
    if balance_update.rows_affected() != 1 {
        return Err(WalletSpendError::PlayerNotFound);
    }

    let receipt = WalletSpendReceipt {
        ledger_transaction_id: OperationId::new().as_uuid(),
        operation_id: request.operation_id,
        player_id: request.player_id,
        amount: request.amount,
        source: request.source.clone(),
        wallet_before,
        wallet_after,
    };
    let ledger_provenance = WalletSpendLedgerProvenance {
        wallet_spend_policy_version: WALLET_SPEND_POLICY_VERSION,
        source: request.source.clone(),
        request_provenance: request.provenance.clone(),
        receipt: receipt.clone(),
    };
    let ledger_provenance = serde_json::to_value(ledger_provenance).map_err(|error| {
        WalletSpendError::InvalidStoredSpend(format!("failed to serialize provenance: {error}"))
    })?;

    sqlx::query(
        "INSERT INTO ledger_transactions (id, operation_id, kind, provenance) VALUES ($1, $2, $3, $4)",
    )
    .bind(receipt.ledger_transaction_id)
    .bind(request.operation_id)
    .bind(WALLET_SPEND_LEDGER_KIND)
    .bind(ledger_provenance)
    .execute(&mut **tx)
    .await?;

    insert_posting(
        tx,
        receipt.ledger_transaction_id,
        0,
        Some(request.player_id),
        "WALLET",
        -request.amount,
    )
    .await?;
    insert_posting(
        tx,
        receipt.ledger_transaction_id,
        1,
        None,
        "SYSTEM",
        request.amount,
    )
    .await?;

    Ok(receipt)
}

fn validate_request(request: &WalletSpendRequest) -> Result<(), WalletSpendError> {
    if request.amount <= 0 {
        return Err(WalletSpendError::InvalidAmount);
    }
    if request.source.trim().is_empty() {
        return Err(WalletSpendError::InvalidSource);
    }
    let Value::Object(fields) = &request.provenance else {
        return Err(WalletSpendError::InvalidProvenance);
    };
    if fields.is_empty() {
        return Err(WalletSpendError::InvalidProvenance);
    }
    Ok(())
}

async fn replay_wallet_spend(
    tx: &mut Transaction<'_, Postgres>,
    row: sqlx::postgres::PgRow,
    request: &WalletSpendRequest,
) -> Result<WalletSpendReceipt, WalletSpendError> {
    let stored_transaction_id: Uuid = row.try_get("id")?;
    let stored_kind: String = row.try_get("kind")?;
    if stored_kind != WALLET_SPEND_LEDGER_KIND {
        return Err(WalletSpendError::MutationConflict);
    }

    let provenance: Value = row.try_get("provenance")?;
    let stored: WalletSpendLedgerProvenance =
        serde_json::from_value(provenance).map_err(|error| {
            WalletSpendError::InvalidStoredSpend(format!("invalid provenance payload: {error}"))
        })?;
    if stored.wallet_spend_policy_version != WALLET_SPEND_POLICY_VERSION
        || stored_transaction_id != stored.receipt.ledger_transaction_id
        || stored.receipt.operation_id != request.operation_id
        || stored.receipt.player_id != request.player_id
        || stored.receipt.amount != request.amount
        || stored.receipt.source != request.source
        || stored.source != request.source
        || stored.request_provenance != request.provenance
    {
        return Err(WalletSpendError::MutationConflict);
    }
    if stored.receipt.wallet_before < stored.receipt.amount
        || stored.receipt.wallet_after
            != stored
                .receipt
                .wallet_before
                .checked_sub(stored.receipt.amount)
                .ok_or(WalletSpendError::ArithmeticOverflow)?
    {
        return Err(WalletSpendError::InvalidStoredSpend(
            "receipt Wallet arithmetic does not reconcile".to_owned(),
        ));
    }

    validate_postings(tx, &stored.receipt).await?;
    Ok(stored.receipt)
}

async fn validate_postings(
    tx: &mut Transaction<'_, Postgres>,
    receipt: &WalletSpendReceipt,
) -> Result<(), WalletSpendError> {
    let rows = sqlx::query(
        r#"
        SELECT sequence, player_id, account_kind, amount
          FROM ledger_postings
         WHERE transaction_id = $1
         ORDER BY sequence ASC
        "#,
    )
    .bind(receipt.ledger_transaction_id)
    .fetch_all(&mut **tx)
    .await?;
    if rows.len() != 2 {
        return Err(WalletSpendError::InvalidStoredSpend(
            "expected exactly two ledger postings".to_owned(),
        ));
    }

    let debit_sequence: i16 = rows[0].try_get("sequence")?;
    let debit_player: Option<Uuid> = rows[0].try_get("player_id")?;
    let debit_account: String = rows[0].try_get("account_kind")?;
    let debit_amount: i64 = rows[0].try_get("amount")?;
    if debit_sequence != 0
        || debit_player != Some(receipt.player_id)
        || debit_account != "WALLET"
        || debit_amount != -receipt.amount
    {
        return Err(WalletSpendError::InvalidStoredSpend(
            "Wallet debit posting does not match the stored receipt".to_owned(),
        ));
    }

    let sink_sequence: i16 = rows[1].try_get("sequence")?;
    let sink_player: Option<Uuid> = rows[1].try_get("player_id")?;
    let sink_account: String = rows[1].try_get("account_kind")?;
    let sink_amount: i64 = rows[1].try_get("amount")?;
    if sink_sequence != 1
        || sink_player.is_some()
        || sink_account != "SYSTEM"
        || sink_amount != receipt.amount
    {
        return Err(WalletSpendError::InvalidStoredSpend(
            "system sink posting does not match the stored receipt".to_owned(),
        ));
    }

    Ok(())
}

async fn insert_posting(
    tx: &mut Transaction<'_, Postgres>,
    transaction_id: Uuid,
    sequence: i16,
    player_id: Option<Uuid>,
    account_kind: &str,
    amount: i64,
) -> Result<(), WalletSpendError> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request() -> WalletSpendRequest {
        WalletSpendRequest {
            operation_id: Uuid::nil(),
            player_id: Uuid::nil(),
            amount: 1,
            source: "TEST_SERVICE_COST".to_owned(),
            provenance: json!({"origin":"unit_test"}),
        }
    }

    #[test]
    fn request_requires_positive_amount_source_and_provenance() {
        let mut input = request();
        input.amount = 0;
        assert!(matches!(
            validate_request(&input),
            Err(WalletSpendError::InvalidAmount)
        ));

        let mut input = request();
        input.source.clear();
        assert!(matches!(
            validate_request(&input),
            Err(WalletSpendError::InvalidSource)
        ));

        let mut input = request();
        input.provenance = json!({});
        assert!(matches!(
            validate_request(&input),
            Err(WalletSpendError::InvalidProvenance)
        ));
    }
}
