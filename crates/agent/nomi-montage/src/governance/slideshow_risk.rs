//! Slideshow-risk scoring — an early-warning signal distinct from
//! `delivery_promise` (which checks the *locked promise*; this checks whether
//! the *edit itself* is at risk of reading as static regardless of promise).

use serde::{Deserialize, Serialize};

/// Score at/above this threshold blocks `compose` and sends the project back
/// to `assets`/`edit` for more motion coverage or shorter still holds.
pub const SLIDESHOW_RISK_BLOCK_THRESHOLD: f32 = 0.6;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlideshowRiskInputs {
    pub total_shots: usize,
    pub still_shots: usize,
    /// Average held-still duration in seconds (0 if no stills).
    pub avg_still_hold_secs: f32,
    /// Number of consecutive-still runs of length >= 3.
    pub long_still_runs: usize,
}

/// Weighted 0.0 (no risk) – 1.0 (certain to read as a slideshow) score.
///
/// Heuristic (not lifted from any third-party tool): combines the still
/// fraction, how long stills are held, and whether stills cluster into runs
/// (a single still amid motion reads fine; three in a row reads as a
/// slideshow even at the same overall fraction).
pub fn compute_slideshow_risk(inputs: &SlideshowRiskInputs) -> f32 {
    if inputs.total_shots == 0 {
        return 0.0;
    }
    let still_fraction = inputs.still_shots as f32 / inputs.total_shots as f32;
    let hold_penalty = (inputs.avg_still_hold_secs / 6.0).clamp(0.0, 1.0);
    let run_penalty = (inputs.long_still_runs as f32 / 3.0).clamp(0.0, 1.0);

    let score = 0.55 * still_fraction + 0.25 * hold_penalty + 0.20 * run_penalty;
    score.clamp(0.0, 1.0)
}

pub fn is_blocked(score: f32) -> bool {
    score >= SLIDESHOW_RISK_BLOCK_THRESHOLD
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_motion_scores_zero_risk() {
        let inputs = SlideshowRiskInputs {
            total_shots: 12,
            still_shots: 0,
            avg_still_hold_secs: 0.0,
            long_still_runs: 0,
        };
        assert_eq!(compute_slideshow_risk(&inputs), 0.0);
    }

    #[test]
    fn all_stills_blocks() {
        let inputs = SlideshowRiskInputs {
            total_shots: 12,
            still_shots: 12,
            avg_still_hold_secs: 5.0,
            long_still_runs: 4,
        };
        let score = compute_slideshow_risk(&inputs);
        assert!(is_blocked(score), "expected block, score={score}");
    }

    #[test]
    fn light_still_sprinkle_does_not_block() {
        let inputs = SlideshowRiskInputs {
            total_shots: 12,
            still_shots: 1,
            avg_still_hold_secs: 2.0,
            long_still_runs: 0,
        };
        let score = compute_slideshow_risk(&inputs);
        assert!(!is_blocked(score), "expected no block, score={score}");
    }

    #[test]
    fn threshold_boundary_is_blocked_inclusive() {
        assert!(is_blocked(SLIDESHOW_RISK_BLOCK_THRESHOLD));
        assert!(!is_blocked(SLIDESHOW_RISK_BLOCK_THRESHOLD - 0.01));
    }
}
