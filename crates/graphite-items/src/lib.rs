mod transactional_stack;

pub use transactional_stack::{
    StackDeliveryMutationError, StackDeliveryMutationReceipt, StackDeliveryMutationRequest,
    apply_stack_delivery_mutation,
};

use graphite_core::{OperationId, RootSeed};
use graphite_store::PgStore;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

const ITEM_POLICY_VERSION: i32 = 1;
const ITEM_BAG_BASE_SLOTS: i64 = 36;
const ITEM_BAG_SLOTS_PER_LEVEL: i64 = 6;
const CATCH_BAG_BASE_GRAMS: i64 = 1_000_000;
const CATCH_BAG_GRAMS_PER_LEVEL: i64 = 250_000;
const READ_LIST_LIMIT: i64 = 25;

#[derive(Clone)]
pub struct ItemService {
    store: PgStore,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ItemMutationKind {
    Equip,
    Unequip,
    DeliverStack,
}

impl ItemMutationKind {
    const fn operation_kind(self) -> &'static str {
        match self {
            Self::Equip => "ITEM_EQUIP",
            Self::Unequip => "ITEM_UNEQUIP",
            Self::DeliverStack => "ITEM_STACK_DELIVER",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ItemMutationReceipt {
    pub operation_id: Uuid,
    pub kind: ItemMutationKind,
    pub item_instance_id: Option<Uuid>,
    pub slot: Option<String>,
    pub displaced_item_instance_id: Option<Uuid>,
    pub definition_key: Option<String>,
    pub quantity: Option<i64>,
    pub pending: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StackView {
    pub definition_key: String,
    pub definition_version: i32,
    pub rarity: String,
    pub quantity: i64,
    pub stack_limit: i64,
    pub occupied_slots: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemBagSnapshot {
    pub player_id: Uuid,
    pub level: i64,
    pub capacity_slots: u64,
    pub used_slots: u64,
    pub pending_deliveries: u64,
    pub stacks: Vec<StackView>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatchView {
    pub item_instance_id: Uuid,
    pub definition_key: String,
    pub definition_version: i32,
    pub rarity: String,
    pub weight_grams: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatchBagSnapshot {
    pub player_id: Uuid,
    pub level: i64,
    pub capacity_grams: i64,
    pub used_grams: i64,
    pub catches: Vec<CatchView>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ItemView {
    pub item_instance_id: Uuid,
    pub definition_key: String,
    pub definition_version: i32,
    pub category: String,
    pub rarity: String,
    pub location: String,
    pub is_starter: bool,
    pub is_account_bound: bool,
    pub is_tradeable: bool,
    pub is_sellable: bool,
    pub is_discardable: bool,
    pub is_enchantable: bool,
    pub is_upgradeable: bool,
    pub is_unbreakable: bool,
    pub is_repairable: bool,
    pub current_durability: Option<i64>,
    pub max_durability: Option<i64>,
    pub catch_weight_grams: Option<i64>,
    pub state: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EquipmentEntry {
    pub slot: String,
    pub item: ItemView,
}

#[derive(Debug, Error)]
pub enum ItemError {
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error("stored item operation result is invalid: {0}")]
    InvalidOperationResult(Box<serde_json::Error>),
    #[error("Discord snowflake is outside the signed BIGINT persistence range")]
    SnowflakeOutOfRange,
    #[error("no active Graphite account exists")]
    PlayerNotFound,
    #[error("item mutation is blocked while account status is {0}")]
    AccountFrozen(String),
    #[error("item instance was not found for this player")]
    ItemNotFound,
    #[error("item is not in the Tool Locker")]
    ItemNotInLocker,
    #[error("item is not currently equipped")]
    ItemNotEquipped,
    #[error("item definition cannot be equipped")]
    NotEquippable,
    #[error("stack definition does not exist, is inactive, or is not stackable")]
    InvalidStackDefinition,
    #[error("stack delivery quantity must be positive")]
    InvalidQuantity,
    #[error("idempotency key was reused with different item input")]
    IdempotencyConflict,
    #[error("item operation is in terminal state {0}")]
    OperationTerminal(String),
    #[error("item operation disappeared after insert-or-conflict resolution")]
    OperationMissingAfterInsert,
    #[error("item/storage arithmetic exceeded the supported range")]
    ArithmeticOverflow,
    #[error("equipment state is internally inconsistent")]
    EquipmentIntegrityMismatch,
}

impl From<sqlx::Error> for ItemError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

struct LockedPlayer {
    player_id: Uuid,
    status: String,
    item_bag_level: i64,
}

struct DefinitionVersion {
    key: String,
    version: i32,
    category: String,
    stackable: bool,
    stack_limit: Option<i64>,
    data: Value,
}

struct LockedItem {
    location: String,
    definition: DefinitionVersion,
}

enum OperationResolution {
    Pending(Uuid),
    Committed(ItemMutationReceipt),
}

impl ItemService {
    #[must_use]
    pub fn new(store: PgStore) -> Self {
        Self { store }
    }

    pub async fn item_bag(&self, discord_user_id: u64) -> Result<ItemBagSnapshot, ItemError> {
        let discord_user_id = snowflake_to_i64(discord_user_id)?;
        let row = sqlx::query(
            r#"
            SELECT p.id, s.item_bag_level
              FROM players p
              JOIN player_storage_profiles s ON s.player_id = p.id
             WHERE p.discord_user_id = $1
               AND p.status <> 'DELETED'
            "#,
        )
        .bind(discord_user_id)
        .fetch_optional(self.store.pool())
        .await?
        .ok_or(ItemError::PlayerNotFound)?;
        let player_id: Uuid = row.try_get("id")?;
        let level: i64 = row.try_get("item_bag_level")?;
        let capacity_slots = item_bag_capacity_slots(level)?;

        let rows = sqlx::query(
            r#"
            SELECT s.definition_key,
                   s.definition_version,
                   s.quantity,
                   d.rarity,
                   d.stack_limit
              FROM item_stacks s
              JOIN item_definition_versions d
                ON d.key = s.definition_key
               AND d.version = s.definition_version
             WHERE s.player_id = $1
               AND s.location = 'ITEM_BAG'
             ORDER BY s.definition_key, s.definition_version
            "#,
        )
        .bind(player_id)
        .fetch_all(self.store.pool())
        .await?;

        let mut used_slots = 0_u64;
        let mut stacks = Vec::with_capacity(rows.len());
        for row in rows {
            let quantity: i64 = row.try_get("quantity")?;
            let stack_limit: i64 = row.try_get("stack_limit")?;
            let occupied_slots = slots_for_quantity(quantity, stack_limit)?;
            used_slots = used_slots
                .checked_add(occupied_slots)
                .ok_or(ItemError::ArithmeticOverflow)?;
            if stacks.len() < usize::try_from(READ_LIST_LIMIT).unwrap_or(25) {
                stacks.push(StackView {
                    definition_key: row.try_get("definition_key")?,
                    definition_version: row.try_get("definition_version")?,
                    rarity: row.try_get("rarity")?,
                    quantity,
                    stack_limit,
                    occupied_slots,
                });
            }
        }

        let pending: i64 = sqlx::query(
            "SELECT COUNT(*) AS count FROM pending_asset_deliveries WHERE player_id = $1 AND state = 'PENDING' AND desired_location = 'ITEM_BAG'",
        )
        .bind(player_id)
        .fetch_one(self.store.pool())
        .await?
        .try_get("count")?;

        Ok(ItemBagSnapshot {
            player_id,
            level,
            capacity_slots,
            used_slots,
            pending_deliveries: u64::try_from(pending)
                .map_err(|_| ItemError::ArithmeticOverflow)?,
            stacks,
        })
    }

    pub async fn catch_bag(&self, discord_user_id: u64) -> Result<CatchBagSnapshot, ItemError> {
        let discord_user_id = snowflake_to_i64(discord_user_id)?;
        let row = sqlx::query(
            r#"
            SELECT p.id, s.catch_bag_level
              FROM players p
              JOIN player_storage_profiles s ON s.player_id = p.id
             WHERE p.discord_user_id = $1
               AND p.status <> 'DELETED'
            "#,
        )
        .bind(discord_user_id)
        .fetch_optional(self.store.pool())
        .await?
        .ok_or(ItemError::PlayerNotFound)?;
        let player_id: Uuid = row.try_get("id")?;
        let level: i64 = row.try_get("catch_bag_level")?;
        let capacity_grams = catch_bag_capacity_grams(level)?;

        let rows = sqlx::query(
            r#"
            SELECT i.id,
                   i.definition_key,
                   i.definition_version,
                   i.catch_weight_grams,
                   d.rarity
              FROM item_instances i
              JOIN item_definition_versions d
                ON d.key = i.definition_key
               AND d.version = i.definition_version
             WHERE i.owner_player_id = $1
               AND i.location = 'CATCH_BAG'
             ORDER BY i.created_at, i.id
            "#,
        )
        .bind(player_id)
        .fetch_all(self.store.pool())
        .await?;

        let mut used = 0_i128;
        let mut catches = Vec::new();
        for row in rows {
            let weight: Option<i64> = row.try_get("catch_weight_grams")?;
            let weight = weight.ok_or(ItemError::EquipmentIntegrityMismatch)?;
            used = used
                .checked_add(i128::from(weight))
                .ok_or(ItemError::ArithmeticOverflow)?;
            if catches.len() < usize::try_from(READ_LIST_LIMIT).unwrap_or(25) {
                catches.push(CatchView {
                    item_instance_id: row.try_get("id")?,
                    definition_key: row.try_get("definition_key")?,
                    definition_version: row.try_get("definition_version")?,
                    rarity: row.try_get("rarity")?,
                    weight_grams: weight,
                });
            }
        }

        Ok(CatchBagSnapshot {
            player_id,
            level,
            capacity_grams,
            used_grams: i64::try_from(used).map_err(|_| ItemError::ArithmeticOverflow)?,
            catches,
        })
    }

    pub async fn locker(&self, discord_user_id: u64) -> Result<Vec<ItemView>, ItemError> {
        let player_id = self.player_id(discord_user_id).await?;
        let rows = sqlx::query(
            r#"
            SELECT i.id,
                   i.definition_key,
                   i.definition_version,
                   d.category,
                   d.rarity,
                   i.location,
                   i.is_starter,
                   i.is_account_bound,
                   i.is_tradeable,
                   i.is_sellable,
                   i.is_discardable,
                   i.is_enchantable,
                   i.is_upgradeable,
                   i.is_unbreakable,
                   i.is_repairable,
                   i.current_durability,
                   i.max_durability,
                   i.catch_weight_grams,
                   i.state
              FROM item_instances i
              JOIN item_definition_versions d
                ON d.key = i.definition_key
               AND d.version = i.definition_version
             WHERE i.owner_player_id = $1
               AND i.location = 'TOOL_LOCKER'
             ORDER BY i.created_at, i.id
             LIMIT $2
            "#,
        )
        .bind(player_id)
        .bind(READ_LIST_LIMIT)
        .fetch_all(self.store.pool())
        .await?;
        rows.into_iter().map(row_to_item_view).collect()
    }

    pub async fn equipment(&self, discord_user_id: u64) -> Result<Vec<EquipmentEntry>, ItemError> {
        let player_id = self.player_id(discord_user_id).await?;
        let rows = sqlx::query(
            r#"
            SELECT e.slot,
                   i.id,
                   i.definition_key,
                   i.definition_version,
                   d.category,
                   d.rarity,
                   i.location,
                   i.is_starter,
                   i.is_account_bound,
                   i.is_tradeable,
                   i.is_sellable,
                   i.is_discardable,
                   i.is_enchantable,
                   i.is_upgradeable,
                   i.is_unbreakable,
                   i.is_repairable,
                   i.current_durability,
                   i.max_durability,
                   i.catch_weight_grams,
                   i.state
              FROM equipment_slots e
              JOIN item_instances i ON i.id = e.item_instance_id
              JOIN item_definition_versions d
                ON d.key = i.definition_key
               AND d.version = i.definition_version
             WHERE e.player_id = $1
             ORDER BY CASE e.slot
                WHEN 'PICKAXE' THEN 1
                WHEN 'SWORD' THEN 2
                WHEN 'FISHING_ROD' THEN 3
                WHEN 'ARMOR_HELMET' THEN 4
                WHEN 'ARMOR_CHEST' THEN 5
                WHEN 'ARMOR_LEGS' THEN 6
                WHEN 'ARMOR_BOOTS' THEN 7
                WHEN 'TOTEM' THEN 8
                ELSE 99 END
            "#,
        )
        .bind(player_id)
        .fetch_all(self.store.pool())
        .await?;

        rows.into_iter()
            .map(|row| {
                let slot: String = row.try_get("slot")?;
                Ok(EquipmentEntry {
                    slot,
                    item: row_to_item_view(row)?,
                })
            })
            .collect()
    }

    pub async fn item(&self, discord_user_id: u64, item_id: Uuid) -> Result<ItemView, ItemError> {
        let player_id = self.player_id(discord_user_id).await?;
        let row = sqlx::query(
            r#"
            SELECT i.id,
                   i.definition_key,
                   i.definition_version,
                   d.category,
                   d.rarity,
                   i.location,
                   i.is_starter,
                   i.is_account_bound,
                   i.is_tradeable,
                   i.is_sellable,
                   i.is_discardable,
                   i.is_enchantable,
                   i.is_upgradeable,
                   i.is_unbreakable,
                   i.is_repairable,
                   i.current_durability,
                   i.max_durability,
                   i.catch_weight_grams,
                   i.state
              FROM item_instances i
              JOIN item_definition_versions d
                ON d.key = i.definition_key
               AND d.version = i.definition_version
             WHERE i.owner_player_id = $1
               AND i.id = $2
            "#,
        )
        .bind(player_id)
        .bind(item_id)
        .fetch_optional(self.store.pool())
        .await?
        .ok_or(ItemError::ItemNotFound)?;
        row_to_item_view(row)
    }

    pub async fn equip(
        &self,
        discord_user_id: u64,
        item_id: Uuid,
        external_request_key: &str,
    ) -> Result<ItemMutationReceipt, ItemError> {
        let discord_user_id = snowflake_to_i64(discord_user_id)?;
        let kind = ItemMutationKind::Equip;
        let request_hash = item_request_hash(kind, item_id.as_bytes(), None, None);
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
            OperationResolution::Pending(id) => id,
        };

        let player = lock_player(&mut tx, discord_user_id).await?;
        ensure_mutable(&player.status)?;
        let item = lock_owned_item(&mut tx, player.player_id, item_id).await?;
        let slot = equipment_slot(&item.definition)?;

        if item.location == "EQUIPPED" {
            let existing_slot: Option<String> = sqlx::query(
                "SELECT slot FROM equipment_slots WHERE player_id = $1 AND item_instance_id = $2 FOR UPDATE",
            )
            .bind(player.player_id)
            .bind(item_id)
            .fetch_optional(&mut *tx)
            .await?
            .map(|row| row.try_get("slot"))
            .transpose()?;
            if existing_slot.as_deref() != Some(slot.as_str()) {
                return Err(ItemError::EquipmentIntegrityMismatch);
            }
            let receipt = ItemMutationReceipt {
                operation_id,
                kind,
                item_instance_id: Some(item_id),
                slot: Some(slot),
                displaced_item_instance_id: None,
                definition_key: None,
                quantity: None,
                pending: false,
            };
            commit_operation(&mut tx, player.player_id, &receipt).await?;
            insert_item_outbox(&mut tx, &receipt, "item.equipped").await?;
            tx.commit().await?;
            return Ok(receipt);
        }
        if item.location != "TOOL_LOCKER" {
            return Err(ItemError::ItemNotInLocker);
        }

        let displaced: Option<Uuid> = sqlx::query(
            "SELECT item_instance_id FROM equipment_slots WHERE player_id = $1 AND slot = $2 FOR UPDATE",
        )
        .bind(player.player_id)
        .bind(&slot)
        .fetch_optional(&mut *tx)
        .await?
        .map(|row| row.try_get("item_instance_id"))
        .transpose()?;

        if let Some(displaced_id) = displaced {
            sqlx::query("UPDATE item_instances SET location = 'TOOL_LOCKER' WHERE id = $1")
                .bind(displaced_id)
                .execute(&mut *tx)
                .await?;
            sqlx::query(
                "UPDATE equipment_slots SET item_instance_id = $1 WHERE player_id = $2 AND slot = $3",
            )
            .bind(item_id)
            .bind(player.player_id)
            .bind(&slot)
            .execute(&mut *tx)
            .await?;
        } else {
            sqlx::query(
                "INSERT INTO equipment_slots (player_id, slot, item_instance_id) VALUES ($1, $2, $3)",
            )
            .bind(player.player_id)
            .bind(&slot)
            .bind(item_id)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query("UPDATE item_instances SET location = 'EQUIPPED' WHERE id = $1")
            .bind(item_id)
            .execute(&mut *tx)
            .await?;

        let receipt = ItemMutationReceipt {
            operation_id,
            kind,
            item_instance_id: Some(item_id),
            slot: Some(slot.clone()),
            displaced_item_instance_id: displaced,
            definition_key: None,
            quantity: None,
            pending: false,
        };
        insert_asset_event(
            &mut tx,
            operation_id,
            player.player_id,
            "ITEM_EQUIPPED",
            json!({
                "item_instance_id": item_id,
                "slot": slot,
                "displaced_item_instance_id": displaced,
            }),
        )
        .await?;
        commit_operation(&mut tx, player.player_id, &receipt).await?;
        insert_item_outbox(&mut tx, &receipt, "item.equipped").await?;
        tx.commit().await?;
        Ok(receipt)
    }

    pub async fn unequip(
        &self,
        discord_user_id: u64,
        item_id: Uuid,
        external_request_key: &str,
    ) -> Result<ItemMutationReceipt, ItemError> {
        let discord_user_id = snowflake_to_i64(discord_user_id)?;
        let kind = ItemMutationKind::Unequip;
        let request_hash = item_request_hash(kind, item_id.as_bytes(), None, None);
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
            OperationResolution::Pending(id) => id,
        };

        let player = lock_player(&mut tx, discord_user_id).await?;
        ensure_mutable(&player.status)?;
        let item = lock_owned_item(&mut tx, player.player_id, item_id).await?;
        if item.location != "EQUIPPED" {
            return Err(ItemError::ItemNotEquipped);
        }
        let slot_row = sqlx::query(
            "SELECT slot FROM equipment_slots WHERE player_id = $1 AND item_instance_id = $2 FOR UPDATE",
        )
        .bind(player.player_id)
        .bind(item_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(ItemError::EquipmentIntegrityMismatch)?;
        let slot: String = slot_row.try_get("slot")?;

        sqlx::query("DELETE FROM equipment_slots WHERE player_id = $1 AND item_instance_id = $2")
            .bind(player.player_id)
            .bind(item_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE item_instances SET location = 'TOOL_LOCKER' WHERE id = $1")
            .bind(item_id)
            .execute(&mut *tx)
            .await?;

        let receipt = ItemMutationReceipt {
            operation_id,
            kind,
            item_instance_id: Some(item_id),
            slot: Some(slot.clone()),
            displaced_item_instance_id: None,
            definition_key: None,
            quantity: None,
            pending: false,
        };
        insert_asset_event(
            &mut tx,
            operation_id,
            player.player_id,
            "ITEM_UNEQUIPPED",
            json!({ "item_instance_id": item_id, "slot": slot }),
        )
        .await?;
        commit_operation(&mut tx, player.player_id, &receipt).await?;
        insert_item_outbox(&mut tx, &receipt, "item.unequipped").await?;
        tx.commit().await?;
        Ok(receipt)
    }

    pub async fn deliver_stack_to_item_bag(
        &self,
        discord_user_id: u64,
        definition_key: &str,
        quantity: i64,
        external_request_key: &str,
    ) -> Result<ItemMutationReceipt, ItemError> {
        if quantity <= 0 {
            return Err(ItemError::InvalidQuantity);
        }
        let discord_user_id = snowflake_to_i64(discord_user_id)?;
        let kind = ItemMutationKind::DeliverStack;
        let request_hash = item_request_hash(
            kind,
            definition_key.as_bytes(),
            Some(quantity),
            Some("ITEM_BAG"),
        );
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
            OperationResolution::Pending(id) => id,
        };

        let player = lock_player(&mut tx, discord_user_id).await?;
        ensure_mutable(&player.status)?;
        let definition = load_current_stack_definition(&mut tx, definition_key).await?;
        let capacity = item_bag_capacity_slots(player.item_bag_level)?;
        let used_before = item_bag_used_slots(&mut tx, player.player_id).await?;
        let existing_quantity: i64 = sqlx::query(
            r#"
            SELECT quantity
              FROM item_stacks
             WHERE player_id = $1
               AND definition_key = $2
               AND definition_version = $3
               AND location = 'ITEM_BAG'
             FOR UPDATE
            "#,
        )
        .bind(player.player_id)
        .bind(&definition.key)
        .bind(definition.version)
        .fetch_optional(&mut *tx)
        .await?
        .map(|row| row.try_get("quantity"))
        .transpose()?
        .unwrap_or(0);
        let stack_limit = definition
            .stack_limit
            .ok_or(ItemError::InvalidStackDefinition)?;
        let before_slots = slots_for_quantity(existing_quantity, stack_limit)?;
        let combined_quantity = existing_quantity
            .checked_add(quantity)
            .ok_or(ItemError::ArithmeticOverflow)?;
        let after_slots = slots_for_quantity(combined_quantity, stack_limit)?;
        let projected_used = used_before
            .checked_sub(before_slots)
            .and_then(|value| value.checked_add(after_slots))
            .ok_or(ItemError::ArithmeticOverflow)?;

        let pending = projected_used > capacity;
        if pending {
            sqlx::query(
                r#"
                INSERT INTO pending_asset_deliveries (
                    id, operation_id, player_id, definition_key, definition_version,
                    quantity, desired_location, reason
                )
                VALUES ($1, $2, $3, $4, $5, $6, 'ITEM_BAG', 'CAPACITY')
                "#,
            )
            .bind(OperationId::new().as_uuid())
            .bind(operation_id)
            .bind(player.player_id)
            .bind(&definition.key)
            .bind(definition.version)
            .bind(quantity)
            .execute(&mut *tx)
            .await?;
        } else {
            sqlx::query(
                r#"
                INSERT INTO item_stacks (
                    player_id, definition_key, definition_version, location, quantity
                )
                VALUES ($1, $2, $3, 'ITEM_BAG', $4)
                ON CONFLICT (player_id, definition_key, definition_version, location)
                DO UPDATE SET quantity = item_stacks.quantity + EXCLUDED.quantity,
                              updated_at = now()
                "#,
            )
            .bind(player.player_id)
            .bind(&definition.key)
            .bind(definition.version)
            .bind(quantity)
            .execute(&mut *tx)
            .await?;
            insert_asset_event(
                &mut tx,
                operation_id,
                player.player_id,
                "STACK_DELIVERED",
                json!({
                    "definition_key": definition.key,
                    "definition_version": definition.version,
                    "quantity": quantity,
                    "location": "ITEM_BAG",
                }),
            )
            .await?;
        }

        let receipt = ItemMutationReceipt {
            operation_id,
            kind,
            item_instance_id: None,
            slot: None,
            displaced_item_instance_id: None,
            definition_key: Some(definition.key),
            quantity: Some(quantity),
            pending,
        };
        commit_operation(&mut tx, player.player_id, &receipt).await?;
        insert_item_outbox(
            &mut tx,
            &receipt,
            if pending {
                "asset.delivery_pending"
            } else {
                "asset.stack_delivered"
            },
        )
        .await?;
        tx.commit().await?;
        Ok(receipt)
    }

    async fn player_id(&self, discord_user_id: u64) -> Result<Uuid, ItemError> {
        let discord_user_id = snowflake_to_i64(discord_user_id)?;
        sqlx::query("SELECT id FROM players WHERE discord_user_id = $1 AND status <> 'DELETED'")
            .bind(discord_user_id)
            .fetch_optional(self.store.pool())
            .await?
            .ok_or(ItemError::PlayerNotFound)?
            .try_get("id")
            .map_err(ItemError::from)
    }
}

async fn resolve_operation(
    tx: &mut Transaction<'_, Postgres>,
    discord_user_id: i64,
    external_request_key: &str,
    kind: ItemMutationKind,
    request_hash: &[u8; 32],
) -> Result<OperationResolution, ItemError> {
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
    .bind(ITEM_POLICY_VERSION)
    .bind(request_hash.as_slice())
    .bind(rng_root.as_bytes().as_slice())
    .execute(&mut **tx)
    .await?;

    let row = select_operation(tx, external_request_key)
        .await?
        .ok_or(ItemError::OperationMissingAfterInsert)?;
    validate_operation_row(row, discord_user_id, kind, request_hash)
}

async fn select_operation(
    tx: &mut Transaction<'_, Postgres>,
    external_request_key: &str,
) -> Result<Option<sqlx::postgres::PgRow>, ItemError> {
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
    kind: ItemMutationKind,
    request_hash: &[u8; 32],
) -> Result<OperationResolution, ItemError> {
    let stored_actor: Option<i64> = row.try_get("actor_discord_user_id")?;
    let stored_kind: String = row.try_get("kind")?;
    let stored_policy: i32 = row.try_get("policy_version")?;
    let stored_request_hash: Vec<u8> = row.try_get("request_hash")?;
    if stored_actor != Some(discord_user_id)
        || stored_kind != kind.operation_kind()
        || stored_policy != ITEM_POLICY_VERSION
        || stored_request_hash.as_slice() != request_hash.as_slice()
    {
        return Err(ItemError::IdempotencyConflict);
    }

    let state: String = row.try_get("state")?;
    if state == "COMMITTED" {
        let value: Value = row.try_get("result")?;
        let receipt = serde_json::from_value(value)
            .map_err(|error| ItemError::InvalidOperationResult(Box::new(error)))?;
        return Ok(OperationResolution::Committed(receipt));
    }
    if state != "PENDING" {
        return Err(ItemError::OperationTerminal(state));
    }
    Ok(OperationResolution::Pending(row.try_get("id")?))
}

async fn lock_player(
    tx: &mut Transaction<'_, Postgres>,
    discord_user_id: i64,
) -> Result<LockedPlayer, ItemError> {
    let row = sqlx::query(
        r#"
        SELECT p.id, p.status, s.item_bag_level
          FROM players p
          JOIN player_storage_profiles s ON s.player_id = p.id
         WHERE p.discord_user_id = $1
           AND p.status <> 'DELETED'
         FOR UPDATE OF p, s
        "#,
    )
    .bind(discord_user_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(ItemError::PlayerNotFound)?;

    Ok(LockedPlayer {
        player_id: row.try_get("id")?,
        status: row.try_get("status")?,
        item_bag_level: row.try_get("item_bag_level")?,
    })
}

fn ensure_mutable(status: &str) -> Result<(), ItemError> {
    if status == "ACTIVE" {
        Ok(())
    } else {
        Err(ItemError::AccountFrozen(status.to_owned()))
    }
}

async fn lock_owned_item(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
    item_id: Uuid,
) -> Result<LockedItem, ItemError> {
    let row = sqlx::query(
        r#"
        SELECT i.location,
               i.definition_key,
               i.definition_version,
               d.category,
               d.stackable,
               d.stack_limit,
               d.data
          FROM item_instances i
          JOIN item_definition_versions d
            ON d.key = i.definition_key
           AND d.version = i.definition_version
         WHERE i.id = $1
           AND i.owner_player_id = $2
         FOR UPDATE OF i
        "#,
    )
    .bind(item_id)
    .bind(player_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(ItemError::ItemNotFound)?;

    Ok(LockedItem {
        location: row.try_get("location")?,
        definition: DefinitionVersion {
            key: row.try_get("definition_key")?,
            version: row.try_get("definition_version")?,
            category: row.try_get("category")?,
            stackable: row.try_get("stackable")?,
            stack_limit: row.try_get("stack_limit")?,
            data: row.try_get("data")?,
        },
    })
}

async fn load_current_stack_definition(
    tx: &mut Transaction<'_, Postgres>,
    key: &str,
) -> Result<DefinitionVersion, ItemError> {
    let row = sqlx::query(
        r#"
        SELECT d.key,
               d.definition_version,
               v.category,
               v.stackable,
               v.stack_limit,
               v.data
          FROM item_definitions d
          JOIN item_definition_versions v
            ON v.key = d.key
           AND v.version = d.definition_version
         WHERE d.key = $1
           AND d.active = TRUE
         FOR SHARE OF d
        "#,
    )
    .bind(key)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(ItemError::InvalidStackDefinition)?;
    let stackable: bool = row.try_get("stackable")?;
    let stack_limit: Option<i64> = row.try_get("stack_limit")?;
    if !stackable || stack_limit.is_none_or(|value| value <= 0) {
        return Err(ItemError::InvalidStackDefinition);
    }
    Ok(DefinitionVersion {
        key: row.try_get("key")?,
        version: row.try_get("definition_version")?,
        category: row.try_get("category")?,
        stackable,
        stack_limit,
        data: row.try_get("data")?,
    })
}

fn equipment_slot(definition: &DefinitionVersion) -> Result<String, ItemError> {
    if definition.stackable {
        return Err(ItemError::NotEquippable);
    }
    let slot = match definition.category.as_str() {
        "PICKAXE" => "PICKAXE",
        "SWORD" => "SWORD",
        "FISHING_ROD" => "FISHING_ROD",
        "TOTEM" => "TOTEM",
        "ARMOR" => definition
            .data
            .get("slot")
            .and_then(Value::as_str)
            .ok_or(ItemError::NotEquippable)?,
        _ => return Err(ItemError::NotEquippable),
    };
    if !matches!(
        slot,
        "PICKAXE"
            | "SWORD"
            | "FISHING_ROD"
            | "ARMOR_HELMET"
            | "ARMOR_CHEST"
            | "ARMOR_LEGS"
            | "ARMOR_BOOTS"
            | "TOTEM"
    ) {
        return Err(ItemError::NotEquippable);
    }
    Ok(slot.to_owned())
}

async fn item_bag_used_slots(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
) -> Result<u64, ItemError> {
    let rows = sqlx::query(
        r#"
        SELECT s.quantity, d.stack_limit
          FROM item_stacks s
          JOIN item_definition_versions d
            ON d.key = s.definition_key
           AND d.version = s.definition_version
         WHERE s.player_id = $1
           AND s.location = 'ITEM_BAG'
         FOR UPDATE OF s
        "#,
    )
    .bind(player_id)
    .fetch_all(&mut **tx)
    .await?;
    let mut used = 0_u64;
    for row in rows {
        let quantity: i64 = row.try_get("quantity")?;
        let stack_limit: i64 = row.try_get("stack_limit")?;
        used = used
            .checked_add(slots_for_quantity(quantity, stack_limit)?)
            .ok_or(ItemError::ArithmeticOverflow)?;
    }
    Ok(used)
}

fn item_bag_capacity_slots(level: i64) -> Result<u64, ItemError> {
    if level < 0 {
        return Err(ItemError::ArithmeticOverflow);
    }
    let capacity = i128::from(ITEM_BAG_BASE_SLOTS)
        .checked_add(
            i128::from(ITEM_BAG_SLOTS_PER_LEVEL)
                .checked_mul(i128::from(level))
                .ok_or(ItemError::ArithmeticOverflow)?,
        )
        .ok_or(ItemError::ArithmeticOverflow)?;
    u64::try_from(capacity).map_err(|_| ItemError::ArithmeticOverflow)
}

fn catch_bag_capacity_grams(level: i64) -> Result<i64, ItemError> {
    if level < 0 {
        return Err(ItemError::ArithmeticOverflow);
    }
    let capacity = i128::from(CATCH_BAG_BASE_GRAMS)
        .checked_add(
            i128::from(CATCH_BAG_GRAMS_PER_LEVEL)
                .checked_mul(i128::from(level))
                .ok_or(ItemError::ArithmeticOverflow)?,
        )
        .ok_or(ItemError::ArithmeticOverflow)?;
    i64::try_from(capacity).map_err(|_| ItemError::ArithmeticOverflow)
}

fn slots_for_quantity(quantity: i64, stack_limit: i64) -> Result<u64, ItemError> {
    if quantity < 0 || stack_limit <= 0 {
        return Err(ItemError::ArithmeticOverflow);
    }
    if quantity == 0 {
        return Ok(0);
    }
    let numerator = i128::from(quantity)
        .checked_add(i128::from(stack_limit) - 1)
        .ok_or(ItemError::ArithmeticOverflow)?;
    u64::try_from(numerator / i128::from(stack_limit)).map_err(|_| ItemError::ArithmeticOverflow)
}

async fn commit_operation(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
    receipt: &ItemMutationReceipt,
) -> Result<(), ItemError> {
    let result = serde_json::to_value(receipt).expect("item receipt is serializable");
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
        return Err(ItemError::OperationTerminal(
            "unexpected transition".to_owned(),
        ));
    }
    Ok(())
}

async fn insert_asset_event(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
    event_kind: &str,
    payload: Value,
) -> Result<(), ItemError> {
    sqlx::query(
        "INSERT INTO asset_events (id, operation_id, player_id, event_kind, payload) VALUES ($1, $2, $3, $4, $5)",
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

async fn insert_item_outbox(
    tx: &mut Transaction<'_, Postgres>,
    receipt: &ItemMutationReceipt,
    topic: &str,
) -> Result<(), ItemError> {
    sqlx::query(
        "INSERT INTO outbox_events (id, operation_id, topic, payload) VALUES ($1, $2, $3, $4) ON CONFLICT (operation_id, topic) DO NOTHING",
    )
    .bind(OperationId::new().as_uuid())
    .bind(receipt.operation_id)
    .bind(topic)
    .bind(json!({
        "kind": receipt.kind,
        "item_instance_id": receipt.item_instance_id,
        "slot": receipt.slot,
        "displaced_item_instance_id": receipt.displaced_item_instance_id,
        "definition_key": receipt.definition_key,
        "quantity": receipt.quantity,
        "pending": receipt.pending,
    }))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn item_request_hash(
    kind: ItemMutationKind,
    subject: &[u8],
    quantity: Option<i64>,
    location: Option<&str>,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"graphite.operation.item.v1\0");
    hasher.update(kind.operation_kind().as_bytes());
    hasher.update(b"\0");
    hasher.update(subject);
    if let Some(quantity) = quantity {
        hasher.update(b"\0quantity\0");
        hasher.update(&quantity.to_be_bytes());
    }
    if let Some(location) = location {
        hasher.update(b"\0location\0");
        hasher.update(location.as_bytes());
    }
    *hasher.finalize().as_bytes()
}

fn row_to_item_view(row: sqlx::postgres::PgRow) -> Result<ItemView, ItemError> {
    Ok(ItemView {
        item_instance_id: row.try_get("id")?,
        definition_key: row.try_get("definition_key")?,
        definition_version: row.try_get("definition_version")?,
        category: row.try_get("category")?,
        rarity: row.try_get("rarity")?,
        location: row.try_get("location")?,
        is_starter: row.try_get("is_starter")?,
        is_account_bound: row.try_get("is_account_bound")?,
        is_tradeable: row.try_get("is_tradeable")?,
        is_sellable: row.try_get("is_sellable")?,
        is_discardable: row.try_get("is_discardable")?,
        is_enchantable: row.try_get("is_enchantable")?,
        is_upgradeable: row.try_get("is_upgradeable")?,
        is_unbreakable: row.try_get("is_unbreakable")?,
        is_repairable: row.try_get("is_repairable")?,
        current_durability: row.try_get("current_durability")?,
        max_durability: row.try_get("max_durability")?,
        catch_weight_grams: row.try_get("catch_weight_grams")?,
        state: row.try_get("state")?,
    })
}

fn snowflake_to_i64(value: u64) -> Result<i64, ItemError> {
    i64::try_from(value).map_err(|_| ItemError::SnowflakeOutOfRange)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_curves_match_frozen_storage_baselines() {
        assert_eq!(item_bag_capacity_slots(0).unwrap(), 36);
        assert_eq!(item_bag_capacity_slots(1).unwrap(), 42);
        assert_eq!(item_bag_capacity_slots(10).unwrap(), 96);
        assert_eq!(catch_bag_capacity_grams(0).unwrap(), 1_000_000);
        assert_eq!(catch_bag_capacity_grams(1).unwrap(), 1_250_000);
    }

    #[test]
    fn stack_slot_math_uses_definition_specific_caps() {
        assert_eq!(slots_for_quantity(0, 64).unwrap(), 0);
        assert_eq!(slots_for_quantity(1, 64).unwrap(), 1);
        assert_eq!(slots_for_quantity(64, 64).unwrap(), 1);
        assert_eq!(slots_for_quantity(65, 64).unwrap(), 2);
    }
}
