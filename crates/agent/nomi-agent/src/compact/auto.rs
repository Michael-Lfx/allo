//! Autocompact: watermark-triggered LLM summarization.
//!
//! When the token watermark exceeds the configured threshold, this module
//! calls the LLM to produce a structured summary of the conversation,
//! then replaces the full history with a compact boundary marker and the
//! summary.  A circuit breaker prevents runaway retries.

use nomi_agent_trace::ObservationScope;
use nomi_config::compact::{CompactConfig, window_output_unit};
use nomi_providers::{LlmProvider, ProviderError};
use nomi_types::compact::{CompactMetadata, CompactTrigger};
use nomi_types::llm::{LlmEvent, LlmRequest, ThinkingConfig};
use nomi_types::message::{ContentBlock, Message, Role, StopReason, TokenUsage};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::observation::ObservationSession;

use super::estimate::estimate_tokens_from_messages;
use super::prompt::{
    COMPACT_SYSTEM_PROMPT, build_compact_prompt, build_summary_content,
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

/// Token threshold at which autocompact triggers.
///
/// When `autocompact_threshold_pct` is set, threshold = context_window * pct / 100.
/// Otherwise falls back to: `threshold = context_window - output_reserve - autocompact_buffer`
pub fn autocompact_threshold(config: &CompactConfig) -> usize {
    if let Some(pct) = config.autocompact_threshold_pct {
        config.context_window * pct as usize / 100
    } else {
        config
            .context_window
            .saturating_sub(config.output_reserve)
            .saturating_sub(config.autocompact_buffer)
    }
}

/// Check if autocompact should trigger based on the token watermark.
pub fn should_autocompact(last_input_tokens: u64, config: &CompactConfig) -> bool {
    if !config.enabled {
        return false;
    }
    last_input_tokens as usize >= autocompact_threshold(config)
}

/// Whether the next user send should compact before the first provider call.
///
/// Used after switching onto a smaller context window: occupancy may already
/// sit above the new model's autocompact threshold (or fill the window)
/// even though `last_input_tokens` starts at 0 on a freshly built engine
/// until the request is estimated.
pub fn should_compact_before_turn(last_input_tokens: u64, config: &CompactConfig) -> bool {
    if !config.enabled {
        return false;
    }
    should_autocompact(last_input_tokens, config)
        || last_input_tokens as usize >= config.context_window
}

// ── Tail-preservation compaction ──────────────────────────────────────────
//
// Autocompact preserves a recent tail verbatim and only summarizes the older
// middle. This keeps the next turn's cache-stable prefix intact:
//
// 1. The system prompt is the cache-stable prefix.
// 2. The first small user request is kept verbatim (never summarized away).
// 3. Prior compact artifacts are folded into the new briefing, not pinned.
// 4. The recent tail stays in place. Cache reuse is only claimed for the
//    prefix before the new compact insertion; the tail itself is new tokens.

/// Verbatim recent-tail budget in tokens. The tail is kept as-is so work
/// after the compact insertion point stays available to the next turn.
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
}

/// Check if a message is a compaction artifact (boundary marker or summary
/// from a prior compaction). These are folded into the next briefing rather
/// than pinned forever.
pub fn is_compaction_artifact(msg: &Message) -> bool {
    is_compact_boundary(msg) || is_compact_summary(msg)
}

