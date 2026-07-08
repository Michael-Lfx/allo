//! Autocompact: watermark-triggered LLM summarization.
//!
//! When the token watermark exceeds the configured threshold, this module
//! calls the LLM to produce a structured summary of the conversation,
//! then replaces the full history with a compact boundary marker and the
//! summary.  A circuit breaker prevents runaway retries.

use nomi_config::compact::CompactConfig;
use nomi_providers::{LlmProvider, ProviderError};
use nomi_types::compact::{CompactMetadata, CompactTrigger};
use nomi_types::llm::{LlmEvent, LlmRequest, ThinkingConfig};
use nomi_types::message::{ContentBlock, Message, Role, TokenUsage};
use std::time::Duration;
use tokio::sync::mpsc;

use super::estimate::estimate_tokens_from_messages;
use super::prompt::{
    COMPACT_MAX_OUTPUT_TOKENS, COMPACT_SYSTEM_PROMPT, build_compact_prompt, build_summary_content,
    format_compact_summary,
};
use super::state::CompactState;

/// Maximum number of prompt-too-long retries.
const MAX_PTL_RETRIES: u32 = 2;

/// Content prefix for the compact boundary marker message.
pub const BOUNDARY_PREFIX: &str = "[Conversation compacted]";

// ── Public types ────────────────────────────────────────────────────────────

/// Result of a successful autocompact operation.
#[derive(Debug, Clone)]
pub struct CompactResult {
    /// Post-compact messages that replace the original conversation.
    /// Contains a boundary marker and a summary message.
    pub messages: Vec<Message>,
    /// How many original messages were summarized.
    pub messages_summarized: usize,
    /// Input token count before compaction (from the last API call).
    pub pre_compact_tokens: u64,
    /// True when the LLM summarizer was unreachable and a deterministic
    /// mechanical fold digest was used instead. The engine can surface a
    /// warning so the user knows the summary is a placeholder.
    pub mechanical_fold: bool,
}

/// Errors specific to autocompact.
#[derive(Debug, thiserror::Error)]
pub enum CompactError {
    #[error("LLM provider error: {0}")]
    Provider(#[from] ProviderError),
    #[error("Prompt too long after {attempts} retries")]
    PromptTooLong { attempts: u32 },
    #[error("Empty response from LLM")]
    EmptyResponse,
    #[error("Stream error: {0}")]
    StreamError(String),
    #[error("Circuit breaker tripped after {failures} consecutive failures")]
    CircuitBroken { failures: u32 },
}

// ── Trigger check ───────────────────────────────────────────────────────────

/// Check if autocompact should trigger based on the token watermark.
///
/// When `autocompact_threshold_pct` is set, threshold = context_window * pct / 100.
/// Otherwise falls back to: `threshold = context_window - output_reserve - autocompact_buffer`
pub fn should_autocompact(last_input_tokens: u64, config: &CompactConfig) -> bool {
    if !config.enabled {
        return false;
    }
    let threshold = if let Some(pct) = config.autocompact_threshold_pct {
        config.context_window * pct as usize / 100
    } else {
        let effective_window = config.context_window.saturating_sub(config.output_reserve);
        effective_window.saturating_sub(config.autocompact_buffer)
    };
    last_input_tokens as usize >= threshold
}

// ── Tail-preservation compaction (Reasonix-style) ──────────────────────────
//
// When the conversation is large enough (>= MIN_MESSAGES_FOR_TAIL_PRESERVATION),
// autocompact preserves a recent tail verbatim and only summarizes the older
// middle. This mirrors DeepSeek-Reasonix's compaction strategy where:
//
// 1. The system prompt is the cache-stable prefix (P0 change).
// 2. Small user turns are kept verbatim (never summarized away).
// 3. Only assistant/tool work is folded into a summary.
// 4. The recent tail stays in place so the prefix cache can stay warm.
//
// For small conversations (< MIN_MESSAGES_FOR_TAIL_PRESERVATION), the legacy
// behavior is used: summarize everything into a single boundary + summary pair.

/// Minimum message count to activate tail-preserving compaction.
/// Below this, the legacy "summarize everything" behavior is used.
const MIN_MESSAGES_FOR_TAIL_PRESERVATION: usize = 20;

/// Verbatim recent-tail budget in tokens. The tail is kept as-is so the
/// prefix cache can stay warm after compaction.
const TAIL_TOKEN_BUDGET: usize = 16384;

/// Never keep fewer recent messages than this in the tail.
const MIN_RECENT_KEEP: usize = 4;

/// Skip compaction below this many compactable messages.
const MIN_COMPACT_MESSAGES: usize = 2;

/// Ceiling on pinning the first user turn verbatim.
const MAX_PINNED_FIRST_USER_TOKENS: usize = 1500;

/// Never pin a first turn worth more than this fraction of the window.
const PINNED_FIRST_USER_WINDOW_FRAC: f64 = 0.15;

/// Minimum foldable tokens to justify the summarization API call.
const MIN_FOLD_TOKENS: u64 = 400;

/// Summary call timeout in seconds. A stalled stream surfaces a clear failure
/// (then a mechanical fold) instead of hanging compaction indefinitely.
/// Mirrors Reasonix's `summaryTimeout = 90 * time.Second`.
const SUMMARY_TIMEOUT_SECS: u64 = 90;

/// Force compaction at this high-water mark even when the foldable region is
/// small. Without this, fold_economics could skip compaction at 90% full,
/// leaving the agent to hit the context limit on the next turn. Mirrors
/// Reasonix's `defaultCompactForceRatio = 0.9`.
const COMPACT_FORCE_RATIO: f64 = 0.9;

/// Plan for a compaction pass.
struct CompactionPlan {
    /// Number of leading messages preserved verbatim (pinned prefix).
    head: usize,
    /// Index where the preserved recent tail begins.
    /// Messages[head..start] is the region to compact.
    start: usize,
    /// Whether to preserve the recent tail (new behavior) or not (legacy).
    preserve_tail: bool,
}

/// Check if a message is a compaction artifact (boundary marker or summary
/// from a prior compaction). These are kept verbatim and never re-summarized.
fn is_compaction_artifact(msg: &Message) -> bool {
    msg.content.iter().any(|block| {
        if let ContentBlock::Text { text } = block {
            text.starts_with(BOUNDARY_PREFIX)
                || text.starts_with("This session is being continued")
        } else {
            false
        }
    })
}

/// Check if a user message is a tool result (contains ToolResult blocks).
fn is_tool_result_message(msg: &Message) -> bool {
    msg.role == Role::User
        && msg
            .content
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolResult { .. }))
}

