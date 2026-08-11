//! Seven-dimension quality rubric shared by stage reviewers and `final_review`.
//!
//! The dimensions are deliberately generic ("does the deliverable succeed at
//! being a film", not "does it match provider X's checklist") so the same
//! rubric applies whether compose used ffmpeg, Remotion, or an avatar render.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoreDimension {
    Concept,
    ScriptStory,
    VisualCraft,
    MotionContinuity,
    Sound,
    PacingDelivery,
    TechnicalRobustness,
}

impl ScoreDimension {
    pub const ALL: [ScoreDimension; 7] = [
        Self::Concept,
        Self::ScriptStory,
        Self::VisualCraft,
        Self::MotionContinuity,
        Self::Sound,
        Self::PacingDelivery,
        Self::TechnicalRobustness,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Concept => "concept",
            Self::ScriptStory => "script_story",
            Self::VisualCraft => "visual_craft",
            Self::MotionContinuity => "motion_continuity",
            Self::Sound => "sound",
            Self::PacingDelivery => "pacing_delivery",
            Self::TechnicalRobustness => "technical_robustness",
        }
    }

    /// Default weight (sums to 1.0 across all dimensions). Sound is weighted
    /// lower until TTS/music land server-side (see `flowy-capability-overlay`).
    pub fn default_weight(self) -> f32 {
        match self {
            Self::Concept => 0.12,
            Self::ScriptStory => 0.18,
            Self::VisualCraft => 0.18,
            Self::MotionContinuity => 0.16,
            Self::Sound => 0.08,
            Self::PacingDelivery => 0.16,
            Self::TechnicalRobustness => 0.12,
        }
    }
}

/// A full set of per-dimension weights, normalized to sum to 1.0.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoringWeights(std::collections::BTreeMap<String, f32>);

impl Default for ScoringWeights {
    fn default() -> Self {
        let map = ScoreDimension::ALL
            .iter()
            .map(|d| (d.as_str().to_string(), d.default_weight()))
            .collect();
        Self(map)
    }
}

impl ScoringWeights {
    pub fn weight(&self, dim: ScoreDimension) -> f32 {
        self.0.get(dim.as_str()).copied().unwrap_or(0.0)
    }

    pub fn sum(&self) -> f32 {
        self.0.values().sum()
    }
}

/// Compute a weighted total (0.0–10.0 scale) from per-dimension raw scores
/// (each expected in 0.0–10.0). Missing dimensions score 0 for that slice.
pub fn total_score(
    scores: &std::collections::BTreeMap<ScoreDimension, f32>,
    weights: &ScoringWeights,
) -> f32 {
    ScoreDimension::ALL
        .iter()
        .map(|d| scores.get(d).copied().unwrap_or(0.0) * weights.weight(*d))
        .sum()
}

/// Minimum weighted score (0-10) a `final_review` must reach for `publish` to proceed
/// without an explicit human override.
pub const PUBLISH_QUALITY_GATE: f32 = 6.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_weights_sum_to_one() {
        let w = ScoringWeights::default();
        assert!((w.sum() - 1.0).abs() < 0.001, "weights sum = {}", w.sum());
    }

    #[test]
    fn total_score_is_weighted_average() {
        let weights = ScoringWeights::default();
        let mut scores = std::collections::BTreeMap::new();
        for d in ScoreDimension::ALL {
            scores.insert(d, 10.0);
        }
        let total = total_score(&scores, &weights);
        assert!((total - 10.0).abs() < 0.01, "all-10s should score ~10, got {total}");
    }

    #[test]
    fn missing_dimension_scores_zero_for_that_slice() {
        let weights = ScoringWeights::default();
        let scores = std::collections::BTreeMap::new();
        assert_eq!(total_score(&scores, &weights), 0.0);
    }
}
