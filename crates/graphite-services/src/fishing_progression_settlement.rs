use graphite_progression::{
    AccountXpSettlementError, AccountXpSettlementReceipt, AccountXpSettlementRequest,
    ActivityXpError, ActivityXpMutationKind, ActivityXpMutationReceipt, ActivityXpMutationRequest,
    apply_account_xp_settlement, apply_activity_xp_mutation,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    FishingRarity, ManualFishingAexpError, ManualFishingAexpOutcome,
    manual_fishing_base_fish_cast_aexp, manual_fishing_base_outcome_aexp,
    manual_fishing_base_treasure_cast_aexp,
};

pub const MANUAL_FISHING_ACCOUNT_XP_REWARD: i64 = 2;
const MANUAL_FISHING_ACTIVITY_XP_MUTATION_KEY: &str = "fishing:aexp";
const MANUAL_FISHING_PROGRESSION_SOURCE: &str = "MANUAL_FISHING";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ManualFishingSuccessfulProgressionOutcomeV1 {
    Junk,
    Treasure { landed_count: u8 },
    Fish { landed_rarities: Vec<FishingRarity> },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ManualFishingProgressionSettlementReceiptV1 {
    pub operation_id: Uuid,
    pub player_id: Uuid,
    pub base_activity_xp: i64,
    pub account_xp: AccountXpSettlementReceipt,
    pub activity_xp: ActivityXpMutationReceipt,
}

#[derive(Debug, Error)]
pub enum ManualFishingProgressionSettlementError {
    #[error(transparent)]
    Aexp(#[from] ManualFishingAexpError),
    #[error(transparent)]
    AccountXp(#[from] AccountXpSettlementError),
    #[error(transparent)]
    ActivityXp(#[from] ActivityXpError),
}

/// Resolves the exact frozen base Activity EXP for one successfully landed manual-Fishing outcome.
///
/// This type deliberately has no escaped-fish or line-break variant. Those terminal paths grant zero
/// Fishing AEXP and are not successful committed manual-Fishing reward outcomes for the fixed
/// 2-Account-XP source matrix. Multi Catch and Multi Treasure caps are delegated to their existing
/// canonical policy owners.
pub fn manual_fishing_successful_base_aexp_v1(
    outcome: &ManualFishingSuccessfulProgressionOutcomeV1,
) -> Result<i64, ManualFishingAexpError> {
    match outcome {
        ManualFishingSuccessfulProgressionOutcomeV1::Junk => Ok(manual_fishing_base_outcome_aexp(
            ManualFishingAexpOutcome::LandedJunk,
        )
        .expect("landed Junk has canonical positive AEXP")),
        ManualFishingSuccessfulProgressionOutcomeV1::Treasure { landed_count } => {
            manual_fishing_base_treasure_cast_aexp(*landed_count)
        }
        ManualFishingSuccessfulProgressionOutcomeV1::Fish { landed_rarities } => {
            manual_fishing_base_fish_cast_aexp(landed_rarities)
        }
    }
}

/// Settles the frozen successful-manual-Fishing progression rewards inside the caller transaction.
///
/// A successful committed manual Fishing action grants exactly 2 Account XP. Its base Fishing
/// Activity EXP is the exact Junk/Treasure/fish-rarity result already owned by the Fishing AEXP
/// policy. Both mutations use the existing transaction-composable progression primitives, so Account
/// Level Money rewards, cap handling, immutable progression events, stable mutation keys, rollback
/// and replay remain owned by the shared progression subsystem.
///
/// This function must be called in the same transaction as the owning cast after terminal outcome
/// validity is known. It does not finalize or commit the operation; therefore a surrounding rollback
/// means the cast was not successfully committed and neither reward survives. The future cast owner
/// must not call this function for escaped-fish or line-break outcomes.
pub async fn settle_manual_fishing_successful_progression_v1(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: Uuid,
    player_id: Uuid,
    outcome: &ManualFishingSuccessfulProgressionOutcomeV1,
) -> Result<ManualFishingProgressionSettlementReceiptV1, ManualFishingProgressionSettlementError> {
    let base_activity_xp = manual_fishing_successful_base_aexp_v1(outcome)?;
    let provenance = json!({
        "activity": "FISHING",
        "mode": "MANUAL",
        "outcome": outcome,
    });

    let account_xp = apply_account_xp_settlement(
        tx,
        &AccountXpSettlementRequest {
            operation_id,
            player_id,
            amount: MANUAL_FISHING_ACCOUNT_XP_REWARD,
            source: MANUAL_FISHING_PROGRESSION_SOURCE.to_owned(),
            provenance: provenance.clone(),
        },
    )
    .await?;

    let activity_xp = apply_activity_xp_mutation(
        tx,
        &ActivityXpMutationRequest {
            operation_id,
            player_id,
            mutation_key: MANUAL_FISHING_ACTIVITY_XP_MUTATION_KEY.to_owned(),
            kind: ActivityXpMutationKind::Grant,
            amount: base_activity_xp,
            source: MANUAL_FISHING_PROGRESSION_SOURCE.to_owned(),
            provenance,
        },
    )
    .await?;

    Ok(ManualFishingProgressionSettlementReceiptV1 {
        operation_id,
        player_id,
        base_activity_xp,
        account_xp,
        activity_xp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_successful_outcomes_use_the_frozen_manual_fishing_aexp_table() {
        assert_eq!(
            manual_fishing_successful_base_aexp_v1(
                &ManualFishingSuccessfulProgressionOutcomeV1::Junk
            ),
            Ok(2)
        );
        assert_eq!(
            manual_fishing_successful_base_aexp_v1(
                &ManualFishingSuccessfulProgressionOutcomeV1::Treasure { landed_count: 3 }
            ),
            Ok(10)
        );
        assert_eq!(
            manual_fishing_successful_base_aexp_v1(
                &ManualFishingSuccessfulProgressionOutcomeV1::Fish {
                    landed_rarities: vec![
                        FishingRarity::Common,
                        FishingRarity::Mythic,
                        FishingRarity::Rare,
                    ],
                }
            ),
            Ok(9)
        );
    }
}
