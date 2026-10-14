use chrono::{DateTime, Utc};
use graphite_core::OperationId;
use graphite_items::{ItemError, catch_bag_capacity_grams, catch_bag_used_grams};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    FISHING_WEIGHT_BRIDGE_POLICY_VERSION, FishingArea, FishingFishInstanceNumericV1, FishingRarity,
    FishingSpecies, FishingWeightBridgeError, MAX_FISH_PER_CAST, fishing_species_policy,
};

pub const FISHING_FISH_ITEM_DEFINITION_VERSION: i32 = 1;
const DELIVERY_EVENT_KIND: &str = "FISHING_FISH_INSTANCE_DELIVERED";
const CLAIM_EVENT_KIND: &str = "FISHING_PENDING_CATCHES_CLAIMED";
const CLAIM_MUTATION_KEY: &str = "fishing:pending-catches:claim";
const PENDING_REASON: &str = "CATCH_BAG_CAPACITY";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FishingFishDeliveryLocation {
    CatchBag,
    PendingCatchBag,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FishingFishInstanceDeliveryRequestV1 {
    pub fish_slot: u8,
    pub area: FishingArea,
    pub numeric: FishingFishInstanceNumericV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FishingFishInstanceDeliveryReceiptV1 {
    pub event_id: Uuid,
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub item_instance_id: Uuid,
    pub definition_key: String,
    pub definition_version: i32,
    pub fish_slot: u8,
    pub area: FishingArea,
    pub species: FishingSpecies,
    pub rarity: FishingRarity,
    pub caught_at: DateTime<Utc>,
    pub catch_weight_grams: i64,
    pub canonical_weight_scaled_grams: i64,
    pub location: FishingFishDeliveryLocation,
    pub pending_delivery_id: Option<Uuid>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FishingClaimedCatchV1 {
    pub pending_delivery_id: Uuid,
    pub item_instance_id: Uuid,
    pub catch_weight_grams: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FishingPendingCatchClaimReceiptV1 {
    pub event_id: Uuid,
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub claimed: Vec<FishingClaimedCatchV1>,
    pub blocked_pending_delivery_id: Option<Uuid>,
    pub used_grams_after: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct FishingFishInstanceDeliveryEventV1 {
    request: FishingFishInstanceDeliveryRequestV1,
    receipt: FishingFishInstanceDeliveryReceiptV1,
}

#[derive(Debug, Error)]
pub enum FishingFishInstanceDeliveryError {
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error(transparent)]
    Item(#[from] ItemError),
    #[error(transparent)]
    WeightBridge(#[from] FishingWeightBridgeError),
    #[error("Fishing fish slot must be within 0..{MAX_FISH_PER_CAST}; got {0}")]
    FishSlotOutOfRange(u8),
    #[error("owning operation does not exist")]
    OperationNotFound,
    #[error("owning operation targets a different player")]
    OperationPlayerMismatch,
    #[error("owning operation cannot deliver a FishInstance in state {0}")]
    OperationTerminal(String),
    #[error("player does not exist")]
    PlayerNotFound,
    #[error("FishInstance delivery requires an ACTIVE account; current status is {0}")]
    AccountFrozen(String),
    #[error("Fishing Fish ItemDefinition is missing or inconsistent for {0:?}")]
    FishDefinitionMismatch(FishingSpecies),
    #[error("CatchBag has an unresolved pending catch from an earlier operation")]
    ExistingPendingCatchDelivery,
    #[error("Fishing FishInstance arithmetic exceeded supported integer bounds")]
    ArithmeticOverflow,
    #[error("stored FishInstance delivery event payload is invalid: {0}")]
    InvalidStoredEvent(Box<serde_json::Error>),
    #[error("the same Fishing fish-slot delivery was replayed with different input")]
    DeliveryMutationConflict,
    #[error("pending Fishing catch {0} is internally inconsistent")]
    PendingCatchIntegrityMismatch(Uuid),
    #[error("stored pending-catch claim event is internally inconsistent")]
    ClaimMutationConflict,
}

impl From<sqlx::Error> for FishingFishInstanceDeliveryError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

/// Persists one already-resolved FishInstance and delivers it to CatchBag or bounded pending overflow.
///
/// This is a transaction-composable child mutation. It does not choose species/variant, derive RNG,
/// resolve line-break/over-cap behavior, award progression, mutate the Rod, consume bait, or finalize
/// the owning operation. The future cast owner supplies the already-authoritative numeric FishInstance
/// and commits every cast consequence in the same transaction.
///
/// A FishInstance is always a unique non-transferable ItemInstance. The canonical 10^12-scaled weight
/// is persisted alongside the one-time round-half-up whole-gram CatchBag projection. Biological noise,
/// size draws, length, NPC value, catch area and the owning operation timestamp are retained in the
/// immutable instance state so retries never recreate biological data from display-rounded values.
///
/// When the current CatchBag lacks capacity, the unique asset is created in TEMP_OVERFLOW and linked
/// to a PENDING CatchBag delivery rather than being destroyed. Existing unresolved pending CatchBag
/// output from a previous operation blocks further catch generation, bounding overflow to at most one
/// in-flight cast. Additional fish slots from the same owning cast may still settle independently.
pub async fn deliver_fishing_fish_instance_v1(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
    request: FishingFishInstanceDeliveryRequestV1,
) -> Result<FishingFishInstanceDeliveryReceiptV1, FishingFishInstanceDeliveryError> {
    if request.fish_slot >= MAX_FISH_PER_CAST {
        return Err(FishingFishInstanceDeliveryError::FishSlotOutOfRange(
            request.fish_slot,
        ));
    }

    let mutation_key = delivery_mutation_key(request.fish_slot);
    let operation = lock_operation(tx, operation_id, player_id).await?;
    if let Some(receipt) =
        replay_delivery(tx, operation_id, player_id, &mutation_key, request).await?
    {
        return Ok(receipt);
    }
    if operation.state != "PENDING" {
        return Err(FishingFishInstanceDeliveryError::OperationTerminal(
            operation.state,
        ));
    }

    let storage = lock_active_player_storage(tx, player_id).await?;
    validate_fish_definition(tx, request.numeric.species).await?;

    let existing_pending_from_other_operation: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
              FROM pending_asset_deliveries
             WHERE player_id = $1
               AND desired_location = 'CATCH_BAG'
               AND state = 'PENDING'
               AND operation_id <> $2
        )
        "#,
    )
    .bind(player_id)
    .bind(operation_id)
    .fetch_one(&mut **tx)
    .await?;
    if existing_pending_from_other_operation {
        return Err(FishingFishInstanceDeliveryError::ExistingPendingCatchDelivery);
    }

    let bridge = request.numeric.catch_weight_bridge()?;
    if bridge.policy_version != FISHING_WEIGHT_BRIDGE_POLICY_VERSION {
        return Err(FishingFishInstanceDeliveryError::ArithmeticOverflow);
    }
    let catch_weight_grams = i64::try_from(bridge.catch_bag_weight_grams)
        .map_err(|_| FishingFishInstanceDeliveryError::ArithmeticOverflow)?;
    let canonical_weight_scaled_grams = i64::try_from(bridge.canonical_weight_scaled_grams)
        .map_err(|_| FishingFishInstanceDeliveryError::ArithmeticOverflow)?;

    let used_grams = catch_bag_used_grams(tx, player_id).await?;
    let capacity_grams = catch_bag_capacity_grams(storage.catch_bag_level)?;
    let projected_used = i128::from(used_grams)
        .checked_add(i128::from(catch_weight_grams))
        .ok_or(FishingFishInstanceDeliveryError::ArithmeticOverflow)?;
    let fits_catch_bag = projected_used <= i128::from(capacity_grams);

    let species = request.numeric.species;
    let rarity = fishing_species_policy(species).rarity;
    let definition_key = species.item_definition_key();
    let item_instance_id = OperationId::new().as_uuid();
    let location = if fits_catch_bag {
        FishingFishDeliveryLocation::CatchBag
    } else {
        FishingFishDeliveryLocation::PendingCatchBag
    };
    let persisted_location = match location {
        FishingFishDeliveryLocation::CatchBag => "CATCH_BAG",
        FishingFishDeliveryLocation::PendingCatchBag => "TEMP_OVERFLOW",
    };

    let state = json!({
        "provenance": "FISHING_CATCH",
        "numeric_policy_version": request.numeric.policy_version,
        "species": species,
        "rarity": rarity,
        "variant": request.numeric.variant,
        "area": request.area,
        "caught_at": operation.created_at,
        "fixed_point_scale": request.numeric.fixed_point_scale(),
        "weight_quantile_ppm": request.numeric.weight_quantile_ppm,
        "weight_ratio_scaled": request.numeric.weight_ratio_scaled(),
        "biological_noise_quantile_ppm": request.numeric.biological_noise_quantile_ppm,
        "biological_noise_scaled": request.numeric.biological_noise_scaled(),
        "length_scaled_millimeters": request.numeric.length_scaled_millimeters(),
        "npc_value_money": request.numeric.npc_value_money,
        "quality_bait_active": request.numeric.quality_bait_active,
    });

    sqlx::query(
        r#"
        INSERT INTO item_instances (
            id, definition_key, definition_version, owner_player_id, created_by_operation_id,
            location, is_starter, is_account_bound, is_tradeable, is_sellable, is_discardable,
            is_enchantable, is_upgradeable, is_unbreakable, is_repairable,
            catch_weight_grams, catch_weight_scaled_grams, state
        )
        VALUES (
            $1, $2, $3, $4, $5,
            $6, FALSE, TRUE, FALSE, TRUE, TRUE,
            FALSE, FALSE, FALSE, FALSE,
            $7, $8, $9
        )
        "#,
    )
    .bind(item_instance_id)
    .bind(definition_key)
    .bind(FISHING_FISH_ITEM_DEFINITION_VERSION)
    .bind(player_id)
    .bind(operation_id)
    .bind(persisted_location)
    .bind(catch_weight_grams)
    .bind(canonical_weight_scaled_grams)
    .bind(state)
    .execute(&mut **tx)
    .await?;

    let pending_delivery_id = if matches!(location, FishingFishDeliveryLocation::PendingCatchBag) {
        let pending_delivery_id = OperationId::new().as_uuid();
        sqlx::query(
            r#"
            INSERT INTO pending_asset_deliveries (
                id, operation_id, mutation_key, player_id, definition_key, definition_version,
                quantity, desired_location, reason, item_instance_id
            )
            VALUES ($1, $2, $3, $4, $5, $6, 1, 'CATCH_BAG', $7, $8)
            "#,
        )
        .bind(pending_delivery_id)
        .bind(operation_id)
        .bind(&mutation_key)
        .bind(player_id)
        .bind(definition_key)
        .bind(FISHING_FISH_ITEM_DEFINITION_VERSION)
        .bind(PENDING_REASON)
        .bind(item_instance_id)
        .execute(&mut **tx)
        .await?;
        Some(pending_delivery_id)
    } else {
        None
    };

    let receipt = FishingFishInstanceDeliveryReceiptV1 {
        event_id: OperationId::new().as_uuid(),
        operation_id,
        player_id,
        item_instance_id,
        definition_key: definition_key.to_owned(),
        definition_version: FISHING_FISH_ITEM_DEFINITION_VERSION,
        fish_slot: request.fish_slot,
        area: request.area,
        species,
        rarity,
        caught_at: operation.created_at,
        catch_weight_grams,
        canonical_weight_scaled_grams,
        location,
        pending_delivery_id,
    };

    let payload = serde_json::to_value(FishingFishInstanceDeliveryEventV1 {
        request,
        receipt: receipt.clone(),
    })
    .map_err(|error| FishingFishInstanceDeliveryError::InvalidStoredEvent(Box::new(error)))?;

    sqlx::query(
        r#"
        INSERT INTO asset_events (
            id, operation_id, mutation_key, player_id, event_kind, payload
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(receipt.event_id)
    .bind(operation_id)
    .bind(&mutation_key)
    .bind(player_id)
    .bind(DELIVERY_EVENT_KIND)
    .bind(payload)
    .execute(&mut **tx)
    .await?;

    Ok(receipt)
}

/// Moves as many FIFO pending unique FishInstances into CatchBag as current capacity permits.
///
/// This is intended to run near the start of a later manual-Fishing owner transaction before new
/// catch generation. The owning operation/player/storage rows serialize capacity state, current
/// CatchBag items are locked before projection, and pending rows plus their unique TEMP_OVERFLOW
/// ItemInstances are then locked in FIFO order. A first item that does not fit stops the pass; later
/// pending fish cannot leapfrog it.
///
/// The mutation is operation-key idempotent. If the surrounding owner rolls back, moved items,
/// pending-state transitions and this audit receipt roll back together. The function does not alter
/// the original catch operation and does not claim stack-shaped/non-fish pending deliveries.
pub async fn claim_pending_fishing_catches_v1(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
) -> Result<FishingPendingCatchClaimReceiptV1, FishingFishInstanceDeliveryError> {
    let operation = lock_operation(tx, operation_id, player_id).await?;
    if let Some(receipt) = replay_claim(tx, operation_id, player_id).await? {
        return Ok(receipt);
    }
    if operation.state != "PENDING" {
        return Err(FishingFishInstanceDeliveryError::OperationTerminal(
            operation.state,
        ));
    }

    let storage = lock_active_player_storage(tx, player_id).await?;
    let capacity_grams = catch_bag_capacity_grams(storage.catch_bag_level)?;
    let mut used_grams = catch_bag_used_grams(tx, player_id).await?;

    let rows = sqlx::query(
        r#"
        SELECT p.id AS pending_delivery_id,
               p.item_instance_id,
               p.definition_key,
               p.definition_version,
               p.quantity,
               i.owner_player_id,
               i.location,
               i.definition_key AS item_definition_key,
               i.definition_version AS item_definition_version,
               i.catch_weight_grams,
               d.category
          FROM pending_asset_deliveries p
          JOIN item_instances i ON i.id = p.item_instance_id
          JOIN item_definition_versions d
            ON d.key = i.definition_key
           AND d.version = i.definition_version
         WHERE p.player_id = $1
           AND p.state = 'PENDING'
           AND p.desired_location = 'CATCH_BAG'
           AND p.item_instance_id IS NOT NULL
         ORDER BY p.created_at, p.id
         FOR UPDATE OF p, i
        "#,
    )
    .bind(player_id)
    .fetch_all(&mut **tx)
    .await?;

    let mut claimed = Vec::with_capacity(rows.len());
    let mut blocked_pending_delivery_id = None;

    for row in rows {
        let pending_delivery_id: Uuid = row.try_get("pending_delivery_id")?;
        let item_instance_id: Uuid = row.try_get("item_instance_id")?;
        let definition_key: String = row.try_get("definition_key")?;
        let definition_version: i32 = row.try_get("definition_version")?;
        let quantity: i64 = row.try_get("quantity")?;
        let owner_player_id: Uuid = row.try_get("owner_player_id")?;
        let location: String = row.try_get("location")?;
        let item_definition_key: String = row.try_get("item_definition_key")?;
        let item_definition_version: i32 = row.try_get("item_definition_version")?;
        let catch_weight_grams: Option<i64> = row.try_get("catch_weight_grams")?;
        let category: String = row.try_get("category")?;

        if quantity != 1
            || owner_player_id != player_id
            || location != "TEMP_OVERFLOW"
            || item_definition_key != definition_key
            || item_definition_version != definition_version
            || category != "FISH"
        {
            return Err(
                FishingFishInstanceDeliveryError::PendingCatchIntegrityMismatch(
                    pending_delivery_id,
                ),
            );
        }

        let catch_weight_grams = catch_weight_grams.ok_or(
            FishingFishInstanceDeliveryError::PendingCatchIntegrityMismatch(pending_delivery_id),
        )?;
        if catch_weight_grams <= 0 {
            return Err(
                FishingFishInstanceDeliveryError::PendingCatchIntegrityMismatch(
                    pending_delivery_id,
                ),
            );
        }

        let projected_used = i128::from(used_grams)
            .checked_add(i128::from(catch_weight_grams))
            .ok_or(FishingFishInstanceDeliveryError::ArithmeticOverflow)?;
        if projected_used > i128::from(capacity_grams) {
            blocked_pending_delivery_id = Some(pending_delivery_id);
            break;
        }

        let moved = sqlx::query(
            "UPDATE item_instances SET location = 'CATCH_BAG' WHERE id = $1 AND owner_player_id = $2 AND location = 'TEMP_OVERFLOW'",
        )
        .bind(item_instance_id)
        .bind(player_id)
        .execute(&mut **tx)
        .await?;
        let resolved = sqlx::query(
            "UPDATE pending_asset_deliveries SET state = 'CLAIMED', resolved_at = now() WHERE id = $1 AND state = 'PENDING'",
        )
        .bind(pending_delivery_id)
        .execute(&mut **tx)
        .await?;
        if moved.rows_affected() != 1 || resolved.rows_affected() != 1 {
            return Err(
                FishingFishInstanceDeliveryError::PendingCatchIntegrityMismatch(
                    pending_delivery_id,
                ),
            );
        }

        used_grams = i64::try_from(projected_used)
            .map_err(|_| FishingFishInstanceDeliveryError::ArithmeticOverflow)?;
        claimed.push(FishingClaimedCatchV1 {
            pending_delivery_id,
            item_instance_id,
            catch_weight_grams,
        });
    }

    let receipt = FishingPendingCatchClaimReceiptV1 {
        event_id: OperationId::new().as_uuid(),
        operation_id,
        player_id,
        claimed,
        blocked_pending_delivery_id,
        used_grams_after: used_grams,
    };
    let payload = serde_json::to_value(&receipt)
        .map_err(|error| FishingFishInstanceDeliveryError::InvalidStoredEvent(Box::new(error)))?;
    sqlx::query(
        r#"
        INSERT INTO asset_events (
            id, operation_id, mutation_key, player_id, event_kind, payload
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(receipt.event_id)
    .bind(operation_id)
    .bind(CLAIM_MUTATION_KEY)
    .bind(player_id)
    .bind(CLAIM_EVENT_KIND)
    .bind(payload)
    .execute(&mut **tx)
    .await?;

    Ok(receipt)
}

async fn replay_claim(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
) -> Result<Option<FishingPendingCatchClaimReceiptV1>, FishingFishInstanceDeliveryError> {
    let Some(row) = sqlx::query(
        r#"
        SELECT id, player_id, event_kind, payload
          FROM asset_events
         WHERE operation_id = $1
           AND mutation_key = $2
         FOR UPDATE
        "#,
    )
    .bind(operation_id)
    .bind(CLAIM_MUTATION_KEY)
    .fetch_optional(&mut **tx)
    .await?
    else {
        return Ok(None);
    };

    let event_id: Uuid = row.try_get("id")?;
    let stored_player_id: Uuid = row.try_get("player_id")?;
    let event_kind: String = row.try_get("event_kind")?;
    let payload: Value = row.try_get("payload")?;
    let receipt: FishingPendingCatchClaimReceiptV1 = serde_json::from_value(payload)
        .map_err(|error| FishingFishInstanceDeliveryError::InvalidStoredEvent(Box::new(error)))?;

    if event_kind != CLAIM_EVENT_KIND
        || event_id != receipt.event_id
        || stored_player_id != player_id
        || receipt.operation_id != operation_id
        || receipt.player_id != player_id
    {
        return Err(FishingFishInstanceDeliveryError::ClaimMutationConflict);
    }

    Ok(Some(receipt))
}

struct LockedOperation {
    state: String,
    created_at: DateTime<Utc>,
}

struct LockedStorage {
    catch_bag_level: i64,
}

async fn lock_operation(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
) -> Result<LockedOperation, FishingFishInstanceDeliveryError> {
    let row =
        sqlx::query("SELECT player_id, state, created_at FROM operations WHERE id = $1 FOR UPDATE")
            .bind(operation_id)
            .fetch_optional(&mut **tx)
            .await?
            .ok_or(FishingFishInstanceDeliveryError::OperationNotFound)?;

    let stored_player_id: Option<Uuid> = row.try_get("player_id")?;
    if stored_player_id != Some(player_id) {
        return Err(FishingFishInstanceDeliveryError::OperationPlayerMismatch);
    }

    Ok(LockedOperation {
        state: row.try_get("state")?,
        created_at: row.try_get("created_at")?,
    })
}

async fn lock_active_player_storage(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
) -> Result<LockedStorage, FishingFishInstanceDeliveryError> {
    let row = sqlx::query(
        r#"
        SELECT p.status, s.catch_bag_level
          FROM players p
          JOIN player_storage_profiles s ON s.player_id = p.id
         WHERE p.id = $1
           AND p.status <> 'DELETED'
         FOR UPDATE OF p, s
        "#,
    )
    .bind(player_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(FishingFishInstanceDeliveryError::PlayerNotFound)?;
    let status: String = row.try_get("status")?;
    if status != "ACTIVE" {
        return Err(FishingFishInstanceDeliveryError::AccountFrozen(status));
    }

    Ok(LockedStorage {
        catch_bag_level: row.try_get("catch_bag_level")?,
    })
}

async fn validate_fish_definition(
    tx: &mut Transaction<'_, Postgres>,
    species: FishingSpecies,
) -> Result<(), FishingFishInstanceDeliveryError> {
    let expected_key = species.item_definition_key();
    let expected_rarity = fishing_species_policy(species).rarity.persisted_key();
    let row = sqlx::query(
        r#"
        SELECT d.definition_version, v.category, v.stackable, v.rarity
          FROM item_definitions d
          JOIN item_definition_versions v
            ON v.key = d.key
           AND v.version = d.definition_version
         WHERE d.key = $1
           AND d.active = TRUE
         FOR SHARE OF d
        "#,
    )
    .bind(expected_key)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(FishingFishInstanceDeliveryError::FishDefinitionMismatch(
        species,
    ))?;

    let definition_version: i32 = row.try_get("definition_version")?;
    let category: String = row.try_get("category")?;
    let stackable: bool = row.try_get("stackable")?;
    let rarity: String = row.try_get("rarity")?;
    if definition_version != FISHING_FISH_ITEM_DEFINITION_VERSION
        || category != "FISH"
        || stackable
        || rarity != expected_rarity
    {
        return Err(FishingFishInstanceDeliveryError::FishDefinitionMismatch(
            species,
        ));
    }

    Ok(())
}

async fn replay_delivery(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
    mutation_key: &str,
    request: FishingFishInstanceDeliveryRequestV1,
) -> Result<Option<FishingFishInstanceDeliveryReceiptV1>, FishingFishInstanceDeliveryError> {
    let Some(row) = sqlx::query(
        r#"
        SELECT id, player_id, event_kind, payload
          FROM asset_events
         WHERE operation_id = $1
           AND mutation_key = $2
         FOR UPDATE
        "#,
    )
    .bind(operation_id)
    .bind(mutation_key)
    .fetch_optional(&mut **tx)
    .await?
    else {
        return Ok(None);
    };

    let event_id: Uuid = row.try_get("id")?;
    let stored_player_id: Uuid = row.try_get("player_id")?;
    let event_kind: String = row.try_get("event_kind")?;
    let payload: Value = row.try_get("payload")?;
    let event: FishingFishInstanceDeliveryEventV1 = serde_json::from_value(payload)
        .map_err(|error| FishingFishInstanceDeliveryError::InvalidStoredEvent(Box::new(error)))?;

    if event_kind != DELIVERY_EVENT_KIND
        || stored_player_id != player_id
        || event.receipt.event_id != event_id
        || event.receipt.operation_id != operation_id
        || event.receipt.player_id != player_id
        || event.request != request
    {
        return Err(FishingFishInstanceDeliveryError::DeliveryMutationConflict);
    }

    Ok(Some(event.receipt))
}

fn delivery_mutation_key(fish_slot: u8) -> String {
    format!("fishing:fish:slot:{fish_slot}:delivery")
}
