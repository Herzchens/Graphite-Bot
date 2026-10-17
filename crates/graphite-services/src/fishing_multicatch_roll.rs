use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    MULTICATCH_MAX_LEVEL, MULTICATCH_PROBABILITY_SCALE_PPM, ManualFishingPpmDrawV1,
    ManualFishingRngContext, ManualFishingRngDrawError, MulticatchCount, MulticatchPolicyError,
    multicatch_count_policy,
};

const OUTCOMES: [MulticatchCount; 5] = [
    MulticatchCount::Single,
    MulticatchCount::Double,
    MulticatchCount::Triple,
    MulticatchCount::Quadruple,
    MulticatchCount::Quintuple,
];

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ManualFishingMulticatchCountV1 {
    pub level: u8,
    pub draw_ppm: u32,
    pub outcome: MulticatchCount,
    pub fish_count: u8,
    pub selected_probability_ppm: u32,
}

#[derive(Debug, Error)]
pub enum ManualFishingMulticatchRollError {
    #[error(transparent)]
    Policy(#[from] MulticatchPolicyError),
    #[error(transparent)]
    Rng(#[from] ManualFishingRngDrawError),
    #[error("canonical Multicatch probabilities do not sum to the manual Fishing PPM scale")]
    InvalidDistribution,
}

/// Resolves the owner-approved Multicatch I-X count from the operation-pinned
/// semantic PPM domain.
///
/// The stable v1 bucket layout is the canonical enum order:
/// Single, Double, Triple, Quadruple, Quintuple. Each interval length is exactly
/// the existing integer-PPM policy weight for the supplied level. This freezes
/// replay mapping without introducing floating-point probability or another RNG
/// stream. Additional FishInstance identity remains independently resolved by
/// the existing per-fish-slot domains.
pub fn resolve_manual_fishing_multicatch_count_v1(
    rng: ManualFishingRngContext,
    level: u8,
) -> Result<ManualFishingMulticatchCountV1, ManualFishingMulticatchRollError> {
    if !(1..=MULTICATCH_MAX_LEVEL).contains(&level) {
        return Err(MulticatchPolicyError::InvalidLevel(level).into());
    }

    let policies = [
        multicatch_count_policy(level, MulticatchCount::Single)?,
        multicatch_count_policy(level, MulticatchCount::Double)?,
        multicatch_count_policy(level, MulticatchCount::Triple)?,
        multicatch_count_policy(level, MulticatchCount::Quadruple)?,
        multicatch_count_policy(level, MulticatchCount::Quintuple)?,
    ];
    let total = policies
        .iter()
        .try_fold(0_u32, |sum, policy| sum.checked_add(policy.probability_ppm))
        .ok_or(ManualFishingMulticatchRollError::InvalidDistribution)?;
    if total != MULTICATCH_PROBABILITY_SCALE_PPM {
        return Err(ManualFishingMulticatchRollError::InvalidDistribution);
    }

    let draw_ppm = rng.sample_ppm_v1(ManualFishingPpmDrawV1::MulticatchCount)?;
    let mut cumulative = 0_u32;
    for (outcome, policy) in OUTCOMES.into_iter().zip(policies) {
        cumulative = cumulative
            .checked_add(policy.probability_ppm)
            .ok_or(ManualFishingMulticatchRollError::InvalidDistribution)?;
        if draw_ppm < cumulative {
            return Ok(ManualFishingMulticatchCountV1 {
                level,
                draw_ppm,
                outcome,
                fish_count: policy.fish_count,
                selected_probability_ppm: policy.probability_ppm,
            });
        }
    }
    Err(ManualFishingMulticatchRollError::InvalidDistribution)
}
