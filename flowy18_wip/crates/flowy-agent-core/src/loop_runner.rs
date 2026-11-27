//! One turn of the pi loop with host-owned messages.
//!
//! The loop is provider-agnostic: [`run_turn`] receives a [`StreamFn`] and
//! converts between the stored [`Message`] vocabulary and pi's agent messages.
//! Tool execution is wired by the embedding host (see `tools`).

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use rpi_agent::hooks::AgentLoopConfig;
use rpi_agent::{
    AgentContext, AgentEmitter, AgentMessage, AgentTool, BeforeToolCallResult,
    StreamFn, run_agent_loop,
};
use rpi_ai::types::{
    Api, AssistantMessage, Content, StopReason, ThinkingContent, ToolCall,
    ToolCallType, ToolResultMessage, ToolResultRole, UserContent, UserMessage,
};

use crate::session::PendingCard;
use crate::types::{CancelToken, ContentBlock, Message, MessageSource, Role};

/// Hard ceiling on assistant/tool-loop turns inside a single submission so a
/// stuck model cannot spin indefinitely.
pub const MAX_TOOL_TURNS_PER_SUBMISSION: usize = 24;

/// Maximum Unicode scalar chars retained per tool result before host context
/// compaction trims the tail.
const MAX_TOOL_RESULT_CHARS: usize = 32_000;

/// Conservative fallback character budget when the provider catalog does not
/// declare `context_window`.
const DEFAULT_CONTEXT_CHAR_BUDGET: usize = 240_000;

const CONTEXT_TRUNCATION_SUFFIX: &str =
    "\n...[truncated by host context guard]";

/// Provider route for one run. Credentials live in the stream function.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelRoute {
    pub provider: String,
    pub model: String,
    pub base_url: String,
    /// Catalog-declared context window; 0 means "provider default".
    pub context_window: u64,
    /// Catalog-declared visible output allowance; 0 means "provider default".
    pub max_tokens: u64,
}

impl ModelRoute {
    #[must_use]
    pub fn rpi_model(&self) -> rpi_ai::Model {
        let mut model = rpi_ai::Model::new(
            self.model.clone(),
            self.model.clone(),
            Api::OpenaiCompletions,
            self.provider.clone(),
            self.base_url.clone(),
        );
        model.context_window = self.context_window;
        model.max_tokens = self.max_tokens;
        model
    }
}

/// Decision returned by a host-owned [`PreToolHook`] inside `before_tool_call`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreToolDecision {
    Allow,
    Deny {
        reason: String,
    },
    Ask {
        target_summary: String,
        args_hash: String,
        risk_reason: String,
        command_preview: String,
    },
    /// Immediately halt the tool batch and persist a host-constructed card
    /// (e.g. Router `ask_user_choice` -> [`PendingCard::Disambiguation`]) into
    /// [`crate::session::TurnState::WaitingForUser`].
    SuspendWithCard {
        card: PendingCard,
        reason: String,
    },
}

/// Host pre-execution hook invoked before each tool call inside `rpi_agent`.
pub type PreToolHook =
    Arc<dyn Fn(&str, &serde_json::Value) -> PreToolDecision + Send + Sync>;

/// Deterministic canonical JSON representation of `(tool_name, args)`.
///
/// Recursively sorts object keys and strips cosmetic shell metadata
/// (`description`) so SHA-256 `args_hash` values are stable across retries.
#[must_use]
pub fn canonical_tool_args_json(
    tool_name: &str,
    args: &serde_json::Value,
) -> String {
    fn canonicalize(
        val: &serde_json::Value,
        strip_shell_desc: bool,
    ) -> serde_json::Value {
        match val {
            serde_json::Value::Object(map) => {
                let mut sorted = BTreeMap::new();
                for (k, v) in map {
                    if strip_shell_desc && k == "description" {
                        continue;
                    }
                    sorted.insert(k.clone(), canonicalize(v, false));
                }
                let mut out = serde_json::Map::new();
                for (k, v) in sorted {
                    out.insert(k, v);
                }
                serde_json::Value::Object(out)
            }
            serde_json::Value::Array(items) => serde_json::Value::Array(
                items.iter().map(|item| canonicalize(item, false)).collect(),
            ),
            serde_json::Value::Number(n) => {
                if n.is_f64()
                    && let Some(f) = n.as_f64()
                    && f.is_finite()
                    && f.fract() == 0.0
                    && let Ok(whole) = format!("{f:.0}").parse::<i64>()
                {
                    return serde_json::Value::Number(
                        serde_json::Number::from(whole),
                    );
                }
                serde_json::Value::Number(n.clone())
            }
            other => other.clone(),
        }
    }
    let strip_shell_desc = matches!(tool_name, "bash" | "powershell");
    let payload = serde_json::json!({
        "args": canonicalize(args, strip_shell_desc),
        "tool": tool_name,
    });
    serde_json::to_string(&payload).unwrap_or_default()
}

fn next_approval_card_id(now: u64) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!("approval-{now}-{}", NEXT.fetch_add(1, Ordering::Relaxed))
}