/// Check if a user turn is small enough to keep verbatim during compaction.
/// Only pins text-only user messages (not tool results).
fn is_pinnable_user_turn(msg: &Message, config: &CompactConfig) -> bool {
    if msg.role != Role::User {
        return false;
    }
    let has_text = msg
        .content
        .iter()
        .any(|b| matches!(b, ContentBlock::Text { .. }));
    let has_tool_result = msg
        .content
        .iter()
        .any(|b| matches!(b, ContentBlock::ToolResult { .. }));
    if !has_text || has_tool_result {
        return false;
    }

    let budget = MAX_PINNED_FIRST_USER_TOKENS
        .min((config.context_window as f64 * PINNED_FIRST_USER_WINDOW_FRAC) as usize);
    let estimate = estimate_tokens_from_messages(std::slice::from_ref(msg)) as usize;
    estimate <= budget
}

/// Count the leading messages to keep verbatim:
/// - The first user message if it's small enough (a "brief" task statement)
/// - Any prior compaction artifacts (boundary + summary pairs)
fn pinned_prefix_len(messages: &[Message], config: &CompactConfig) -> usize {
    let mut i = 0;

    // Pin the first user message if it's a real user turn (not a compaction
    // artifact) and small enough to be a "brief".
    if i < messages.len()
        && messages[i].role == Role::User
        && !is_compaction_artifact(&messages[i])
        && is_pinnable_user_turn(&messages[i], config)
    {
        i += 1;
    }

    // Keep all subsequent compaction artifacts (boundary + summary pairs
    // from prior compactions) so they accumulate rather than being re-summarized.
    while i < messages.len() && is_compaction_artifact(&messages[i]) {
        i += 1;
    }

    i
}

/// Walk newest→oldest, growing the verbatim tail until the next message
/// would push its token estimate past the budget. Then align the boundary
/// off tool results so the tail never begins with an orphan whose assistant
/// tool_calls were summarized away.
fn tail_start(messages: &[Message], head: usize, config: &CompactConfig) -> usize {
    let budget = TAIL_TOKEN_BUDGET.min((config.context_window as f64 * 0.5) as usize);

    let mut start = messages.len();
    let mut acc = 0u64;

    for i in (head..messages.len()).rev() {
        let msg_tokens = estimate_tokens_from_messages(&messages[i..i + 1]);
        if messages.len() - i > MIN_RECENT_KEEP && acc + msg_tokens > budget as u64 {
            break;
        }
        acc += msg_tokens;
        start = i;
    }

    // Align off tool results: don't start the tail with a tool result
    // whose assistant tool_call was summarized away.
    while start > head && start < messages.len() && is_tool_result_message(&messages[start]) {
        start -= 1;
    }

    start.max(head)
}

