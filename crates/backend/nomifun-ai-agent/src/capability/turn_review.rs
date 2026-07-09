//! Per-turn lightweight memory review (optimization 2).
//!
//! A lighter, more timely counterpart to the session-end distillation pipeline.
//! After each human-origin turn the reviewer asynchronously evaluates whether
//! any memories should be extracted and persisted — using the same `DISTILL_SYSTEM`
//! prompt as the full distill path but with a smaller token ceiling (1024 vs 2048)
//! and a cap of at most 2 memories per turn.
//!
//! Discipline: this hook is always fire-and-forget. The caller (`run_post_turn_review`
//! or the agent manager) spawns it via `tokio::spawn`; any failure degrades silently
//! (debug/warn log) and never surfaces as a turn error.

use std::path::PathBuf;
use std::sync::Arc;

use nomi_config::config::Config;
use nomi_memory::distill::{
    DistillOutput, apply_distilled, build_distill_prompt, parse_distill_output, DISTILL_SYSTEM,
};
use nomi_redact::redact_secrets_owned;

use crate::capability::session_lifecycle::{PostTurnReviewHook, TurnContext};
use crate::factory::provider_config::{one_shot_completion, user_message};

/// Token ceiling for the lightweight per-turn review — half of the full distill
/// budget, keeping the incremental cost of each reviewed turn bounded.
const REVIEW_MAX_TOKENS: u32 = 1024;

/// Maximum memories to persist from a single per-turn review. The full session-end
/// distill has no hard cap (relies on dedup); the per-turn path is intentionally
/// more conservative to avoid flooding memory with low-value fragments.
const REVIEW_MAX_MEMORIES: usize = 2;

/// Lightweight per-turn reviewer: reuses the distill system prompt and redact
/// gates, but with a smaller token budget and memory cap.
///
/// Constructed in the agent factory and registered on the `NomiAgentManager`
/// (or `SessionLifecycleCoordinator`) for normal (non-companion) sessions.
pub struct LightweightTurnReviewer {
    cfg: Arc<Config>,
    memory_dir: PathBuf,
}

impl LightweightTurnReviewer {
    pub fn new(cfg: Arc<Config>, memory_dir: PathBuf) -> Self {
        Self { cfg, memory_dir }
    }
}

#[async_trait::async_trait]
impl PostTurnReviewHook for LightweightTurnReviewer {
    async fn on_post_turn_review(
        &self,
        _ctx: &TurnContext<'_>,
        reply: &str,
        messages: &[serde_json::Value],
    ) {
        // Build a compact transcript from the recent messages + the assistant reply.
        // We include at most the last 6 messages to keep the prompt small.
        let transcript = build_review_transcript(messages, reply);
        let transcript = redact_secrets_owned(transcript);
        if transcript.trim().is_empty() {
            return;
        }

        let prompt = build_distill_prompt(&transcript);

        // One parse retry (same policy as full distill).
        let mut parsed: Option<DistillOutput> = None;
        for _ in 0..2 {
            match one_shot_completion(
                &self.cfg,
                DISTILL_SYSTEM,
                vec![user_message(&prompt)],
                REVIEW_MAX_TOKENS,
            )
            .await
            {
                Ok(raw) => match parse_distill_output(&raw) {
                    Ok(out) => {
                        parsed = Some(out);
                        break;
                    }
                    Err(e) => tracing::debug!(error = %e, "turn_review: output unparseable"),
                },
                Err(e) => {
                    tracing::debug!(error = %e, "turn_review: provider call failed");
                    break;
                }
            }
        }

        let Some(mut out) = parsed else {
            return;
        };

        // Cap at REVIEW_MAX_MEMORIES — truncate, don't discard the whole batch.
        if out.memories.len() > REVIEW_MAX_MEMORIES {
            out.memories.truncate(REVIEW_MAX_MEMORIES);
        }
        if out.memories.is_empty() {
            return;
        }

        // Gate 2: redact every distilled field before it touches disk (same as distill).
        for m in &mut out.memories {
            m.content = redact_secrets_owned(std::mem::take(&mut m.content));
            m.description = redact_secrets_owned(std::mem::take(&mut m.description));
        }

        let dir = self.memory_dir.clone();
        let _ = tokio::task::spawn_blocking(move || match apply_distilled(&dir, &out) {
            Ok(n) if n > 0 => {
                tracing::info!(written = n, "turn_review: lightweight review persisted memories")
            }
            Ok(_) => {} // all candidates deduped / filtered
            Err(e) => tracing::warn!(error = %e, "turn_review: apply failed"),
        })
        .await;
    }
}

/// Build a compact role-tagged transcript from the most recent messages plus the
/// assistant reply. Mirrors the format expected by `build_distill_prompt` but
/// only includes the tail of the conversation to keep the LLM input small.
fn build_review_transcript(messages: &[serde_json::Value], reply: &str) -> String {
    let take = messages.len().min(6);
    let tail = &messages[messages.len().saturating_sub(take)..];
    let mut buf = String::new();
    for msg in tail {
        let role = msg
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let content = msg
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !content.is_empty() {
            buf.push_str(role);
            buf.push_str(": ");
            buf.push_str(content);
            buf.push('\n');
        }
    }
    if !reply.is_empty() {
        buf.push_str("assistant: ");
        buf.push_str(reply);
        buf.push('\n');
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_review_transcript_handles_empty() {
        let transcript = build_review_transcript(&[], "");
        assert!(transcript.is_empty());
    }

    #[test]
    fn build_review_transcript_includes_reply() {
        let messages = vec![
            serde_json::json!({"role": "user", "content": "hello"}),
            serde_json::json!({"role": "assistant", "content": "hi there"}),
        ];
        let transcript = build_review_transcript(&messages, "final reply");
        assert!(transcript.contains("user: hello"));
        assert!(transcript.contains("assistant: hi there"));
        assert!(transcript.contains("assistant: final reply"));
    }

    #[test]
    fn build_review_transcript_caps_at_six() {
        let messages: Vec<serde_json::Value> = (0..10)
            .map(|i| serde_json::json!({"role": "user", "content": format!("msg {i}")}))
            .collect();
        let transcript = build_review_transcript(&messages, "");
        // Should only include the last 6 messages (indices 4-9)
        assert!(!transcript.contains("msg 3"));
        assert!(transcript.contains("msg 4"));
        assert!(transcript.contains("msg 9"));
    }

    #[test]
    fn review_max_tokens_is_lighter_than_distill() {
        // Per-turn review uses half the token budget of full session distill
        assert!(REVIEW_MAX_TOKENS < 2048);
        assert_eq!(REVIEW_MAX_TOKENS, 1024);
    }

    #[test]
    fn review_max_memories_is_capped() {
        assert_eq!(REVIEW_MAX_MEMORIES, 2);
    }
}
