use serde::Serialize;
use thiserror::Error;

use crate::{
    FishingNumericError, ManualFishingPpmDrawV1, ManualFishingRngContext,
    ManualFishingRngDrawError, ManualFishingUnmodifiedCapabilityPreviewV1,
    preview_manual_fishing_line_break_chance_v1,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "stage", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ManualFishingUnmodifiedLineStageV1 {
    LandedWithoutLineRisk,
    LineBroken { draw_ppm: u32, chance_ppm: u32 },
    LineSurvivedOverCapCatchPending { draw_ppm: u32, chance_ppm: u32 },
}

#[derive(Debug, Error)]
pub enum ManualFishingUnmodifiedLineStageError {
    #[error(transparent)]
    Rng(#[from] ManualFishingRngDrawError),
    #[error(transparent)]
    Numeric(#[from] FishingNumericError),
    #[error("line-break preview disagrees with the canonical exact-ratio numeric policy")]
    MismatchedLineBreakPreview,
}

/// Decides only the first ordered over-cap stage using the operation-pinned PPM domain.
///
/// Within-capability and Starter Pool Fish never draw LineBreak RNG. For over-cap Fish,
/// the exact-ratio line-break probability is independently revalidated before comparing
/// the persisted seed's stable PPM bucket with the versioned PPM chance. On survival,
/// over-cap catch/escape remains explicitly unresolved; no assets/XP/durability/result
/// or operation commit is authorized by this provisional resolution.
pub(crate) fn resolve_manual_unmodified_line_stage_v1(
    rng: ManualFishingRngContext,
    preview: ManualFishingUnmodifiedCapabilityPreviewV1,
) -> Result<ManualFishingUnmodifiedLineStageV1, ManualFishingUnmodifiedLineStageError> {
    let ManualFishingUnmodifiedCapabilityPreviewV1::OverCapPending {
        ratio, line_break, ..
    } = preview
    else {
        return Ok(ManualFishingUnmodifiedLineStageV1::LandedWithoutLineRisk);
    };
    if preview_manual_fishing_line_break_chance_v1(ratio, false)? != line_break {
        return Err(ManualFishingUnmodifiedLineStageError::MismatchedLineBreakPreview);
    }
    let draw_ppm = rng.sample_ppm_v1(ManualFishingPpmDrawV1::LineBreak)?;
    let chance_ppm = line_break.final_probability_ppm;
    Ok(if line_break_triggered(draw_ppm, chance_ppm) {
        ManualFishingUnmodifiedLineStageV1::LineBroken {
            draw_ppm,
            chance_ppm,
        }
    } else {
        ManualFishingUnmodifiedLineStageV1::LineSurvivedOverCapCatchPending {
            draw_ppm,
            chance_ppm,
        }
    })
}

#[must_use]
const fn line_break_triggered(draw_ppm: u32, chance_ppm: u32) -> bool {
    draw_ppm < chance_ppm
}

#[cfg(test)]
mod tests {
    use super::line_break_triggered;

    #[test]
    fn zero_and_maximum_ppm_thresholds_are_exclusive() {
        assert!(!line_break_triggered(0, 0));
        assert!(line_break_triggered(0, 1));
        assert!(line_break_triggered(149_999, 150_000));
        assert!(!line_break_triggered(150_000, 150_000));
    }
}
