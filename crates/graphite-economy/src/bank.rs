use chrono::{DateTime, Utc};
use graphite_core::{OperationId, RootSeed};
use graphite_store::PgStore;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::fees::{BANK_MIN_WITHDRAWAL, FeeLot, withdrawal_fee};

const BANK_POLICY_VERSION: i32 = 1;

#[derive(Clone)]
pub struct BankService {
    store: PgStore,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BankMutationKind {
    Deposit,
    Withdraw,
}

impl BankMutationKind {
    const fn operation_kind(self) -> &'static str {
        match self {
            Self::Deposit => "BANK_DEPOSIT",
            Self::Withdraw => "BANK_WITHDRAW",
        }
    }

    const fn outbox_topic(self) -> &'static str {
        match self {
            Self::Deposit => "bank.deposited",
            Self::Withdraw => "bank.withdrawn",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BankSnapshot {
    pub player_id: Uuid,
    pub wallet: i64,
    pub bank: i64,
    pub liability: i64,
    pub active_lot_count: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BankMutationReceipt {
    pub operation_id: Uuid,
    pub kind: BankMutationKind,
    pub gross_amount: i64,
    pub fee_amount: i64,
    pub net_amount: i64,
    pub wallet: i64,
    pub bank: i64,
}

#[derive(Debug, Error)]
pub enum BankError {
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error("stored bank operation result is invalid: {0}")]
    InvalidOperationResult(Box<serde_json::Error>),
    #[error("Discord snowflake is outside the signed BIGINT persistence range")]
    SnowflakeOutOfRange,
    #[error("bank amount must be a positive integer")]
    InvalidAmount,
    #[error("normal Bank withdrawals must be at least {BANK_MIN_WITHDRAWAL} Money")]
    BelowMinimumWithdrawal,
    #[error("no active Graphite account exists")]
    PlayerNotFound,
    #[error("bank mutations are blocked while account status is {0}")]
    AccountFrozen(String),
    #[error("Wallet has {available} Money but {requested} is required")]
    InsufficientWallet { available: i64, requested: i64 },
    #[error("Bank has {available} Money but {requested} is required")]
    InsufficientBank { available: i64, requested: i64 },
    #[error("bank lot principal does not reconcile with the materialized Bank balance")]
    LotIntegrityMismatch,
    #[error("idempotency key was reused with different bank input")]
    IdempotencyConflict,
    #[error("bank operation is in terminal state {0}")]
    OperationTerminal(String),
    #[error("bank operation disappeared after insert-or-conflict resolution")]
    OperationMissingAfterInsert,
    #[error("bank arithmetic exceeded the supported persistence range")]
    ArithmeticOverflow,
    #[error("calculated withdrawal fee would consume the entire withdrawal")]
    InvalidFee,
}

impl From<sqlx::Error> for BankError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

struct LockedPlayer {
    player_id: Uuid,
    status: String,
    wallet: i64,
    bank: i64,
}

struct LotDebit {
    id: Uuid,
    amount: i64,
    deposited_at: DateTime<Utc>,
}

impl BankService {
    #[must_use]
    pub fn new(store: PgStore) -> Self {
        Self { store }
    }

    pub async fn snapshot(&self, discord_user_id: u64) -> Result<BankSnapshot, BankError> {
        let discord_user_id = snowflake_to_i64(discord_user_id)?;
        let row = sqlx::query(
            r#"
            SELECT p.id,
                   b.wallet,
                   b.bank,
                   b.liability,
                   COUNT(bl.id) FILTER (WHERE bl.principal_remaining > 0) AS active_lot_count
              FROM players p
              JOIN player_balances b ON b.player_id = p.id
              LEFT JOIN bank_lots bl ON bl.player_id = p.id
             WHERE p.discord_user_id = $1
               AND p.status <> 'DELETED'
             GROUP BY p.id, b.wallet, b.bank, b.liability
            "#,
        )
        .bind(discord_user_id)
        .fetch_optional(self.store.pool())
        .await?
        .ok_or(BankError::PlayerNotFound)?;

        let lot_count: i64 = row.try_get("active_lot_count")?;
        Ok(BankSnapshot {
            player_id: row.try_get("id")?,
            wallet: row.try_get("wallet")?,
            bank: row.try_get("bank")?,
            liability: row.try_get("liability")?,
            active_lot_count: u32::try_from(lot_count)
                .map_err(|_| BankError::ArithmeticOverflow)?,
        })
    }

    pub async fn deposit(
        &self,
        discord_user_id: u64,
        amount: i64,
        external_request_key: &str,
    ) -> Result<BankMutationReceipt, BankError> {
        validate_deposit_amount(amount)?;

        let discord_user_id = snowflake_to_i64(discord_user_id)?;
        let kind = BankMutationKind::Deposit;
        let request_hash = bank_request_hash(kind, amount);
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
            OperationResolution::Committed(receipt) => {
                tx.commit().await?;
                return Ok(receipt);
            }
            OperationResolution::Pending(operation_id) => operation_id,
        };

        let player = lock_player(&mut tx, discord_user_id).await?;
        ensure_mutable(&player.status)?;
        if player.wallet < amount {
            return Err(BankError::InsufficientWallet {
                available: player.wallet,
                requested: amount,
            });
        }

        let wallet = checked_sub(player.wallet, amount)?;
        let bank = checked_add(player.bank, amount)?;
        update_balances(&mut tx, player.player_id, wallet, bank).await?;
        create_bank_lot(&mut tx, player.player_id, operation_id, amount).await?;
        insert_ledger(&mut tx, operation_id, player.player_id, kind, amount, 0).await?;

        let receipt = BankMutationReceipt {
            operation_id,
            kind,
            gross_amount: amount,
            fee_amount: 0,
            net_amount: amount,
            wallet,
            bank,
        };
        commit_operation(&mut tx, player.player_id, &receipt).await?;
        insert_outbox(&mut tx, &receipt).await?;
        tx.commit().await?;
        Ok(receipt)
    }

    pub async fn withdraw(
        &self,
        discord_user_id: u64,
        amount: i64,
        external_request_key: &str,
    ) -> Result<BankMutationReceipt, BankError> {
        validate_withdraw_amount(amount)?;

        let discord_user_id = snowflake_to_i64(discord_user_id)?;
        let kind = BankMutationKind::Withdraw;
        let request_hash = bank_request_hash(kind, amount);
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
            OperationResolution::Committed(receipt) => {
                tx.commit().await?;
                return Ok(receipt);
            }
            OperationResolution::Pending(operation_id) => operation_id,
        };

        let player = lock_player(&mut tx, discord_user_id).await?;
        ensure_mutable(&player.status)?;
        if player.bank < amount {
            return Err(BankError::InsufficientBank {
                available: player.bank,
                requested: amount,
            });
        }

        let now = transaction_now(&mut tx).await?;
        let debits = load_fifo_lot_debits(&mut tx, player.player_id, amount).await?;
        let fee_lots = debits
            .iter()
            .map(|debit| FeeLot {
                amount: debit.amount,
                deposited_at: debit.deposited_at.to_owned(),
            })
            .collect::<Vec<_>>();
        let prior_24h_gross = rolling_24h_gross(&mut tx, player.player_id).await?;
        let fee_amount = withdrawal_fee(amount, player.bank, prior_24h_gross, &fee_lots, now)?;
        let net_amount = checked_sub(amount, fee_amount)?;
        if net_amount <= 0 {
            return Err(BankError::InvalidFee);
        }

        apply_lot_debits(&mut tx, &debits).await?;
        let bank = checked_sub(player.bank, amount)?;
        let wallet = checked_add(player.wallet, net_amount)?;
        update_balances(&mut tx, player.player_id, wallet, bank).await?;
        insert_withdrawal_audit(&mut tx, operation_id, player.player_id, amount, fee_amount)
            .await?;
        insert_ledger(
            &mut tx,
            operation_id,
            player.player_id,
            kind,
            amount,
            fee_amount,
        )
        .await?;

        let receipt = BankMutationReceipt {
            operation_id,
            kind,
            gross_amount: amount,
            fee_amount,
            net_amount,
            wallet,
            bank,
        };
        commit_operation(&mut tx, player.player_id, &receipt).await?;
        insert_outbox(&mut tx, &receipt).await?;
        tx.commit().await?;
        Ok(receipt)
    }
}