/// Inputs for one turn.
pub struct TurnRequest {
    pub session_id: Option<String>,
    pub system: String,
    pub history: Vec<Message>,
    pub prompt: String,
    pub route: ModelRoute,
    pub tools: Vec<Arc<dyn AgentTool>>,
    pub signal: CancelToken,
    pub pre_tool_hook: Option<PreToolHook>,
}

impl std::fmt::Debug for TurnRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TurnRequest")
            .field("history", &self.history.len())
            .field("prompt_len", &self.prompt.len())
            .field("route", &self.route)
            .field("tools", &self.tools.len())
            .field("has_pre_tool_hook", &self.pre_tool_hook.is_some())
            .finish_non_exhaustive()
    }
}

/// What the loop produced, in host vocabulary.
#[derive(Debug)]
pub struct TurnResult {
    pub messages: Vec<Message>,
    pub outcome: LoopOutcome,
    /// Last assistant error text when [`LoopOutcome::Failed`].
    pub error: Option<String>,
    /// Populated when [`LoopOutcome::WaitingForUser`] suspended before a
    /// medium-risk tool call requiring explicit confirmation.
    pub pending_approval: Option<PendingCard>,
}

/// Truncate text for diagnostic logs (UTF-8 safe by character count).
#[must_use]
pub fn log_preview(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let preview: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{preview}…")
    } else {
        preview
    }
}

/// Terminal state of one turn, derived from the last assistant message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopOutcome {
    Completed,
    Failed,
    Aborted,
    WaitingForUser,
}

