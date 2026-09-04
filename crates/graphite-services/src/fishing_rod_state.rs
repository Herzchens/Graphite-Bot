use graphite_core::CANONICAL_ENCHANT_COUNT;
use graphite_items::{ItemError, lock_owned_item_ordinary_equipment_classification};
use serde::Serialize;
use serde_json::Value;
use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    CanonicalEnchant, EnchantApplyError, EnchantSlotCapacity, EnchantSlotFamily, EquipmentSlot,
    EquipmentTier, canonical_enchant_max_resulting_level, enchant_placement_policy,
};

pub(crate) const STARTER_BASIC_ROD_DEFINITION_KEY: &str = "equipment.rod.basic.starter";
const EMBEDDED_ENCHANT_ROW_QUERY_LIMIT: i64 = CANONICAL_ENCHANT_COUNT as i64 + 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EquippedFishingRodKind {
    StarterBasic,
    Ordinary { tier: EquipmentTier },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct EquippedFishingRodEnchantState {
    pub enchant: CanonicalEnchant,
    pub level: u8,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EquippedFishingRodCastSnapshot {
    pub player_id: Uuid,
    pub item_instance_id: Uuid,
    pub definition_key: String,
    pub definition_version: i32,
    pub kind: EquippedFishingRodKind,
    pub current_durability: Option<u32>,
    pub max_durability: Option<u32>,
    pub is_broken: bool,
    pub normal_enchant_slot_capacity: u8,
    pub special_enchant_slot_capacity: u8,
    pub embedded_enchants: Vec<EquippedFishingRodEnchantState>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LockedEquippedFishingRodState {
    pub player_id: Uuid,
    pub item_instance_id: Uuid,
    pub definition_key: String,
    pub definition_version: i32,
    pub kind: EquippedFishingRodKind,
    pub is_starter: bool,
    pub is_unbreakable: bool,
    pub is_repairable: bool,
    pub current_durability: Option<i64>,
    pub max_durability: Option<i64>,
    pub is_broken: bool,
}

#[derive(Debug, Error)]
pub enum EquippedFishingRodStateError {
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error(transparent)]
    Item(#[from] ItemError),
    #[error("no Fishing Rod is currently equipped")]
    NoEquippedFishingRod,
    #[error("equipped Fishing Rod state is internally inconsistent")]
    EquippedRodIntegrityMismatch,
    #[error("Starter Basic Rod identity is internally inconsistent")]
    StarterBasicRodIntegrityMismatch,
    #[error("the equipped Fishing Rod is not classified as ordinary equipment")]
    NonOrdinaryFishingRod,
    #[error("the equipped ordinary Fishing Rod has invalid or unsupported tier metadata")]
    InvalidOrdinaryRodTierMetadata,
}

impl From<sqlx::Error> for EquippedFishingRodStateError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

#[derive(Debug, Error)]
pub enum EquippedFishingRodCastSnapshotError {
    #[error(transparent)]
    State(#[from] EquippedFishingRodStateError),
    #[error("database error: {0}")]
    Database(Box<sqlx::Error>),
    #[error("Starter Basic Rod cast state is internally inconsistent")]
    StarterBasicCastStateIntegrityMismatch,
    #[error("ordinary Fishing Rod durability state is internally inconsistent")]
    InvalidOrdinaryRodDurabilityState,
    #[error("ordinary Fishing Rod structural state is missing")]
    OrdinaryRodStructuralStateMissing,
    #[error(transparent)]
    EnchantCapacity(#[from] EnchantApplyError),
    #[error("persisted embedded enchant key is not canonical: {0}")]
    UnknownEmbeddedEnchantKey(String),
    #[error(
        "persisted embedded enchant {enchant:?} has invalid resulting level {level}; maximum is {maximum}"
    )]
    InvalidEmbeddedEnchantLevel {
        enchant: CanonicalEnchant,
        level: i16,
        maximum: u8,
    },
    #[error("persisted embedded enchant {0:?} cannot be placed on a Fishing Rod")]
    EmbeddedEnchantWrongEquipmentSlot(CanonicalEnchant),
    #[error("embedded enchant row count exceeds the canonical catalog cardinality")]
    TooManyEmbeddedEnchantRows,
    #[error(
        "persisted {family:?} Fishing Rod enchant occupancy {occupied} exceeds unlocked capacity {capacity}"
    )]
    EmbeddedEnchantOccupancyExceedsCapacity {
        family: EnchantSlotFamily,
        occupied: u8,
        capacity: u8,
    },
}

impl From<sqlx::Error> for EquippedFishingRodCastSnapshotError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(Box::new(value))
    }
}

