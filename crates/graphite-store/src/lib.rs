use chrono::{DateTime, Utc};
use graphite_core::{Money, MoneyError, OperationId, PlayerId, RootSeed};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgPoolOptions};
use thiserror::Error;
use uuid::Uuid;

#[derive(Clone)]
pub struct PgStore {
    pool: PgPool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TosDocument {
    pub version: i32,
    pub document_url: String,
    pub document_sha256: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RegistrationReceipt {
    pub player_id: Uuid,
    pub operation_id: Uuid,
    pub tos_version: i32,
    pub starter_item_count: u32,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerProfile {
    pub player_id: Uuid,
    pub discord_user_id: u64,
    pub created_at: DateTime<Utc>,
    pub wallet: Money,
    pub bank: Money,
    pub liability: Money,
    pub starter_item_count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransactionLine {
    pub transaction_id: Uuid,
    pub kind: String,
    pub amount: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("database migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("money invariant failed: {0}")]
    Money(#[from] MoneyError),
    #[error("no current Terms of Service version is configured")]
    NoCurrentTos,
    #[error("Terms of Service version {provided} is not current; current version is {current}")]
    TosVersionMismatch { provided: i32, current: i32 },
    #[error("Terms of Service version {version} already exists with different immutable content")]
    ImmutableTosVersion { version: i32 },
    #[error("refusing to move current Terms of Service backwards from {current} to {requested}")]
    TosVersionRegression { current: i32, requested: i32 },
    #[error("account registration is blocked by the deletion cooldown until {0}")]
    RegistrationCooldown(DateTime<Utc>),
    #[error("idempotency key was reused with different request input")]
    IdempotencyConflict,
    #[error("operation is in terminal state {0}")]
    OperationTerminal(String),
    #[error("stored operation result is invalid: {0}")]
    InvalidOperationResult(serde_json::Error),
    #[error("Discord snowflake is outside the signed BIGINT persistence range")]
    SnowflakeOutOfRange,
    #[error("stored 32-byte digest has invalid length {0}")]
    InvalidDigestLength(usize),
}

impl PgStore {
    pub async fn connect(database_url: &str) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new()
            .max_connections(16)
            .connect(database_url)
            .await?;
        Ok(Self { pool })
    }

    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn migrate(&self) -> Result<(), StoreError> {
        sqlx::migrate!("../../migrations").run(&self.pool).await?;
        Ok(())
    }

    pub async fn ensure_tos_document(&self, document: &TosDocument) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        let current =
            sqlx::query("SELECT version FROM tos_versions WHERE is_current = TRUE FOR UPDATE")
                .fetch_optional(&mut *tx)
                .await?;

        if let Some(row) = current {
            let current_version: i32 = row.try_get("version")?;
            if document.version < current_version {
                return Err(StoreError::TosVersionRegression {
                    current: current_version,
                    requested: document.version,
                });
            }
        }

        let existing = sqlx::query(
            "SELECT document_url, document_sha256 FROM tos_versions WHERE version = $1 FOR UPDATE",
        )
        .bind(document.version)
        .fetch_optional(&mut *tx)
        .await?;

        if let Some(row) = existing {
            let url: String = row.try_get("document_url")?;
            let hash: Vec<u8> = row.try_get("document_sha256")?;
            if url != document.document_url
                || hash.as_slice() != document.document_sha256.as_slice()
            {
                return Err(StoreError::ImmutableTosVersion {
                    version: document.version,
                });
            }
        } else {
            sqlx::query(
                "INSERT INTO tos_versions (version, document_url, document_sha256, is_current) VALUES ($1, $2, $3, FALSE)",
            )
            .bind(document.version)
            .bind(&document.document_url)
            .bind(document.document_sha256.as_slice())
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query(
            "UPDATE tos_versions SET is_current = FALSE WHERE is_current = TRUE AND version <> $1",
        )
        .bind(document.version)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE tos_versions SET is_current = TRUE WHERE version = $1")
            .bind(document.version)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn current_tos(&self) -> Result<Option<TosDocument>, StoreError> {
        let row = sqlx::query(
            "SELECT version, document_url, document_sha256 FROM tos_versions WHERE is_current = TRUE",
        )
        .fetch_optional(&self.pool)
        .await?;

        row.map(row_to_tos).transpose()
    }

    pub async fn register_player(
        &self,
        discord_user_id: u64,
        accepted_tos_version: i32,
        identity_hmac: &[u8; 32],
        external_request_key: &str,
    ) -> Result<RegistrationReceipt, StoreError> {
        let discord_user_id = snowflake_to_i64(discord_user_id)?;
        let request_hash = registration_request_hash(discord_user_id, accepted_tos_version);
        let mut tx = self.pool.begin().await?;

        let existing_operation = sqlx::query(
            r#"
            SELECT id, actor_discord_user_id, kind, state, policy_version, request_hash, result
              FROM operations
             WHERE external_request_key = $1
             FOR UPDATE
            "#,
        )
        .bind(external_request_key)
        .fetch_optional(&mut *tx)
        .await?;

        let (operation_id, policy_version) = if let Some(operation) = existing_operation {
            let operation_id: Uuid = operation.try_get("id")?;
            validate_registration_operation(
                &operation,
                discord_user_id,
                accepted_tos_version,
                &request_hash,
            )?;

            let state: String = operation.try_get("state")?;
            if state == "COMMITTED" {
                let value: serde_json::Value = operation.try_get("result")?;
                let receipt =
                    serde_json::from_value(value).map_err(StoreError::InvalidOperationResult)?;
                tx.commit().await?;
                return Ok(receipt);
            }
            if state != "PENDING" {
                return Err(StoreError::OperationTerminal(state));
            }

            let policy_version: i32 = operation.try_get("policy_version")?;
            (operation_id, policy_version)
        } else {
            let current_tos =
                sqlx::query("SELECT version FROM tos_versions WHERE is_current = TRUE FOR SHARE")
                    .fetch_optional(&mut *tx)
                    .await?
                    .ok_or(StoreError::NoCurrentTos)?;
            let current_tos_version: i32 = current_tos.try_get("version")?;
            if accepted_tos_version != current_tos_version {
                return Err(StoreError::TosVersionMismatch {
                    provided: accepted_tos_version,
                    current: current_tos_version,
                });
            }

            let cooldown = sqlx::query(
                "SELECT expires_at FROM deletion_cooldowns WHERE identity_hmac = $1 AND expires_at > now() FOR SHARE",
            )
            .bind(identity_hmac.as_slice())
            .fetch_optional(&mut *tx)
            .await?;
            if let Some(row) = cooldown {
                return Err(StoreError::RegistrationCooldown(row.try_get("expires_at")?));
            }

            let proposed_operation_id = OperationId::new();
            let proposed_rng_root = RootSeed::generate();
            sqlx::query(
                r#"
                INSERT INTO operations (
                    id, external_request_key, actor_discord_user_id, kind, state,
                    policy_version, request_hash, rng_root
                )
                VALUES ($1, $2, $3, 'ACCOUNT_REGISTER', 'PENDING', $4, $5, $6)
                ON CONFLICT (external_request_key) DO NOTHING
                "#,
            )
            .bind(proposed_operation_id.as_uuid())
            .bind(external_request_key)
            .bind(discord_user_id)
            .bind(current_tos_version)
            .bind(request_hash.as_slice())
            .bind(proposed_rng_root.as_bytes().as_slice())
            .execute(&mut *tx)
            .await?;

            let operation = sqlx::query(
                r#"
                SELECT id, actor_discord_user_id, kind, state, policy_version, request_hash, result
                  FROM operations
                 WHERE external_request_key = $1
                 FOR UPDATE
                "#,
            )
            .bind(external_request_key)
            .fetch_one(&mut *tx)
            .await?;
            validate_registration_operation(
                &operation,
                discord_user_id,
                accepted_tos_version,
                &request_hash,
            )?;

            let state: String = operation.try_get("state")?;
            if state == "COMMITTED" {
                let value: serde_json::Value = operation.try_get("result")?;
                let receipt =
                    serde_json::from_value(value).map_err(StoreError::InvalidOperationResult)?;
                tx.commit().await?;
                return Ok(receipt);
            }
            if state != "PENDING" {
                return Err(StoreError::OperationTerminal(state));
            }

            (
                operation.try_get("id")?,
                operation.try_get("policy_version")?,
            )
        };

        let player_row = sqlx::query(
            "SELECT id, created_at FROM players WHERE discord_user_id = $1 AND status <> 'DELETED' FOR UPDATE",
        )
        .bind(discord_user_id)
        .fetch_optional(&mut *tx)
        .await?;

        let (player_id, created_at, starter_item_count) = if let Some(row) = player_row {
            let player_id: Uuid = row.try_get("id")?;
            let created_at: DateTime<Utc> = row.try_get("created_at")?;
            sqlx::query(
                "INSERT INTO tos_acceptances (player_id, tos_version) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            )
            .bind(player_id)
            .bind(policy_version)
            .execute(&mut *tx)
            .await?;
            let starter_count = count_starter_items(&mut tx, player_id).await?;
            (player_id, created_at, starter_count)
        } else {
            let player_id = PlayerId::new().as_uuid();
            let created_at: DateTime<Utc> = sqlx::query(
                "INSERT INTO players (id, discord_user_id) VALUES ($1, $2) RETURNING created_at",
            )
            .bind(player_id)
            .bind(discord_user_id)
            .fetch_one(&mut *tx)
            .await?
            .try_get("created_at")?;

            sqlx::query("INSERT INTO player_balances (player_id) VALUES ($1)")
                .bind(player_id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("INSERT INTO tos_acceptances (player_id, tos_version) VALUES ($1, $2)")
                .bind(player_id)
                .bind(policy_version)
                .execute(&mut *tx)
                .await?;

            create_starter_loadout(&mut tx, player_id, operation_id).await?;
            (player_id, created_at, 7)
        };

        let receipt = RegistrationReceipt {
            player_id,
            operation_id,
            tos_version: policy_version,
            starter_item_count,
            created_at,
        };
        let result = serde_json::to_value(&receipt).expect("registration receipt is serializable");

        sqlx::query(
            r#"
            UPDATE operations
               SET player_id = $1,
                   state = 'COMMITTED',
                   result = $2,
                   committed_at = now()
             WHERE id = $3
            "#,
        )
        .bind(player_id)
        .bind(&result)
        .bind(operation_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO outbox_events (id, operation_id, topic, payload) VALUES ($1, $2, 'account.registered', $3) ON CONFLICT (operation_id, topic) DO NOTHING",
        )
        .bind(OperationId::new().as_uuid())
        .bind(operation_id)
        .bind(json!({
            "player_id": player_id,
            "discord_user_id": discord_user_id,
            "tos_version": policy_version,
        }))
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(receipt)
    }

    pub async fn profile_for_discord(
        &self,
        discord_user_id: u64,
    ) -> Result<Option<PlayerProfile>, StoreError> {
        let discord_user_id = snowflake_to_i64(discord_user_id)?;
        let row = sqlx::query(
            r#"
            SELECT p.id,
                   p.discord_user_id,
                   p.created_at,
                   b.wallet,
                   b.bank,
                   b.liability,
                   COUNT(i.id) FILTER (WHERE i.is_starter) AS starter_item_count
              FROM players p
              JOIN player_balances b ON b.player_id = p.id
              LEFT JOIN item_instances i ON i.owner_player_id = p.id
             WHERE p.discord_user_id = $1
               AND p.status <> 'DELETED'
             GROUP BY p.id, p.discord_user_id, p.created_at, b.wallet, b.bank, b.liability
            "#,
        )
        .bind(discord_user_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(row_to_profile).transpose()
    }

    pub async fn recent_transactions(
        &self,
        discord_user_id: u64,
        limit: i64,
    ) -> Result<Vec<TransactionLine>, StoreError> {
        let discord_user_id = snowflake_to_i64(discord_user_id)?;
        let limit = limit.clamp(1, 50);
        let rows = sqlx::query(
            r#"
            SELECT lt.id, lt.kind, lp.amount, lt.created_at
              FROM players p
              JOIN ledger_postings lp ON lp.player_id = p.id
              JOIN ledger_transactions lt ON lt.id = lp.transaction_id
             WHERE p.discord_user_id = $1
             ORDER BY lt.created_at DESC, lp.sequence ASC
             LIMIT $2
            "#,
        )
        .bind(discord_user_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                Ok(TransactionLine {
                    transaction_id: row.try_get("id")?,
                    kind: row.try_get("kind")?,
                    amount: row.try_get("amount")?,
                    created_at: row.try_get("created_at")?,
                })
            })
            .collect()
    }
}

fn validate_registration_operation(
    operation: &sqlx::postgres::PgRow,
    discord_user_id: i64,
    accepted_tos_version: i32,
    request_hash: &[u8; 32],
) -> Result<(), StoreError> {
    let stored_actor: i64 = operation.try_get("actor_discord_user_id")?;
    let stored_kind: String = operation.try_get("kind")?;
    let stored_policy: i32 = operation.try_get("policy_version")?;
    let stored_request_hash: Vec<u8> = operation.try_get("request_hash")?;

    if stored_actor != discord_user_id
        || stored_kind != "ACCOUNT_REGISTER"
        || stored_policy != accepted_tos_version
        || stored_request_hash.as_slice() != request_hash.as_slice()
    {
        return Err(StoreError::IdempotencyConflict);
    }
    Ok(())
}

fn row_to_tos(row: sqlx::postgres::PgRow) -> Result<TosDocument, StoreError> {
    let hash: Vec<u8> = row.try_get("document_sha256")?;
    Ok(TosDocument {
        version: row.try_get("version")?,
        document_url: row.try_get("document_url")?,
        document_sha256: digest32(hash)?,
    })
}

fn row_to_profile(row: sqlx::postgres::PgRow) -> Result<PlayerProfile, StoreError> {
    let stored_discord_id: i64 = row.try_get("discord_user_id")?;
    let discord_user_id =
        u64::try_from(stored_discord_id).map_err(|_| StoreError::SnowflakeOutOfRange)?;
    let starter_item_count: i64 = row.try_get("starter_item_count")?;
    Ok(PlayerProfile {
        player_id: row.try_get("id")?,
        discord_user_id,
        created_at: row.try_get("created_at")?,
        wallet: Money::new(row.try_get("wallet")?)?,
        bank: Money::new(row.try_get("bank")?)?,
        liability: Money::new(row.try_get("liability")?)?,
        starter_item_count: u32::try_from(starter_item_count).unwrap_or(u32::MAX),
    })
}

async fn count_starter_items(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
) -> Result<u32, StoreError> {
    let count: i64 = sqlx::query(
        "SELECT COUNT(*) AS count FROM item_instances WHERE owner_player_id = $1 AND is_starter = TRUE",
    )
    .bind(player_id)
    .fetch_one(&mut **tx)
    .await?
    .try_get("count")?;
    Ok(u32::try_from(count).unwrap_or(u32::MAX))
}

async fn create_starter_loadout(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
    operation_id: Uuid,
) -> Result<(), StoreError> {
    struct Starter<'a> {
        definition: &'a str,
        slot: &'a str,
        durability: Option<i64>,
        repairable: bool,
        unbreakable: bool,
    }

    let starters = [
        Starter {
            definition: "equipment.pickaxe.wood.starter",
            slot: "PICKAXE",
            durability: None,
            repairable: false,
            unbreakable: true,
        },
        Starter {
            definition: "equipment.sword.wood.starter",
            slot: "SWORD",
            durability: None,
            repairable: false,
            unbreakable: true,
        },
        Starter {
            definition: "equipment.rod.basic.starter",
            slot: "FISHING_ROD",
            durability: None,
            repairable: false,
            unbreakable: true,
        },
        Starter {
            definition: "equipment.armor.leather.helmet.starter",
            slot: "ARMOR_HELMET",
            durability: Some(1),
            repairable: true,
            unbreakable: false,
        },
        Starter {
            definition: "equipment.armor.leather.chest.starter",
            slot: "ARMOR_CHEST",
            durability: Some(3),
            repairable: true,
            unbreakable: false,
        },
        Starter {
            definition: "equipment.armor.leather.legs.starter",
            slot: "ARMOR_LEGS",
            durability: Some(2),
            repairable: true,
            unbreakable: false,
        },
        Starter {
            definition: "equipment.armor.leather.boots.starter",
            slot: "ARMOR_BOOTS",
            durability: Some(1),
            repairable: true,
            unbreakable: false,
        },
    ];

    let mut created_ids = Vec::with_capacity(starters.len());
    for starter in starters {
        let item_id = PlayerId::new().as_uuid();
        sqlx::query(
            r#"
            INSERT INTO item_instances (
                id, definition_key, owner_player_id, created_by_operation_id, location,
                is_starter, is_account_bound, is_tradeable, is_sellable, is_discardable,
                is_enchantable, is_upgradeable, is_unbreakable, is_repairable,
                current_durability, max_durability
            )
            VALUES (
                $1, $2, $3, $4, 'EQUIPPED',
                TRUE, TRUE, FALSE, FALSE, FALSE,
                FALSE, FALSE, $5, $6, $7, $7
            )
            "#,
        )
        .bind(item_id)
        .bind(starter.definition)
        .bind(player_id)
        .bind(operation_id)
        .bind(starter.unbreakable)
        .bind(starter.repairable)
        .bind(starter.durability)
        .execute(&mut **tx)
        .await?;

        sqlx::query(
            "INSERT INTO equipment_slots (player_id, slot, item_instance_id) VALUES ($1, $2, $3)",
        )
        .bind(player_id)
        .bind(starter.slot)
        .bind(item_id)
        .execute(&mut **tx)
        .await?;
        created_ids.push(item_id);
    }

    sqlx::query(
        "INSERT INTO asset_events (id, operation_id, player_id, event_kind, payload) VALUES ($1, $2, $3, 'STARTER_LOADOUT_ISSUED', $4)",
    )
    .bind(OperationId::new().as_uuid())
    .bind(operation_id)
    .bind(player_id)
    .bind(json!({ "item_instance_ids": created_ids }))
    .execute(&mut **tx)
    .await?;

    Ok(())
}

fn registration_request_hash(discord_user_id: i64, tos_version: i32) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"graphite.operation.account-register.v1\0");
    hasher.update(&discord_user_id.to_be_bytes());
    hasher.update(&tos_version.to_be_bytes());
    *hasher.finalize().as_bytes()
}

fn digest32(bytes: Vec<u8>) -> Result<[u8; 32], StoreError> {
    if bytes.len() != 32 {
        return Err(StoreError::InvalidDigestLength(bytes.len()));
    }
    let mut digest = [0_u8; 32];
    digest.copy_from_slice(&bytes);
    Ok(digest)
}

fn snowflake_to_i64(value: u64) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::SnowflakeOutOfRange)
}