/// Run one turn.
///
/// `Err` means the loop itself failed to start or returned an error; a provider
/// failure that settled as a terminal message yields
/// `Ok(TurnResult { outcome: Failed, .. })` so callers cannot mistake it for
/// success.
pub async fn run_turn(
    request: TurnRequest,
    stream_fn: StreamFn,
    emit: Arc<dyn AgentEmitter>,
) -> Result<TurnResult, String> {
    tracing::debug!(
        target: "flowy.prompt",
        stage = "run_turn",
        model = %request.route.model,
        base_url = %request.route.base_url,
        history = request.history.len(),
        tools = request.tools.len(),
        chars = request.prompt.chars().count(),
        "→ run_agent_loop"
    );
    let prompt_preview = log_preview(&request.prompt, 200);
    let prompts = vec![AgentMessage::User(UserMessage::new(
        UserContent::Text(request.prompt),
        now_ms(),
    ))];
    let mut history = Vec::new();
    for message in &request.history {
        if let Some(converted) = to_agent_message(message)? {
            history.push(converted);
        }
    }
    let history = repair_tool_call_history(history);
    let context = AgentContext {
        system_prompt: request.system,
        messages: history,
        tools: request.tools,
    };
    let pi_model = request.route.rpi_model();
    let char_budget = if request.route.context_window > 0 {
        usize::try_from(request.route.context_window)
            .unwrap_or(usize::MAX / 4)
            .saturating_mul(3)
            .max(16_000)
    } else {
        DEFAULT_CONTEXT_CHAR_BUDGET
    };
    let transform_context: rpi_agent::hooks::TransformContext =
        Arc::new(move |messages, _signal| {
            Box::pin(
                async move { compact_context_messages(messages, char_budget) },
            )
        });
    let suspended_card: Arc<Mutex<Option<PendingCard>>> =
        Arc::new(Mutex::new(None));
    let cancel_for_hook = request.signal.clone();
    let before_tool_call: Option<rpi_agent::hooks::BeforeToolCall> =
        request.pre_tool_hook.map(|hook| {
            let slot = Arc::clone(&suspended_card);
            let cb: rpi_agent::hooks::BeforeToolCall =
                Arc::new(move |ctx, _signal| {
                    if cancel_for_hook.is_cancelled() {
                        return Box::pin(async {
                            Some(BeforeToolCallResult {
                                block: true,
                                reason: Some(
                                    "CANCELLED: turn was cancelled before tool execution"
                                        .to_owned(),
                                ),
                                terminate: true,
                                args: None,
                            })
                        });
                    }
                    let already_suspended = slot
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .is_some();
                    if already_suspended {
                        return Box::pin(async {
                            Some(BeforeToolCallResult {
                                block: true,
                                reason: Some(
                                    "TOOL_APPROVAL_REQUIRED: turn suspended awaiting user confirmation"
                                        .to_owned(),
                                ),
                                terminate: true,
                                args: None,
                            })
                        });
                    }
                    let decision = hook(&ctx.tool_call.name, ctx.args);
                    let result = match decision {
                        PreToolDecision::Allow => None,
                        PreToolDecision::Deny { reason } => {
                            Some(BeforeToolCallResult {
                                block: true,
                                reason: Some(reason),
                                terminate: false,
                                args: None,
                            })
                        }
                        PreToolDecision::Ask {
                            target_summary,
                            args_hash,
                            risk_reason,
                            command_preview,
                        } => {
                            let now = u64::try_from(now_ms()).unwrap_or(0);
                            let card = PendingCard::ToolApproval {
                                card_id: next_approval_card_id(now),
                                created_at_ms: now,
                                tool_name: ctx.tool_call.name.clone(),
                                target_summary,
                                args_hash,
                                risk_reason,
                                command_preview,
                            };
                            *slot
                                .lock()
                                .unwrap_or_else(
                                    std::sync::PoisonError::into_inner,
                                ) = Some(card);
                            Some(BeforeToolCallResult {
                                block: true,
                                reason: Some(
                                    "TOOL_APPROVAL_REQUIRED: awaiting explicit user confirmation"
                                        .to_owned(),
                                ),
                                terminate: true,
                                args: None,
                            })
                        }
                        PreToolDecision::SuspendWithCard { card, reason } => {
                            *slot
                                .lock()
                                .unwrap_or_else(
                                    std::sync::PoisonError::into_inner,
                                ) = Some(card);
                            Some(BeforeToolCallResult {
                                block: true,
                                reason: Some(reason),
                                terminate: true,
                                args: None,
                            })
                        }
                    };
                    Box::pin(async move { result })
                });
            cb
        });
    let stop_slot = Arc::clone(&suspended_card);
    let tripped_breaker: Arc<Mutex<Option<SameErrorTrip>>> =
        Arc::new(Mutex::new(None));
    let breaker_slot = Arc::clone(&tripped_breaker);
    let should_stop_after_turn: rpi_agent::hooks::ShouldStopAfterTurn =
        Arc::new(move |ctx| {
            let awaiting_approval = stop_slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_some();
            let assistant_turns = ctx
                .new_messages
                .iter()
                .filter(|msg| matches!(msg, AgentMessage::Assistant(_)))
                .count();
            let hit_cap = assistant_turns >= MAX_TOOL_TURNS_PER_SUBMISSION;
            if hit_cap {
                tracing::warn!(
                    target: "flowy.prompt",
                    stage = "run_turn",
                    assistant_turns,
                    max_turns = MAX_TOOL_TURNS_PER_SUBMISSION,
                    "max tool turns reached; stopping submission loop"
                );
            }
            let same_error_trip = (!awaiting_approval)
                .then(|| should_trip_same_error_breaker(ctx.new_messages))
                .flatten();
            if let Some(trip) = &same_error_trip {
                tracing::warn!(
                    target: "flowy.prompt",
                    stage = "run_turn",
                    assistant_turns,
                    tool = %trip.tool_name,
                    target = %log_preview(&trip.target_sig, 80),
                    error_sig = %log_preview(&trip.error_sig, 120),
                    streak = trip.streak,
                    transient = trip.transient,
                    "same-error zero-new-evidence circuit breaker tripped; stopping submission loop"
                );
                *breaker_slot
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) =
                    Some(trip.clone());
            }
            let stop =
                awaiting_approval || hit_cap || same_error_trip.is_some();
            Box::pin(async move { stop })
        });
    let signal = request.signal.inner();
    let config = AgentLoopConfig {
        model: pi_model,
        convert_to_llm: rpi_agent::hooks::default_convert_to_llm_fn(),
        convert_to_llm_incremental: None,
        transform_context: Some(transform_context),
        get_api_key: None,
        should_stop_after_turn: Some(should_stop_after_turn),
        prepare_next_turn: None,
        after_tool_results: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        pending_message_count: None,
        before_tool_call,
        after_tool_call: None,
        tool_execution: rpi_agent::ToolExecutionMode::Sequential,
        thinking_level: rpi_ai::types::ThinkingLevel::Off,
        api_key: None,
        timeout: None,
        stream_idle_timeout: None,
        max_retries: Some(0),
        max_retry_delay: None,
        cache_retention: rpi_ai::provider::CacheRetention::default(),
        session_id: request.session_id,
        signal,
    };
    let new_messages =
        run_agent_loop(prompts, context, config, emit, stream_fn)
            .await
            .map_err(|error| error.to_string())?;
    let mut pending_approval = suspended_card
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    let breaker_trip = tripped_breaker
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    let raw_outcome = outcome_of(&new_messages);
    let (outcome, error) = if raw_outcome == LoopOutcome::Aborted
        || request.signal.is_cancelled()
    {
        pending_approval = None;
        (LoopOutcome::Aborted, None)
    } else if pending_approval.is_some() {
        (LoopOutcome::WaitingForUser, None)
    } else if let Some(trip) = breaker_trip {
        (
            LoopOutcome::Failed,
            Some(format!(
                "SAME_ERROR_CIRCUIT_BREAKER: tool `{}` failed {} consecutive times with identical error ({}) and zero new evidence",
                trip.tool_name,
                trip.streak,
                log_preview(&trip.error_sig, 120),
            )),
        )
    } else {
        (raw_outcome, failure_detail(&new_messages))
    };
    let messages = to_host_messages(&new_messages);
    if outcome == LoopOutcome::Failed {
        tracing::warn!(
            target: "flowy.prompt",
            stage = "run_turn",
            ?outcome,
            new_messages = messages.len(),
            error = error.as_deref().unwrap_or(""),
            prompt_preview = %prompt_preview,
            "done"
        );
    } else {
        tracing::debug!(
            target: "flowy.prompt",
            stage = "run_turn",
            ?outcome,
            new_messages = messages.len(),
            "done"
        );
    }
    Ok(TurnResult {
        messages,
        outcome,
        error,
        pending_approval,
    })
}

