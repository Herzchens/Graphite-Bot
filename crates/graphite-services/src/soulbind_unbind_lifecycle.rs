use blake3::Hasher;
use chrono::{DateTime, Utc};
use graphite_core::{OperationId, RootSeed};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    OrdinarySoulBindUnbindPreflight, OrdinarySoulBindUnbindPreflightError, PersistedSoulBindState,
};

const SOULBIND_UNBIND_POLICY_VERSION: i32 = 1;
const SOULBIND_UNBIND_OPERATION_KIND: &str = "SOULBIND_UNBIND";
const SOULBIND_UNBIND_OUTBOX_TOPIC: &str = "soulbind.unbound";
const SOULBIND_UNBIND_ASSET_EVENT_KIND: &str = "SOULBIND_UNBOUND";
const SOULBIND_UNBIND_ASSET_MUTATION_KEY: &str = "soulbind:unbind";

#[derive(Clone)]
pub struct SoulBindUnbindService {
    pool: PgPool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SoulBindUnbindReceipt {
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub item_instance_id: Uuid,
    pub current_enhanced_appraisal: i64,
    pub money_fee: i64,
    pub wallet_before: i64,
    pub wallet_after: i64,
    pub evaluated_at: DateTime<Utc>,
    pub rebind_not_before: DateTime<Utc>,
    pub refunds_binding_resources: bool,
}

#[derive(Debug, Error)]
pub enum SoulBindUnbindLifecycleError {
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error(transparent)]
    Settlement(#[from] OrdinarySoulBindUnbindPreflightError),
    #[error("Discord snowflake is outside the signed BIGINT persistence range")]
    SnowflakeOutOfRange,
    #[error("no non-deleted Graphite account exists")]
    PlayerNotFound,
    #[error("idempotency key was reused with different SoulBind unbind input")]
    IdempotencyConflict,
    #[error("SoulBind unbind operation is in terminal state {0}")]
    OperationTerminal(String),
    #[error("SoulBind unbind operation disappeared after insert-or-conflict resolution")]
    OperationMissingAfterInsert,
    #[error("stored SoulBind unbind operation result is invalid: {0}")]
    InvalidOperationResult(Box<serde_json::Error>),
    #[error("stored SoulBind unbind operation state does not match its committed receipt")]
    StoredOperationIntegrityMismatch,
    #[error("SoulBind unbind settlement returned internally inconsistent authoritative state")]
    SettlementIntegrityMismatch,
}

impl From<sqlx::Error> for SoulBindUnbindLifecycleError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

enum OperationResolution {
    Pending(Uuid),
    Committed(SoulBindUnbindReceipt),
}

impl SoulBindUnbindService {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Owns the complete persistent SoulBind removal operation lifecycle, while deliberately
    /// leaving Discord command routing to a later application-adapter slice.
    ///
    /// The operation row is resolved first so duplicate requests replay a committed receipt before
    /// any current account/item state is consulted. A new request then resolves the actor's player
    /// id without taking a later-domain lock and delegates the authoritative mutation to
    /// [`OrdinarySoulBindUnbindPreflight::settle_for_owned_ordinary_equipment`], which acquires the
    /// canonical `operation -> player/balance -> item -> structural state -> embedded enchants ->
    /// SoulBind child` lock chain and atomically settles the exact Wallet fee plus seven-day state
    /// transition. This owner finally records one immutable asset event, commits a typed receipt on
    /// the operation, and inserts the transactional outbox event before the SQL transaction commits.
    ///
    /// The request hash is domain-separated and covers the only caller-controlled business input,
    /// `item_id`. A retry with the same external key and item returns the same committed receipt;
    /// reusing the key for a different item fails closed. No Bank auto-pull, binding-resource refund,
    /// Favorite/Protected mutation, or Discord command activation is introduced here.
    pub async fn unbind(
        &self,
        discord_user_id: u64,
        item_id: Uuid,
        external_request_key: &str,
    ) -> Result<SoulBindUnbindReceipt, SoulBindUnbindLifecycleError> {
        let discord_user_id = snowflake_to_i64(discord_user_id)?;
        let request_hash = soulbind_unbind_request_hash(item_id);
        let mut tx = self.pool.begin().await?;

        let operation_id = match resolve_operation(
            &mut tx,
            discord_user_id,
            external_request_key,
            item_id,
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

        let player_id = load_player_id(&mut tx, discord_user_id).await?;
        let (preflight, wallet_spend, state_transition) =
            OrdinarySoulBindUnbindPreflight::settle_for_owned_ordinary_equipment(
                &mut tx,
                operation_id,
                player_id,
                item_id,
            )
            .await?;

        if wallet_spend.operation_id != operation_id
            || wallet_spend.player_id != player_id
            || wallet_spend.amount != preflight.preview.money_fee
        {
            return Err(SoulBindUnbindLifecycleError::SettlementIntegrityMismatch);
        }
        if state_transition.previous_state != PersistedSoulBindState::Bound {
            return Err(SoulBindUnbindLifecycleError::SettlementIntegrityMismatch);
        }
        let PersistedSoulBindState::Unbound { rebind_not_before } = state_transition.new_state
        else {
            return Err(SoulBindUnbindLifecycleError::SettlementIntegrityMismatch);
        };

        let receipt = SoulBindUnbindReceipt {
            operation_id,
            player_id,
            item_instance_id: item_id,
            current_enhanced_appraisal: preflight.preview.current_enhanced_appraisal,
            money_fee: preflight.preview.money_fee,
            wallet_before: wallet_spend.wallet_before,
            wallet_after: wallet_spend.wallet_after,
            evaluated_at: state_transition.evaluated_at,
            rebind_not_before,
            refunds_binding_resources: preflight.preview.refunds_binding_resources,
        };

        insert_asset_event(&mut tx, &receipt).await?;
        commit_operation(&mut tx, &receipt).await?;
        insert_outbox(&mut tx, &receipt).await?;
        tx.commit().await?;
        Ok(receipt)
    }
}

async fn resolve_operation(
    tx: &mut Transaction<'_, Postgres>,
    discord_user_id: i64,
    external_request_key: &str,
    item_id: Uuid,
    request_hash: &[u8; 32],
) -> Result<OperationResolution, SoulBindUnbindLifecycleError> {
    if let Some(row) = select_operation(tx, external_request_key).await? {
        return validate_operation_row(row, discord_user_id, item_id, request_hash);
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
    .bind(SOULBIND_UNBIND_OPERATION_KIND)
    .bind(SOULBIND_UNBIND_POLICY_VERSION)
    .bind(request_hash.as_slice())
    .bind(rng_root.as_bytes().as_slice())
    .execute(&mut **tx)
    .await?;

    let row = select_operation(tx, external_request_key)
        .await?
        .ok_or(SoulBindUnbindLifecycleError::OperationMissingAfterInsert)?;
    validate_operation_row(row, discord_user_id, item_id, request_hash)
}

async fn select_operation(
    tx: &mut Transaction<'_, Postgres>,
    external_request_key: &str,
) -> Result<Option<sqlx::postgres::PgRow>, SoulBindUnbindLifecycleError> {
    Ok(sqlx::query(
        r#"
        SELECT id, actor_discord_user_id, player_id, kind, state, policy_version,
               request_hash, result
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
    item_id: Uuid,
    request_hash: &[u8; 32],
) -> Result<OperationResolution, SoulBindUnbindLifecycleError> {
    let operation_id: Uuid = row.try_get("id")?;
    let stored_actor: i64 = row.try_get("actor_discord_user_id")?;
    let stored_kind: String = row.try_get("kind")?;
    let stored_policy: i32 = row.try_get("policy_version")?;
    let stored_request_hash: Vec<u8> = row.try_get("request_hash")?;
    if stored_actor != discord_user_id
        || stored_kind != SOULBIND_UNBIND_OPERATION_KIND
        || stored_policy != SOULBIND_UNBIND_POLICY_VERSION
        || stored_request_hash.as_slice() != request_hash.as_slice()
    {
        return Err(SoulBindUnbindLifecycleError::IdempotencyConflict);
    }

    let state: String = row.try_get("state")?;
    if state == "COMMITTED" {
        let value: Value = row.try_get("result")?;
        let receipt: SoulBindUnbindReceipt = serde_json::from_value(value).map_err(|error| {
            SoulBindUnbindLifecycleError::InvalidOperationResult(Box::new(error))
        })?;
        let stored_player_id: Option<Uuid> = row.try_get("player_id")?;
        if receipt.operation_id != operation_id
            || receipt.item_instance_id != item_id
            || stored_player_id != Some(receipt.player_id)
        {
            return Err(SoulBindUnbindLifecycleError::StoredOperationIntegrityMismatch);
        }
        return Ok(OperationResolution::Committed(receipt));
    }
    if state != "PENDING" {
        return Err(SoulBindUnbindLifecycleError::OperationTerminal(state));
    }
    Ok(OperationResolution::Pending(operation_id))
}

async fn load_player_id(
    tx: &mut Transaction<'_, Postgres>,
    discord_user_id: i64,
) -> Result<Uuid, SoulBindUnbindLifecycleError> {
    sqlx::query("SELECT id FROM players WHERE discord_user_id = $1 AND status <> 'DELETED'")
        .bind(discord_user_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(SoulBindUnbindLifecycleError::PlayerNotFound)?
        .try_get("id")
        .map_err(SoulBindUnbindLifecycleError::from)
}

async fn insert_asset_event(
    tx: &mut Transaction<'_, Postgres>,
    receipt: &SoulBindUnbindReceipt,
) -> Result<(), SoulBindUnbindLifecycleError> {
    sqlx::query(
        r#"
        INSERT INTO asset_events (
            id, operation_id, mutation_key, player_id, event_kind, payload
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(OperationId::new().as_uuid())
    .bind(receipt.operation_id)
    .bind(SOULBIND_UNBIND_ASSET_MUTATION_KEY)
    .bind(receipt.player_id)
    .bind(SOULBIND_UNBIND_ASSET_EVENT_KIND)
    .bind(json!({
        "item_instance_id": receipt.item_instance_id,
        "current_enhanced_appraisal": receipt.current_enhanced_appraisal,
        "money_fee": receipt.money_fee,
        "rebind_not_before": receipt.rebind_not_before,
        "refunds_binding_resources": receipt.refunds_binding_resources,
    }))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn commit_operation(
    tx: &mut Transaction<'_, Postgres>,
    receipt: &SoulBindUnbindReceipt,
) -> Result<(), SoulBindUnbindLifecycleError> {
    let result = serde_json::to_value(receipt).expect("SoulBind unbind receipt is serializable");
    let updated = sqlx::query(
        r#"
        UPDATE operations
           SET player_id = $1,
               state = 'COMMITTED',
               result = $2,
               committed_at = clock_timestamp()
         WHERE id = $3
           AND state = 'PENDING'
        "#,
    )
    .bind(receipt.player_id)
    .bind(result)
    .bind(receipt.operation_id)
    .execute(&mut **tx)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(SoulBindUnbindLifecycleError::OperationTerminal(
            "unexpected transition".to_owned(),
        ));
    }
    Ok(())
}

async fn insert_outbox(
    tx: &mut Transaction<'_, Postgres>,
    receipt: &SoulBindUnbindReceipt,
) -> Result<(), SoulBindUnbindLifecycleError> {
    sqlx::query(
        r#"
        INSERT INTO outbox_events (id, operation_id, topic, payload)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (operation_id, topic) DO NOTHING
        "#,
    )
    .bind(OperationId::new().as_uuid())
    .bind(receipt.operation_id)
    .bind(SOULBIND_UNBIND_OUTBOX_TOPIC)
    .bind(json!({
        "player_id": receipt.player_id,
        "item_instance_id": receipt.item_instance_id,
        "current_enhanced_appraisal": receipt.current_enhanced_appraisal,
        "money_fee": receipt.money_fee,
        "wallet_before": receipt.wallet_before,
        "wallet_after": receipt.wallet_after,
        "evaluated_at": receipt.evaluated_at,
        "rebind_not_before": receipt.rebind_not_before,
        "refunds_binding_resources": receipt.refunds_binding_resources,
    }))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn soulbind_unbind_request_hash(item_id: Uuid) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(b"graphite.operation.soulbind-unbind.v1\0");
    hasher.update(item_id.as_bytes());
    *hasher.finalize().as_bytes()
}

fn snowflake_to_i64(value: u64) -> Result<i64, SoulBindUnbindLifecycleError> {
    i64::try_from(value).map_err(|_| SoulBindUnbindLifecycleError::SnowflakeOutOfRange)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_hash_is_stable_and_item_scoped() {
        let first = Uuid::parse_str("018f3a2f-21a0-7b4b-8a44-1a41d87e3cf5").unwrap();
        let second = Uuid::parse_str("018f3a2f-21a0-7b4b-8a44-1a41d87e3cf6").unwrap();
        assert_eq!(
            soulbind_unbind_request_hash(first),
            soulbind_unbind_request_hash(first)
        );
        assert_ne!(
            soulbind_unbind_request_hash(first),
            soulbind_unbind_request_hash(second)
        );
    }
}