/// Plan a compaction pass. Returns None when there's too little to compact.
fn plan_compaction(messages: &[Message], config: &CompactConfig) -> Option<CompactionPlan> {
    // For small conversations, use the legacy behavior: summarize everything.
    // The tail-preservation strategy only benefits larger conversations where
    // the recent tail can maintain cache continuity after compaction.
    if messages.len() < MIN_MESSAGES_FOR_TAIL_PRESERVATION {
        if messages.len() >= MIN_COMPACT_MESSAGES {
            return Some(CompactionPlan {
                head: 0,
                start: messages.len(),
                preserve_tail: false,
            });
        }
        return None;
    }

    // Large conversation — try tail-preserving compaction.
    let head = pinned_prefix_len(messages, config);
    let start = tail_start(messages, head, config);

    if start.saturating_sub(head) < MIN_COMPACT_MESSAGES {
        // Tail preservation didn't leave enough to compact (e.g. all messages
        // fit within the tail token budget). Fall back to legacy behavior:
        // summarize everything into a single boundary + summary pair.
        if messages.len() >= MIN_COMPACT_MESSAGES {
            return Some(CompactionPlan {
                head: 0,
                start: messages.len(),
                preserve_tail: false,
            });
        }
        return None;
    }

    Some(CompactionPlan {
        head,
        start,
        preserve_tail: true,
    })
}

/// Split a compaction region into what is kept verbatim — small user turns
/// (a fact the user stated is never summarized away) and prior compaction
/// summaries — and the rest, which folds. Order within each group is preserved.
fn partition_fold(region: &[Message], config: &CompactConfig) -> (Vec<Message>, Vec<Message>) {
    let mut kept = Vec::new();
    let mut fold = Vec::new();

    for msg in region {
        if is_compaction_artifact(msg) {
            kept.push(msg.clone());
        } else if msg.role == Role::User && is_pinnable_user_turn(msg, config) {
            kept.push(msg.clone());
        } else {
            fold.push(msg.clone());
        }
    }

    (kept, fold)
}

/// Estimate whether compacting the given region saves enough tokens to
/// justify the summarization API call.
fn fold_economics(fold: &[Message]) -> bool {
    estimate_tokens_from_messages(fold) >= MIN_FOLD_TOKENS
}

// ── Core autocompact ────────────────────────────────────────────────────────