/// One-shot text completion with no tools or history, used for sidecar tasks
/// such as title generation.
pub async fn complete_text(
    session_id: Option<String>,
    system: &str,
    prompt: &str,
    route: ModelRoute,
    api_key: Option<String>,
    signal: CancelToken,
) -> Result<String, String> {
    let stream = crate::transport::openai_stream_fn(&route, api_key);
    let (emitter, _events) = rpi_agent::CollectorEmitter::new();
    let result = run_turn(
        TurnRequest {
            session_id,
            system: system.to_owned(),
            history: Vec::new(),
            prompt: prompt.to_owned(),
            route,
            tools: Vec::new(),
            signal,
            pre_tool_hook: None,
        },
        stream,
        Arc::new(emitter),
    )
    .await?;
    Ok(result
        .messages
        .iter()
        .rev()
        .find(|message| message.role == Role::Assistant)
        .map_or_default(Message::text))
}

/// Inputs for one-shot structured JSON completion via a single typed tool call.
#[derive(Clone, Debug)]
pub struct StructuredRequest {
    pub session_id: Option<String>,
    pub system: String,
    pub prompt: String,
    pub tool_name: String,
    pub tool_description: String,
    pub schema: serde_json::Value,
    pub route: ModelRoute,
    pub signal: CancelToken,
}

/// One-shot structured JSON completion backed by a single typed tool call
/// (`tool_choice` pinned to `request.tool_name`) over a single HTTP round-trip.
///
/// Using a typed tool instead of `response_format` works reliably across
/// OpenAI-compatible gateways and thinking models where `json_object` returns
/// HTTP 500 or `json_schema` degrades output quality.
pub async fn complete_structured(
    request: StructuredRequest,
    api_key: Option<String>,
) -> Result<serde_json::Value, String> {
    let forced_stream = crate::transport::openai_structured_stream_fn(
        &request.route,
        api_key.clone(),
        &request.tool_name,
    );
    let route = request.route.clone();
    let fallback_request = StructuredRequest {
        session_id: request.session_id.clone(),
        system: request.system.clone(),
        prompt: request.prompt.clone(),
        route: request.route.clone(),
        tool_name: request.tool_name.clone(),
        tool_description: request.tool_description.clone(),
        schema: request.schema.clone(),
        signal: request.signal.clone(),
    };
    match complete_structured_with_stream(request, forced_stream).await {
        Ok(value) => Ok(value),
        Err(err) => {
            let lower = err.to_ascii_lowercase();
            let tool_choice_rejected = lower.contains("tool_choice")
                && (lower.contains("400")
                    || lower.contains("422")
                    || lower.contains("unsupported")
                    || lower.contains("invalid"));
            if tool_choice_rejected && !fallback_request.signal.is_cancelled() {
                tracing::debug!(
                    target: "flowy.prompt",
                    error = %err,
                    "gateway rejected pinned tool_choice; retrying without forced tool_choice"
                );
                let fallback_stream =
                    crate::transport::openai_stream_fn(&route, api_key);
                complete_structured_with_stream(
                    fallback_request,
                    fallback_stream,
                )
                .await
            } else {
                Err(err)
            }
        }
    }
}

/// Testable core of [`complete_structured`] accepting an explicit [`StreamFn`].
pub async fn complete_structured_with_stream(
    request: StructuredRequest,
    stream_fn: StreamFn,
) -> Result<serde_json::Value, String> {
    if request.signal.is_cancelled() {
        return Err("structured completion cancelled".to_owned());
    }
    let model = request.route.rpi_model();
    let context = rpi_ai::types::Context {
        system_prompt: Some(request.system),
        messages: vec![rpi_ai::types::Message::User(UserMessage::new(
            UserContent::Text(request.prompt),
            now_ms(),
        ))],
        tools: vec![rpi_ai::types::Tool {
            name: request.tool_name.clone(),
            description: request.tool_description,
            parameters: rpi_ai::types::Schema::new(request.schema),
            constrained_sampling: None,
        }],
    };
    let options = rpi_ai::provider::SimpleStreamOptions {
        session_id: request.session_id,
        signal: request.signal.inner(),
        max_retries: Some(0),
        reasoning: Some(rpi_ai::types::ThinkingLevel::Off),
        ..rpi_ai::provider::SimpleStreamOptions::default()
    };

    let mut stream = stream_fn(&model, &context, &options);
    let mut terminal: Option<(rpi_ai::types::DoneReason, AssistantMessage)> =
        None;
    loop {
        tokio::select! {
            () = request.signal.cancelled() => {
                return Err("structured completion cancelled".to_owned());
            }
            maybe_event = stream.next() => {
                let Some(event) = maybe_event else {
                    break;
                };
                match event {
                    rpi_ai::types::AssistantMessageEvent::Done {
                        reason,
                        message,
                    } => {
                        terminal = Some((reason, message));
                        break;
                    }
                    rpi_ai::types::AssistantMessageEvent::Error {
                        error,
                        ..
                    } => {
                        let detail = assistant_error_detail(&error)
                            .unwrap_or_else(|| {
                                "structured completion stream failed".to_owned()
                            });
                        return Err(detail);
                    }
                    _ => {}
                }
            }
        }
    }
    drop(stream);
    let Some((done_reason, message)) = terminal else {
        return Err(
            "structured completion stream ended without terminal message"
                .to_owned(),
        );
    };
    if done_reason == rpi_ai::types::DoneReason::Length
        || matches!(
            message.stop_reason,
            StopReason::Length | StopReason::Error | StopReason::Aborted
        )
    {
        return Err(format!(
            "structured completion truncated or aborted (reason={done_reason:?}, stop={:?})",
            message.stop_reason
        ));
    }
    if let Some(call) = message
        .content
        .iter()
        .find_map(|block| match block {
            Content::ToolCall(call) if call.name == request.tool_name => {
                Some(call)
            }
            _ => None,
        })
        .or_else(|| {
            message.content.iter().find_map(|block| match block {
                Content::ToolCall(call) => Some(call),
                _ => None,
            })
        })
    {
        return match &call.arguments {
            serde_json::Value::Object(_) => Ok(call.arguments.clone()),
            serde_json::Value::String(encoded) => {
                let cleaned = strip_json_fence(encoded);
                match serde_json::from_str::<serde_json::Value>(cleaned) {
                    Ok(value @ serde_json::Value::Object(_)) => Ok(value),
                    _ => Err(
                        "structured tool call arguments were not a JSON object"
                            .to_owned(),
                    ),
                }
            }
            _ => Err("structured tool call arguments were not a JSON object"
                .to_owned()),
        };
    }
    // Fallback for providers or test doubles that emit inline JSON text instead
    // of a tool call.
    let raw = assistant_plain_text(&message);
    let cleaned = strip_json_fence(&raw);
    match serde_json::from_str::<serde_json::Value>(cleaned) {
        Ok(value @ serde_json::Value::Object(_)) => Ok(value),
        _ => Err(
            "structured completion returned neither tool call nor valid JSON object"
                .to_owned(),
        ),
    }
}