fn validate_deposit_amount(amount: i64) -> Result<(), BankError> {
    if amount <= 0 {
        Err(BankError::InvalidAmount)
    } else {
        Ok(())
    }
}

fn validate_withdraw_amount(amount: i64) -> Result<(), BankError> {
    validate_deposit_amount(amount)?;
    if amount < BANK_MIN_WITHDRAWAL {
        Err(BankError::BelowMinimumWithdrawal)
    } else {
        Ok(())
    }
}

enum OperationResolution {
    Pending(Uuid),
    Committed(BankMutationReceipt),
}

async fn resolve_operation(
    tx: &mut Transaction<'_, Postgres>,
    discord_user_id: i64,
    external_request_key: &str,
    kind: BankMutationKind,
    request_hash: &[u8; 32],
) -> Result<OperationResolution, BankError> {
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
    .bind(BANK_POLICY_VERSION)
    .bind(request_hash.as_slice())
    .bind(rng_root.as_bytes().as_slice())
    .execute(&mut **tx)
    .await?;

    let row = select_operation(tx, external_request_key)
        .await?
        .ok_or(BankError::OperationMissingAfterInsert)?;
    validate_operation_row(row, discord_user_id, kind, request_hash)
}

async fn select_operation(
    tx: &mut Transaction<'_, Postgres>,
    external_request_key: &str,
) -> Result<Option<sqlx::postgres::PgRow>, BankError> {
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
    kind: BankMutationKind,
    request_hash: &[u8; 32],
) -> Result<OperationResolution, BankError> {
    let stored_actor: i64 = row.try_get("actor_discord_user_id")?;
    let stored_kind: String = row.try_get("kind")?;
    let stored_policy: i32 = row.try_get("policy_version")?;
    let stored_request_hash: Vec<u8> = row.try_get("request_hash")?;
    if stored_actor != discord_user_id
        || stored_kind != kind.operation_kind()
        || stored_policy != BANK_POLICY_VERSION
        || stored_request_hash.as_slice() != request_hash.as_slice()
    {
        return Err(BankError::IdempotencyConflict);
    }

    let state: String = row.try_get("state")?;
    if state == "COMMITTED" {
        let value: serde_json::Value = row.try_get("result")?;
        let receipt = serde_json::from_value(value)
            .map_err(|error| BankError::InvalidOperationResult(Box::new(error)))?;
        return Ok(OperationResolution::Committed(receipt));
    }
    if state != "PENDING" {
        return Err(BankError::OperationTerminal(state));
    }
    Ok(OperationResolution::Pending(row.try_get("id")?))
}

