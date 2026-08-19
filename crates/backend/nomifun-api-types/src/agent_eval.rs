//! Eval run DTOs for the developer-mode agent evaluation lab.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalSuiteDescriptor {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub default_task_profile: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    pub default_limit: usize,
    pub max_limit: usize,
    pub notes: String,
    pub requires_download: bool,
    pub cached: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StartEvalRunRequest {
    pub suite: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct PullEvalDatasetResponse {
    pub suite: String,
    pub corpus_version: String,
    pub cases: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalScorerView {
    pub scorer_type: String,
    pub passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalCaseView {
    pub case_id: String,
    pub category: String,
    pub success: bool,
    pub elapsed_ms: u128,
    pub turns: u32,
    pub tool_call_count: u32,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub tool_error_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub scorer_results: Vec<EvalScorerView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default)]
    pub trajectory_event_count: u32,
    #[serde(default)]
    pub artifact_count: u32,
    #[serde(default)]
    pub has_trace: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct EvalTrajectoryEventView {
    pub kind: String,
    pub ts_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvalArtifactView {
    pub path: String,
    pub size_bytes: u64,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct EvalCaseTraceView {
    pub case_id: String,
    #[serde(default)]
    pub live: bool,
    #[serde(default)]
    pub assistant_text: String,
    #[serde(default)]
    pub events: Vec<EvalTrajectoryEventView>,
    #[serde(default)]
    pub artifacts: Vec<EvalArtifactView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalCategoryView {
    pub category: String,
    pub total: usize,
    pub passed: usize,
    pub success_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct EvalSummaryView {
    pub total_cases: usize,
    pub passed: usize,
    pub failed: usize,
    pub success_rate: f64,
    pub avg_turns: f64,
    pub avg_elapsed_ms: f64,
    pub avg_input_tokens: f64,
    pub avg_output_tokens: f64,
    #[serde(default)]
    pub by_category: Vec<EvalCategoryView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalRunView {
    pub run_id: String,
    pub status: String,
    pub suite: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    pub planned: usize,
    pub completed: usize,
    pub passed: usize,
    pub failed: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_case_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<EvalSummaryView>,
    #[serde(default)]
    pub cases: Vec<EvalCaseView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_trace: Option<EvalCaseTraceView>,
}