fn strip_json_fence(raw: &str) -> &str {
    let trimmed = raw.trim();
    let Some(rest) = trimmed.strip_prefix("```") else {
        return trimmed;
    };
    let rest = rest
        .strip_prefix("json")
        .or_else(|| rest.strip_prefix("JSON"))
        .unwrap_or(rest)
        .trim_start();
    rest.strip_suffix("```").map_or(trimmed, str::trim)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SameErrorTrip {
    tool_name: String,
    target_sig: String,
    error_sig: String,
    streak: usize,
    transient: bool,
}

fn is_readonly_idempotent_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "read_file"
            | "glob"
            | "grep"
            | "ls"
            | "list_dir"
            | "find_files"
            | "verify_deck"
            | "verify_artifact"
    )
}

fn has_transient_http_status(lower: &str) -> bool {
    const CODES: [&str; 4] = ["429", "502", "503", "504"];
    CODES.iter().any(|code| {
        lower.contains(&format!("http {code}"))
            || lower.contains(&format!("http/{code}"))
            || lower.contains(&format!("status {code}"))
            || lower.contains(&format!("status: {code}"))
            || lower.contains(&format!("status={code}"))
            || lower.contains(&format!("status code {code}"))
            || lower.contains(&format!("error {code}"))
            || lower.contains(&format!("code {code}"))
            || lower.contains(&format!("code: {code}"))
            || lower.contains(&format!("code={code}"))
            || lower.contains(&format!("{code} too many requests"))
            || lower.contains(&format!("{code} bad gateway"))
            || lower.contains(&format!("{code} service unavailable"))
            || lower.contains(&format!("{code} gateway timeout"))
    })
}

fn is_transient_tool_error(raw_error: &str) -> bool {
    let lower = raw_error.to_ascii_lowercase();
    has_transient_http_status(&lower)
        || [
            "timeout",
            "timed out",
            "temporarily unavailable",
            "temporary",
            "rate limit",
            "too many requests",
            "bad gateway",
            "service unavailable",
            "gateway timeout",
            "connection reset",
            "broken pipe",
            "sharing violation",
            "try again",
            "eagain",
            "ebusy",
            "resource busy",
        ]
        .iter()
        .any(|cue| lower.contains(cue))
}

fn normalize_signature(raw: &str, max_chars: usize) -> String {
    let collapsed: String = raw
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    collapsed.chars().take(max_chars).collect()
}