/// Execute autocompact: call LLM to summarize the conversation.
///
/// 1. Build a summary prompt and send conversation + prompt to the LLM.
/// 2. If the prompt is too long, truncate oldest 20% messages and retry
///    (up to [`MAX_PTL_RETRIES`] times).
/// 3. Parse the `<summary>` from the response.
/// 4. Return a [`CompactResult`] with boundary marker + summary messages.
///
/// On failure, increments `state.consecutive_failures`.
/// On success, resets the failure counter.
pub async fn autocompact(
    provider: &dyn LlmProvider,
    messages: &[Message],
    model: &str,
    config: &CompactConfig,
    state: &mut CompactState,
) -> Result<CompactResult, CompactError> {
    // Circuit breaker check
    if state.is_circuit_broken(config) {
        return Err(CompactError::CircuitBroken {
            failures: state.consecutive_failures,
        });
    }

    let pre_compact_tokens = state.last_input_tokens;

    // Plan compaction: determine head (pinned prefix), start (tail start),
    // and whether to preserve the recent tail.
    // Mirrors DeepSeek-Reasonix's planCompaction: the system prompt is the
    // cache-stable prefix, the recent tail is kept verbatim, and only the
    // older middle is summarized.
    let Some(plan) = plan_compaction(messages, config) else {
        // Not enough to compact — no-op
        state.record_success();
        return Ok(CompactResult {
            messages: messages.to_vec(),
            messages_summarized: 0,
            pre_compact_tokens,
            mechanical_fold: false,
        });
    };

    // Partition the compaction region into kept (user msgs, prior summaries)
    // and fold (assistant/tool work to be summarized).
    let region = &messages[plan.head..plan.start];
    // Force compaction at 90% of the window even when the foldable region is
    // small — without this, fold_economics could skip compaction and the next
    // turn might hit the context limit. Mirrors Reasonix's `compactForceRatio`.
    let force = pre_compact_tokens as f64
        >= config.context_window as f64 * COMPACT_FORCE_RATIO;

    let (kept, fold) = if plan.preserve_tail {
        let (k, f) = partition_fold(region, config);
        // Economic check: skip if foldable region is too small to justify
        // the summarization API call. Bypassed when force-compacting.
        if f.is_empty() || (!force && !fold_economics(&f)) {
            state.record_success();
            return Ok(CompactResult {
                messages: messages.to_vec(),
                messages_summarized: 0,
                pre_compact_tokens,
                mechanical_fold: false,
            });
        }
        (k, f)
    } else {
        // Legacy behavior for small conversations: fold everything
        (Vec::new(), region.to_vec())
    };

    let messages_summarized = fold.len();

    // Attempt LLM summarization. On failure, fall back to a mechanical fold
    // digest — a deterministic stand-in that notes the gap. This ensures
    // compaction always frees context and auto-compaction can't loop on a
    // still-full window. Mirrors Reasonix's `mechanicalFoldDigest`.
    let (summary_text, mechanical_fold) = match summarize_with_retry(
        provider,
        &fold,
        model,
    )
    .await
    {
        Ok(text) => (text, false),
        Err(e) => {
            tracing::warn!(target: "nomi_agent", error = %e, "compaction summary unavailable; folding mechanically");
            (mechanical_fold_digest(messages_summarized), true)
        }
    };

    // Format and build post-compact messages
    let formatted = format_compact_summary(&summary_text);
    let summary_content = build_summary_content(&formatted, true);

    let metadata = CompactMetadata {
        trigger: CompactTrigger::Auto,
        pre_compact_tokens,
        messages_summarized,
    };

    let boundary_text = format!(
        "{BOUNDARY_PREFIX}\n{}",
        serde_json::to_string(&metadata).expect("CompactMetadata serialization cannot fail")
    );

    let boundary_msg = Message::new(
        Role::User,
        vec![ContentBlock::Text {
            text: boundary_text,
        }],
    );

    let summary_msg = Message::new(
        Role::User,
        vec![ContentBlock::Text {
            text: summary_content,
        }],
    );

    state.record_success();

    // Assemble post-compact messages.
    if plan.preserve_tail {
        // [pinned prefix] + [kept user msgs] + [boundary + summary] + [recent tail]
        // The recent tail stays in place so the DeepSeek prefix cache can
        // remain warm after compaction — only the older middle is replaced.
        let mut result =
            Vec::with_capacity(plan.head + kept.len() + 2 + (messages.len() - plan.start));
        result.extend_from_slice(&messages[..plan.head]);
        result.extend(kept);
        result.push(boundary_msg);
        result.push(summary_msg);
        result.extend_from_slice(&messages[plan.start..]);
        Ok(CompactResult {
            messages: result,
            messages_summarized,
            pre_compact_tokens,
            mechanical_fold,
        })
    } else {
        // Legacy: just boundary + summary
        Ok(CompactResult {
            messages: vec![boundary_msg, summary_msg],
            messages_summarized,
            pre_compact_tokens,
            mechanical_fold,
        })
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Attempt LLM summarization of the foldable region with PTL retry.
///
/// Builds the compact prompt, sends the foldable region to the LLM, and
/// retries up to [`MAX_PTL_RETRIES`] times on PromptTooLong by truncating
/// the oldest messages. Returns the summary text on success.
///
/// On any failure (exhausted retries, provider error, empty response),
/// returns an error so the caller can fall back to a mechanical fold.
async fn summarize_with_retry(
    provider: &dyn LlmProvider,
    fold: &[Message],
    model: &str,
) -> Result<String, CompactError> {
    let prompt = build_compact_prompt();
    let mut conv_messages = fold.to_vec();
    // Ensure the conversation starts with a User message for API compatibility
    if conv_messages.first().map(|m| m.role) == Some(Role::Assistant) {
        conv_messages.insert(
            0,
            Message::new(
                Role::User,
                vec![ContentBlock::Text {
                    text: "[earlier conversation work being summarized]".to_string(),
                }],
            ),
        );
    }
    conv_messages.push(Message::new(
        Role::User,
        vec![ContentBlock::Text { text: prompt }],
    ));

    let mut ptl_attempts = 0u32;
    let mut retried_transient = false;

    loop {
        let request = LlmRequest {
            model: model.to_string(),
            system: COMPACT_SYSTEM_PROMPT.to_string(),
            messages: conv_messages.clone(),
            tools: vec![],
            max_tokens: COMPACT_MAX_OUTPUT_TOKENS,
            thinking: Some(ThinkingConfig::Disabled),
            reasoning_effort: None,
        };

        // Wrap the stream + collection in a timeout so a stalled stream
        // surfaces a clear failure (then a mechanical fold) instead of
        // hanging compaction indefinitely. Mirrors Reasonix's
        // `summaryTimeout = 90 * time.Second`.
        let timeout_result = tokio::time::timeout(
            Duration::from_secs(SUMMARY_TIMEOUT_SECS),
            async {
                let rx = provider
                    .stream(&request)
                    .await
                    .map_err(CompactError::Provider)?;
                collect_stream_text(rx).await
            },
        )
        .await;

        match timeout_result {
            // Timed out — don't retry, return for mechanical fold.
            Err(_) => {
                return Err(CompactError::StreamError(format!(
                    "summarize timed out after {SUMMARY_TIMEOUT_SECS}s"
                )));
            }
            Ok(Ok((text, _usage))) => {
                if text.trim().is_empty() {
                    return Err(CompactError::EmptyResponse);
                }
                return Ok(text);
            }
            Ok(Err(CompactError::Provider(ProviderError::PromptTooLong(_))))
                if ptl_attempts < MAX_PTL_RETRIES =>
            {
                ptl_attempts += 1;
                // Remove the summary prompt (last msg), truncate, re-add prompt
                let conversation_part = &conv_messages[..conv_messages.len() - 1];
                match truncate_for_retry(conversation_part) {
                    Some(mut truncated) => {
                        truncated.push(Message::new(
                            Role::User,
                            vec![ContentBlock::Text {
                                text: build_compact_prompt(),
                            }],
                        ));
                        conv_messages = truncated;
                    }
                    None => {
                        return Err(CompactError::PromptTooLong {
                            attempts: ptl_attempts,
                        });
                    }
                }
            }
            Ok(Err(CompactError::Provider(ProviderError::PromptTooLong(_)))) => {
                return Err(CompactError::PromptTooLong {
                    attempts: ptl_attempts,
                });
            }
            Ok(Err(e)) => {
                // Non-PTL error: retry once on transient failures (network
                // blips, rate limits) before falling to mechanical fold.
                // Mirrors Reasonix's `summarizeWithRetry` which retries one
                // non-timeout failure.
                if !retried_transient {
                    retried_transient = true;
                    continue;
                }
                return Err(e);
            }
        }
    }
}

/// Deterministic stand-in used when the summarizer is unreachable.
///
/// The foldable region is dropped to free context, so the digest just notes
/// the gap and points the model at the user for anything it needs from before
/// it. Mirrors Reasonix's `mechanicalFoldDigest`.
fn mechanical_fold_digest(n: usize) -> String {
    format!(
        "{n} earlier message(s) were folded here to free context, but the automatic summary was unavailable. \
         Ask the user if you need details from before this point."
    )
}

/// Collect all text from a streaming LLM response.
async fn collect_stream_text(
    mut rx: mpsc::Receiver<LlmEvent>,
) -> Result<(String, TokenUsage), CompactError> {
    let mut text = String::new();

    while let Some(event) = rx.recv().await {
        match event {
            LlmEvent::TextDelta(delta) => text.push_str(&delta),
            LlmEvent::Done { usage, .. } => return Ok((text, usage)),
            LlmEvent::Error(e) => return Err(CompactError::StreamError(e)),
            // Ignore thinking deltas and tool calls (shouldn't happen in compact)
            _ => {}
        }
    }

    // Channel closed without a Done event
    Err(CompactError::EmptyResponse)
}

/// Truncate the oldest ~20% of messages for PTL retry.
///
/// Returns `None` if there are too few messages to truncate meaningfully.
fn truncate_for_retry(messages: &[Message]) -> Option<Vec<Message>> {
    if messages.len() < 2 {
        return None;
    }

    let drop_count = (messages.len() / 5).max(1);
    if drop_count >= messages.len() {
        return None;
    }

    let remaining = &messages[drop_count..];
    let mut result = Vec::with_capacity(remaining.len() + 1);

    // Ensure the first message is User role for API compatibility
    if remaining.first().map(|m| m.role) != Some(Role::User) {
        result.push(Message::new(
            Role::User,
            vec![ContentBlock::Text {
                text: "[earlier conversation truncated for compaction retry]".to_string(),
            }],
        ));
    }

    result.extend_from_slice(remaining);
    Some(result)
}

/// Check if a message is a compact boundary marker.
pub fn is_compact_boundary(message: &Message) -> bool {
    message.content.iter().any(|block| {
        if let ContentBlock::Text { text } = block {
            text.starts_with(BOUNDARY_PREFIX)
        } else {
            false
        }
    })
}

/// Extract [`CompactMetadata`] from a boundary marker message.
pub fn extract_compact_metadata(message: &Message) -> Option<CompactMetadata> {
    for block in &message.content {
        if let ContentBlock::Text { text } = block
            && let Some(json_str) = text.strip_prefix(BOUNDARY_PREFIX)
        {
            let json_str = json_str.trim_start_matches('\n');
            return serde_json::from_str(json_str).ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomi_types::compact::CompactTrigger;

    fn default_config() -> CompactConfig {
        CompactConfig::default()
    }

    // ── should_autocompact (TC-2.4-01..03, TC-2.4-14) ──────────────────

    #[test]
    fn above_threshold_triggers() {
        // threshold = 200k - 20k - 13k = 167k
        let config = default_config();
        assert!(should_autocompact(170_000, &config));
    }

    #[test]
    fn below_threshold_does_not_trigger() {
        let config = default_config();
        assert!(!should_autocompact(160_000, &config));
    }

    #[test]
    fn at_exact_threshold_triggers() {
        let config = default_config();
        assert!(should_autocompact(167_000, &config));
    }

    #[test]
    fn disabled_config_never_triggers() {
        let config = CompactConfig {
            enabled: false,
            ..default_config()
        };
        assert!(!should_autocompact(999_999, &config));
    }

    #[test]
    fn custom_config_threshold() {
        let config = CompactConfig {
            context_window: 100_000,
            output_reserve: 10_000,
            autocompact_buffer: 5_000,
            ..default_config()
        };
        // threshold = 100k - 10k - 5k = 85k
        assert!(!should_autocompact(80_000, &config));
        assert!(should_autocompact(85_000, &config));
        assert!(should_autocompact(90_000, &config));
    }

    #[test]
    fn zero_tokens_does_not_trigger() {
        let config = default_config();
        assert!(!should_autocompact(0, &config));
    }

    #[test]
    fn threshold_pct_overrides_default_calculation() {
        let config = CompactConfig {
            context_window: 200_000,
            autocompact_threshold_pct: Some(50),
            ..default_config()
        };
        // threshold = 200k * 50 / 100 = 100k
        assert!(!should_autocompact(99_999, &config));
        assert!(should_autocompact(100_000, &config));
        assert!(should_autocompact(150_000, &config));
    }

    #[test]
    fn threshold_pct_zero_triggers_immediately() {
        let config = CompactConfig {
            autocompact_threshold_pct: Some(0),
            ..default_config()
        };
        // threshold = 0, any non-negative triggers
        assert!(should_autocompact(0, &config));
        assert!(should_autocompact(1, &config));
    }

    #[test]
    fn threshold_pct_100_never_triggers() {
        let config = CompactConfig {
            context_window: 200_000,
            autocompact_threshold_pct: Some(100),
            ..default_config()
        };
        // threshold = 200k, provider never reports 200k input_tokens
        assert!(!should_autocompact(199_999, &config));
        assert!(should_autocompact(200_000, &config));
    }

    #[test]
    fn threshold_pct_none_uses_default_logic() {
        let config = CompactConfig {
            autocompact_threshold_pct: None,
            ..default_config()
        };
        // Same as default: threshold = 200k - 20k - 13k = 167k
        assert!(!should_autocompact(166_999, &config));
        assert!(should_autocompact(167_000, &config));
    }

    // ── truncate_for_retry ──────────────────────────────────────────────

    #[test]
    fn truncate_drops_20_percent() {
        let msgs: Vec<Message> = (0..10)
            .map(|i| {
                let role = if i % 2 == 0 {
                    Role::User
                } else {
                    Role::Assistant
                };
                Message::new(
                    role,
                    vec![ContentBlock::Text {
                        text: format!("msg-{i}"),
                    }],
                )
            })
            .collect();

        let result = truncate_for_retry(&msgs).unwrap();
        // Drop 20% of 10 = 2 messages, remaining 8
        assert_eq!(result.len(), 8);
    }

    #[test]
    fn truncate_ensures_user_first() {
        let msgs: Vec<Message> = (0..5)
            .map(|i| {
                Message::new(
                    Role::Assistant,
                    vec![ContentBlock::Text {
                        text: format!("msg-{i}"),
                    }],
                )
            })
            .collect();

        let result = truncate_for_retry(&msgs).unwrap();
        assert_eq!(result[0].role, Role::User);
    }

    #[test]
    fn truncate_too_few_returns_none() {
        let msgs = vec![Message::new(
            Role::User,
            vec![ContentBlock::Text {
                text: "only one".to_string(),
            }],
        )];
        assert!(truncate_for_retry(&msgs).is_none());
    }

    #[test]
    fn truncate_empty_returns_none() {
        assert!(truncate_for_retry(&[]).is_none());
    }

    #[test]
    fn truncate_preserves_user_first_without_placeholder() {
        // First remaining message is already User — no placeholder needed
        let msgs: Vec<Message> = (0..10)
            .map(|i| {
                let role = if i % 2 == 0 {
                    Role::User
                } else {
                    Role::Assistant
                };
                Message::new(
                    role,
                    vec![ContentBlock::Text {
                        text: format!("msg-{i}"),
                    }],
                )
            })
            .collect();

        let result = truncate_for_retry(&msgs).unwrap();
        // msgs[2] (User) should be first; no placeholder prepended
        assert_eq!(result.len(), 8);
        match &result[0].content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "msg-2"),
            _ => panic!("expected Text"),
        }
    }

    // ── boundary detection / extraction ─────────────────────────────────

    #[test]
    fn detect_boundary_message() {
        let metadata = CompactMetadata {
            trigger: CompactTrigger::Auto,
            pre_compact_tokens: 150_000,
            messages_summarized: 42,
        };
        let text = format!(
            "{BOUNDARY_PREFIX}\n{}",
            serde_json::to_string(&metadata).unwrap()
        );
        let msg = Message::new(Role::User, vec![ContentBlock::Text { text }]);
        assert!(is_compact_boundary(&msg));
    }

    #[test]
    fn non_boundary_message() {
        let msg = Message::new(
            Role::User,
            vec![ContentBlock::Text {
                text: "hello".to_string(),
            }],
        );
        assert!(!is_compact_boundary(&msg));
    }

    #[test]
    fn extract_metadata_from_boundary() {
        let metadata = CompactMetadata {
            trigger: CompactTrigger::Auto,
            pre_compact_tokens: 150_000,
            messages_summarized: 42,
        };
        let text = format!(
            "{BOUNDARY_PREFIX}\n{}",
            serde_json::to_string(&metadata).unwrap()
        );
        let msg = Message::new(Role::User, vec![ContentBlock::Text { text }]);
        let extracted = extract_compact_metadata(&msg).unwrap();
        assert_eq!(extracted, metadata);
    }

    #[test]
    fn extract_metadata_from_non_boundary_returns_none() {
        let msg = Message::new(
            Role::User,
            vec![ContentBlock::Text {
                text: "not a boundary".to_string(),
            }],
        );
        assert!(extract_compact_metadata(&msg).is_none());
    }

    // ── Tail-preservation compaction tests ─────────────────────────────

    use serde_json::json;

    fn text_msg(role: Role, text: &str) -> Message {
        Message::new(role, vec![ContentBlock::Text { text: text.to_string() }])
    }

    fn tool_use_msg(id: &str, name: &str) -> Message {
        Message::new(
            Role::Assistant,
            vec![ContentBlock::ToolUse {
                id: id.to_string(),
                name: name.to_string(),
                input: json!({}),
                extra: None,
            }],
        )
    }

    fn tool_result_msg(id: &str, content: &str) -> Message {
        Message::new(
            Role::User,
            vec![ContentBlock::ToolResult {
                tool_use_id: id.to_string(),
                content: content.to_string(),
                is_error: false,
                images: Vec::new(),
            }],
        )
    }

    #[test]
    fn is_compaction_artifact_detects_boundary() {
        let msg = text_msg(Role::User, "[Conversation compacted]\n{}");
        assert!(is_compaction_artifact(&msg));
    }

    #[test]
    fn is_compaction_artifact_detects_summary() {
        let msg = text_msg(Role::User, "This session is being continued from...");
        assert!(is_compaction_artifact(&msg));
    }

    #[test]
    fn is_compaction_artifact_ignores_normal_text() {
        let msg = text_msg(Role::User, "Hello world");
        assert!(!is_compaction_artifact(&msg));
    }

    #[test]
    fn is_tool_result_message_detects_tool_result() {
        let msg = tool_result_msg("t1", "output");
        assert!(is_tool_result_message(&msg));
    }

    #[test]
    fn is_tool_result_message_ignores_text() {
        let msg = text_msg(Role::User, "Hello");
        assert!(!is_tool_result_message(&msg));
    }

    #[test]
    fn is_pinnable_user_turn_small_text() {
        let config = default_config();
        let msg = text_msg(Role::User, "Fix the bug in auth.rs");
        assert!(is_pinnable_user_turn(&msg, &config));
    }

    #[test]
    fn is_pinnable_user_turn_rejects_tool_result() {
        let config = default_config();
        let msg = tool_result_msg("t1", "output");
        assert!(!is_pinnable_user_turn(&msg, &config));
    }

    #[test]
    fn is_pinnable_user_turn_rejects_large_text() {
        let config = default_config();
        // MAX_PINNED_FIRST_USER_TOKENS * 4 + 5 chars → just over the token budget
        let large_text = "x".repeat(MAX_PINNED_FIRST_USER_TOKENS * 4 + 5);
        let msg = text_msg(Role::User, &large_text);
        assert!(!is_pinnable_user_turn(&msg, &config));
    }

    #[test]
    fn pinned_prefix_len_pins_first_user() {
        let config = default_config();
        let msgs = vec![
            text_msg(Role::User, "Start work"),
            text_msg(Role::Assistant, "OK"),
        ];
        assert_eq!(pinned_prefix_len(&msgs, &config), 1);
    }

    #[test]
    fn pinned_prefix_len_keeps_compaction_artifacts() {
        let config = default_config();
        let msgs = vec![
            text_msg(Role::User, "[Conversation compacted]\n{}"),
            text_msg(Role::User, "This session is being continued..."),
            text_msg(Role::Assistant, "OK"),
        ];
        assert_eq!(pinned_prefix_len(&msgs, &config), 2);
    }

    #[test]
    fn plan_compaction_small_conversation_legacy() {
        let config = default_config();
        let msgs = vec![
            text_msg(Role::User, "Hi"),
            text_msg(Role::Assistant, "Hello"),
        ];
        let plan = plan_compaction(&msgs, &config).unwrap();
        assert!(!plan.preserve_tail);
        assert_eq!(plan.head, 0);
        assert_eq!(plan.start, 2);
    }

    #[test]
    fn plan_compaction_large_conversation_preserves_tail() {
        let config = default_config();
        let mut msgs = vec![text_msg(Role::User, "Start")];
        for i in 0..15 {
            msgs.push(tool_use_msg(&format!("t{i}"), "Read"));
            // 5000 chars ≈ 1250 tokens; 15 results = 18750 > 16384 budget
            msgs.push(tool_result_msg(&format!("t{i}"), &"x".repeat(5000)));
        }
        // 1 + 30 = 31 messages
        let plan = plan_compaction(&msgs, &config).unwrap();
        assert!(plan.preserve_tail);
        assert!(plan.head >= 1);
        assert!(plan.start < msgs.len());
        assert!(plan.start > plan.head);
    }

    #[test]
    fn plan_compaction_too_few_returns_none() {
        let config = default_config();
        let msgs = vec![text_msg(Role::User, "Hi")];
        assert!(plan_compaction(&msgs, &config).is_none());
    }

    #[test]
    fn partition_fold_separates_user_and_work() {
        let config = default_config();
        let region = vec![
            text_msg(Role::User, "What is the status?"),
            text_msg(Role::Assistant, "Let me check."),
            tool_use_msg("t1", "Read"),
            tool_result_msg("t1", "file content"),
        ];
        let (kept, fold) = partition_fold(&region, &config);
        // Small user message is kept
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].role, Role::User);
        // Assistant + tool use + tool result are folded
        assert_eq!(fold.len(), 3);
    }

    #[test]
    fn partition_fold_keeps_compaction_artifacts() {
        let config = default_config();
        let region = vec![
            text_msg(Role::User, "[Conversation compacted]\n{}"),
            text_msg(Role::User, "This session is being continued..."),
            text_msg(Role::Assistant, "Working"),
        ];
        let (kept, fold) = partition_fold(&region, &config);
        assert_eq!(kept.len(), 2); // both compaction artifacts
        assert_eq!(fold.len(), 1); // assistant message
    }

    #[test]
    fn fold_economics_rejects_small_fold() {
        let fold = vec![text_msg(Role::Assistant, "OK")];
        assert!(!fold_economics(&fold));
    }

    #[test]
    fn fold_economics_accepts_large_fold() {
        let fold = vec![text_msg(Role::Assistant, &"x".repeat(2000))];
        assert!(fold_economics(&fold));
    }

    #[test]
    fn tail_start_preserves_recent_messages() {
        let config = default_config();
        let mut msgs = vec![text_msg(Role::User, "Start")];
        for i in 0..15 {
            msgs.push(tool_use_msg(&format!("t{i}"), "Read"));
            // 5000 chars ≈ 1250 tokens; 15 results = 18750 > 16384 budget
            msgs.push(tool_result_msg(&format!("t{i}"), &"x".repeat(5000)));
        }
        let head = 1;
        let start = tail_start(&msgs, head, &config);
        // Tail should be somewhere in the middle, not at the very start
        assert!(start > head);
        assert!(start < msgs.len());
        // Tail should not start with a tool result
        assert!(!is_tool_result_message(&msgs[start]));
    }

    #[test]
    fn tail_start_aligns_off_tool_results() {
        let config = default_config();
        // Build messages where the tail boundary would land on a tool result
        let mut msgs = vec![text_msg(Role::User, "Start")];
        for i in 0..20 {
            msgs.push(tool_use_msg(&format!("t{i}"), "Read"));
            msgs.push(tool_result_msg(&format!("t{i}"), &"x".repeat(10000)));
        }
        let head = 1;
        let start = tail_start(&msgs, head, &config);
        // The tail must not start with a tool result
        if start < msgs.len() {
            assert!(!is_tool_result_message(&msgs[start]));
        }
    }

    // ── Mechanical fold digest tests ────────────────────────────────────

    #[test]
    fn mechanical_fold_digest_contains_count() {
        let digest = mechanical_fold_digest(42);
        assert!(digest.contains("42"));
        assert!(digest.contains("folded"));
        assert!(digest.contains("unavailable"));
    }

    #[test]
    fn mechanical_fold_digest_is_deterministic() {
        let a = mechanical_fold_digest(10);
        let b = mechanical_fold_digest(10);
        assert_eq!(a, b);
    }

    #[test]
    fn mechanical_fold_digest_different_counts() {
        let a = mechanical_fold_digest(5);
        let b = mechanical_fold_digest(20);
        assert_ne!(a, b);
    }
}
