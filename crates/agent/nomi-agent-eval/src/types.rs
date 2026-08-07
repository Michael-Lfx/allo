//! Corpus, scorer, and evidence types for conversation-agent evaluation.

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;
pub const SCORING_VERSION: &str = "session-dialogue-v1";

/// Per-case turn / token budgets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct CaseBudgets {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CategorySummary {
    pub category: String,
    pub total: usize,
    pub passed: usize,
    pub success_rate: f64,
}