fn tool_error_signature(result: &ToolResultMessage) -> (String, String) {
    let raw = result
        .content
        .iter()
        .filter_map(|block| match block {
            Content::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ");
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
        let code = val
            .get("code")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let err = val
            .get("error")
            .and_then(serde_json::Value::as_str)
            .or_else(|| val.get("message").and_then(serde_json::Value::as_str))
            .unwrap_or("");
        if !code.is_empty() || !err.is_empty() {
            return (normalize_signature(&format!("{code}:{err}"), 160), raw);
        }
    }
    (normalize_signature(&raw, 160), raw)
}

/// Detect repeating `(tool_name, canonical_args_json, error_sig)` failures
/// across turns with zero intervening successful tool evidence.
///
/// - Non-transient errors (or any mutating tool error) trip immediately upon
///   identical recurrence (`streak >= 2`: initial failure + 1 futile identical
///   repeat, 0 extra retries).
/// - Transient errors on read-only/idempotent tools allow 1 identical retry
///   and trip if the subsequent identical attempt still fails (`streak >= 3`).
/// - Any successful tool result (`!is_error`) provides new evidence and resets
///   the streak.
fn should_trip_same_error_breaker(
    new_messages: &[AgentMessage],
) -> Option<SameErrorTrip> {
    let mut call_targets: HashMap<&str, (String, usize)> = HashMap::new();
    let mut assistant_turn: usize = 0;
    let mut last_key: Option<(String, String, String)> = None;
    let mut last_turn: Option<usize> = None;
    let mut streak: usize = 0;
    let mut trailing_trip: Option<SameErrorTrip> = None;

    for message in new_messages {
        match message {
            AgentMessage::Assistant(assistant) => {
                assistant_turn = assistant_turn.saturating_add(1);
                for block in &assistant.content {
                    if let Content::ToolCall(call) = block {
                        call_targets.insert(
                            call.id.as_str(),
                            (
                                canonical_tool_args_json(
                                    &call.name,
                                    &call.arguments,
                                ),
                                assistant_turn,
                            ),
                        );
                    }
                }
            }
            AgentMessage::ToolResult(result) => {
                if !result.is_error {
                    last_key = None;
                    last_turn = None;
                    streak = 0;
                    trailing_trip = None;
                    continue;
                }
                let tool_name = normalize_signature(&result.tool_name, 80);
                let (target_sig, turn_idx) = call_targets
                    .get(result.tool_call_id.as_str())
                    .cloned()
                    .unwrap_or_else(|| (String::new(), assistant_turn));
                let (error_sig, raw_error) = tool_error_signature(result);
                let key =
                    (tool_name.clone(), target_sig.clone(), error_sig.clone());
                if last_key.as_ref() == Some(&key) {
                    if last_turn != Some(turn_idx) {
                        streak += 1;
                        last_turn = Some(turn_idx);
                    }
                } else {
                    last_key = Some(key);
                    last_turn = Some(turn_idx);
                    streak = 1;
                    trailing_trip = None;
                }
                let transient = is_readonly_idempotent_tool(&tool_name)
                    && is_transient_tool_error(&raw_error);
                let limit = if transient { 3 } else { 2 };
                if streak >= limit {
                    trailing_trip = Some(SameErrorTrip {
                        tool_name,
                        target_sig,
                        error_sig,
                        streak,
                        transient,
                    });
                }
            }
            _ => {}
        }
    }
    trailing_trip
}

fn message_char_len(message: &AgentMessage) -> usize {
    match message {
        AgentMessage::User(user) => match &user.content {
            UserContent::Text(text) => text.chars().count(),
            UserContent::Blocks(blocks) => blocks
                .iter()
                .map(|b| match b {
                    Content::Text(t) => t.text.chars().count(),
                    _ => 0,
                })
                .sum(),
        },
        AgentMessage::Assistant(assistant) => assistant
            .content
            .iter()
            .map(|c| match c {
                Content::Text(t) => t.text.chars().count(),
                Content::Thinking(t) => t.thinking.chars().count(),
                Content::ToolCall(call) => {
                    call.name.len() + call.arguments.to_string().chars().count()
                }
                Content::Image(_) => 256,
            })
            .sum(),
        AgentMessage::ToolResult(result) => result
            .content
            .iter()
            .map(|c| match c {
                Content::Text(t) => t.text.chars().count(),
                Content::Thinking(t) => t.thinking.chars().count(),
                Content::ToolCall(_) => 64,
                Content::Image(_) => 256,
            })
            .sum(),
        AgentMessage::Custom(_) => 0,
    }
}

fn truncate_text_chars(text: &str, max_chars: usize) -> Option<String> {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(max_chars).collect();
    chars
        .next()
        .map(|_| format!("{head}{CONTEXT_TRUNCATION_SUFFIX}"))
}

/// Bound per-tool-result length and trim oldest history turns while preserving
/// strict `Assistant(ToolCall)` -> `ToolResult` pairing for OpenAI-compatible
/// gateways.
fn compact_context_messages(
    mut messages: Vec<AgentMessage>,
    char_budget: usize,
) -> Vec<AgentMessage> {
    for message in &mut messages {
        if let AgentMessage::ToolResult(result) = message {
            for block in &mut result.content {
                if let Content::Text(text) = block
                    && let Some(trimmed) =
                        truncate_text_chars(&text.text, MAX_TOOL_RESULT_CHARS)
                {
                    text.text = trimmed;
                }
            }
        }
    }
    let mut total_chars: usize = messages.iter().map(message_char_len).sum();
    if total_chars <= char_budget || messages.len() <= 2 {
        return messages;
    }
    let mut start = 0_usize;
    while total_chars > char_budget && messages.len().saturating_sub(start) > 2
    {
        // Drop one atomic unit: either a single non-tool message, or an
        // assistant message together with all immediately following tool results.
        let mut next_start = start + 1;
        if matches!(messages.get(start), Some(AgentMessage::Assistant(_))) {
            while next_start < messages.len()
                && matches!(
                    messages.get(next_start),
                    Some(AgentMessage::ToolResult(_))
                )
            {
                next_start += 1;
            }
        }
        if messages.len().saturating_sub(next_start) < 1 {
            break;
        }
        for dropped in &messages[start..next_start] {
            total_chars = total_chars.saturating_sub(message_char_len(dropped));
        }
        start = next_start;
    }
    // Never start the retained window on an orphaned ToolResult.
    while start < messages.len().saturating_sub(1)
        && matches!(messages.get(start), Some(AgentMessage::ToolResult(_)))
    {
        start += 1;
    }
    if start > 0 {
        messages.drain(..start);
    }
    messages
}

fn outcome_of(new_messages: &[AgentMessage]) -> LoopOutcome {
    let last = new_messages.iter().rev().find_map(|message| match message {
        AgentMessage::Assistant(assistant) => Some(assistant),
        _ => None,
    });
    match last.map(|assistant| assistant.stop_reason) {
        Some(StopReason::Error) => LoopOutcome::Failed,
        Some(StopReason::Aborted) => LoopOutcome::Aborted,
        Some(_) | None => LoopOutcome::Completed,
    }
}

fn failure_detail(new_messages: &[AgentMessage]) -> Option<String> {
    let last = new_messages
        .iter()
        .rev()
        .find_map(|message| match message {
            AgentMessage::Assistant(assistant) => Some(assistant),
            _ => None,
        })?;
    if last.stop_reason != StopReason::Error {
        return None;
    }
    assistant_error_detail(last)
}

fn assistant_error_detail(assistant: &AssistantMessage) -> Option<String> {
    assistant
        .error_message
        .as_deref()
        .map(str::trim)
        .filter(|msg| !msg.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            let text = assistant_plain_text(assistant);
            (!text.trim().is_empty()).then_some(text)
        })
}