async fn lock_player(
    tx: &mut Transaction<'_, Postgres>,
    discord_user_id: i64,
) -> Result<LockedPlayer, BankError> {
    let row = sqlx::query(
        r#"
        SELECT p.id, p.status, b.wallet, b.bank
          FROM players p
          JOIN player_balances b ON b.player_id = p.id
         WHERE p.discord_user_id = $1
           AND p.status <> 'DELETED'
         FOR UPDATE OF p, b
        "#,
    )
    .bind(discord_user_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(BankError::PlayerNotFound)?;

    Ok(LockedPlayer {
        player_id: row.try_get("id")?,
        status: row.try_get("status")?,
        wallet: row.try_get("wallet")?,
        bank: row.try_get("bank")?,
    })
}

fn ensure_mutable(status: &str) -> Result<(), BankError> {
    if status == "ACTIVE" {
        Ok(())
    } else {
        Err(BankError::AccountFrozen(status.to_owned()))
    }
}

async fn create_bank_lot(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
    operation_id: Uuid,
    amount: i64,
) -> Result<(), BankError> {
    sqlx::query(
        r#"
        INSERT INTO bank_lots (
            id, player_id, principal_remaining, interest_remainder,
            deposited_at, created_by_operation_id
        )
        VALUES ($1, $2, $3, 0, now(), $4)
        "#,
    )
    .bind(OperationId::new().as_uuid())
    .bind(player_id)
    .bind(amount)
    .bind(operation_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn load_fifo_lot_debits(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
    amount: i64,
) -> Result<Vec<LotDebit>, BankError> {
    let rows = sqlx::query(
        r#"
        SELECT id, principal_remaining, deposited_at
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

    let mut remaining = amount;
    let mut debits = Vec::new();
    for row in rows {
        if remaining == 0 {
            break;
        }
        let principal: i64 = row.try_get("principal_remaining")?;
        let take = principal.min(remaining);
        if take > 0 {
            debits.push(LotDebit {
                id: row.try_get("id")?,
                amount: take,
                deposited_at: row.try_get("deposited_at")?,
            });
            remaining -= take;
        }
    }

    if remaining != 0 {
        return Err(BankError::LotIntegrityMismatch);
    }
    Ok(debits)
}

async fn apply_lot_debits(
    tx: &mut Transaction<'_, Postgres>,
    debits: &[LotDebit],
) -> Result<(), BankError> {
    for debit in debits {
        let result = sqlx::query(
            "UPDATE bank_lots SET principal_remaining = principal_remaining - $1 WHERE id = $2 AND principal_remaining >= $1",
        )
        .bind(debit.amount)
        .bind(debit.id)
        .execute(&mut **tx)
        .await?;
        if result.rows_affected() != 1 {
            return Err(BankError::LotIntegrityMismatch);
        }
    }
    Ok(())
}

async fn transaction_now(tx: &mut Transaction<'_, Postgres>) -> Result<DateTime<Utc>, BankError> {
    let row = sqlx::query("SELECT now() AS now")
        .fetch_one(&mut **tx)
        .await?;
    Ok(row.try_get("now")?)
}

async fn rolling_24h_gross(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
) -> Result<i128, BankError> {
    let row = sqlx::query(
        r#"
        SELECT COALESCE(SUM(gross_amount::NUMERIC), 0)::TEXT AS gross
          FROM bank_withdrawals
         WHERE player_id = $1
           AND created_at >= now() - interval '24 hours'
        "#,
    )
    .bind(player_id)
    .fetch_one(&mut **tx)
    .await?;
    let gross: String = row.try_get("gross")?;
    gross
        .parse::<i128>()
        .map_err(|_| BankError::ArithmeticOverflow)
}

async fn update_balances(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
    wallet: i64,
    bank: i64,
) -> Result<(), BankError> {
    sqlx::query(
        "UPDATE player_balances SET wallet = $1, bank = $2, updated_at = now() WHERE player_id = $3",
    )
    .bind(wallet)
    .bind(bank)
    .bind(player_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn insert_withdrawal_audit(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
    gross_amount: i64,
    fee_amount: i64,
) -> Result<(), BankError> {
    sqlx::query(
        "INSERT INTO bank_withdrawals (operation_id, player_id, gross_amount, fee_amount) VALUES ($1, $2, $3, $4)",
    )
    .bind(operation_id)
    .bind(player_id)
    .bind(gross_amount)
    .bind(fee_amount)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn insert_ledger(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
    kind: BankMutationKind,
    gross_amount: i64,
    fee_amount: i64,
) -> Result<(), BankError> {
    let transaction_id = OperationId::new().as_uuid();
    sqlx::query(
        "INSERT INTO ledger_transactions (id, operation_id, kind, provenance) VALUES ($1, $2, $3, $4)",
    )
    .bind(transaction_id)
    .bind(operation_id)
    .bind(kind.operation_kind())
    .bind(json!({ "bank_policy_version": BANK_POLICY_VERSION }))
    .execute(&mut **tx)
    .await?;

    match kind {
        BankMutationKind::Deposit => {
            insert_posting(
                tx,
                transaction_id,
                0,
                Some(player_id),
                "WALLET",
                -gross_amount,
            )
            .await?;
            insert_posting(tx, transaction_id, 1, Some(player_id), "BANK", gross_amount).await?;
        }
        BankMutationKind::Withdraw => {
            let net_amount = checked_sub(gross_amount, fee_amount)?;
            insert_posting(
                tx,
                transaction_id,
                0,
                Some(player_id),
                "BANK",
                -gross_amount,
            )
            .await?;
            insert_posting(tx, transaction_id, 1, Some(player_id), "WALLET", net_amount).await?;
            if fee_amount > 0 {
                insert_posting(tx, transaction_id, 2, None, "SYSTEM", fee_amount).await?;
            }
        }
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
) -> Result<(), BankError> {
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

async fn commit_operation(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
    receipt: &BankMutationReceipt,
) -> Result<(), BankError> {
    let result = serde_json::to_value(receipt).expect("bank receipt is serializable");
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
    .bind(receipt.operation_id)
    .execute(&mut **tx)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(BankError::OperationTerminal(
            "unexpected transition".to_owned(),
        ));
    }
    Ok(())
}

async fn insert_outbox(
    tx: &mut Transaction<'_, Postgres>,
    receipt: &BankMutationReceipt,
) -> Result<(), BankError> {
    sqlx::query(
        "INSERT INTO outbox_events (id, operation_id, topic, payload) VALUES ($1, $2, $3, $4) ON CONFLICT (operation_id, topic) DO NOTHING",
    )
    .bind(OperationId::new().as_uuid())
    .bind(receipt.operation_id)
    .bind(receipt.kind.outbox_topic())
    .bind(json!({
        "gross_amount": receipt.gross_amount,
        "fee_amount": receipt.fee_amount,
        "net_amount": receipt.net_amount,
        "wallet": receipt.wallet,
        "bank": receipt.bank,
    }))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn bank_request_hash(kind: BankMutationKind, amount: i64) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"graphite.operation.bank.v1\0");
    hasher.update(kind.operation_kind().as_bytes());
    hasher.update(b"\0");
    hasher.update(&amount.to_be_bytes());
    *hasher.finalize().as_bytes()
}

fn checked_add(left: i64, right: i64) -> Result<i64, BankError> {
    let value = i128::from(left)
        .checked_add(i128::from(right))
        .ok_or(BankError::ArithmeticOverflow)?;
    i64::try_from(value).map_err(|_| BankError::ArithmeticOverflow)
}

fn checked_sub(left: i64, right: i64) -> Result<i64, BankError> {
    let value = i128::from(left)
        .checked_sub(i128::from(right))
        .ok_or(BankError::ArithmeticOverflow)?;
    i64::try_from(value).map_err(|_| BankError::ArithmeticOverflow)
}

fn snowflake_to_i64(value: u64) -> Result<i64, BankError> {
    i64::try_from(value).map_err(|_| BankError::SnowflakeOutOfRange)
}
