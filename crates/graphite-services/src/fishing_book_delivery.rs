use graphite_items::{
    UniqueItemBagDeliveryMutationError, UniqueItemBagDeliveryMutationReceipt,
    UniqueItemBagDeliveryMutationRequest, UniqueItemInstanceFlags,
    apply_unique_item_bag_delivery_mutation,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    FishingDirectBookCandidateV1, direct_fishing_book_pool_membership,
    direct_fishing_finished_book_level_weight,
};

pub const FINISHED_ENCHANT_BOOK_DEFINITION_KEY: &str = "enchant.book.finished";
pub const FINISHED_ENCHANT_BOOK_DEFINITION_VERSION: i32 = 1;
const BOOK_DELIVERY_SOURCE: &str = "MANUAL_FISHING";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FishingDirectBookDeliveryReceiptV1 {
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub candidate: FishingDirectBookCandidateV1,
    pub delivery: UniqueItemBagDeliveryMutationReceipt,
}

#[derive(Debug, Error)]
pub enum FishingDirectBookDeliveryError {
    #[error(transparent)]
    ItemBag(#[from] UniqueItemBagDeliveryMutationError),
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error("direct Fishing Book candidate is outside the frozen pool/level policy")]
    InvalidCandidate,
    #[error("persisted finished Enchant Book identity disagrees with its delivery receipt")]
    PersistedBookIntegrityMismatch,
}

impl From<sqlx::Error> for FishingDirectBookDeliveryError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

/// Creates one canonical Finished Enchant Book ItemInstance for an already-selected direct
/// Fishing Book candidate.
///
/// The Book identity is authoritative in item_instance_finished_enchant_books. Pool/slot data stays
/// in immutable Fishing provenance rather than becoming part of the reusable Book asset identity.
/// Item Bag overflow is lossless and bounded by the shared unique-delivery primitive.
///
/// Trade/NPC/discard behavior is deliberately not activated here. Those lifecycles do not yet have
/// an owning Book policy, so Fishing issuance creates an owned, usable unique asset while transfer
/// and liquidation flags remain fail-closed.
pub async fn deliver_fishing_direct_book_v1(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
    candidate: FishingDirectBookCandidateV1,
) -> Result<FishingDirectBookDeliveryReceiptV1, FishingDirectBookDeliveryError> {
    validate_candidate(candidate)?;

    let mutation_key = format!("fishing:book:slot:{}", candidate.treasure_slot);
    let delivery = apply_unique_item_bag_delivery_mutation(
        tx,
        &UniqueItemBagDeliveryMutationRequest {
            operation_id,
            player_id,
            mutation_key,
            definition_key: FINISHED_ENCHANT_BOOK_DEFINITION_KEY.to_owned(),
            definition_version: FINISHED_ENCHANT_BOOK_DEFINITION_VERSION,
            flags: UniqueItemInstanceFlags {
                is_account_bound: false,
                is_tradeable: false,
                is_sellable: false,
                is_discardable: false,
                is_enchantable: false,
                is_upgradeable: false,
                is_unbreakable: false,
                is_repairable: false,
            },
            state: json!({
                "asset_kind": "FINISHED_ENCHANT_BOOK",
            }),
            source: BOOK_DELIVERY_SOURCE.to_owned(),
            provenance: json!({
                "activity": "FISHING",
                "mode": "MANUAL",
                "treasure_slot": candidate.treasure_slot,
                "pool": candidate.pool,
                "enchant": candidate.enchant,
                "level": candidate.level,
            }),
        },
    )
    .await?;

    sqlx::query(
        r#"
        INSERT INTO item_instance_finished_enchant_books (
            item_instance_id, enchant_key, level
        )
        VALUES ($1, $2, $3)
        ON CONFLICT (item_instance_id) DO NOTHING
        "#,
    )
    .bind(delivery.item_instance_id)
    .bind(candidate.enchant.persisted_key())
    .bind(i16::from(candidate.level))
    .execute(&mut **tx)
    .await?;

    let row = sqlx::query(
        r#"
        SELECT i.owner_player_id,
               i.definition_key,
               i.definition_version,
               i.location,
               i.is_account_bound,
               i.is_tradeable,
               i.is_sellable,
               i.is_discardable,
               i.is_enchantable,
               i.is_upgradeable,
               i.is_unbreakable,
               i.is_repairable,
               b.enchant_key,
               b.level
          FROM item_instances i
          JOIN item_instance_finished_enchant_books b
            ON b.item_instance_id = i.id
         WHERE i.id = $1
         FOR UPDATE OF i
        "#,
    )
    .bind(delivery.item_instance_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(FishingDirectBookDeliveryError::PersistedBookIntegrityMismatch)?;

    let owner_player_id: Uuid = row.try_get("owner_player_id")?;
    let definition_key: String = row.try_get("definition_key")?;
    let definition_version: i32 = row.try_get("definition_version")?;
    let location: String = row.try_get("location")?;
    let enchant_key: String = row.try_get("enchant_key")?;
    let level: i16 = row.try_get("level")?;
    let valid_location = if delivery.pending {
        location == "TEMP_OVERFLOW" || location == "ITEM_BAG"
    } else {
        location == "ITEM_BAG"
    };
    if owner_player_id != player_id
        || definition_key != FINISHED_ENCHANT_BOOK_DEFINITION_KEY
        || definition_version != FINISHED_ENCHANT_BOOK_DEFINITION_VERSION
        || !valid_location
        || enchant_key != candidate.enchant.persisted_key()
        || level != i16::from(candidate.level)
        || row.try_get::<bool, _>("is_account_bound")?
        || row.try_get::<bool, _>("is_tradeable")?
        || row.try_get::<bool, _>("is_sellable")?
        || row.try_get::<bool, _>("is_discardable")?
        || row.try_get::<bool, _>("is_enchantable")?
        || row.try_get::<bool, _>("is_upgradeable")?
        || row.try_get::<bool, _>("is_unbreakable")?
        || row.try_get::<bool, _>("is_repairable")?
    {
        return Err(FishingDirectBookDeliveryError::PersistedBookIntegrityMismatch);
    }

    Ok(FishingDirectBookDeliveryReceiptV1 {
        operation_id,
        player_id,
        candidate,
        delivery,
    })
}

fn validate_candidate(
    candidate: FishingDirectBookCandidateV1,
) -> Result<(), FishingDirectBookDeliveryError> {
    if direct_fishing_book_pool_membership(candidate.enchant) != Some(candidate.pool)
        || candidate.level == 0
        || direct_fishing_finished_book_level_weight(candidate.enchant, candidate.level)
            .map_err(|_| FishingDirectBookDeliveryError::InvalidCandidate)?
            == 0
    {
        return Err(FishingDirectBookDeliveryError::InvalidCandidate);
    }
    Ok(())
}
