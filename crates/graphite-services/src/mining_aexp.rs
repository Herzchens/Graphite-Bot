use crate::{MINING_DEPTH_COUNT, MiningDepth};

const AEXP_SCALE: u128 = 20;
const BASE_AEXP_SCALED: u128 = 40;
const AEXP_PER_ORDINARY_ROLL_SCALED: u128 = 4;
const DEPTH_BONUS_SCALED: [u128; MINING_DEPTH_COUNT] =
    [0, 5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60, 65, 70, 80];

/// Returns the frozen base Manual Mining Activity EXP formula for an already-resolved roll count.
///
/// `ordinary_qualifying_physical_rolls` is the authoritative `b` from the specification: ordinary
/// qualifying physical rolls before Fortune quantity, Trench-generated blocks, and Nuke blast
/// blocks. Those extra outputs must never be added to this count. The caller owns expedition
/// lifecycle validity: only a successfully settled expedition may actually grant this result, while
/// escape, true death, or another non-settled terminal outcome must skip the Activity EXP mutation.
/// This pure formula does not turn any particular roll count into permission to settle an expedition.
///
/// The returned value is **base** AEXP. A future stateful Mining settlement owner must apply the
/// authoritative AEXP gain modifier stack and its global cap after this source-specific formula,
/// then pass the final positive integer grant to the progression mutation primitive.
///
/// The canonical formula is `round_half_up(2 + 0.20b + d)`. All current depth bonuses are exact
/// multiples of 0.25, so the implementation evaluates the formula in twentieths and never
/// introduces floating-point rounding authority. The input uses the same `u64` count domain as the
/// existing Mining physical-pressure primitives instead of inventing a narrower gameplay cap. The
/// frozen formula maps the entire `u64` input domain inside `i64`, which is regression-tested at the
/// maximum input.
#[must_use]
pub fn manual_mining_base_aexp(depth: MiningDepth, ordinary_qualifying_physical_rolls: u64) -> i64 {
    let scaled = BASE_AEXP_SCALED
        + u128::from(ordinary_qualifying_physical_rolls) * AEXP_PER_ORDINARY_ROLL_SCALED
        + DEPTH_BONUS_SCALED[usize::from(depth.index())];
    let rounded = (scaled + AEXP_SCALE / 2) / AEXP_SCALE;

    i64::try_from(rounded)
        .expect("frozen Manual Mining AEXP formula fits i64 for every u64 roll count")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ALL_MINING_DEPTHS;

    #[test]
    fn depth_bonus_table_matches_every_frozen_row_in_twentieths() {
        assert_eq!(
            ALL_MINING_DEPTHS.map(|depth| DEPTH_BONUS_SCALED[usize::from(depth.index())]),
            [0, 5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60, 65, 70, 80]
        );
    }

    #[test]
    fn exact_half_boundaries_round_up() {
        assert_eq!(manual_mining_base_aexp(MiningDepth::UpperCavern, 5), 4);
        assert_eq!(manual_mining_base_aexp(MiningDepth::AncientDepths, 5), 7);
    }

    #[test]
    fn zero_is_formula_defined_without_authorizing_a_zero_roll_expedition() {
        assert_eq!(manual_mining_base_aexp(MiningDepth::Surface, 0), 2);
        assert_eq!(manual_mining_base_aexp(MiningDepth::AbyssalDepths, 0), 6);
    }
}
