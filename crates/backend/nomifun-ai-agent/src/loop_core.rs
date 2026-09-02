//! Shared agent-loop core: the budget constants, the `LoopEventSink` seam and
//! the parameterized tool loop that `learning_graph_loop`, `course_outline_loop`
//! and `lesson_content_loop` all run on.
//!
//! Extracted from `learning_graph_loop.rs` so each domain loop keeps only its
//! prompts, tool set and draft/publish context. The fail-closed whitelist
//! execution (an unknown tool name gets an error result and never reaches any
//! other surface) is the security contract of this module — keep it intact.

use std::sync::Arc;
use std::time::Duration;

use nomi_providers::{LlmProvider, ProviderError};
use nomi_types::llm::{LlmEvent, LlmRequest, ThinkingConfig};
use nomi_types::message::{ContentBlock, Message, Role, StopReason};
use nomi_types::tool::ToolDef;
use nomifun_common::AppError;
use tokio::sync::mpsc::Receiver;

use crate::one_shot::OneShotTool;

/// Upper bound on model rounds inside the generation loop.
pub(crate) const GENERATE_MAX_ROUNDS: usize = 50;
/// Upper bound on model rounds inside ONE repair loop.
pub(crate) const REPAIR_MAX_ROUNDS: usize = 20;
/// How many complete repair loops run before the gate verdict is final.
pub(crate) const REPAIR_LOOP_LIMIT: usize = 3;
/// Wall-clock budget for the WHOLE pipeline (generation + repairs + audit
/// gates) — same order of magnitude as the legacy call timeout.
pub(crate) const TOTAL_TIMEOUT_SECS: u64 = 600;
/// Token budget per model round. Generous because a round often carries
/// planning prose AND a large patch JSON; 4096 was observed cut off
/// mid-arguments (EOF in a tool-call JSON → whole pipeline aborts). 8192
/// matches the scope-analysis budget. A loop that needs more headroom, or
/// wants thinking, declares its OWN budget locally (see the learning-graph
/// loop's `ROUND_TOKEN_BUDGET`) instead of raising this shared constant.
pub(crate) const AGENT_MAX_TOKENS: u32 = 8192;

/// Total same-round retries per loop for a corrupted tool-call arguments
/// JSON (see [`is_round_retryable_stream_error`]). A long multi-round
/// generation dies as a whole when ANY round hits it — one retry squares
/// that per-round probability; the cap bounds the cost when a provider is
/// systematically broken.
const ROUND_RETRY_LIMIT: usize = 3;

/// Whether a mid-stream `LlmEvent::Error` may be answered by re-sending the
/// identical round request. Malformed tool-call arguments JSON (a model
/// syntax slip, or a gateway dropping a delta chunk while still finishing
/// cleanly) is safe to replay: the fail-closed argument parser guarantees
/// the failed round produced NO executable tool call, so the conversation
/// state is unchanged and the request goes out verbatim. Transport-level
/// stream failures stay fatal — replaying those is deliberately outside the
/// contract (`ProviderError::StreamTruncated`).
fn is_round_retryable_stream_error(message: &str) -> bool {
    message.contains("malformed JSON arguments")
}

/// One open-phase retry for transient provider faults — the loop sibling of
/// the legacy single-call pipeline's one retry (`image_analyze` applies the
/// same policy to `BadGateway`). A dropped connect (`error sending request`)
/// or a 429 at the very last round would otherwise kill a multi-minute
/// generation. Only the stream OPEN is retried here: an open that already
/// produced events is never replayed and transport-level stream failures
/// stay fatal (replay safety — see `ProviderError::StreamTruncated`'s
/// contract). The one mid-stream exception is a corrupted tool-call
/// arguments JSON — see [`is_round_retryable_stream_error`].
const STREAM_OPEN_RETRY_DELAY_MS: u64 = 2_000;
const STREAM_OPEN_RETRY_MAX_DELAY_MS: u64 = 30_000;

/// Backoff for one retryable stream-open failure; `None` marks the error as
/// terminal at this boundary.
fn stream_open_retry_delay(error: &ProviderError) -> Option<Duration> {
    // `is_retryable` covers 429 / connect / 5xx-API / truncation; plain
    // send-phase transport failures (`Http`, e.g. "error sending request")
    // are equally transient at the open boundary but absent from that set.
    if !error.is_retryable() && !matches!(error, ProviderError::Http(_)) {
        return None;
    }
    let delay_ms = match error {
        ProviderError::RateLimited { retry_after_ms, .. } => {
            (*retry_after_ms).clamp(STREAM_OPEN_RETRY_DELAY_MS, STREAM_OPEN_RETRY_MAX_DELAY_MS)
        }
        _ => STREAM_OPEN_RETRY_DELAY_MS,
    };
    Some(Duration::from_millis(delay_ms))
}