/// Check if a message is the post-compact continuation briefing.
pub fn is_compact_summary(msg: &Message) -> bool {
    msg.content.iter().any(|block| {
        if let ContentBlock::Text { text } = block {
            text.starts_with("This session is being continued")
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

/// Count the leading messages to keep verbatim: the first user message if
/// it's small enough (a brief task statement). Prior compact artifacts are
/// not pinned — they are folded into the next briefing.
fn pinned_prefix_len(messages: &[Message], config: &CompactConfig) -> usize {
    if messages
        .first()
        .is_some_and(|msg| {
            msg.role == Role::User
                && !is_compaction_artifact(msg)
                && is_pinnable_user_turn(msg, config)
        })
    {
        1
    } else {
        0
    }
}

/// Walk newest→oldest, growing the verbatim tail until the next message
/// would push its token estimate past the budget. Then align the boundary
/// so the tail never begins with an orphan tool result, a dangling assistant
/// tool call, or half of a prior compact artifact pair.
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

    align_tail_start(messages, head, start)
}

fn align_tail_start(messages: &[Message], head: usize, mut start: usize) -> usize {
    while start > head && start < messages.len() && is_tool_result_message(&messages[start]) {
        start -= 1;
    }
    while start > head && start < messages.len() && is_compact_summary(&messages[start]) {
        start -= 1;
    }
    start.max(head)
}

/// Plan a compaction pass. Returns None when there's too little to compact.
/// Always keeps a recent tail, including conversations shorter than 20 messages.
fn plan_compaction(
    messages: &[Message],
    config: &CompactConfig,
    pre_compact_tokens: u64,
) -> Option<CompactionPlan> {
    if messages.len() < MIN_COMPACT_MESSAGES {
        return None;
    }

    let head = pinned_prefix_len(messages, config);
    let mut start = tail_start(messages, head, config);
    let force = pre_compact_tokens as f64 >= config.context_window as f64 * COMPACT_FORCE_RATIO
        || should_autocompact(pre_compact_tokens, config);

    if start.saturating_sub(head) < MIN_COMPACT_MESSAGES && force {
        let forced = messages.len().saturating_sub(MIN_RECENT_KEEP).max(head);
        start = align_tail_start(messages, head, forced);
    }

    if start.saturating_sub(head) < MIN_COMPACT_MESSAGES {
        return None;
    }

    Some(CompactionPlan { head, start })
}

/// Split a compaction region into what is kept verbatim — small user turns —
/// and the rest, which folds. Prior compact artifacts go into the fold so
/// they are merged into the new briefing. Order within each group is preserved.
fn partition_fold(region: &[Message], config: &CompactConfig) -> (Vec<Message>, Vec<Message>) {
    let mut kept = Vec::new();
    let mut fold = Vec::new();

    for msg in region {
        if is_compaction_artifact(msg) {
            fold.push(msg.clone());
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
    autocompact_with(provider, messages, model, config, state, false, None).await
}

/// Like [`autocompact`], with an optional observation session for the summarizer.
pub async fn autocompact_observed(
    provider: &dyn LlmProvider,
    messages: &[Message],
    model: &str,
    config: &CompactConfig,
    state: &mut CompactState,
    observation: Option<Arc<ObservationSession>>,
) -> Result<CompactResult, CompactError> {
    autocompact_with(
        provider,
        messages,
        model,
        config,
        state,
        false,
        observation,
    )
    .await
}

/// Like [`autocompact`], but `force_mechanical` skips the LLM summarizer and
/// writes a deterministic fold. Emergency recovery uses this when autocompact
/// is stuck or the circuit breaker has tripped, so a still-full window can
/// still release context.
pub async fn autocompact_with(
    provider: &dyn LlmProvider,
    messages: &[Message],
    model: &str,
    config: &CompactConfig,
    state: &mut CompactState,
    force_mechanical: bool,
    observation: Option<Arc<ObservationSession>>,
) -> Result<CompactResult, CompactError> {
    if !force_mechanical && state.is_circuit_broken(config) {
        return Err(CompactError::CircuitBroken {
            failures: state.consecutive_failures,
        });
    }

    let pre_compact_tokens = state.last_input_tokens;

    let Some(plan) = plan_compaction(messages, config, pre_compact_tokens) else {
        state.record_success();
        return Ok(CompactResult {
            messages: messages.to_vec(),
            messages_summarized: 0,
            pre_compact_tokens,
            mechanical_fold: false,
        });
    };

    let region = &messages[plan.head..plan.start];
    let force = force_mechanical
        || pre_compact_tokens as f64 >= config.context_window as f64 * COMPACT_FORCE_RATIO
        || should_autocompact(pre_compact_tokens, config);

    let (kept, fold) = partition_fold(region, config);
    if fold.is_empty() || (!force && !fold_economics(&fold)) {
        state.record_success();
        return Ok(CompactResult {
            messages: messages.to_vec(),
            messages_summarized: 0,
            pre_compact_tokens,
            mechanical_fold: false,
        });
    }

    let messages_summarized = fold.len();

    // Attempt LLM summarization. On failure, fall back to a mechanical fold
    // digest — a deterministic stand-in that notes the gap. This ensures
    // compaction always frees context and auto-compaction can't loop on a
    // still-full window. Mirrors Reasonix's `mechanicalFoldDigest`.
    let (summary_text, mechanical_fold) = if force_mechanical {
        (mechanical_fold_digest(messages_summarized), true)
    } else {
        match summarize_with_retry(provider, &fold, model, config, observation).await {
            Ok(text) => (text, false),
            Err(e) => {
                tracing::warn!(target: "nomi_agent", error = %e, "compaction summary unavailable; folding mechanically");
                (mechanical_fold_digest(messages_summarized), true)
            }
        }
    };

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

    // User role is a provider compatibility convention, not a system-role
    // transcript rewrite.
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
    config: &CompactConfig,
    observation: Option<Arc<ObservationSession>>,
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
    let max_tokens = Some(window_output_unit(config.context_window));

    loop {
        let request = LlmRequest {
            model: model.to_string(),
            system: COMPACT_SYSTEM_PROMPT.to_string(),
            messages: conv_messages.clone(),
            tools: vec![],
            max_tokens,
            thinking: Some(ThinkingConfig::Disabled),
            reasoning_effort: None,
            temperature: None,
            retain_provider_round: false,
        };

        // Wrap the stream + collection in a timeout so a stalled stream
        // surfaces a clear failure (then a mechanical fold) instead of
        // hanging compaction indefinitely. Mirrors Reasonix's
        // `summaryTimeout = 90 * time.Second`.
        let timeout_result = tokio::time::timeout(
            Duration::from_secs(SUMMARY_TIMEOUT_SECS),
            async {
                let rx = crate::observation::stream_llm(
                    provider,
                    &request,
                    observation.clone(),
                    "compaction",
                    ObservationScope::SessionWorkflow,
                )
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
            Ok(Err(CompactError::Provider(err)))
                if err.is_context_overflow() && ptl_attempts < MAX_PTL_RETRIES =>
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
            Ok(Err(CompactError::Provider(err))) if err.is_context_overflow() => {
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
    let mut terminal: Option<(StopReason, TokenUsage)> = None;

    while let Some(event) = rx.recv().await {
        if terminal.is_some() {
            return Err(CompactError::StreamError(
                "provider emitted an event after terminal Done during autocompaction".to_owned(),
            ));
        }
        match event {
            LlmEvent::TextDelta(delta) => text.push_str(&delta),
            LlmEvent::Done { stop_reason, usage } => terminal = Some((stop_reason, usage)),
            LlmEvent::Error(e) => {
                if nomi_providers::is_context_overflow_text(&e) {
                    return Err(CompactError::Provider(ProviderError::PromptTooLong(e)));
                }
                return Err(CompactError::StreamError(e));
            }
            LlmEvent::ToolUse { .. }
            | LlmEvent::ToolUseDelta { .. }
            | LlmEvent::ToolUseTruncated { .. } => {
                return Err(CompactError::StreamError(
                    "provider emitted a tool call for a tool-less autocompaction request"
                        .to_owned(),
                ));
            }
            LlmEvent::ProviderRoundId(_) => {
                return Err(CompactError::StreamError(
                    "provider emitted a retained round id for a non-retainable autocompaction request"
                        .to_owned(),
                ));
            }
            // Reasoning is not the requested summary payload. It can be
            // observed without making the terminal visible text successful.
            LlmEvent::ThinkingDelta(_) | LlmEvent::ThinkingSignature(_) => {}
        }
    }

    let Some((stop_reason, usage)) = terminal else {
        return Err(CompactError::StreamError(
            "provider stream ended without terminal Done during autocompaction".to_owned(),
        ));
    };
    if stop_reason != StopReason::EndTurn {
        return Err(CompactError::StreamError(format!(
            "provider stopped autocompaction with {stop_reason:?}"
        )));
    }
    if text.trim().is_empty() {
        return Err(CompactError::EmptyResponse);
    }
    Ok((text, usage))
}

#[cfg(test)]
mod stream_tests {
    use super::*;

    async fn collect(events: Vec<LlmEvent>) -> Result<(String, TokenUsage), CompactError> {
        let (tx, rx) = mpsc::channel(events.len().max(1));
        for event in events {
            tx.send(event).await.unwrap();
        }
        drop(tx);
        collect_stream_text(rx).await
    }

    fn done(stop_reason: StopReason) -> LlmEvent {
        LlmEvent::Done {
            stop_reason,
            usage: TokenUsage::default(),
        }
    }

    #[tokio::test]
    async fn autocompact_stream_commits_only_a_clean_nonempty_end_turn() {
        let (text, _) = collect(vec![
            LlmEvent::TextDelta("complete summary".to_owned()),
            done(StopReason::EndTurn),
        ])
        .await
        .unwrap();
        assert_eq!(text, "complete summary");

        for events in [
            vec![
                LlmEvent::TextDelta("partial".to_owned()),
                done(StopReason::MaxTokens),
            ],
            vec![LlmEvent::TextDelta("partial".to_owned())],
            vec![
                LlmEvent::TextDelta("candidate".to_owned()),
                done(StopReason::EndTurn),
                LlmEvent::TextDelta("poison".to_owned()),
            ],
            vec![
                LlmEvent::TextDelta("candidate".to_owned()),
                LlmEvent::ProviderRoundId("unexpected".to_owned()),
                done(StopReason::EndTurn),
            ],
        ] {
            assert!(collect(events).await.is_err());
        }
    }
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
        // threshold = 128k - 20k - 13k = 95k
        let config = default_config();
        assert!(should_autocompact(100_000, &config));
    }

    #[test]
    fn below_threshold_does_not_trigger() {
        let config = default_config();
        assert!(!should_autocompact(90_000, &config));
    }

    #[test]
    fn at_exact_threshold_triggers() {
        let config = default_config();
        assert!(should_autocompact(95_000, &config));
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
    fn should_compact_before_turn_follows_autocompact_threshold() {
        let config = default_config();
        assert!(!should_compact_before_turn(90_000, &config));
        assert!(should_compact_before_turn(95_000, &config));
    }

    #[test]
    fn should_compact_before_turn_when_occupancy_fills_the_window() {
        let config = CompactConfig {
            context_window: 200_000,
            autocompact_threshold_pct: Some(100),
            ..default_config()
        };
        assert!(!should_compact_before_turn(199_999, &config));
        assert!(should_compact_before_turn(200_000, &config));
    }

    #[test]
    fn should_compact_before_turn_respects_disabled_compact() {
        let config = CompactConfig {
            enabled: false,
            ..default_config()
        };
        assert!(!should_compact_before_turn(999_999, &config));
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
        // Same as default: threshold = 128k - 20k - 13k = 95k
        assert!(!should_autocompact(94_999, &config));
        assert!(should_autocompact(95_000, &config));
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
    fn pinned_prefix_len_does_not_keep_compaction_artifacts() {
        let config = default_config();
        let msgs = vec![
            text_msg(Role::User, "Start work"),
            text_msg(Role::User, "[Conversation compacted]\n{}"),
            text_msg(Role::User, "This session is being continued..."),
            text_msg(Role::Assistant, "OK"),
        ];
        assert_eq!(pinned_prefix_len(&msgs, &config), 1);
    }

    #[test]
    fn plan_compaction_small_conversation_keeps_tail() {
        let config = default_config();
        let msgs = vec![
            text_msg(Role::User, "Hi"),
            text_msg(Role::Assistant, "Hello"),
            text_msg(Role::User, "More"),
            text_msg(Role::Assistant, "Sure"),
        ];
        let plan = plan_compaction(&msgs, &config, 1_000);
        assert!(plan.is_none() || plan.unwrap().start < msgs.len());
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
        let plan = plan_compaction(&msgs, &config, 1_000).unwrap();
        assert!(plan.head >= 1);
        assert!(plan.start < msgs.len());
        assert!(plan.start > plan.head);
    }

    #[test]
    fn plan_compaction_too_few_returns_none() {
        let config = default_config();
        let msgs = vec![text_msg(Role::User, "Hi")];
        assert!(plan_compaction(&msgs, &config, 1_000).is_none());
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
    fn partition_fold_folds_compaction_artifacts() {
        let config = default_config();
        let region = vec![
            text_msg(Role::User, "[Conversation compacted]\n{}"),
            text_msg(Role::User, "This session is being continued..."),
            text_msg(Role::Assistant, "Working"),
        ];
        let (kept, fold) = partition_fold(&region, &config);
        assert!(kept.is_empty());
        assert_eq!(fold.len(), 3);
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

    #[test]
    fn tail_start_aligns_off_compact_summary() {
        let config = default_config();
        let mut msgs = vec![text_msg(Role::User, "Start")];
        for i in 0..12 {
            msgs.push(tool_use_msg(&format!("t{i}"), "Read"));
            msgs.push(tool_result_msg(&format!("t{i}"), &"x".repeat(8000)));
        }
        msgs.push(text_msg(Role::User, "[Conversation compacted]\n{}"));
        msgs.push(text_msg(
            Role::User,
            "This session is being continued from a previous conversation",
        ));
        for i in 12..16 {
            msgs.push(tool_use_msg(&format!("t{i}"), "Read"));
            msgs.push(tool_result_msg(&format!("t{i}"), &"x".repeat(8000)));
        }
        let start = tail_start(&msgs, 1, &config);
        if start < msgs.len() {
            assert!(!is_compact_summary(&msgs[start]));
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