/// Locks the authoritative currently-equipped Fishing Rod identity/state for a caller-owned
/// transaction.
///
/// The caller must already hold its owning player lock. That lock serializes same-player equipment
/// mutations while this helper first discovers the current `FISHING_ROD` slot without taking the
/// slot lock early. The ItemInstance is then locked through `graphite-items`, followed by an exact
/// slot recheck with `FOR UPDATE`. This preserves the repository's `player -> item -> equipment slot`
/// order while deriving ordinary/special classification from the immutable ItemDefinition version
/// pinned by the ItemInstance.
///
/// This base resolver deliberately validates only Rod identity, ownership, location, slot membership,
/// Starter identity, and ordinary-tier metadata. It does not decide whether a Broken ordinary Rod may
/// qualify for a particular action and does not require structural/enchant state. Context-specific
/// callers such as first-area unlock and resolved durability mutation therefore keep their existing
/// semantics while sharing one authoritative Rod identity bridge.
pub(crate) async fn lock_equipped_fishing_rod_state(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
) -> Result<LockedEquippedFishingRodState, EquippedFishingRodStateError> {
    let item_id: Uuid = sqlx::query_scalar(
        "SELECT item_instance_id FROM equipment_slots WHERE player_id = $1 AND slot = 'FISHING_ROD'",
    )
    .bind(player_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(EquippedFishingRodStateError::NoEquippedFishingRod)?;

    let classification =
        lock_owned_item_ordinary_equipment_classification(tx, player_id, item_id).await?;
    let row = sqlx::query(
        r#"
        SELECT i.location,
               i.is_starter,
               i.is_unbreakable,
               i.is_repairable,
               i.current_durability,
               i.max_durability,
               i.is_broken,
               d.category,
               d.data
          FROM item_instances i
          JOIN item_definition_versions d
            ON d.key = i.definition_key
           AND d.version = i.definition_version
         WHERE i.id = $1
           AND i.owner_player_id = $2
           AND i.definition_key = $3
           AND i.definition_version = $4
        "#,
    )
    .bind(item_id)
    .bind(player_id)
    .bind(&classification.definition_key)
    .bind(classification.definition_version)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(EquippedFishingRodStateError::EquippedRodIntegrityMismatch)?;

    let slot_item_id: Uuid = sqlx::query_scalar(
        "SELECT item_instance_id FROM equipment_slots WHERE player_id = $1 AND slot = 'FISHING_ROD' FOR UPDATE",
    )
    .bind(player_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(EquippedFishingRodStateError::EquippedRodIntegrityMismatch)?;
    if slot_item_id != item_id {
        return Err(EquippedFishingRodStateError::EquippedRodIntegrityMismatch);
    }

    let location: String = row.try_get("location")?;
    let category: String = row.try_get("category")?;
    if location != "EQUIPPED" || category != "FISHING_ROD" {
        return Err(EquippedFishingRodStateError::EquippedRodIntegrityMismatch);
    }

    let is_starter: bool = row.try_get("is_starter")?;
    let kind = if classification.definition_key == STARTER_BASIC_ROD_DEFINITION_KEY {
        if !is_starter || classification.is_ordinary_equipment {
            return Err(EquippedFishingRodStateError::StarterBasicRodIntegrityMismatch);
        }
        EquippedFishingRodKind::StarterBasic
    } else {
        if is_starter {
            return Err(EquippedFishingRodStateError::StarterBasicRodIntegrityMismatch);
        }
        if !classification.is_ordinary_equipment {
            return Err(EquippedFishingRodStateError::NonOrdinaryFishingRod);
        }
        let data: Value = row.try_get("data")?;
        let tier = ordinary_rod_tier_from_definition(&data)
            .ok_or(EquippedFishingRodStateError::InvalidOrdinaryRodTierMetadata)?;
        EquippedFishingRodKind::Ordinary { tier }
    };

    Ok(LockedEquippedFishingRodState {
        player_id,
        item_instance_id: item_id,
        definition_key: classification.definition_key,
        definition_version: classification.definition_version,
        kind,
        is_starter,
        is_unbreakable: row.try_get("is_unbreakable")?,
        is_repairable: row.try_get("is_repairable")?,
        current_durability: row.try_get("current_durability")?,
        max_durability: row.try_get("max_durability")?,
        is_broken: row.try_get("is_broken")?,
    })
}

/// Locks a cast-ready authoritative snapshot for the currently equipped Fishing Rod.
///
/// The caller must acquire its operation/player locks before entering this resolver. On top of the
/// shared Rod identity bridge, ordinary Rods lock their structural row only to obtain the authoritative
/// Normal/class and Special/universal slot capacities; Creation Roll and +N appraisal state are not
/// interpreted here. Embedded enchant rows are then locked in deterministic key order, mapped through
/// the shared canonical persistence vocabulary, validated against resulting-level ceilings and the
/// shared Fishing-Rod placement mask, and checked against the currently unlocked family capacities.
///
/// Starter Basic remains a separate system Rod: its canonical state is unbreakable, non-repairable,
/// has no mutable durability, has no embedded enchants, and does not require ordinary structural
/// state. Ordinary Rods may be returned in a consistent Broken state so the future cast owner can
/// reject or route that action explicitly; malformed durability/Broken combinations fail closed.
///
/// This resolver performs no RNG, cooldown decision, bait consumption, durability mutation, Mending,
/// catch generation/delivery, AEXP settlement, operation finalization, audit/outbox write, or command
/// exposure. The returned snapshot is authoritative only while the caller transaction remains open.
pub async fn lock_equipped_fishing_rod_cast_snapshot(
    tx: &mut Transaction<'_, Postgres>,
    player_id: Uuid,
) -> Result<EquippedFishingRodCastSnapshot, EquippedFishingRodCastSnapshotError> {
    let rod = lock_equipped_fishing_rod_state(tx, player_id).await?;

    if rod.kind == EquippedFishingRodKind::StarterBasic {
        let embedded_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM item_instance_embedded_enchants WHERE item_instance_id = $1",
        )
        .bind(rod.item_instance_id)
        .fetch_one(&mut **tx)
        .await?;
        if !rod.is_starter
            || !rod.is_unbreakable
            || rod.is_repairable
            || rod.current_durability.is_some()
            || rod.max_durability.is_some()
            || rod.is_broken
            || embedded_count != 0
        {
            return Err(
                EquippedFishingRodCastSnapshotError::StarterBasicCastStateIntegrityMismatch,
            );
        }

        return Ok(EquippedFishingRodCastSnapshot {
            player_id: rod.player_id,
            item_instance_id: rod.item_instance_id,
            definition_key: rod.definition_key,
            definition_version: rod.definition_version,
            kind: rod.kind,
            current_durability: None,
            max_durability: None,
            is_broken: false,
            normal_enchant_slot_capacity: 0,
            special_enchant_slot_capacity: 0,
            embedded_enchants: Vec::new(),
        });
    }

    if rod.is_starter || rod.is_unbreakable || !rod.is_repairable {
        return Err(EquippedFishingRodCastSnapshotError::InvalidOrdinaryRodDurabilityState);
    }
    let current_durability = rod
        .current_durability
        .and_then(|value| u32::try_from(value).ok())
        .ok_or(EquippedFishingRodCastSnapshotError::InvalidOrdinaryRodDurabilityState)?;
    let max_durability = rod
        .max_durability
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or(EquippedFishingRodCastSnapshotError::InvalidOrdinaryRodDurabilityState)?;
    if current_durability > max_durability || rod.is_broken != (current_durability == 0) {
        return Err(EquippedFishingRodCastSnapshotError::InvalidOrdinaryRodDurabilityState);
    }

    let capacity_row = sqlx::query(
        r#"
        SELECT normal_enchant_slot_capacity, special_enchant_slot_capacity
          FROM item_instance_equipment_structural_state
         WHERE item_instance_id = $1
         FOR UPDATE
        "#,
    )
    .bind(rod.item_instance_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(EquippedFishingRodCastSnapshotError::OrdinaryRodStructuralStateMissing)?;
    let normal_capacity = u8::try_from(capacity_row.try_get::<i16, _>("normal_enchant_slot_capacity")?)
        .map_err(|_| EquippedFishingRodCastSnapshotError::OrdinaryRodStructuralStateMissing)?;
    let special_capacity =
        u8::try_from(capacity_row.try_get::<i16, _>("special_enchant_slot_capacity")?)
            .map_err(|_| EquippedFishingRodCastSnapshotError::OrdinaryRodStructuralStateMissing)?;
    let capacity = EnchantSlotCapacity::try_new(normal_capacity, special_capacity)?;

    let rows = sqlx::query(
        r#"
        SELECT enchant_key, level
          FROM item_instance_embedded_enchants
         WHERE item_instance_id = $1
         ORDER BY enchant_key
         LIMIT $2
         FOR UPDATE
        "#,
    )
    .bind(rod.item_instance_id)
    .bind(EMBEDDED_ENCHANT_ROW_QUERY_LIMIT)
    .fetch_all(&mut **tx)
    .await?;
    if rows.len() > CANONICAL_ENCHANT_COUNT {
        return Err(EquippedFishingRodCastSnapshotError::TooManyEmbeddedEnchantRows);
    }

    let mut normal_occupied = 0_u8;
    let mut special_occupied = 0_u8;
    let mut embedded_enchants = Vec::with_capacity(rows.len());
    for row in rows {
        let persisted_key: String = row.try_get("enchant_key")?;
        let stored_level: i16 = row.try_get("level")?;
        let enchant = CanonicalEnchant::from_persisted_key(&persisted_key).ok_or_else(|| {
            EquippedFishingRodCastSnapshotError::UnknownEmbeddedEnchantKey(persisted_key.clone())
        })?;
        let maximum = canonical_enchant_max_resulting_level(enchant);
        let level = u8::try_from(stored_level).map_err(|_| {
            EquippedFishingRodCastSnapshotError::InvalidEmbeddedEnchantLevel {
                enchant,
                level: stored_level,
                maximum,
            }
        })?;
        if level == 0 || level > maximum {
            return Err(EquippedFishingRodCastSnapshotError::InvalidEmbeddedEnchantLevel {
                enchant,
                level: stored_level,
                maximum,
            });
        }

        let placement = enchant_placement_policy(enchant);
        if !placement.applies_to(EquipmentSlot::FishingRod) {
            return Err(
                EquippedFishingRodCastSnapshotError::EmbeddedEnchantWrongEquipmentSlot(enchant),
            );
        }
        match placement.slot_family {
            EnchantSlotFamily::NormalClass => {
                normal_occupied = normal_occupied.checked_add(1).ok_or(
                    EquippedFishingRodCastSnapshotError::TooManyEmbeddedEnchantRows,
                )?;
            }
            EnchantSlotFamily::SpecialUniversal => {
                special_occupied = special_occupied.checked_add(1).ok_or(
                    EquippedFishingRodCastSnapshotError::TooManyEmbeddedEnchantRows,
                )?;
            }
        }

        embedded_enchants.push(EquippedFishingRodEnchantState { enchant, level });
    }

    if normal_occupied > capacity.normal_class {
        return Err(
            EquippedFishingRodCastSnapshotError::EmbeddedEnchantOccupancyExceedsCapacity {
                family: EnchantSlotFamily::NormalClass,
                occupied: normal_occupied,
                capacity: capacity.normal_class,
            },
        );
    }
    if special_occupied > capacity.special_universal {
        return Err(
            EquippedFishingRodCastSnapshotError::EmbeddedEnchantOccupancyExceedsCapacity {
                family: EnchantSlotFamily::SpecialUniversal,
                occupied: special_occupied,
                capacity: capacity.special_universal,
            },
        );
    }

    Ok(EquippedFishingRodCastSnapshot {
        player_id: rod.player_id,
        item_instance_id: rod.item_instance_id,
        definition_key: rod.definition_key,
        definition_version: rod.definition_version,
        kind: rod.kind,
        current_durability: Some(current_durability),
        max_durability: Some(max_durability),
        is_broken: rod.is_broken,
        normal_enchant_slot_capacity: capacity.normal_class,
        special_enchant_slot_capacity: capacity.special_universal,
        embedded_enchants,
    })
}

fn ordinary_rod_tier_from_definition(data: &Value) -> Option<EquipmentTier> {
    match data.get("tier").and_then(Value::as_str) {
        Some("WOOD") => Some(EquipmentTier::Wood),
        Some("STONE") => Some(EquipmentTier::Stone),
        Some("COPPER") => Some(EquipmentTier::Copper),
        Some("GOLD") => Some(EquipmentTier::Gold),
        Some("IRON") => Some(EquipmentTier::Iron),
        Some("DIAMOND") => Some(EquipmentTier::Diamond),
        Some("OBSIDIAN") => Some(EquipmentTier::Obsidian),
        Some("NETHERITE") => Some(EquipmentTier::Netherite),
        Some("GRAPHITE") => Some(EquipmentTier::Graphite),
        _ => None,
    }
}