/// [`LlmProvider::stream`] with one retry for transient open failures.
async fn open_stream_with_retry(
    provider: &dyn LlmProvider,
    request: &LlmRequest,
    sink: Option<&dyn LoopEventSink>,
) -> Result<Receiver<LlmEvent>, AppError> {
    let mut retried = false;
    loop {
        match provider.stream(request).await {
            Ok(rx) => return Ok(rx),
            Err(error) => {
                let delay = if retried { None } else { stream_open_retry_delay(&error) };
                let Some(delay) = delay else {
                    return Err(AppError::BadGateway(format!("LLM provider error: {error}")));
                };
                retried = true;
                if let Some(sink) = sink {
                    sink.log("stream_open_retry", serde_json::json!({
                        "error": error.to_string(),
                        "retry_in_ms": delay.as_millis() as u64,
                    }));
                }
                tokio::time::sleep(delay).await;
            }
        }
    }
}

/// Where loop events go. The learning-graph loop appends them to the shared
/// JSONL log file; richer hosts may also mirror them onto the WebSocket so
/// the UI can render the generation live. `Send + Sync` so the loop future
/// stays `Send` (the engines run behind `async_trait`, which demands it).
pub(crate) trait LoopEventSink: Send + Sync {
    fn log(&self, event: &str, fields: serde_json::Value);
}

/// Compact JSON without \uXXXX escapes (the default serializer already
/// keeps non-ASCII; this is the single formatting seam for tool replies).
pub(crate) fn json_compact<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|error| format!("序列化失败: {error}"))
}

/// Truncate model text for log events (the full text stays in the message
/// window; logs only need a fingerprint of what the model said).
pub(crate) fn log_text(text: &str) -> String {
    const CAP: usize = 200;
    let chars = text.chars();
    if chars.clone().count() <= CAP {
        return text.to_owned();
    }
    let truncated: String = chars.take(CAP).collect();
    format!("{truncated}…")
}

/// Human-readable stop reason for log events (distinguishes a model that
/// finished naturally from one that hit the token cap mid-generation).
pub(crate) fn stop_reason_name(reason: Option<StopReason>) -> String {
    match reason {
        Some(StopReason::EndTurn) => "end_turn",
        Some(StopReason::ToolUse) => "tool_use",
        Some(StopReason::MaxTokens) => "max_tokens",
        Some(StopReason::MaxTurns) => "max_turns",
        Some(StopReason::Refusal) => "refusal",
        None => "none",
    }
    .to_owned()
}

