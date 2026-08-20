//! Deterministic conversation-agent evaluation harness.
//!
//! Inspired by `flowy-web` fetch-eval patterns: feature-gated example binary,
//! resumable JSONL evidence, and sanitized prompts (no raw `sk-…` secrets).
//!
//! The library always builds. Enable the `agent-eval` feature to compile the
//! `agent_eval` example CLI:
//!
//! ```bash
//! cargo run -p nomi-agent-eval --example agent_eval --features agent-eval -- demo --output /tmp/demo.jsonl
//! ```

pub mod corpus;
pub mod datasets;
pub mod harness;
pub mod runner;
pub mod scorer;
pub mod types;
pub mod workspace;

pub use corpus::{load_bundled_manifest, load_manifest, validate_manifest, CorpusError};
pub use datasets::{
    cache_dir, is_download_cached, list_suites, load_suite_manifest, suite_descriptor, DatasetError,
    SuiteDescriptor, SUITE_AGENT_WORKFLOWS, SUITE_AIDER_POLYGLOT, SUITE_CLASSEVAL,
    SUITE_HARNESS_CONTROL, SUITE_OFFICE_TASKS, SUITE_SESSION_DIALOGUE,
};
pub use harness::{ConversationEvalHarness, HarnessError, OfflineDemoHarness};
pub use runner::{
    append_evidence, completed_case_ids, run, run_demo, run_loaded_manifest, run_with_progress,
    sanitize_prompt, summarize, RunConfig, RunReport, RunnerError,
};
pub use scorer::{score_all, score_one};
pub use types::{
    Case, CaseBudgets, CategorySummary, EvalArtifactMeta, EvalCaseTrace, EvalResult,
    EvalTrajectoryEvent, Manifest, RunProgress, RunProgressPhase, ScorerResult, ScorerSpec,
    Summary, TurnTranscript, SCORING_VERSION, SCHEMA_VERSION,
};
pub use workspace::{collect_workspace_artifacts, materialize_files, safe_join};

/// On-disk agent-trace schema version (for live harness / evidence alignment).
pub const TRACE_SCHEMA_VERSION: u32 = nomi_agent_trace::SCHEMA_VERSION;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn bundled_offline_demo_corpus_loads() {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("evaluation/corpus.conversation.json");
        let manifest = load_manifest(&path).expect("corpus should load");
        assert_eq!(manifest.suite, "session_dialogue");
        assert_eq!(manifest.cases.len(), 5);
        assert!(manifest.cases.iter().all(|c| c.enabled));
    }

    #[test]
    fn bundled_harness_control_loads() {
        let manifest = load_bundled_manifest("harness_control").expect("harness_control");
        assert_eq!(manifest.suite, "harness_control");
        assert!(manifest.cases.iter().any(|c| c.id == "write-marker"));
        assert!(manifest
            .cases
            .iter()
            .any(|c| c.task_profile.as_deref() == Some("coding")));
    }

    #[test]
    fn bundled_office_tasks_load() {
        let manifest = load_bundled_manifest("office_tasks").expect("office_tasks");
        assert_eq!(manifest.suite, "office_tasks");
        assert!(manifest
            .cases
            .iter()
            .all(|c| c.task_profile.as_deref() == Some("office")));
        assert!(manifest.cases.iter().any(|c| c.id == "memo-write"));
    }

    #[test]
    fn bundled_agent_workflows_load() {
        let manifest = load_bundled_manifest("agent_workflows").expect("agent_workflows");
        assert_eq!(manifest.suite, "agent_workflows");
        assert_eq!(manifest.cases.len(), 5);
        assert!(manifest.cases.iter().any(|c| c.id == "fix-failing-tests"));
        assert!(manifest.cases.iter().any(|c| c.id == "synthesize-briefing"));
    }
}