fn assistant_plain_text(assistant: &AssistantMessage) -> String {
    assistant
        .content
        .iter()
        .filter_map(|content| match content {
            Content::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect()
}

/// Convert stored messages to pi agent messages.
///
/// A tool result must carry its originating tool name via
/// [`MessageSource::Tool`]; legacy rows that predate that field are skipped so
/// one stale history entry cannot fail a whole turn.
fn to_agent_message(message: &Message) -> Result<Option<AgentMessage>, String> {
    if let Some(tool_result) =
        message.content.iter().find_map(|block| match block {
            ContentBlock::ToolResult {
                tool_call_id,
                content,
                is_error,
                ..
            } => Some((tool_call_id, content, is_error)),
            _ => None,
        })
    {
        let MessageSource::Tool { tool } = &message.source else {
            return Ok(None);
        };
        let tool_name = tool.clone();
        let (tool_call_id, content, is_error) = tool_result;
        return Ok(Some(AgentMessage::ToolResult(Box::new(
            ToolResultMessage {
                role: ToolResultRole,
                tool_call_id: tool_call_id.clone(),
                tool_name,
                content: to_pi_content(content)?,
                details: None,
                usage: None,
                added_tool_names: Vec::new(),
                is_error: is_error.unwrap_or(false),
                timestamp: now_ms(),
            },
        ))));
    }
    match message.role {
        Role::System => {
            Err("system messages belong in the system prompt".into())
        }
        Role::User => Ok(Some(AgentMessage::User(UserMessage::new(
            UserContent::Text(message.text()),
            now_ms(),
        )))),
        Role::Assistant => {
            let (provider, model) = match &message.source {
                MessageSource::Model { provider, model } => {
                    (provider.clone(), model.clone())
                }
                _ => ("flowy".to_owned(), String::new()),
            };
            let mut assistant = AssistantMessage::empty(
                Api::OpenaiCompletions,
                provider,
                model,
                now_ms(),
            );
            assistant.content = to_pi_content(&message.content)?;
            assistant.stop_reason = if assistant
                .content
                .iter()
                .any(|content| matches!(content, Content::ToolCall(_)))
            {
                StopReason::ToolUse
            } else {
                StopReason::Stop
            };
            Ok(Some(AgentMessage::Assistant(Box::new(assistant))))
        }
    }
}

/// Text recorded for a tool call whose result never made it to disk.
const RECOVERY_TOOL_RESULT: &str = concat!(
    "Tool call was interrupted before its result was persisted. ",
    "The tool was not re-executed during session recovery."
);

/// Repair tool-call pairing before history reaches the provider.
///
/// Policy mirrors pi-rust `rpi-harness/src/messages.rs`
/// (`repair_orphaned_tool_calls`, MIT licensed), adapted to the host message
/// vocabulary instead of depending on the harness crate.
///
/// Stored history can be truncated mid-turn or predate the current schema, and
/// strict OpenAI-compatible gateways reject both orphaned tool results and
/// unanswered tool calls. Keep matching results next to their assistant, drop
/// unmatched results, and synthesize an error result for a call that never
/// recorded one (same policy as the pi harness repair).
fn repair_tool_call_history(history: Vec<AgentMessage>) -> Vec<AgentMessage> {
    let mut input = history.into_iter().peekable();
    let mut repaired: Vec<AgentMessage> = Vec::new();
    while let Some(message) = input.next() {
        let AgentMessage::Assistant(assistant) = &message else {
            // A tool result never matched a call: providers reject it.
            if !matches!(message, AgentMessage::ToolResult(_)) {
                repaired.push(message);
            }
            continue;
        };
        let timestamp = assistant.timestamp;
        let mut pending: Vec<(String, String)> = assistant
            .content
            .iter()
            .filter_map(|content| match content {
                Content::ToolCall(call) => {
                    Some((call.id.clone(), call.name.clone()))
                }
                _ => None,
            })
            .collect();
        repaired.push(message);
        let mut interleaved = Vec::new();
        while input.peek().is_some_and(|message| {
            !matches!(message, AgentMessage::Assistant(_))
        }) {
            let message = input.next().expect("peeked message");
            if let AgentMessage::ToolResult(result) = &message {
                if let Some(index) = pending.iter().position(|(id, name)| {
                    id == &result.tool_call_id && name == &result.tool_name
                }) {
                    pending.remove(index);
                    repaired.push(message);
                }
            } else {
                interleaved.push(message);
            }
        }
        for (tool_call_id, tool_name) in pending {
            repaired.push(AgentMessage::ToolResult(Box::new(
                ToolResultMessage {
                    role: ToolResultRole,
                    tool_call_id,
                    tool_name,
                    content: vec![Content::text(RECOVERY_TOOL_RESULT)],
                    details: None,
                    usage: None,
                    added_tool_names: Vec::new(),
                    is_error: true,
                    timestamp,
                },
            )));
        }
        repaired.extend(interleaved);
    }
    repaired
}

fn to_pi_content(blocks: &[ContentBlock]) -> Result<Vec<Content>, String> {
    blocks
        .iter()
        .map(|block| match block {
            ContentBlock::Text { text } => Ok(Content::text(text.clone())),
            ContentBlock::Reasoning { text } => {
                Ok(Content::Thinking(ThinkingContent {
                    kind: rpi_ai::types::ThinkingContentType,
                    thinking: text.clone(),
                    thinking_signature: None,
                    redacted: false,
                }))
            }
            ContentBlock::ToolCall {
                id,
                name,
                arguments,
            } => {
                // Legacy rows may hold truncation or manual edits; the
                // provider requires syntactically valid JSON, so a corrupted
                // blob degrades to an empty object instead of failing replay.
                let arguments: serde_json::Value =
                    serde_json::from_str(arguments)
                        .unwrap_or_else(|_| serde_json::json!({}));
                Ok(Content::ToolCall(ToolCall {
                    kind: ToolCallType,
                    id: id.clone(),
                    name: name.clone(),
                    arguments,
                    thought_signature: None,
                    namespace: None,
                    recorded: None,
                }))
            }
            ContentBlock::ToolResult { .. } => {
                Err("tool result blocks must be their own message".into())
            }
        })
        .collect()
}

/// Convert the loop's new messages back to host messages for durable storage.
#[must_use]
fn to_host_messages(new_messages: &[AgentMessage]) -> Vec<Message> {
    let mut out = Vec::new();
    for message in new_messages {
        match message {
            AgentMessage::User(user) => {
                out.push(Message {
                    id: next_id(),
                    role: Role::User,
                    content: vec![ContentBlock::text(user_text(&user.content))],
                    source: MessageSource::User,
                    display_text: None,
                });
            }
            AgentMessage::Assistant(assistant) => {
                let content: Vec<ContentBlock> = assistant
                    .content
                    .iter()
                    .filter_map(|content| match content {
                        Content::Text(text) => {
                            Some(ContentBlock::text(text.text.clone()))
                        }
                        Content::Thinking(thinking) => {
                            Some(ContentBlock::Reasoning {
                                text: thinking.thinking.clone(),
                            })
                        }
                        Content::ToolCall(call) => {
                            Some(ContentBlock::ToolCall {
                                id: call.id.clone(),
                                name: call.name.clone(),
                                arguments: call.arguments.to_string(),
                            })
                        }
                        Content::Image(_) => None,
                    })
                    .collect();
                out.push(Message {
                    id: next_id(),
                    role: Role::Assistant,
                    content,
                    source: MessageSource::Model {
                        provider: assistant.provider.clone(),
                        model: assistant.model.clone(),
                    },
                    display_text: None,
                });
            }
            AgentMessage::ToolResult(result) => {
                let content = result
                    .content
                    .iter()
                    .filter_map(|content| match content {
                        Content::Text(text) => {
                            Some(ContentBlock::text(text.text.clone()))
                        }
                        Content::Thinking(thinking) => {
                            Some(ContentBlock::text(thinking.thinking.clone()))
                        }
                        _ => None,
                    })
                    .collect();
                out.push(Message {
                    id: next_id(),
                    role: Role::User,
                    content: vec![ContentBlock::ToolResult {
                        tool_call_id: result.tool_call_id.clone(),
                        content,
                        is_error: Some(result.is_error),
                        // Only Engine's private RegistryTool receipt channel
                        // can attach host-verified artifact metadata.
                        artifact: None,
                    }],
                    source: MessageSource::Tool {
                        tool: result.tool_name.clone(),
                    },
                    display_text: None,
                });
            }
            AgentMessage::Custom(_) => {}
        }
    }
    out
}

fn user_text(content: &UserContent) -> String {
    match content {
        UserContent::Text(text) => text.clone(),
        UserContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|block| match block {
                Content::Text(text) => Some(text.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(""),
    }
}

fn next_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "m-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
        })
}

#[cfg(test)]
#[path = "loop_runner_tests.rs"]
mod tests;
