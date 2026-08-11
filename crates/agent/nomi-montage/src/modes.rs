//! Video-generation product modes — sibling surfaces that a pipeline belongs to.
//!
//! `Agent` is the OpenMontage-faithful multi-stage producer covered by this crate.
//! `Avatar` groups pipelines whose deliverable is a digital-human performance
//! (spokesperson / talking-head) rather than a cinematic edit. `TalkingHead` is
//! kept as a distinct mode value (not merely a pipeline) because product surfaces
//! (home page, model pickers) key off it directly. `Creation` is the existing,
//! fully independent Canvas surface — represented here only as a placeholder so
//! callers can enumerate "all video-generation modes" without special-casing it.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoGenMode {
    /// Multi-stage Executive-Producer pipelines (this crate's core mechanism).
    Agent,
    /// Freeform Canvas creation — independent surface, not implemented here.
    Creation,
    /// Digital-human spokesperson / avatar performance pipelines.
    Avatar,
    /// Single-shot talking-head narration (script → talking avatar clip).
    TalkingHead,
}

impl Default for VideoGenMode {
    fn default() -> Self {
        Self::Agent
    }
}

impl VideoGenMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Creation => "creation",
            Self::Avatar => "avatar",
            Self::TalkingHead => "talking_head",
        }
    }

    pub fn all() -> &'static [VideoGenMode] {
        &[
            VideoGenMode::Agent,
            VideoGenMode::Creation,
            VideoGenMode::Avatar,
            VideoGenMode::TalkingHead,
        ]
    }
}

impl std::fmt::Display for VideoGenMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_mode_is_agent() {
        assert_eq!(VideoGenMode::default(), VideoGenMode::Agent);
    }

    #[test]
    fn serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&VideoGenMode::TalkingHead).unwrap(),
            "\"talking_head\""
        );
    }
}
