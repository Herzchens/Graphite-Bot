use thiserror::Error;

use crate::{
    FishingCatchBranch, FishingRodLevelXPolicyError, ManualFishingRngContext,
    ManualFishingRngDrawError, ManualFishingWeightedDrawV1, preview_treasure_level_x_branch_weight,
};

const BRANCHES: [FishingCatchBranch; 3] = [
    FishingCatchBranch::Fish,
    FishingCatchBranch::Junk,
    FishingCatchBranch::Treasure,
];

#[derive(Debug, Error)]
pub enum ManualFishingTreasureXBranchError {
    #[error(transparent)]
    Policy(#[from] FishingRodLevelXPolicyError),
    #[error(transparent)]
    Rng(#[from] ManualFishingRngDrawError),
    #[error("Treasure X branch weights cannot be represented on one exact integer scale")]
    UnsupportedIntegerScale,
}

/// Resolves the exact isolated Treasure-X catch-branch draw.
///
/// Treasure X multiplies only the Treasure relative branch weight by 9/5 before
/// normalization. This helper is restricted to the zero-other-modifier path;
/// Gold Rod, active bait, or another Rod enchant must use the shared capped
/// modifier composer instead.
pub fn resolve_manual_fishing_treasure_x_branch_v1(
    rng: ManualFishingRngContext,
) -> Result<FishingCatchBranch, ManualFishingTreasureXBranchError> {
    let weights = treasure_x_integer_weights_v1()?;
    let selected = rng.sample_weighted_v1(ManualFishingWeightedDrawV1::CatchBranch, &weights)?;
    Ok(BRANCHES[selected])
}

fn treasure_x_integer_weights_v1() -> Result<[u64; 3], ManualFishingTreasureXBranchError> {
    let previews = [
        preview_treasure_level_x_branch_weight(FishingCatchBranch::Fish)?,
        preview_treasure_level_x_branch_weight(FishingCatchBranch::Junk)?,
        preview_treasure_level_x_branch_weight(FishingCatchBranch::Treasure)?,
    ];
    let mut scale = 1_u64;
    for preview in previews {
        scale = checked_lcm(
            scale,
            u64::from(preview.adjusted_relative_weight_denominator()),
        )
        .ok_or(ManualFishingTreasureXBranchError::UnsupportedIntegerScale)?;
    }

    let mut weights = [0_u64; 3];
    for (index, preview) in previews.into_iter().enumerate() {
        let denominator = u64::from(preview.adjusted_relative_weight_denominator());
        if denominator == 0 || !scale.is_multiple_of(denominator) {
            return Err(ManualFishingTreasureXBranchError::UnsupportedIntegerScale);
        }
        weights[index] = u64::from(preview.adjusted_relative_weight_numerator())
            .checked_mul(scale / denominator)
            .ok_or(ManualFishingTreasureXBranchError::UnsupportedIntegerScale)?;
        if weights[index] == 0 {
            return Err(ManualFishingTreasureXBranchError::UnsupportedIntegerScale);
        }
    }
    Ok(weights)
}

fn checked_lcm(left: u64, right: u64) -> Option<u64> {
    if left == 0 || right == 0 {
        return None;
    }
    let gcd = gcd(left, right);
    (left / gcd).checked_mul(right)
}

fn gcd(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn treasure_x_exact_integer_branch_weights_are_frozen() {
        assert_eq!(treasure_x_integer_weights_v1().unwrap(), [880, 85, 63]);
        assert_eq!(
            treasure_x_integer_weights_v1()
                .unwrap()
                .into_iter()
                .sum::<u64>(),
            1_028
        );
    }
}
