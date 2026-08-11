//! Montage runtime configuration — a thin, Flowy-shaped subset of the
//! OpenMontage `config.yaml` surface (see `assets/CONTRACT.md`).

use serde::{Deserialize, Serialize};

/// Checkpoint policy, chosen per-project or defaulted from the pipeline manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointPolicy {
    /// Human approval only at stages the pipeline marks `human_approval_default: true`.
    Guided,
    /// Every stage pauses for human approval regardless of the pipeline default.
    ManualAll,
    /// Only creative stages pause; non-creative stages (e.g. compose, publish
    /// bookkeeping) auto-advance. Used by CI / smoke pipelines.
    AutoNoncreative,
}

impl Default for CheckpointPolicy {
    fn default() -> Self {
        Self::Guided
    }
}

impl CheckpointPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Guided => "guided",
            Self::ManualAll => "manual_all",
            Self::AutoNoncreative => "auto_noncreative",
        }
    }

    /// Non-creative stage names that `auto_noncreative` fast-forwards through even
    /// when the pipeline requests human approval.
    const NONCREATIVE_STAGES: &'static [&'static str] =
        &["compose", "publish", "cost", "preflight"];

    /// Whether `stage` should still pause for human approval under this policy,
    /// given the pipeline's own default for that stage.
    pub fn requires_human(self, stage_name: &str, pipeline_default: bool) -> bool {
        match self {
            Self::Guided => pipeline_default,
            Self::ManualAll => true,
            Self::AutoNoncreative => {
                pipeline_default && !Self::NONCREATIVE_STAGES.contains(&stage_name)
            }
        }
    }
}

/// Default model triplet (chat / image / video) resolved from user preferences or
/// the Flowy server default when empty.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelSelection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<String>,
}

/// Output format defaults for a project (aspect / resolution / fps pickers).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputSpec {
    #[serde(default = "default_aspect")]
    pub aspect: String,
    #[serde(default = "default_resolution")]
    pub resolution: String,
    #[serde(default = "default_fps")]
    pub fps: u32,
}

impl Default for OutputSpec {
    fn default() -> Self {
        Self {
            aspect: default_aspect(),
            resolution: default_resolution(),
            fps: default_fps(),
        }
    }
}

fn default_aspect() -> String {
    "16:9".to_string()
}
fn default_resolution() -> String {
    "1080p".to_string()
}
fn default_fps() -> u32 {
    24
}

/// Orchestrator-wide defaults, overridable per pipeline manifest / per project.
#[derive(Debug, Clone, Copy)]
pub struct OrchestratorDefaults {
    /// Max LLM↔tool turns inside a single stage before it is forced to fail.
    pub max_tool_turns_per_stage: u32,
    /// Max advisory reviewer rounds per stage.
    pub max_reviewer_rounds: u32,
    pub default_max_revisions_per_stage: u32,
    pub default_max_send_backs: u32,
    pub default_max_wall_time_minutes: u32,
}

impl Default for OrchestratorDefaults {
    fn default() -> Self {
        Self {
            max_tool_turns_per_stage: 24,
            max_reviewer_rounds: 2,
            default_max_revisions_per_stage: 3,
            default_max_send_backs: 3,
            default_max_wall_time_minutes: 45,
        }
    }
}

/// Runtime configuration snapshot used by `MontageService` / the orchestrator.
#[derive(Debug, Clone)]
pub struct MontageRuntimeConfig {
    pub default_checkpoint_policy: CheckpointPolicy,
    pub default_output: OutputSpec,
    pub default_budget_credits: u64,
    pub orchestrator: OrchestratorDefaults,
}

impl Default for MontageRuntimeConfig {
    fn default() -> Self {
        Self {
            default_checkpoint_policy: CheckpointPolicy::default(),
            default_output: OutputSpec::default(),
            default_budget_credits: 1_500,
            orchestrator: OrchestratorDefaults::default(),
        }
    }
}

impl From<&nomi_config::GatewayConfig> for MontageRuntimeConfig {
    fn from(cfg: &nomi_config::GatewayConfig) -> Self {
        Self {
            default_output: OutputSpec {
                aspect: crate_normalize_aspect(&cfg.media.video.default_aspect_ratio),
                resolution: cfg.media.video.default_resolution.clone(),
                fps: default_fps(),
            },
            ..Self::default()
        }
    }
}

fn crate_normalize_aspect(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() { default_aspect() } else { t.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_noncreative_skips_compose_but_keeps_creative_gates() {
        let p = CheckpointPolicy::AutoNoncreative;
        assert!(!p.requires_human("compose", true));
        assert!(p.requires_human("script", true));
        assert!(!p.requires_human("script", false));
    }

    #[test]
    fn manual_all_always_requires_human() {
        assert!(CheckpointPolicy::ManualAll.requires_human("compose", false));
    }

    #[test]
    fn guided_follows_pipeline_default() {
        let p = CheckpointPolicy::Guided;
        assert!(p.requires_human("script", true));
        assert!(!p.requires_human("script", false));
    }
}
