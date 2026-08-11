//! Typed shape of `assets/pipeline_defs/*.yaml`.

use serde::{Deserialize, Serialize};

use crate::modes::VideoGenMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stability {
    Production,
    Beta,
}

impl Stability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Production => "production",
            Self::Beta => "beta",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointPolicyDefault {
    Guided,
    ManualAll,
    AutoNoncreative,
}

impl From<CheckpointPolicyDefault> for crate::config::CheckpointPolicy {
    fn from(v: CheckpointPolicyDefault) -> Self {
        match v {
            CheckpointPolicyDefault::Guided => Self::Guided,
            CheckpointPolicyDefault::ManualAll => Self::ManualAll,
            CheckpointPolicyDefault::AutoNoncreative => Self::AutoNoncreative,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationSpec {
    #[serde(default = "default_orchestration_mode")]
    pub mode: String,
    pub skill: String,
    #[serde(default = "default_max_revisions")]
    pub max_revisions_per_stage: u32,
    #[serde(default = "default_max_send_backs")]
    pub max_send_backs: u32,
    #[serde(default = "default_max_wall_time")]
    pub max_wall_time_minutes: u32,
    #[serde(default)]
    pub budget_default: Option<u64>,
}

fn default_orchestration_mode() -> String {
    "executive-producer".to_string()
}
fn default_max_revisions() -> u32 {
    3
}
fn default_max_send_backs() -> u32 {
    3
}
fn default_max_wall_time() -> u32 {
    45
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StageSpec {
    pub name: String,
    pub skill: String,
    #[serde(default)]
    pub produces: Vec<String>,
    #[serde(default)]
    pub required_artifacts_in: Vec<String>,
    #[serde(default)]
    pub tools_available: Vec<String>,
    #[serde(default)]
    pub checkpoint_required: bool,
    #[serde(default)]
    pub human_approval_default: bool,
    #[serde(default)]
    pub review_focus: Vec<String>,
    #[serde(default)]
    pub success_criteria: Vec<String>,
}

impl StageSpec {
    /// Canonical artifact produced by this stage (first of `produces`, if any).
    pub fn canonical_artifact(&self) -> Option<&str> {
        self.produces.first().map(String::as_str)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineManifest {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub category: String,
    pub stability: Stability,
    #[serde(default)]
    pub default_checkpoint_policy: CheckpointPolicyDefault,
    #[serde(default)]
    pub mode: VideoGenMode,
    pub orchestration: OrchestrationSpec,
    pub stages: Vec<StageSpec>,
}

impl Default for CheckpointPolicyDefault {
    fn default() -> Self {
        Self::Guided
    }
}

impl PipelineManifest {
    pub fn stage(&self, name: &str) -> Option<&StageSpec> {
        self.stages.iter().find(|s| s.name == name)
    }

    pub fn stage_index(&self, name: &str) -> Option<usize> {
        self.stages.iter().position(|s| s.name == name)
    }

    pub fn first_stage(&self) -> Option<&StageSpec> {
        self.stages.first()
    }

    pub fn next_stage(&self, current: &str) -> Option<&StageSpec> {
        let idx = self.stage_index(current)?;
        self.stages.get(idx + 1)
    }

    pub fn is_last_stage(&self, name: &str) -> bool {
        self.stage_index(name)
            .is_some_and(|idx| idx + 1 == self.stages.len())
    }

    /// Every distinct tool name referenced across all stages (used by preflight).
    pub fn all_tool_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .stages
            .iter()
            .flat_map(|s| s.tools_available.iter().cloned())
            .collect();
        names.sort();
        names.dedup();
        names
    }
}

/// Lightweight summary surfaced by `MontageService::list_pipelines`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineSummary {
    pub name: String,
    pub version: String,
    pub description: String,
    pub category: String,
    pub stability: Stability,
    pub mode: VideoGenMode,
    pub stage_count: usize,
    pub stage_names: Vec<String>,
}

impl From<&PipelineManifest> for PipelineSummary {
    fn from(m: &PipelineManifest) -> Self {
        Self {
            name: m.name.clone(),
            version: m.version.clone(),
            description: m.description.clone(),
            category: m.category.clone(),
            stability: m.stability,
            mode: m.mode,
            stage_count: m.stages.len(),
            stage_names: m.stages.iter().map(|s| s.name.clone()).collect(),
        }
    }
}
