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
pub mod harness;
pub mod runner;
pub mod scorer;
pub mod types;

pub use corpus::{load_manifest, validate_manifest, CorpusError};
pub use harness::{ConversationEvalHarness, HarnessError, OfflineDemoHarness};
pub use runner::{
    append_evidence, completed_case_ids, run, run_demo, sanitize_prompt, summarize, RunConfig,
    RunReport, RunnerError,
};
pub use scorer::{score_all, score_one};
pub use types::{
    Case, CaseBudgets, CategorySummary, EvalResult, Manifest, ScorerResult, ScorerSpec, Summary,
    TurnTranscript, SCORING_VERSION, SCHEMA_VERSION,
};

/// On-disk agent-trace schema version (for live harness / evidence alignment).
pub const TRACE_SCHEMA_VERSION: u32 = nomi_agent_trace::SCHEMA_VERSION;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn bundled_corpus_loads() {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("evaluation/corpus.conversation.json");
        let manifest = load_manifest(&path).expect("corpus should load");
        assert_eq!(manifest.suite, "session_dialogue");
        assert_eq!(manifest.cases.len(), 5);
        assert!(manifest.cases.iter().all(|c| c.enabled));
    }
}
