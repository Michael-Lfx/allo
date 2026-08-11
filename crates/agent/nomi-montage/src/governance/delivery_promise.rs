//! Delivery promise — locked at `proposal`, enforced before `compose`.
//!
//! Rule Zero (see `assets/CONTRACT.md`): once a proposal promises a motion
//! film, `compose` must not silently fall back to a slideshow of stills. If
//! reality can't meet the promise, the orchestrator must surface that to the
//! human via `awaiting_human`, not quietly degrade the deliverable.

use serde::{Deserialize, Serialize};

use crate::error::{MontageError, MontageResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryPromise {
    /// Every shot is a generated/sourced motion clip.
    Motion,
    /// A deliberate mix of motion clips and held stills (e.g. explainer beats).
    HybridMotionStill,
    /// Explicitly promised as a still-image slideshow (rare; must be an
    /// explicit proposal choice, never an implicit default).
    Slideshow,
}

impl DeliveryPromise {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Motion => "motion",
            Self::HybridMotionStill => "hybrid_motion_still",
            Self::Slideshow => "slideshow",
        }
    }

    /// Minimum fraction (0.0–1.0) of shots that must be real motion clips to
    /// satisfy this promise at compose time.
    pub fn min_motion_fraction(self) -> f32 {
        match self {
            Self::Motion => 0.95,
            Self::HybridMotionStill => 0.4,
            Self::Slideshow => 0.0,
        }
    }
}

/// Checks the actual shot mix against the locked promise. Returns an error
/// (never a silent pass) when the deliverable would under-deliver.
pub fn check_no_silent_downgrade(
    promise: DeliveryPromise,
    total_shots: usize,
    motion_shots: usize,
) -> MontageResult<()> {
    if total_shots == 0 {
        return Err(MontageError::GovernanceBlocked(
            "delivery_promise check ran with zero shots — asset_manifest is empty".into(),
        ));
    }
    let fraction = motion_shots as f32 / total_shots as f32;
    let required = promise.min_motion_fraction();
    if fraction + f32::EPSILON < required {
        return Err(MontageError::GovernanceBlocked(format!(
            "compose would violate the locked delivery_promise '{}': only {motion_shots}/{total_shots} \
             shots ({:.0}%) are motion clips, need >= {:.0}%. Send back to `assets`/`edit`, or have the \
             human explicitly re-promise a lower tier — never downgrade silently.",
            promise.as_str(),
            fraction * 100.0,
            required * 100.0
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_promise_blocks_mostly_stills() {
        let err = check_no_silent_downgrade(DeliveryPromise::Motion, 10, 3).unwrap_err();
        assert!(matches!(err, MontageError::GovernanceBlocked(_)));
    }

    #[test]
    fn motion_promise_allows_almost_all_motion() {
        check_no_silent_downgrade(DeliveryPromise::Motion, 10, 10).unwrap();
    }

    #[test]
    fn hybrid_promise_allows_partial_stills() {
        check_no_silent_downgrade(DeliveryPromise::HybridMotionStill, 10, 5).unwrap();
    }

    #[test]
    fn slideshow_promise_never_blocks() {
        check_no_silent_downgrade(DeliveryPromise::Slideshow, 10, 0).unwrap();
    }

    #[test]
    fn empty_manifest_is_blocked_not_silently_ok() {
        let err = check_no_silent_downgrade(DeliveryPromise::Motion, 0, 0).unwrap_err();
        assert!(matches!(err, MontageError::GovernanceBlocked(_)));
    }
}