/// Run one isolated agent loop: model tool calls execute strictly against
/// the given whitelist, results feed back, until the model stops without
/// pending calls or `max_rounds` is exhausted. This is
/// `one_shot::tool_loop` with the round cap, token budget and window
/// parameterized — the one-shot entry's 8-round cap is too tight for a
/// 30-round generation. `loop_label` names the loop and `sink` (when given)
/// streams one `agent_round` event per round into the host's event channel
/// so a failed run stays diagnosable offline. Transient provider faults at
/// the stream open get one retry (see [`open_stream_with_retry`]).
pub(crate) async fn run_agent_loop(
    provider: Arc<dyn LlmProvider>,
    model: &str,
    system: &str,
    user_text: &str,
    tools: &[OneShotTool],
    max_rounds: usize,
    max_tokens: u32,
    thinking: ThinkingConfig,
    round_retry: bool,
    loop_label: &str,
    sink: Option<&dyn LoopEventSink>,
) -> Result<String, AppError> {
    // The tool defs sent to the model and the handler table derive from the
    // SAME whitelist; there is no other tool source in this code path.
    let tool_defs: Vec<ToolDef> = tools
        .iter()
        .map(|tool| ToolDef {
            name: tool.name.clone(),
            description: tool.description.clone(),
            input_schema: tool.input_schema.clone(),
            deferred: false,
        })
        .collect();

    let mut messages = vec![Message::new(
        Role::User,
        vec![ContentBlock::Text { text: user_text.to_owned() }],
    )];

    // Manual round counter (not a `for` loop): a corrupted-arguments retry
    // re-runs the SAME round, so the round index must not advance on retry.
    let mut round = 0usize;
    let mut round_retries: usize = 0;
    let mut retried_round: Option<usize> = None;
    while round < max_rounds {
        let request = LlmRequest {
            model: model.to_owned(),
            system: system.to_owned(),
            messages: messages.clone(),
            tools: tool_defs.clone(),
            max_tokens: Some(max_tokens),
            // DeepSeek-style gateways default to chain-of-thought and only
            // emit `content` after thinking is disabled; without this field
            // the whole token budget is silently consumed by reasoning and
            // rounds end with an empty `max_tokens` cut-off (mirrors
            // `one_shot_completion` / `llm_chat.rs`). The value is per-loop
            // policy, cloned per round.
            thinking: Some(thinking.clone()),
            reasoning_effort: None,
            temperature: None,
            retain_provider_round: false,
        };
        let mut rx = open_stream_with_retry(provider.as_ref(), &request, sink).await?;

        let mut text = String::new();
        let mut tool_uses: Vec<(String, String, serde_json::Value, Option<serde_json::Value>)> =
            Vec::new();
        let mut stop_reason: Option<StopReason> = None;
        let mut done = false;
        let mut retry_round = false;
        while let Some(event) = rx.recv().await {
            match event {
                LlmEvent::TextDelta(delta) => text.push_str(&delta),
                LlmEvent::ToolUse { id, name, input, extra } => {
                    tool_uses.push((id, name, input, extra));
                }
                LlmEvent::Done { stop_reason: reason, .. } => {
                    // Record WHY the model stopped (end_turn vs max_tokens
                    // matters for diagnostics) and fall through to the
                    // shared paths below — a non-tool stop with no pending
                    // calls ends the loop there, keeping the per-round
                    // logging on one code path.
                    done = true;
                    stop_reason = Some(reason);
                    break;
                }
                LlmEvent::Error(message) => {
                    // One same-round retry for a corrupted tool-call
                    // arguments JSON: the failed round executed nothing
                    // (fail-closed parse), so the identical request is
                    // re-sent and the round index stays put. Gated by
                    // `round_retry` — loops that did not opt in keep the
                    // exact fail-fast behavior.
                    if round_retry
                        && round_retries < ROUND_RETRY_LIMIT
                        && retried_round != Some(round)
                        && is_round_retryable_stream_error(&message)
                    {
                        round_retries += 1;
                        retried_round = Some(round);
                        if let Some(sink) = sink {
                            sink.log("round_retry", serde_json::json!({
                                "loop": loop_label,
                                "round": round + 1,
                                "retries_used": round_retries,
                                "error": message,
                            }));
                        }
                        retry_round = true;
                        break;
                    }
                    if let Some(sink) = sink {
                        sink.log("agent_loop_end", serde_json::json!({
                            "loop": loop_label,
                            "rounds": round + 1,
                            "exit": "stream_error",
                            "error": message,
                        }));
                    }
                    return Err(AppError::BadGateway(format!("LLM stream error: {message}")));
                }
                _ => {}
            }
        }
        if retry_round {
            continue;
        }
        if tool_uses.is_empty() {
            // No tool calls this round: log what the model said and why the
            // loop ended (the repair loop's idle replies were invisible
            // before — this is how we see a model refusing to work).
            if let Some(sink) = sink {
                sink.log("agent_round", serde_json::json!({
                    "loop": loop_label,
                    "round": round + 1,
                    "text": log_text(&text),
                    "stop_reason": stop_reason_name(stop_reason),
                    "tool_calls": Vec::<serde_json::Value>::new(),
                }));
                sink.log("agent_loop_end", serde_json::json!({
                    "loop": loop_label,
                    "rounds": round + 1,
                    "exit": "end_turn",
                    "stop_reason": stop_reason_name(stop_reason),
                    "text": log_text(&text),
                }));
            }
            if !done && text.is_empty() {
                return Err(AppError::BadGateway(
                    "LLM stream ended without producing a response".into(),
                ));
            }
            return Ok(text);
        }

        // Assistant message replaying the model's tool calls (and any text).
        let mut assistant_blocks: Vec<ContentBlock> = Vec::new();
        if !text.is_empty() {
            assistant_blocks.push(ContentBlock::Text { text: text.clone() });
        }
        for (id, name, input, extra) in &tool_uses {
            assistant_blocks.push(ContentBlock::ToolUse {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
                extra: extra.clone(),
            });
        }
        messages.push(Message::new(Role::Assistant, assistant_blocks));

        // Execute strictly against the whitelist: an unknown name gets an
        // error result and NEVER reaches any other execution surface.
        let mut result_blocks: Vec<ContentBlock> = Vec::new();
        let mut outcomes: Vec<(String, bool)> = Vec::new();
        for (id, name, input, _extra) in tool_uses {
            let outcome = match tools.iter().find(|tool| tool.name == name) {
                Some(tool) => (tool.handler)(input).await,
                None => Err(format!("tool '{name}' is not available in this session")),
            };
            let (content, is_error) = match outcome {
                Ok(content) => (content, false),
                Err(message) => (message, true),
            };
            outcomes.push((name.clone(), is_error));
            result_blocks.push(ContentBlock::ToolResult {
                tool_use_id: id,
                content,
                is_error,
                images: Vec::new(),
            });
        }
        if let Some(sink) = sink {
            let calls: Vec<serde_json::Value> = outcomes
                .iter()
                .map(|(name, is_error)| serde_json::json!({ "name": name, "is_error": is_error }))
                .collect();
            sink.log("agent_round", serde_json::json!({
                "loop": loop_label,
                "round": round + 1,
                "text": log_text(&text),
                "stop_reason": stop_reason_name(stop_reason),
                "tool_calls": calls,
            }));
        }
        messages.push(Message::new(Role::User, result_blocks));
        round += 1;
    }

    if let Some(sink) = sink {
        sink.log("agent_loop_end", serde_json::json!({
            "loop": loop_label,
            "rounds": max_rounds,
            "exit": "max_rounds",
        }));
    }
    Err(AppError::BadGateway(format!(
        "agent loop exceeded {max_rounds} tool rounds without a final answer"
    )))
}
