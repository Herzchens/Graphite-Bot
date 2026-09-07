use std::fmt;

use graphite_core::{OperationId, RootSeed};
use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

const RNG_ROOT_BYTES: usize = 32;

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ManualFishingRngContext {
    operation_id: OperationId,
    root_seed: RootSeed,
}

impl ManualFishingRngContext {
    #[must_use]
    pub const fn operation_id(self) -> Uuid {
        self.operation_id.as_uuid()
    }
}

impl fmt::Debug for ManualFishingRngContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ManualFishingRngContext")
            .field("operation_id", &self.operation_id)
            .field("root_seed", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Error)]
pub enum ManualFishingRngContextError {
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error("owning operation does not exist")]
    OperationNotFound,
    #[error("owning operation targets a different player")]
    OperationPlayerMismatch,
    #[error("owning operation cannot resolve new Fishing RNG in state {0}")]
    OperationTerminal(String),
    #[error("stored owning-operation RNG root is not exactly 32 bytes")]
    InvalidStoredRngRoot,
}

impl From<sqlx::Error> for ManualFishingRngContextError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

/// Pins the owning operation's persisted RNG root inside the caller-owned manual-Fishing
/// transaction.
///
/// The operation row is locked and must still be `PENDING` and bound to `player_id`. The persisted
/// 32-byte root remains private inside the returned opaque context, so Discord/application adapters
/// cannot become RNG authority and retry timing or unrelated random draws cannot replace the
/// operation-owned source of truth.
///
/// This function deliberately does not expose a draw API and does not draw a catch branch, species,
/// variant, quantity proc, capability result, durability result, weight, biological noise, or reward.
/// Typed Fishing domain derivation belongs in the future Services-owned cast composition slice where
/// canonical draw order has an actual production consumer. In particular this primitive does not
/// resolve the currently unfrozen Fish weight/length sampler or additional-FishInstance identity
/// semantics.
pub async fn lock_manual_fishing_rng_context(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
) -> Result<ManualFishingRngContext, ManualFishingRngContextError> {
    let row =
        sqlx::query("SELECT player_id, state, rng_root FROM operations WHERE id = $1 FOR UPDATE")
            .bind(operation_id)
            .fetch_optional(&mut **tx)
            .await?
            .ok_or(ManualFishingRngContextError::OperationNotFound)?;

    let stored_player_id: Option<Uuid> = row.try_get("player_id")?;
    if stored_player_id != Some(player_id) {
        return Err(ManualFishingRngContextError::OperationPlayerMismatch);
    }

    let state: String = row.try_get("state")?;
    if state != "PENDING" {
        return Err(ManualFishingRngContextError::OperationTerminal(state));
    }

    let root: Vec<u8> = row.try_get("rng_root")?;
    let bytes: [u8; RNG_ROOT_BYTES] = root
        .try_into()
        .map_err(|_| ManualFishingRngContextError::InvalidStoredRngRoot)?;

    Ok(ManualFishingRngContext {
        operation_id: OperationId::from_uuid(operation_id),
        root_seed: RootSeed::from_bytes(bytes),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> ManualFishingRngContext {
        ManualFishingRngContext {
            operation_id: OperationId::from_uuid(
                Uuid::parse_str("018f3a2f-21a0-7b4b-8a44-1a41d87e3cf5").unwrap(),
            ),
            root_seed: RootSeed::from_bytes([23; RNG_ROOT_BYTES]),
        }
    }

    #[test]
    fn operation_identity_is_exposed_without_seed_access() {
        let context = context();
        assert_eq!(
            context.operation_id(),
            Uuid::parse_str("018f3a2f-21a0-7b4b-8a44-1a41d87e3cf5").unwrap()
        );
    }

    #[test]
    fn debug_output_redacts_the_persisted_rng_root() {
        let rendered = format!("{:?}", context());
        assert!(rendered.contains("[REDACTED]"));
        assert!(!rendered.contains("23, 23, 23"));
    }
}
