use std::fmt;

use graphite_core::{DomainRng, OperationId, RngError, RootSeed};
use serde::Serialize;
use sqlx::{Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{FISHING_NUMERIC_DRAW_SCALE_PPM, MAX_FISH_PER_CAST, MULTI_TREASURE_MAX_ITEMS};

const RNG_ROOT_BYTES: usize = 32;

pub const MANUAL_FISHING_RNG_POLICY_VERSION: u16 = 1;
pub const MANUAL_FISHING_RNG_PPM_SCALE: u32 = FISHING_NUMERIC_DRAW_SCALE_PPM;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ManualFishingWeightedDrawV1 {
    CatchBranch,
    TreasureResult,
    JunkPayload,
    NonBookTreasurePayload { treasure_slot: u8 },
    FishSpecies { fish_slot: u8 },
    FishVariant { fish_slot: u8 },
    EnchantBookPool { treasure_slot: u8 },
    EnchantBookMember { treasure_slot: u8 },
    EnchantBookLevel { treasure_slot: u8 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ManualFishingPpmDrawV1 {
    MulticatchCount,
    SchoolBaitProc,
    MultiTreasureCount,
    FishWeight { fish_slot: u8 },
    FishBiologicalNoise { fish_slot: u8 },
    LineBreak,
    OverCapCatch,
    UnbreakingOrdinaryWear,
}

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

    /// Resolves one exact weighted selection in its fixed semantic RNG domain.
    ///
    /// Every call derives a fresh domain RNG from the persisted operation root, operation identity,
    /// policy version, semantic stage and stable slot index. No shared sequential stream exists
    /// across Fishing stages, so adding or skipping an unrelated draw cannot shift this result.
    pub fn sample_weighted_v1(
        self,
        draw: ManualFishingWeightedDrawV1,
        weights: &[u64],
    ) -> Result<usize, ManualFishingRngDrawError> {
        let domain = weighted_domain_label(draw)?;
        let mut rng = DomainRng::derive(self.root_seed, self.operation_id, &domain);
        Ok(rng.sample_weighted(weights)?)
    }

    /// Resolves one canonical 1,000,000-PPM bucket in its fixed semantic RNG domain.
    ///
    /// The returned bucket is always in 0..MANUAL_FISHING_RNG_PPM_SCALE. Callers compare it to
    /// already-authoritative PPM thresholds or feed it to the versioned FishInstance numeric kernel;
    /// this context does not invent probability tables.
    pub fn sample_ppm_v1(
        self,
        draw: ManualFishingPpmDrawV1,
    ) -> Result<u32, ManualFishingRngDrawError> {
        let domain = ppm_domain_label(draw)?;
        let mut rng = DomainRng::derive(self.root_seed, self.operation_id, &domain);
        let value = rng.sample_uniform_below(u64::from(MANUAL_FISHING_RNG_PPM_SCALE))?;
        u32::try_from(value).map_err(|_| ManualFishingRngDrawError::ArithmeticOverflow)
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

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ManualFishingRngDrawError {
    #[error("Fishing fish RNG slot must be within 0..{MAX_FISH_PER_CAST}; got {0}")]
    FishSlotOutOfRange(u8),
    #[error("Fishing Treasure RNG slot must be within 0..{MULTI_TREASURE_MAX_ITEMS}; got {0}")]
    TreasureSlotOutOfRange(u8),
    #[error("Fishing RNG draw conversion exceeded supported integer bounds")]
    ArithmeticOverflow,
    #[error(transparent)]
    Core(#[from] RngError),
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
/// The operation row is locked and must still be PENDING and bound to player_id. The persisted
/// 32-byte root remains private inside the returned opaque context, so Discord/application adapters
/// cannot become RNG authority and retry timing or unrelated random draws cannot replace the
/// operation-owned source of truth.
///
/// The returned context exposes only the versioned typed Fishing domain contract. It does not accept
/// arbitrary domain strings and it derives a fresh RNG per semantic stage plus stable slot index.
/// The lock itself draws nothing; the owning cast lifecycle still decides which already-authoritative
/// stages are eligible and supplies their exact weight/probability policies.
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

fn weighted_domain_label(
    draw: ManualFishingWeightedDrawV1,
) -> Result<String, ManualFishingRngDrawError> {
    Ok(match draw {
        ManualFishingWeightedDrawV1::CatchBranch => "fishing.manual.v1.catch_branch".to_owned(),
        ManualFishingWeightedDrawV1::TreasureResult => {
            "fishing.manual.v1.treasure_result".to_owned()
        }
        ManualFishingWeightedDrawV1::JunkPayload => "fishing.manual.v1.junk.payload".to_owned(),
        ManualFishingWeightedDrawV1::NonBookTreasurePayload { treasure_slot } => {
            validate_treasure_slot(treasure_slot)?;
            format!("fishing.manual.v1.treasure.{treasure_slot}.nonbook_payload")
        }
        ManualFishingWeightedDrawV1::FishSpecies { fish_slot } => {
            validate_fish_slot(fish_slot)?;
            format!("fishing.manual.v1.fish.{fish_slot}.species")
        }
        ManualFishingWeightedDrawV1::FishVariant { fish_slot } => {
            validate_fish_slot(fish_slot)?;
            format!("fishing.manual.v1.fish.{fish_slot}.variant")
        }
        ManualFishingWeightedDrawV1::EnchantBookPool { treasure_slot } => {
            validate_treasure_slot(treasure_slot)?;
            format!("fishing.manual.v1.treasure.{treasure_slot}.book.pool")
        }
        ManualFishingWeightedDrawV1::EnchantBookMember { treasure_slot } => {
            validate_treasure_slot(treasure_slot)?;
            format!("fishing.manual.v1.treasure.{treasure_slot}.book.member")
        }
        ManualFishingWeightedDrawV1::EnchantBookLevel { treasure_slot } => {
            validate_treasure_slot(treasure_slot)?;
            format!("fishing.manual.v1.treasure.{treasure_slot}.book.level")
        }
    })
}

fn ppm_domain_label(draw: ManualFishingPpmDrawV1) -> Result<String, ManualFishingRngDrawError> {
    Ok(match draw {
        ManualFishingPpmDrawV1::MulticatchCount => "fishing.manual.v1.multicatch_count".to_owned(),
        ManualFishingPpmDrawV1::SchoolBaitProc => "fishing.manual.v1.school_bait_proc".to_owned(),
        ManualFishingPpmDrawV1::MultiTreasureCount => {
            "fishing.manual.v1.multi_treasure_count".to_owned()
        }
        ManualFishingPpmDrawV1::FishWeight { fish_slot } => {
            validate_fish_slot(fish_slot)?;
            format!("fishing.manual.v1.fish.{fish_slot}.weight")
        }
        ManualFishingPpmDrawV1::FishBiologicalNoise { fish_slot } => {
            validate_fish_slot(fish_slot)?;
            format!("fishing.manual.v1.fish.{fish_slot}.biological_noise")
        }
        ManualFishingPpmDrawV1::LineBreak => "fishing.manual.v1.line_break".to_owned(),
        ManualFishingPpmDrawV1::OverCapCatch => "fishing.manual.v1.over_cap_catch".to_owned(),
        ManualFishingPpmDrawV1::UnbreakingOrdinaryWear => {
            "fishing.manual.v1.unbreaking_ordinary_wear".to_owned()
        }
    })
}

const fn validate_fish_slot(slot: u8) -> Result<(), ManualFishingRngDrawError> {
    if slot < MAX_FISH_PER_CAST {
        Ok(())
    } else {
        Err(ManualFishingRngDrawError::FishSlotOutOfRange(slot))
    }
}

const fn validate_treasure_slot(slot: u8) -> Result<(), ManualFishingRngDrawError> {
    if slot < MULTI_TREASURE_MAX_ITEMS {
        Ok(())
    } else {
        Err(ManualFishingRngDrawError::TreasureSlotOutOfRange(slot))
    }
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

    #[test]
    fn domain_labels_are_versioned_semantic_and_slot_stable() {
        assert_eq!(
            weighted_domain_label(ManualFishingWeightedDrawV1::CatchBranch).unwrap(),
            "fishing.manual.v1.catch_branch"
        );
        assert_eq!(
            weighted_domain_label(ManualFishingWeightedDrawV1::FishSpecies { fish_slot: 4 })
                .unwrap(),
            "fishing.manual.v1.fish.4.species"
        );
        assert_eq!(
            ppm_domain_label(ManualFishingPpmDrawV1::FishBiologicalNoise { fish_slot: 4 }).unwrap(),
            "fishing.manual.v1.fish.4.biological_noise"
        );
        assert_eq!(
            weighted_domain_label(ManualFishingWeightedDrawV1::EnchantBookLevel {
                treasure_slot: 2,
            })
            .unwrap(),
            "fishing.manual.v1.treasure.2.book.level"
        );
    }

    #[test]
    fn non_fish_payload_domains_are_distinct_and_versioned() {
        assert_eq!(
            weighted_domain_label(ManualFishingWeightedDrawV1::JunkPayload).unwrap(),
            "fishing.manual.v1.junk.payload"
        );
        for slot in 0..MULTI_TREASURE_MAX_ITEMS {
            let domain =
                weighted_domain_label(ManualFishingWeightedDrawV1::NonBookTreasurePayload {
                    treasure_slot: slot,
                })
                .unwrap();
            assert_eq!(
                domain,
                format!("fishing.manual.v1.treasure.{slot}.nonbook_payload")
            );
            assert!(
                context()
                    .sample_weighted_v1(
                        ManualFishingWeightedDrawV1::NonBookTreasurePayload {
                            treasure_slot: slot,
                        },
                        &[1, 1, 1, 1, 1],
                    )
                    .is_ok()
            );
        }
        assert_eq!(
            context().sample_weighted_v1(
                ManualFishingWeightedDrawV1::NonBookTreasurePayload {
                    treasure_slot: MULTI_TREASURE_MAX_ITEMS,
                },
                &[1, 1],
            ),
            Err(ManualFishingRngDrawError::TreasureSlotOutOfRange(
                MULTI_TREASURE_MAX_ITEMS
            ))
        );
    }

    #[test]
    fn out_of_range_slots_fail_closed_before_any_rng_draw() {
        assert_eq!(
            context().sample_ppm_v1(ManualFishingPpmDrawV1::FishWeight {
                fish_slot: MAX_FISH_PER_CAST,
            }),
            Err(ManualFishingRngDrawError::FishSlotOutOfRange(
                MAX_FISH_PER_CAST
            ))
        );
        assert_eq!(
            context().sample_weighted_v1(
                ManualFishingWeightedDrawV1::EnchantBookPool {
                    treasure_slot: MULTI_TREASURE_MAX_ITEMS,
                },
                &[58, 24, 12, 4, 2],
            ),
            Err(ManualFishingRngDrawError::TreasureSlotOutOfRange(
                MULTI_TREASURE_MAX_ITEMS
            ))
        );
    }

    #[test]
    fn call_order_and_unrelated_domains_do_not_shift_a_semantic_draw() {
        let context = context();
        let base_weight = context
            .sample_ppm_v1(ManualFishingPpmDrawV1::FishWeight { fish_slot: 0 })
            .unwrap();

        let _ = context
            .sample_ppm_v1(ManualFishingPpmDrawV1::SchoolBaitProc)
            .unwrap();
        let _ = context
            .sample_weighted_v1(ManualFishingWeightedDrawV1::CatchBranch, &[176, 17, 7])
            .unwrap();
        let _ = context
            .sample_ppm_v1(ManualFishingPpmDrawV1::FishWeight { fish_slot: 4 })
            .unwrap();

        assert_eq!(
            context
                .sample_ppm_v1(ManualFishingPpmDrawV1::FishWeight { fish_slot: 0 })
                .unwrap(),
            base_weight
        );
    }

    #[test]
    fn fixed_operation_seed_golden_values_are_stable_for_v1() {
        let context = context();
        let ppm = [
            context
                .sample_ppm_v1(ManualFishingPpmDrawV1::MulticatchCount)
                .unwrap(),
            context
                .sample_ppm_v1(ManualFishingPpmDrawV1::SchoolBaitProc)
                .unwrap(),
            context
                .sample_ppm_v1(ManualFishingPpmDrawV1::MultiTreasureCount)
                .unwrap(),
            context
                .sample_ppm_v1(ManualFishingPpmDrawV1::FishWeight { fish_slot: 0 })
                .unwrap(),
            context
                .sample_ppm_v1(ManualFishingPpmDrawV1::FishBiologicalNoise { fish_slot: 0 })
                .unwrap(),
            context
                .sample_ppm_v1(ManualFishingPpmDrawV1::FishWeight { fish_slot: 4 })
                .unwrap(),
            context
                .sample_ppm_v1(ManualFishingPpmDrawV1::LineBreak)
                .unwrap(),
            context
                .sample_ppm_v1(ManualFishingPpmDrawV1::OverCapCatch)
                .unwrap(),
            context
                .sample_ppm_v1(ManualFishingPpmDrawV1::UnbreakingOrdinaryWear)
                .unwrap(),
        ];
        let weighted = [
            context
                .sample_weighted_v1(ManualFishingWeightedDrawV1::CatchBranch, &[176, 17, 7])
                .unwrap(),
            context
                .sample_weighted_v1(
                    ManualFishingWeightedDrawV1::TreasureResult,
                    &[20, 15, 4, 4, 5, 2],
                )
                .unwrap(),
            context
                .sample_weighted_v1(
                    ManualFishingWeightedDrawV1::EnchantBookPool { treasure_slot: 2 },
                    &[58, 24, 12, 4, 2],
                )
                .unwrap(),
        ];

        assert_eq!(
            ppm,
            [
                672_042, 756_708, 939_892, 351_744, 591_284, 460_676, 220_792, 214_850, 424_152,
            ]
        );
        assert_eq!(weighted, [0, 1, 0]);
    }
}
