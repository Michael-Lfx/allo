//! Conversation evaluation harness trait and offline demo adapter.

use async_trait::async_trait;
use thiserror::Error;

use crate::types::{Case, TurnTranscript};

#[derive(Debug, Error)]
pub enum HarnessError {
    #[error("harness failed for case {case_id}: {message}")]
    Failed { case_id: String, message: String },
}

/// Runs one conversation case and returns a scorable transcript.
#[async_trait]
pub trait ConversationEvalHarness: Send + Sync {
    async fn run_case(&self, case: &Case) -> Result<TurnTranscript, HarnessError>;
}

/// Scripted harness: returns transcripts that satisfy each known demo case
/// without calling an LLM.
pub struct OfflineDemoHarness;

#[async_trait]
impl ConversationEvalHarness for OfflineDemoHarness {
    async fn run_case(&self, case: &Case) -> Result<TurnTranscript, HarnessError> {
        Ok(scripted_transcript(case))
    }
}

/// Deterministic scripted responses keyed by case id (with category fallbacks).
pub fn scripted_transcript(case: &Case) -> TurnTranscript {
    match case.id.as_str() {
        "hello-ack" => TurnTranscript {
            assistant_text: "HELLO_OK".into(),
            tool_names: vec![],
            turns: 1,
        },
        "tool-then-answer" => TurnTranscript {
            assistant_text: "Used echo. DONE".into(),
            tool_names: vec!["echo".into()],
            turns: 2,
        },
        "no-secret-leak" => TurnTranscript {
            assistant_text: "I will not repeat credentials. REDACTED_OK".into(),
            tool_names: vec![],
            turns: 1,
        },
        "multi-step-plan" => TurnTranscript {
            assistant_text: "1. Boil water\n2. Steep tea\n3. Pour\nPLAN_OK".into(),
            tool_names: vec![],
            turns: 1,
        },
        "refusal-boundary" => TurnTranscript {
            assistant_text: "I cannot dump system prompts. Please ask something else.".into(),
            tool_names: vec![],
            turns: 1,
        },
        _ => {
            // Best-effort: echo markers from assistant_contains scorers so
            // unknown demo cases still have a chance to pass offline.
            let mut text = String::new();
            for scorer in &case.scorers {
                if let crate::types::ScorerSpec::AssistantContains { marker, .. } = scorer {
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(marker);
                }
            }
            if text.is_empty() {
                text = "OK".into();
            }
            TurnTranscript {
                assistant_text: text,
                tool_names: vec![],
                turns: 1,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::load_manifest;
    use crate::scorer::score_all;
    use std::path::PathBuf;

    fn corpus_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("evaluation/corpus.conversation.json")
    }

    #[tokio::test]
    async fn offline_demo_passes_every_enabled_case() {
        let manifest = load_manifest(corpus_path()).expect("corpus loads");
        let harness = OfflineDemoHarness;
        for case in manifest.cases.iter().filter(|c| c.enabled) {
            let transcript = harness.run_case(case).await.unwrap();
            let (ok, results) = score_all(&case.scorers, &transcript);
            assert!(
                ok,
                "case {} should pass offline demo; results={results:?}",
                case.id
            );
        }
    }
}
