use graphite_items::{
    StackDeliveryMutationError, StackDeliveryMutationReceipt, StackDeliveryMutationRequest,
    apply_stack_delivery_mutation,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    FishingPayload, FishingPayloadPolicyError, FishingTreasureResult, MULTI_TREASURE_MAX_ITEMS,
    fishing_junk_payloads, fishing_non_book_treasure_payloads, fishing_payload_policy,
};

pub const FISHING_PAYLOAD_ITEM_DEFINITION_VERSION_V1: i32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "branch", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FishingResolvedNonFishPayloadV1 {
    Junk {
        payload: FishingPayload,
    },
    Treasure {
        treasure_slot: u8,
        result: FishingTreasureResult,
        payload: FishingPayload,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FishingNonFishPayloadDeliveryReceiptV1 {
    pub outcome: FishingResolvedNonFishPayloadV1,
    pub delivery: StackDeliveryMutationReceipt,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "branch", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FishingResolvedNonFishOutcomeV1 {
    Junk {
        payload: FishingPayload,
    },
    Treasure {
        result: FishingTreasureResult,
        payloads: Vec<FishingPayload>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FishingNonFishOutcomeDeliveryReceiptV1 {
    pub outcome: FishingResolvedNonFishOutcomeV1,
    pub deliveries: Vec<FishingNonFishPayloadDeliveryReceiptV1>,
}

#[derive(Debug, Error)]
pub enum FishingNonFishPayloadDeliveryError {
    #[error(transparent)]
    Stack(#[from] StackDeliveryMutationError),
    #[error(transparent)]
    Payload(#[from] FishingPayloadPolicyError),
    #[error("resolved Junk payload does not belong to the canonical Junk table")]
    InvalidJunkPayload,
    #[error("resolved Treasure payload does not belong to its canonical result table")]
    InvalidTreasurePayload,
    #[error("resolved Treasure slot must be within 0..{MULTI_TREASURE_MAX_ITEMS}")]
    InvalidTreasureSlot,
    #[error("resolved Treasure must contain between 1 and {MULTI_TREASURE_MAX_ITEMS} payloads")]
    InvalidTreasureCount,
}

fn validate_payload(
    outcome: FishingResolvedNonFishPayloadV1,
) -> Result<(String, FishingPayload), FishingNonFishPayloadDeliveryError> {
    match outcome {
        FishingResolvedNonFishPayloadV1::Junk { payload } => {
            if !fishing_junk_payloads().contains(&payload) {
                return Err(FishingNonFishPayloadDeliveryError::InvalidJunkPayload);
            }
            Ok(("fishing:junk:payload".to_owned(), payload))
        }
        FishingResolvedNonFishPayloadV1::Treasure {
            treasure_slot,
            result,
            payload,
        } => {
            if treasure_slot >= MULTI_TREASURE_MAX_ITEMS {
                return Err(FishingNonFishPayloadDeliveryError::InvalidTreasureSlot);
            }
            let allowed = fishing_non_book_treasure_payloads(result)?;
            if !allowed.contains(&payload) {
                return Err(FishingNonFishPayloadDeliveryError::InvalidTreasurePayload);
            }
            Ok((format!("fishing:treasure:payload:{treasure_slot}"), payload))
        }
    }
}

/// Delivers one *already resolved* non-Fish manual Fishing payload in the cast owner's transaction.
///
/// The payload must originate from the pinned manual-cast RNG/result owner; this child does not
/// select the outcome or authorize a Discord command. The category/slot/payload relationship is
/// checked against the canonical v1 tables before mutation. Stable per-slot mutation keys make
/// retries and conflicting same-slot resolutions rely on the existing generic stack-delivery
/// idempotency invariant. Item Bag overflow is preserved as pending rather than dropped.
///
/// This function never commits the transaction, awards XP, consumes bait, mutates a rod, or
/// transitions the operation. The complete cast owner must commit all effects atomically.
pub async fn deliver_fishing_non_fish_payload_v1(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
    outcome: FishingResolvedNonFishPayloadV1,
) -> Result<FishingNonFishPayloadDeliveryReceiptV1, FishingNonFishPayloadDeliveryError> {
    let (mutation_key, payload) = validate_payload(outcome)?;
    let policy = fishing_payload_policy(payload);
    let delivery = apply_stack_delivery_mutation(
        tx,
        &StackDeliveryMutationRequest {
            operation_id,
            player_id,
            mutation_key,
            definition_key: policy.item_definition_key.to_owned(),
            definition_version: FISHING_PAYLOAD_ITEM_DEFINITION_VERSION_V1,
            quantity: i64::from(policy.quantity),
            source: "MANUAL_FISHING".to_owned(),
            provenance: json!({
                "activity": "FISHING",
                "mode": "MANUAL",
                "resolved": outcome,
            }),
        },
    )
    .await?;

    Ok(FishingNonFishPayloadDeliveryReceiptV1 { outcome, delivery })
}

fn plan_non_fish_outcome(
    outcome: &FishingResolvedNonFishOutcomeV1,
) -> Result<Vec<FishingResolvedNonFishPayloadV1>, FishingNonFishPayloadDeliveryError> {
    let deliveries = match outcome {
        FishingResolvedNonFishOutcomeV1::Junk { payload } => {
            vec![FishingResolvedNonFishPayloadV1::Junk { payload: *payload }]
        }
        FishingResolvedNonFishOutcomeV1::Treasure { result, payloads } => {
            if payloads.is_empty() || payloads.len() > usize::from(MULTI_TREASURE_MAX_ITEMS) {
                return Err(FishingNonFishPayloadDeliveryError::InvalidTreasureCount);
            }
            payloads
                .iter()
                .enumerate()
                .map(
                    |(slot, payload)| FishingResolvedNonFishPayloadV1::Treasure {
                        treasure_slot: slot as u8,
                        result: *result,
                        payload: *payload,
                    },
                )
                .collect()
        }
    };
    for delivery in &deliveries {
        validate_payload(*delivery)?;
    }
    Ok(deliveries)
}

/// Delivers one already-resolved Junk or Treasure cast outcome as a bounded child batch.
///
/// All input is validated before the first SQL mutation. Multi Treasure uses one immutable outer
/// result category and ordered, independently resolved payloads at stable slot indices 0..2.
/// Item Bag delivery, overflow and retry semantics remain owned by the canonical stack writer.
/// The caller must roll back its PostgreSQL transaction if any child delivery fails; it must
/// settle rod, bait, progression and operation finalization before committing.
pub async fn deliver_fishing_non_fish_outcome_v1(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
    outcome: &FishingResolvedNonFishOutcomeV1,
) -> Result<FishingNonFishOutcomeDeliveryReceiptV1, FishingNonFishPayloadDeliveryError> {
    let planned = plan_non_fish_outcome(outcome)?;
    let mut deliveries = Vec::with_capacity(planned.len());
    for resolved in planned {
        deliveries.push(
            deliver_fishing_non_fish_payload_v1(tx, operation_id, player_id, resolved).await?,
        );
    }
    Ok(FishingNonFishOutcomeDeliveryReceiptV1 {
        outcome: outcome.clone(),
        deliveries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_fish_outcome_batch_has_single_branch_and_contiguous_treasure_slots() {
        let junk = FishingResolvedNonFishOutcomeV1::Junk {
            payload: FishingPayload::JunkDriftwood,
        };
        assert_eq!(plan_non_fish_outcome(&junk).unwrap().len(), 1);

        let treasure = FishingResolvedNonFishOutcomeV1::Treasure {
            result: FishingTreasureResult::MaterialBundle,
            payloads: vec![
                FishingPayload::MaterialBundleCommon,
                FishingPayload::MaterialBundleRefined,
                FishingPayload::MaterialBundleCommon,
            ],
        };
        let planned = plan_non_fish_outcome(&treasure).unwrap();
        assert_eq!(planned.len(), usize::from(MULTI_TREASURE_MAX_ITEMS));
        for (index, outcome) in planned.iter().enumerate() {
            assert!(matches!(
                outcome,
                FishingResolvedNonFishPayloadV1::Treasure { treasure_slot, .. }
                    if usize::from(*treasure_slot) == index
            ));
        }

        let empty = FishingResolvedNonFishOutcomeV1::Treasure {
            result: FishingTreasureResult::MaterialBundle,
            payloads: vec![],
        };
        assert!(matches!(
            plan_non_fish_outcome(&empty),
            Err(FishingNonFishPayloadDeliveryError::InvalidTreasureCount)
        ));
        let too_many = FishingResolvedNonFishOutcomeV1::Treasure {
            result: FishingTreasureResult::MaterialBundle,
            payloads: vec![FishingPayload::MaterialBundleCommon; 4],
        };
        assert!(matches!(
            plan_non_fish_outcome(&too_many),
            Err(FishingNonFishPayloadDeliveryError::InvalidTreasureCount)
        ));
        let mixed = FishingResolvedNonFishOutcomeV1::Treasure {
            result: FishingTreasureResult::MaterialBundle,
            payloads: vec![
                FishingPayload::MaterialBundleCommon,
                FishingPayload::WeatheredCrate,
            ],
        };
        assert!(matches!(
            plan_non_fish_outcome(&mixed),
            Err(FishingNonFishPayloadDeliveryError::InvalidTreasurePayload)
        ));
    }

    #[test]
    fn only_canonical_junk_payloads_are_accepted() {
        for payload in fishing_junk_payloads() {
            assert!(
                validate_payload(FishingResolvedNonFishPayloadV1::Junk { payload: *payload })
                    .is_ok()
            );
        }
        assert!(matches!(
            validate_payload(FishingResolvedNonFishPayloadV1::Junk {
                payload: FishingPayload::AncientCoin,
            }),
            Err(FishingNonFishPayloadDeliveryError::InvalidJunkPayload)
        ));
    }

    #[test]
    fn treasure_payloads_require_matching_category_and_valid_slot() {
        for result in [
            FishingTreasureResult::MaterialBundle,
            FishingTreasureResult::CrateOrChest,
            FishingTreasureResult::OrbOrCatalystFragmentOrItem,
            FishingTreasureResult::RareBaitOrUtilityItem,
            FishingTreasureResult::RelicOrCollectible,
        ] {
            for payload in fishing_non_book_treasure_payloads(result).unwrap() {
                for slot in 0..MULTI_TREASURE_MAX_ITEMS {
                    assert!(
                        validate_payload(FishingResolvedNonFishPayloadV1::Treasure {
                            treasure_slot: slot,
                            result,
                            payload: *payload,
                        })
                        .is_ok()
                    );
                }
            }
        }

        assert!(matches!(
            validate_payload(FishingResolvedNonFishPayloadV1::Treasure {
                treasure_slot: MULTI_TREASURE_MAX_ITEMS,
                result: FishingTreasureResult::MaterialBundle,
                payload: FishingPayload::MaterialBundleCommon,
            }),
            Err(FishingNonFishPayloadDeliveryError::InvalidTreasureSlot)
        ));
        assert!(matches!(
            validate_payload(FishingResolvedNonFishPayloadV1::Treasure {
                treasure_slot: 0,
                result: FishingTreasureResult::MaterialBundle,
                payload: FishingPayload::WeatheredCrate,
            }),
            Err(FishingNonFishPayloadDeliveryError::InvalidTreasurePayload)
        ));
        assert!(matches!(
            validate_payload(FishingResolvedNonFishPayloadV1::Treasure {
                treasure_slot: 0,
                result: FishingTreasureResult::EnchantBook,
                payload: FishingPayload::MaterialBundleCommon,
            }),
            Err(FishingNonFishPayloadDeliveryError::Payload(
                FishingPayloadPolicyError::EnchantBookUsesDedicatedPolicy
            ))
        ));
    }

    #[test]
    fn treasure_slots_have_independent_mutation_keys() {
        for slot in 0..MULTI_TREASURE_MAX_ITEMS {
            let (key, _) = validate_payload(FishingResolvedNonFishPayloadV1::Treasure {
                treasure_slot: slot,
                result: FishingTreasureResult::CrateOrChest,
                payload: FishingPayload::WeatheredCrate,
            })
            .unwrap();
            assert_eq!(key, format!("fishing:treasure:payload:{slot}"));
        }
    }
}
