//! Corpus, scorer, and evidence types for conversation-agent evaluation.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;
pub const SCORING_VERSION: &str = "agent-eval-v2";

/// Per-case turn / token budgets.
///
/// `max_tokens` is a **cumulative output-token** runaway cap for the case, not
/// context/input. Coding turns routinely send 8k–30k input (system + tools +
/// files) per model call; counting those against this field false-fails every case.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct CaseBudgets {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
    /// Cumulative `output_tokens` across the live episode. Omit to disable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

/// Deterministic scorer specification (corpus JSON).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScorerSpec {
    AssistantContains {
        marker: String,
        #[serde(default = "default_minimum_hits")]
        minimum_hits: usize,
    },
    AssistantNotContains {
        marker: String,
    },
    ToolCalled {
        name: String,
    },
    ToolNotCalled {
        name: String,
    },
    MaxToolCalls {
        max: u32,
    },
    MaxTurns {
        max: u32,
    },
    RegexMatch {
        pattern: String,
        #[serde(default = "default_minimum_hits")]
        minimum_hits: usize,
    },
    FileContains {
        path: String,
        marker: String,
    },
    FileExists {
        path: String,
    },
    CommandExitZero {
        command: String,
    },
    /// Run `find_python()` with the given args in the isolated workspace (e.g. `-m pytest …`).
    PythonModule {
        args: Vec<String>,
    },
    StopReasonIn {
        reasons: Vec<String>,
    },
    /// Hidden Python oracle (HumanEval / MBPP). Tests are not shown in the prompt.
    PythonHiddenCheck {
        entry_point: String,
        test: String,
    },
}

fn default_minimum_hits() -> usize {
    1
}

/// One evaluation case in a conversation corpus.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub id: String,
    pub category: String,
    pub prompt: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub budgets: CaseBudgets,
    pub scorers: Vec<ScorerSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// `office` or `coding`. Live harness installs CodingHarness when coding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_profile: Option<String>,
    /// Relative files materialized into the isolated workspace before the turn.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub workspace_files: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
}

fn default_enabled() -> bool {
    true
}

/// Top-level corpus manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub corpus_version: String,
    pub suite: String,
    pub cases: Vec<Case>,
}

/// Scripted / live turn outcome consumed by scorers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TurnTranscript {
    pub assistant_text: String,
    #[serde(default)]
    pub tool_names: Vec<String>,
    pub turns: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub tool_error_count: u32,
    /// Isolated workspace used by the live harness. Never serialized into evidence.
    #[serde(skip)]
    pub workspace: Option<PathBuf>,
    /// Tool/text events captured during the live turn. Never serialized into JSONL.
    #[serde(skip)]
    pub trajectory: Vec<EvalTrajectoryEvent>,
    /// Workspace files after the turn. Never serialized into JSONL.
    #[serde(skip)]
    pub artifacts: Vec<EvalArtifactMeta>,
}

/// One redacted event on an eval trajectory (text / thinking / tool).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct EvalTrajectoryEvent {
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

/// Workspace artifact metadata (no absolute paths, no binaries).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvalArtifactMeta {
    pub path: String,
    pub size_bytes: u64,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

/// Full per-case trajectory persisted beside JSONL evidence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct EvalCaseTrace {
    pub case_id: String,
    #[serde(default)]
    pub live: bool,
    #[serde(default)]
    pub assistant_text: String,
    #[serde(default)]
    pub events: Vec<EvalTrajectoryEvent>,
    #[serde(default)]
    pub artifacts: Vec<EvalArtifactMeta>,
}

/// Result of applying one [`ScorerSpec`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScorerResult {
    pub scorer_type: String,
    pub passed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Sanitized per-case evidence line (JSONL). Never stores raw secrets from prompts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalResult {
    pub schema_version: u32,
    pub scoring_version: String,
    pub run_id: String,
    pub corpus_version: String,
    pub suite: String,
    pub case_id: String,
    pub category: String,
    /// Prompt with `sk-…` patterns redacted.
    pub prompt: String,
    pub success: bool,
    pub scorer_results: Vec<ScorerResult>,
    pub elapsed_ms: u128,
    pub turns: u32,
    pub tool_call_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub tool_error_count: u32,
    #[serde(default)]
    pub trajectory_event_count: u32,
    #[serde(default)]
    pub artifact_count: u32,
}

/// Aggregated summary over one or more JSONL evidence files.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Summary {
    pub schema_version: u32,
    pub scoring_version: String,
    pub total_cases: usize,
    pub passed: usize,
    pub failed: usize,
    pub success_rate: f64,
    pub by_category: Vec<CategorySummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corpus_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suite: Option<String>,
    #[serde(default)]
    pub avg_turns: f64,
    #[serde(default)]
    pub avg_elapsed_ms: f64,
    #[serde(default)]
    pub avg_input_tokens: f64,
    #[serde(default)]
    pub avg_output_tokens: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CategorySummary {
    pub category: String,
    pub total: usize,
    pub passed: usize,
    pub success_rate: f64,
}

/// Per-case progress event emitted while a run is in flight.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunProgress {
    pub run_id: String,
    pub case_id: String,
    pub category: String,
    /// 1-based index of the case now starting or just finished.
    pub index: usize,
    pub total: usize,
    pub phase: RunProgressPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunProgressPhase {
    Started,
    Scored,
    Cancelled,
}
