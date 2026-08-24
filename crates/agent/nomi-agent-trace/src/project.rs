//! Project JSONL events into per-turn workflow views. Sort only by `event_seq`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::event::{
    ids_from_payload, ExecutionStatus, Integrity, ObservationEvent, ObservationScope,
    EVENT_LLM_REQUEST, EVENT_LLM_RESPONSE, EVENT_OBSERVATION_GAP, EVENT_TOOL_EXECUTION_CANCELLED,
    EVENT_TOOL_EXECUTION_COMPLETED, EVENT_TOOL_EXECUTION_FAILED, EVENT_TOOL_EXECUTION_STARTED,
    EVENT_TURN_END, EVENT_TURN_START,
};
use crate::redact::{redact_preview, truncate_chars};

/// Scan-line preview kept after payload strip. Not the captured body.
const SCAN_PREVIEW_CHARS: usize = 80;
/// Reserved marker used by the runtime for the leading context block in a
/// user message. It is intentionally kept out of user-facing previews.
const CONTEXT_PREFIX: &str = "[Context]";

pub const COVERAGE_RETAINED_OBSERVATION_HISTORY: &str = "retained_observation_history";

/// Observation-layer token view. Provider `TokenUsage.input_tokens` is not
/// uniform (OpenAI folds cache into input; Anthropic does not), so this copies
/// raw fields and never invents `input_uncached` or sums cache into input.
pub type NormalizedObservationUsage = ProjectedTokenUsage;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectedTokenUsage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolExecutionStatus {
    Started,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObservationSummary {
    pub turn_count: u64,
    pub model_call_count: u64,
    pub tool_count: u64,
    pub active_duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_span_ms: Option<u64>,
    pub integrity: Integrity,
    pub coverage: String,
    pub max_event_seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectedTurn {
    pub root_turn_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_attempt_id: Option<String>,
    pub status: ExecutionStatus,
    pub integrity: Integrity,
    pub interrupted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_preview: Option<String>,
    /// True when the latest request contained only the runtime-injected
    /// leading Context block and therefore has no user text preview.
    #[serde(default)]
    pub prompt_preview_context_only: bool,
    #[serde(default)]
    pub max_event_seq: u64,
    #[serde(default)]
    pub has_turn_start: bool,
    #[serde(default)]
    pub has_turn_end: bool,
    pub gap_count: u32,
    pub model_calls: Vec<ProjectedModelCall>,
    pub gaps: Vec<ProjectedGap>,
}

/// Counts extracted from a captured `llm/request` before bodies are stripped.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ProjectedRequestSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub has_system: bool,
    #[serde(default)]
    pub message_count: u32,
    #[serde(default)]
    pub tool_definition_count: u32,
    #[serde(default)]
    pub system_omitted: bool,
    #[serde(default)]
    pub messages_omitted: bool,
    #[serde(default)]
    pub tools_omitted: bool,
}

/// Counts extracted from a captured `llm/response` before bodies are stripped.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ProjectedResponseSummary {
    #[serde(default)]
    pub has_text: bool,
    #[serde(default)]
    pub has_thinking: bool,
    #[serde(default)]
    pub text_omitted: bool,
    #[serde(default)]
    pub thinking_omitted: bool,
    #[serde(default)]
    pub tool_use_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttft_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectedModelCall {
    pub model_call_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation_scope: Option<ObservationScope>,
    pub status: ExecutionStatus,
    pub integrity: Integrity,
    pub interrupted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<ProjectedTokenUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_summary: Option<ProjectedRequestSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_summary: Option<ProjectedResponseSummary>,
    #[serde(default)]
    pub tools: Vec<ProjectedToolExecution>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectedToolExecution {
    pub tool_call_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at_ms: Option<u64>,
    pub status: ToolExecutionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argument_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancelled: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectedGap {
    pub event_seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_seq: Option<u64>,
}

/// Group by turn / execution boundary, then order each group by `event_seq` only.
pub fn project_turns(events: &[ObservationEvent]) -> Vec<ProjectedTurn> {
    project_event_refs(events.iter())
}

pub(crate) fn project_event_refs<'a, I>(events: I) -> Vec<ProjectedTurn>
where
    I: IntoIterator<Item = &'a ObservationEvent>,
{
    let mut grouped: Vec<(String, Vec<&ObservationEvent>)> = Vec::new();
    for event in events {
        let key = turn_key(event);
        if let Some((_, bucket)) = grouped.iter_mut().find(|(k, _)| k == &key) {
            bucket.push(event);
        } else {
            grouped.push((key, vec![event]));
        }
    }

    grouped
        .into_iter()
        .map(|(_, mut bucket)| {
            bucket.sort_by_key(|event| event.event_seq);
            project_one(&bucket)
        })
        .collect()
}

pub fn project_turn_by_id(events: &[ObservationEvent], root_turn_id: &str) -> Option<ProjectedTurn> {
    project_turns(events)
        .into_iter()
        .find(|turn| turn.root_turn_id == root_turn_id)
}

fn turn_key(event: &ObservationEvent) -> String {
    let ids = ids_from_payload(&event.payload);
    ids.root_turn_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| ids.boundary_id())
}

/// Whether this event belongs to the projected turn identified by `root_turn_id`.
pub fn event_belongs_to_turn(event: &ObservationEvent, root_turn_id: &str) -> bool {
    turn_key(event) == root_turn_id
}

/// Drop request/response/tool payloads so list APIs can return counts only.
pub fn strip_projected_turn_payloads(turn: &mut ProjectedTurn) {
    for call in &mut turn.model_calls {
        call.request = None;
        call.response = None;
        for tool in &mut call.tools {
            tool.started = None;
            tool.completed = None;
            tool.failed = None;
            tool.cancelled = None;
        }
    }
}

fn project_one(events: &[&ObservationEvent]) -> ProjectedTurn {
    let first_ids = events
        .first()
        .map(|event| ids_from_payload(&event.payload))
        .unwrap_or_default();
    let root_turn_id = first_ids
        .root_turn_id
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| first_ids.boundary_id());

    let mut calls: Vec<ProjectedModelCall> = Vec::new();
    let mut gaps = Vec::new();
    let mut has_turn_start = false;
    let mut has_turn_end = false;
    let mut turn_end_status: Option<ExecutionStatus> = None;
    let mut turn_end_elapsed_ms: Option<u64> = None;
    let mut turn_start_preview: Option<String> = None;
    let mut latest_request: Option<&ObservationEvent> = None;
    let mut started_at_ms = events.first().map(|event| event.timestamp_ms);
    let mut ended_at_ms = events.last().map(|event| event.timestamp_ms);
    let mut max_event_seq = 0u64;

    for event in events {
        max_event_seq = max_event_seq.max(event.event_seq);
        let ids = ids_from_payload(&event.payload);
        match event.event_type.as_str() {
            EVENT_TURN_START => {
                has_turn_start = true;
                started_at_ms = Some(event.timestamp_ms);
                if turn_start_preview.is_none() {
                    turn_start_preview = string_field(&event.payload, "prompt_preview")
                        .filter(|preview| !preview.trim().is_empty());
                }
            }
            EVENT_TURN_END => {
                has_turn_end = true;
                ended_at_ms = Some(event.timestamp_ms);
                turn_end_status = string_field(&event.payload, "status")
                    .and_then(|status| serde_json::from_value(Value::String(status)).ok());
                turn_end_elapsed_ms = event
                    .payload
                    .get("elapsed_ms")
                    .and_then(Value::as_u64)
                    .or_else(|| {
                        started_at_ms.map(|start| event.timestamp_ms.saturating_sub(start))
                    });
            }
            EVENT_LLM_REQUEST => {
                let model_call_id = ids
                    .model_call_id
                    .clone()
                    .unwrap_or_else(|| format!("anon-{}", event.event_seq));
                let call = upsert_call(&mut calls, &model_call_id);
                call.call_kind = string_field(&event.payload, "call_kind");
                call.observation_scope = event
                    .payload
                    .get("observation_scope")
                    .and_then(|v| serde_json::from_value(v.clone()).ok());
                call.request = Some(event.payload.clone());
                call.request_summary = Some(request_summary_from_payload(&event.payload));
                call.started_at_ms = Some(event.timestamp_ms);
                latest_request = Some(event);
            }
            EVENT_LLM_RESPONSE => {
                let model_call_id = ids
                    .model_call_id
                    .clone()
                    .unwrap_or_else(|| format!("anon-{}", event.event_seq));
                let call = upsert_call(&mut calls, &model_call_id);
                if call.call_kind.is_none() {
                    call.call_kind = string_field(&event.payload, "call_kind");
                }
                call.response = Some(event.payload.clone());
                call.response_summary = Some(response_summary_from_payload(&event.payload));
                call.ended_at_ms = Some(event.timestamp_ms);
                if call.usage.is_none() {
                    call.usage = usage_from_payload(&event.payload);
                }
            }
            EVENT_TOOL_EXECUTION_STARTED
            | EVENT_TOOL_EXECUTION_COMPLETED
            | EVENT_TOOL_EXECUTION_FAILED
            | EVENT_TOOL_EXECUTION_CANCELLED => {
                let model_call_id = ids
                    .model_call_id
                    .clone()
                    .unwrap_or_else(|| format!("anon-{}", event.event_seq));
                let tool_call_id = tool_call_id_from_event(event, &model_call_id);
                let call = upsert_call(&mut calls, &model_call_id);
                let tool = upsert_tool(&mut call.tools, &tool_call_id);
                if tool.name.is_none() {
                    tool.name = string_field(&event.payload, "name")
                        .or_else(|| string_field(&event.payload, "tool_name"));
                }
                match event.event_type.as_str() {
                    EVENT_TOOL_EXECUTION_STARTED => {
                        tool.started = Some(event.payload.clone());
                        tool.started_at_ms = Some(event.timestamp_ms);
                        if tool.argument_preview.is_none() {
                            tool.argument_preview = argument_preview_from_payload(&event.payload);
                        }
                    }
                    EVENT_TOOL_EXECUTION_COMPLETED => {
                        tool.completed = Some(event.payload.clone());
                        tool.ended_at_ms = Some(event.timestamp_ms);
                    }
                    EVENT_TOOL_EXECUTION_FAILED => {
                        tool.failed = Some(event.payload.clone());
                        tool.ended_at_ms = Some(event.timestamp_ms);
                    }
                    EVENT_TOOL_EXECUTION_CANCELLED => {
                        tool.cancelled = Some(event.payload.clone());
                        tool.ended_at_ms = Some(event.timestamp_ms);
                    }
                    _ => {}
                }
            }
            EVENT_OBSERVATION_GAP => {
                gaps.push(ProjectedGap {
                    event_seq: event.event_seq,
                    reason: string_field(&event.payload, "reason"),
                    from_seq: event.payload.get("from_seq").and_then(Value::as_u64),
                    to_seq: event.payload.get("to_seq").and_then(Value::as_u64),
                });
            }
            _ => {}
        }
    }

    let turn_ended = has_turn_end;
    for call in &mut calls {
        if call.ended_at_ms.is_none() {
            call.ended_at_ms = call
                .tools
                .iter()
                .filter_map(|tool| tool.ended_at_ms)
                .max();
        }
        call.status = model_call_status(call, turn_ended);
        call.interrupted = call.status == ExecutionStatus::Interrupted;
    }

    let interrupted = calls.iter().any(|call| call.interrupted);
    let integrity = turn_integrity(turn_ended, &calls, &gaps);
    for call in &mut calls {
        call.integrity = call_integrity(turn_ended, call, &gaps);
        for tool in &mut call.tools {
            tool.status = tool_status(tool);
        }
    }
    let request_preview = latest_request.map(|event| last_user_text_projection(&event.payload));
    // A correctly bound turn/start is authoritative. If an older or alternate
    // producer accidentally persisted the injected Context as its preview,
    // prefer the sanitized request fallback when one exists.
    let (prompt_preview, prompt_preview_context_only) = match (turn_start_preview, request_preview) {
        (Some(start), Some(fallback)) if is_leading_context_block(&start) => {
            if fallback.preview.is_some() {
                (fallback.preview, false)
            } else {
                (None, true)
            }
        }
        (Some(start), _) if is_leading_context_block(&start) => (None, true),
        (Some(start), _) => (Some(start), false),
        (None, Some(fallback)) => (fallback.preview, fallback.context_only),
        (None, None) => (None, false),
    };
    let status = if let Some(status) = turn_end_status {
        status
    } else if has_turn_end {
        if interrupted {
            ExecutionStatus::Interrupted
        } else if calls.iter().any(|call| call.status == ExecutionStatus::Failed) {
            ExecutionStatus::Failed
        } else {
            ExecutionStatus::Completed
        }
    } else if has_turn_start {
        ExecutionStatus::Running
    } else {
        ExecutionStatus::Unknown
    };

    ProjectedTurn {
        root_turn_id,
        conversation_id: first_ids.conversation_id,
        msg_id: first_ids.msg_id,
        session_kind: first_ids.session_kind,
        execution_id: first_ids.execution_id,
        step_id: first_ids.step_id,
        execution_attempt_id: first_ids.execution_attempt_id,
        status,
        integrity,
        interrupted,
        started_at_ms,
        ended_at_ms,
        elapsed_ms: turn_end_elapsed_ms,
        prompt_preview,
        prompt_preview_context_only,
        max_event_seq,
        has_turn_start,
        has_turn_end,
        gap_count: gaps.len() as u32,
        model_calls: calls,
        gaps,
    }
}

fn tool_is_terminal(tool: &ProjectedToolExecution) -> bool {
    tool.completed.is_some() || tool.failed.is_some() || tool.cancelled.is_some()
}

fn model_call_status(call: &ProjectedModelCall, turn_ended: bool) -> ExecutionStatus {
    let stop_reason = call
        .response
        .as_ref()
        .and_then(|payload| payload.get("stop_reason"))
        .and_then(Value::as_str);
    let has_error = call
        .response
        .as_ref()
        .and_then(|payload| payload.get("error"))
        .is_some_and(|error| !error.is_null())
        || stop_reason == Some("error");
    if has_error || call.tools.iter().any(|tool| tool.failed.is_some()) {
        return ExecutionStatus::Failed;
    }
    if call.tools.iter().any(|tool| tool.cancelled.is_some()) {
        return ExecutionStatus::Cancelled;
    }
    if stop_reason == Some("max_tokens") {
        return ExecutionStatus::Truncated;
    }
    let missing_response = call.request.is_some() && call.response.is_none();
    let open_tool = call
        .tools
        .iter()
        .any(|tool| tool.started.is_some() && !tool_is_terminal(tool));
    if missing_response || open_tool {
        return if turn_ended {
            ExecutionStatus::Interrupted
        } else {
            ExecutionStatus::Running
        };
    }
    if call.response.is_some() {
        return ExecutionStatus::Completed;
    }
    ExecutionStatus::Running
}

fn turn_integrity(
    turn_ended: bool,
    calls: &[ProjectedModelCall],
    gaps: &[ProjectedGap],
) -> Integrity {
    if !gaps.is_empty() {
        return Integrity::Degraded;
    }
    if turn_ended
        && calls.iter().any(|call| {
            (call.request.is_some() && call.response.is_none())
                || call
                    .tools
                    .iter()
                    .any(|tool| tool.started.is_some() && !tool_is_terminal(tool))
        })
    {
        return Integrity::Degraded;
    }
    Integrity::Complete
}

fn upsert_call<'a>(calls: &'a mut Vec<ProjectedModelCall>, model_call_id: &str) -> &'a mut ProjectedModelCall {
    if let Some(index) = calls.iter().position(|call| call.model_call_id == model_call_id) {
        return &mut calls[index];
    }
    calls.push(ProjectedModelCall {
        model_call_id: model_call_id.to_owned(),
        call_kind: None,
        observation_scope: None,
        status: ExecutionStatus::Running,
        integrity: Integrity::Complete,
        interrupted: false,
        started_at_ms: None,
        ended_at_ms: None,
        usage: None,
        request: None,
        response: None,
        request_summary: None,
        response_summary: None,
        tools: Vec::new(),
    });
    calls.last_mut().expect("just pushed")
}

fn upsert_tool<'a>(
    tools: &'a mut Vec<ProjectedToolExecution>,
    tool_call_id: &str,
) -> &'a mut ProjectedToolExecution {
    if let Some(index) = tools.iter().position(|tool| tool.tool_call_id == tool_call_id) {
        return &mut tools[index];
    }
    tools.push(ProjectedToolExecution {
        tool_call_id: tool_call_id.to_owned(),
        name: None,
        started_at_ms: None,
        ended_at_ms: None,
        status: ToolExecutionStatus::Started,
        argument_preview: None,
        started: None,
        completed: None,
        failed: None,
        cancelled: None,
    });
    tools.last_mut().expect("just pushed")
}

fn string_field(payload: &Value, key: &str) -> Option<String> {
    payload.get(key).and_then(Value::as_str).map(ToOwned::to_owned)
}

fn tool_status(tool: &ProjectedToolExecution) -> ToolExecutionStatus {
    if tool.cancelled.is_some() {
        ToolExecutionStatus::Cancelled
    } else if tool.failed.is_some() {
        ToolExecutionStatus::Failed
    } else if tool.completed.is_some() {
        ToolExecutionStatus::Completed
    } else {
        ToolExecutionStatus::Started
    }
}

fn call_integrity(turn_ended: bool, call: &ProjectedModelCall, gaps: &[ProjectedGap]) -> Integrity {
    if !gaps.is_empty() {
        return Integrity::Degraded;
    }
    if turn_ended
        && ((call.request.is_some() && call.response.is_none())
            || call
                .tools
                .iter()
                .any(|tool| tool.started.is_some() && !tool_is_terminal(tool)))
    {
        return Integrity::Degraded;
    }
    Integrity::Complete
}

fn request_object(payload: &Value) -> &Value {
    payload.get("request").unwrap_or(payload)
}

fn value_is_omitted(value: &Value) -> bool {
    value.get("omitted_reason").and_then(Value::as_str).is_some()
}

fn field_is_omitted(object: &Value, field: &str) -> bool {
    object.get(field).is_some_and(value_is_omitted)
}

fn tool_call_id_from_event(event: &ObservationEvent, model_call_id: &str) -> String {
    event
        .payload
        .get("tool_call_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("anon-{model_call_id}"))
}

fn array_len_present(value: Option<&Value>) -> u32 {
    match value {
        Some(item) if !value_is_omitted(item) => {
            item.as_array().map(|items| items.len() as u32).unwrap_or(0)
        }
        _ => 0,
    }
}

fn request_summary_from_payload(payload: &Value) -> ProjectedRequestSummary {
    let request = request_object(payload);
    let envelope_omitted = value_is_omitted(request);
    let model = request
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let has_system = match request.get("system") {
        Some(Value::String(text)) => !text.trim().is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(other) if value_is_omitted(other) => false,
        _ => false,
    };
    ProjectedRequestSummary {
        model,
        has_system,
        message_count: array_len_present(request.get("messages")),
        tool_definition_count: array_len_present(request.get("tools")),
        system_omitted: envelope_omitted || field_is_omitted(request, "system"),
        messages_omitted: envelope_omitted || field_is_omitted(request, "messages"),
        tools_omitted: envelope_omitted || field_is_omitted(request, "tools"),
    }
}

fn scan_text_preview(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(truncate_chars(&redact_preview(trimmed), SCAN_PREVIEW_CHARS))
}

fn argument_preview_from_payload(payload: &Value) -> Option<String> {
    let arguments = payload.get("arguments")?;
    if value_is_omitted(arguments) {
        return None;
    }
    match arguments {
        Value::Null => None,
        Value::String(text) => scan_text_preview(text),
        other => scan_text_preview(&serde_json::to_string(other).ok()?),
    }
}

fn response_summary_from_payload(payload: &Value) -> ProjectedResponseSummary {
    let envelope_omitted = value_is_omitted(payload);
    let text = payload.get("text").and_then(Value::as_str).unwrap_or("");
    let thinking = payload.get("thinking").and_then(Value::as_str).unwrap_or("");
    ProjectedResponseSummary {
        has_text: !text.trim().is_empty(),
        has_thinking: !thinking.trim().is_empty(),
        text_omitted: envelope_omitted || field_is_omitted(payload, "text"),
        thinking_omitted: envelope_omitted || field_is_omitted(payload, "thinking"),
        tool_use_count: array_len_present(payload.get("tool_use")),
        elapsed_ms: payload.get("elapsed_ms").and_then(Value::as_u64),
        ttft_ms: payload.get("ttft_ms").and_then(Value::as_u64),
        stop_reason: payload
            .get("stop_reason")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        text_preview: scan_text_preview(text),
    }
}

fn usage_from_payload(payload: &Value) -> Option<ProjectedTokenUsage> {
    let usage = payload.get("usage")?;
    let input_tokens = usage.get("input_tokens").and_then(Value::as_u64);
    let output_tokens = usage.get("output_tokens").and_then(Value::as_u64);
    let cache_read_tokens = usage
        .get("cache_read_tokens")
        .or_else(|| usage.get("cache_read"))
        .and_then(Value::as_u64);
    let cache_creation_tokens = usage
        .get("cache_creation_tokens")
        .or_else(|| usage.get("cache_write"))
        .or_else(|| usage.get("cache_creation"))
        .and_then(Value::as_u64);
    if input_tokens.is_none()
        && output_tokens.is_none()
        && cache_read_tokens.is_none()
        && cache_creation_tokens.is_none()
    {
        return None;
    }
    Some(ProjectedTokenUsage {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_creation_tokens,
    })
}

#[derive(Debug, Default)]
struct UserTextProjection {
    preview: Option<String>,
    context_only: bool,
}

/// Last user Text in `messages`, walking from the tail. Skips pure ToolResult.
pub fn last_user_text_preview(payload: &Value) -> Option<String> {
    last_user_text_projection(payload).preview
}

fn last_user_text_projection(payload: &Value) -> UserTextProjection {
    let request = payload.get("request").unwrap_or(payload);
    let Some(messages) = request.get("messages").and_then(Value::as_array) else {
        return UserTextProjection::default();
    };
    for message in messages.iter().rev() {
        let Some(role) = message.get("role").and_then(Value::as_str) else {
            continue;
        };
        if role != "user" {
            continue;
        }
        if let Some(text) = message.get("content").and_then(Value::as_str) {
            if text.trim().is_empty() {
                continue;
            }
            if is_leading_context_block(text) {
                return UserTextProjection {
                    preview: None,
                    context_only: true,
                };
            }
            return UserTextProjection {
                preview: Some(redact_preview(text)),
                context_only: false,
            };
        }
        let Some(blocks) = message.get("content").and_then(Value::as_array) else {
            continue;
        };
        let mut texts = Vec::new();
        let mut has_text = false;
        let mut has_context = false;
        for (index, block) in blocks.iter().enumerate() {
            let Some(block) = block.as_object() else {
                continue;
            };
            let ty = block.get("type").and_then(Value::as_str).unwrap_or("");
            if ty == "tool_result" || ty == "tool_use" {
                continue;
            }
            if ty == "text" {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    if index == 0 && is_leading_context_block(text) {
                        has_context = true;
                        continue;
                    }
                    if !text.trim().is_empty() {
                        texts.push(text);
                        has_text = true;
                    }
                }
            }
        }
        if has_text {
            return UserTextProjection {
                preview: Some(redact_preview(&texts.join("\n"))),
                context_only: false,
            };
        }
        if has_context {
            return UserTextProjection {
                preview: None,
                context_only: true,
            };
        }
    }
    UserTextProjection::default()
}

fn is_leading_context_block(text: &str) -> bool {
    let trimmed = text.trim_start();
    let Some(rest) = trimmed.strip_prefix(CONTEXT_PREFIX) else {
        return false;
    };
    rest.is_empty() || rest.chars().next().is_some_and(|character| character.is_whitespace())
}

/// Fold counters without keeping event payloads. Used for list summary.
pub fn fold_observation_summary<'a, I>(events: I) -> ObservationSummary
where
    I: IntoIterator<Item = &'a ObservationEvent>,
{
    let mut fold = ObservationSummaryFold::default();
    for event in events {
        fold.observe(event);
    }
    fold.finish()
}

#[derive(Default)]
pub struct ObservationSummaryFold {
    inner: SummaryFold,
}

impl ObservationSummaryFold {
    pub fn observe(&mut self, event: &ObservationEvent) {
        self.inner.observe(event);
    }

    pub fn finish(self) -> ObservationSummary {
        self.inner.finish()
    }
}

#[derive(Default)]
struct SummaryFold {
    turns: std::collections::HashSet<String>,
    calls: std::collections::HashSet<String>,
    tools: std::collections::HashSet<String>,
    max_event_seq: u64,
    min_ts: Option<u64>,
    max_ts: Option<u64>,
    active_duration_ms: u64,
    has_gap: bool,
    turn_ended: std::collections::HashSet<String>,
    request_without_response: std::collections::HashSet<(String, String)>,
    responded: std::collections::HashSet<(String, String)>,
    open_tools: std::collections::HashSet<(String, String)>,
}

impl SummaryFold {
    fn observe(&mut self, event: &ObservationEvent) {
        self.max_event_seq = self.max_event_seq.max(event.event_seq);
        self.min_ts = Some(self.min_ts.map_or(event.timestamp_ms, |ts| ts.min(event.timestamp_ms)));
        self.max_ts = Some(self.max_ts.map_or(event.timestamp_ms, |ts| ts.max(event.timestamp_ms)));
        let ids = ids_from_payload(&event.payload);
        let turn = turn_key(event);
        self.turns.insert(turn.clone());
        match event.event_type.as_str() {
            EVENT_TURN_END => {
                self.turn_ended.insert(turn);
                if let Some(elapsed) = event.payload.get("elapsed_ms").and_then(Value::as_u64) {
                    self.active_duration_ms = self.active_duration_ms.saturating_add(elapsed);
                }
            }
            EVENT_LLM_REQUEST => {
                if let Some(call) = ids.model_call_id.clone() {
                    let key = (turn, call.clone());
                    self.calls.insert(call);
                    if !self.responded.contains(&key) {
                        self.request_without_response.insert(key);
                    }
                }
            }
            EVENT_LLM_RESPONSE => {
                if let Some(call) = ids.model_call_id.clone() {
                    let key = (turn, call.clone());
                    self.calls.insert(call);
                    self.responded.insert(key.clone());
                    self.request_without_response.remove(&key);
                }
            }
            EVENT_TOOL_EXECUTION_STARTED => {
                let call = ids
                    .model_call_id
                    .clone()
                    .unwrap_or_else(|| format!("anon-{}", event.event_seq));
                let tool = tool_call_id_from_event(event, &call);
                self.tools.insert(tool.clone());
                self.open_tools.insert((turn, tool));
            }
            EVENT_TOOL_EXECUTION_COMPLETED
            | EVENT_TOOL_EXECUTION_FAILED
            | EVENT_TOOL_EXECUTION_CANCELLED => {
                let call = ids
                    .model_call_id
                    .clone()
                    .unwrap_or_else(|| format!("anon-{}", event.event_seq));
                let tool = tool_call_id_from_event(event, &call);
                self.tools.insert(tool.clone());
                self.open_tools.remove(&(turn, tool));
            }
            EVENT_OBSERVATION_GAP => {
                self.has_gap = true;
            }
            _ => {}
        }
    }

    fn finish(self) -> ObservationSummary {
        let degraded = self.has_gap
            || self.request_without_response.iter().any(|(turn, _)| self.turn_ended.contains(turn))
            || self.open_tools.iter().any(|(turn, _)| self.turn_ended.contains(turn));
        let wall_span_ms = match (self.min_ts, self.max_ts) {
            (Some(min), Some(max)) if max >= min => Some(max.saturating_sub(min)),
            _ => None,
        };
        ObservationSummary {
            turn_count: self.turns.len() as u64,
            model_call_count: self.calls.len() as u64,
            tool_count: self.tools.len() as u64,
            active_duration_ms: self.active_duration_ms,
            wall_span_ms,
            integrity: if degraded {
                Integrity::Degraded
            } else {
                Integrity::Complete
            },
            coverage: COVERAGE_RETAINED_OBSERVATION_HISTORY.to_owned(),
            max_event_seq: self.max_event_seq,
        }
    }
}

pub fn project_call_detail(
    events: &[ObservationEvent],
    root_turn_id: &str,
    model_call_id: &str,
) -> Option<ProjectedModelCall> {
    project_turn_by_id(events, root_turn_id)?
        .model_calls
        .into_iter()
        .find(|call| call.model_call_id == model_call_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{
        ObservationEvent, ObservationIds, EVENT_LLM_REQUEST, EVENT_LLM_RESPONSE,
        EVENT_TOOL_EXECUTION_FAILED, EVENT_TOOL_EXECUTION_STARTED, EVENT_TURN_END,
        EVENT_TURN_START,
    };

    fn event(event_type: &str, seq: u64, ids: ObservationIds, extra: Value) -> ObservationEvent {
        let mut payload = extra.as_object().cloned().unwrap_or_default();
        payload.insert("ids".into(), serde_json::to_value(ids).unwrap());
        ObservationEvent::new(
            event_type,
            seq,
            "2026-08-18T10:00:00Z",
            1_000 + seq,
            Value::Object(payload),
        )
    }

    fn turn_ids(turn: &str, model_call: &str) -> ObservationIds {
        ObservationIds {
            conversation_id: Some("c1".into()),
            root_turn_id: Some(turn.into()),
            model_call_id: Some(model_call.into()),
            ..ObservationIds::default()
        }
    }

    #[test]
    fn running_without_response_is_complete() {
        let events = vec![event(
            EVENT_LLM_REQUEST,
            1,
            turn_ids("t1", "mc1"),
            serde_json::json!({ "call_kind": "agent_turn" }),
        )];
        let turns = project_turns(&events);
        assert_eq!(turns.len(), 1);
        assert!(!turns[0].interrupted);
        assert_eq!(turns[0].status, ExecutionStatus::Unknown);
        assert_eq!(turns[0].integrity, Integrity::Complete);
        assert_eq!(turns[0].model_calls[0].status, ExecutionStatus::Running);
        assert!(!turns[0].model_calls[0].interrupted);
        assert!(turns[0].model_calls[0].response.is_none());
        assert_eq!(turns[0].model_calls[0].started_at_ms, Some(1_001));
    }

    #[test]
    fn gap_degrades_without_marking_interrupted() {
        let events = vec![
            event(
                EVENT_LLM_REQUEST,
                1,
                turn_ids("t1", "mc1"),
                serde_json::json!({ "call_kind": "agent_turn" }),
            ),
            event(
                EVENT_LLM_RESPONSE,
                2,
                turn_ids("t1", "mc1"),
                serde_json::json!({}),
            ),
            event(
                EVENT_OBSERVATION_GAP,
                3,
                turn_ids("t1", "mc1"),
                serde_json::json!({ "reason": "sink_failed", "from_seq": 2, "to_seq": 4 }),
            ),
        ];
        let turns = project_turns(&events);
        assert_eq!(turns[0].integrity, Integrity::Degraded);
        assert!(!turns[0].interrupted);
        assert_eq!(turns[0].gap_count, 1);
        assert_eq!(turns[0].gaps[0].reason.as_deref(), Some("sink_failed"));
    }

    #[test]
    fn sorts_by_event_seq_not_timestamp() {
        let late = ObservationEvent::new(
            EVENT_LLM_RESPONSE,
            2,
            "2026-08-18T09:00:00Z",
            1,
            serde_json::json!({
                "ids": turn_ids("t1", "mc1")
            }),
        );
        let early = ObservationEvent::new(
            EVENT_LLM_REQUEST,
            1,
            "2026-08-18T12:00:00Z",
            9_999,
            serde_json::json!({
                "ids": turn_ids("t1", "mc1"),
                "call_kind": "agent_turn"
            }),
        );
        let turns = project_turns(&[late, early]);
        assert!(!turns[0].interrupted);
        assert!(turns[0].model_calls[0].request.is_some());
        assert!(turns[0].model_calls[0].response.is_some());
    }

    #[test]
    fn complete_request_response_is_complete() {
        let events = vec![
            event(EVENT_LLM_REQUEST, 1, turn_ids("t1", "mc1"), serde_json::json!({})),
            event(EVENT_LLM_RESPONSE, 2, turn_ids("t1", "mc1"), serde_json::json!({})),
        ];
        let turns = project_turns(&events);
        assert_eq!(turns[0].integrity, Integrity::Complete);
        assert!(!turns[0].interrupted);
    }

    #[test]
    fn strip_projected_turn_payloads_keeps_counts() {
        let events = vec![
            event(EVENT_LLM_REQUEST, 1, turn_ids("t1", "mc1"), serde_json::json!({})),
            event(EVENT_LLM_RESPONSE, 2, turn_ids("t1", "mc1"), serde_json::json!({})),
        ];
        let mut turns = project_turns(&events);
        assert!(turns[0].model_calls[0].request.is_some());
        strip_projected_turn_payloads(&mut turns[0]);
        assert!(turns[0].model_calls[0].request.is_none());
        assert!(turns[0].model_calls[0].response.is_none());
        assert_eq!(turns[0].model_calls.len(), 1);
    }

    #[test]
    fn strip_keeps_request_and_response_summaries() {
        let events = vec![
            event(
                EVENT_LLM_REQUEST,
                1,
                turn_ids("t1", "mc1"),
                serde_json::json!({
                    "request": {
                        "model": "test-model",
                        "system": "be brief",
                        "messages": [
                            { "role": "user", "content": [{ "type": "text", "text": "hello" }] },
                            { "role": "assistant", "content": [{ "type": "text", "text": "hi" }] }
                        ],
                        "tools": [{ "name": "bash" }, { "name": "read" }]
                    }
                }),
            ),
            event(
                EVENT_LLM_RESPONSE,
                2,
                turn_ids("t1", "mc1"),
                serde_json::json!({
                    "text": "done with the task",
                    "thinking": "check tools",
                    "tool_use": [{ "name": "bash" }],
                    "elapsed_ms": 1200,
                    "ttft_ms": 80,
                    "stop_reason": "tool_use"
                }),
            ),
        ];
        let mut turns = project_turns(&events);
        let call = &turns[0].model_calls[0];
        assert!(call.request.is_some());
        assert!(call.response.is_some());
        let request = call.request_summary.clone().expect("request summary");
        assert_eq!(request.model.as_deref(), Some("test-model"));
        assert!(request.has_system);
        assert_eq!(request.message_count, 2);
        assert_eq!(request.tool_definition_count, 2);
        let response = call.response_summary.clone().expect("response summary");
        assert!(response.has_text);
        assert!(response.has_thinking);
        assert_eq!(response.tool_use_count, 1);
        assert_eq!(response.elapsed_ms, Some(1200));
        assert_eq!(response.ttft_ms, Some(80));
        assert_eq!(response.stop_reason.as_deref(), Some("tool_use"));
        assert_eq!(response.text_preview.as_deref(), Some("done with the task"));

        strip_projected_turn_payloads(&mut turns[0]);
        let call = &turns[0].model_calls[0];
        assert!(call.request.is_none());
        assert!(call.response.is_none());
        assert_eq!(call.request_summary.as_ref().map(|s| s.model.as_deref()), Some(Some("test-model")));
        assert_eq!(
            call.response_summary.as_ref().map(|s| s.tool_use_count),
            Some(1)
        );
    }

    #[test]
    fn strip_keeps_tool_argument_preview() {
        let events = vec![
            event(EVENT_LLM_REQUEST, 1, turn_ids("t1", "mc1"), serde_json::json!({})),
            event(
                EVENT_TOOL_EXECUTION_STARTED,
                2,
                turn_ids("t1", "mc1"),
                serde_json::json!({
                    "tool_call_id": "tool-1",
                    "name": "web_search",
                    "arguments": { "query": "chengdu weather" }
                }),
            ),
        ];
        let mut turns = project_turns(&events);
        let tool = &turns[0].model_calls[0].tools[0];
        assert!(tool.started.is_some());
        assert_eq!(
            tool.argument_preview.as_deref(),
            Some("{\"query\":\"chengdu weather\"}")
        );
        strip_projected_turn_payloads(&mut turns[0]);
        let tool = &turns[0].model_calls[0].tools[0];
        assert!(tool.started.is_none());
        assert_eq!(
            tool.argument_preview.as_deref(),
            Some("{\"query\":\"chengdu weather\"}")
        );
    }

    #[test]
    fn argument_preview_does_not_invent_omitted_arguments() {
        let events = vec![event(
            EVENT_TOOL_EXECUTION_STARTED,
            1,
            turn_ids("t1", "mc1"),
            serde_json::json!({
                "tool_call_id": "tool-1",
                "name": "bash",
                "arguments": { "omitted_reason": "event_size_limit" }
            }),
        )];
        let tool = &project_turns(&events)[0].model_calls[0].tools[0];
        assert!(tool.argument_preview.is_none());
    }

    #[test]
    fn summaries_do_not_invent_omitted_counts() {
        let events = vec![event(
            EVENT_LLM_REQUEST,
            1,
            turn_ids("t1", "mc1"),
            serde_json::json!({
                "request": {
                    "model": "m",
                    "system": { "omitted_reason": "event_size_limit" },
                    "messages": { "omitted_reason": "event_size_limit" },
                    "tools": { "omitted_reason": "event_size_limit" }
                }
            }),
        )];
        let summary = project_turns(&events)[0].model_calls[0]
            .request_summary
            .clone()
            .expect("summary");
        assert!(!summary.has_system);
        assert_eq!(summary.message_count, 0);
        assert_eq!(summary.tool_definition_count, 0);
        assert!(summary.system_omitted);
        assert!(summary.messages_omitted);
        assert!(summary.tools_omitted);
    }

    #[test]
    fn omitted_response_fields_are_flagged_without_inventing_text() {
        let events = vec![event(
            EVENT_LLM_RESPONSE,
            1,
            turn_ids("t1", "mc1"),
            serde_json::json!({
                "text": { "omitted_reason": "event_size_limit" },
                "thinking": { "omitted_reason": "event_size_limit" },
                "tool_use": { "omitted_reason": "event_size_limit" }
            }),
        )];
        let summary = project_turns(&events)[0].model_calls[0]
            .response_summary
            .clone()
            .expect("summary");
        assert!(!summary.has_text);
        assert!(!summary.has_thinking);
        assert!(summary.text_omitted);
        assert!(summary.thinking_omitted);
        assert_eq!(summary.tool_use_count, 0);
        assert!(summary.text_preview.is_none());
    }

    #[test]
    fn missing_tool_call_id_stays_paired_per_model_call() {
        let events = vec![
            event(
                EVENT_TOOL_EXECUTION_STARTED,
                3,
                turn_ids("t1", "mc1"),
                serde_json::json!({ "name": "bash" }),
            ),
            event(
                EVENT_TOOL_EXECUTION_COMPLETED,
                4,
                turn_ids("t1", "mc1"),
                serde_json::json!({ "tool_call_id": "", "name": "bash" }),
            ),
        ];
        let tools = &project_turns(&events)[0].model_calls[0].tools;
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool_call_id, "anon-mc1");
        assert!(tools[0].started.is_some());
        assert!(tools[0].completed.is_some());
    }

    #[test]
    fn tool_failure_is_failed_and_complete() {
        let events = vec![
            event(EVENT_LLM_REQUEST, 1, turn_ids("t1", "mc1"), serde_json::json!({})),
            event(EVENT_LLM_RESPONSE, 2, turn_ids("t1", "mc1"), serde_json::json!({})),
            event(
                EVENT_TOOL_EXECUTION_STARTED,
                3,
                turn_ids("t1", "mc1"),
                serde_json::json!({ "tool_call_id": "tool-1", "name": "bash" }),
            ),
            event(
                EVENT_TOOL_EXECUTION_FAILED,
                4,
                turn_ids("t1", "mc1"),
                serde_json::json!({ "tool_call_id": "tool-1", "name": "bash" }),
            ),
        ];
        let turns = project_turns(&events);
        assert_eq!(turns[0].model_calls[0].status, ExecutionStatus::Failed);
        assert_eq!(turns[0].integrity, Integrity::Complete);
        assert!(!turns[0].interrupted);
    }

    #[test]
    fn turn_end_without_response_is_interrupted_and_degraded() {
        let events = vec![
            event(EVENT_LLM_REQUEST, 1, turn_ids("t1", "mc1"), serde_json::json!({})),
            event(
                EVENT_TURN_END,
                2,
                turn_ids("t1", "mc1"),
                serde_json::json!({ "status": "completed" }),
            ),
        ];
        let turns = project_turns(&events);
        assert_eq!(turns[0].status, ExecutionStatus::Completed);
        assert_eq!(turns[0].model_calls[0].status, ExecutionStatus::Interrupted);
        assert!(turns[0].interrupted);
        assert_eq!(turns[0].integrity, Integrity::Degraded);
    }

    #[test]
    fn completed_call_does_not_wait_for_turn_end() {
        let events = vec![
            event(EVENT_LLM_REQUEST, 1, turn_ids("t1", "mc1"), serde_json::json!({})),
            event(EVENT_LLM_RESPONSE, 2, turn_ids("t1", "mc1"), serde_json::json!({})),
            event(EVENT_LLM_REQUEST, 3, turn_ids("t1", "mc2"), serde_json::json!({})),
        ];
        let turns = project_turns(&events);
        assert_eq!(turns[0].model_calls[0].status, ExecutionStatus::Completed);
        assert_eq!(turns[0].model_calls[1].status, ExecutionStatus::Running);
        assert_eq!(turns[0].integrity, Integrity::Complete);
        assert_eq!(turns[0].status, ExecutionStatus::Unknown);
    }

    #[test]
    fn preview_uses_last_user_text_not_first() {
        let events = vec![event(
            EVENT_LLM_REQUEST,
            1,
            turn_ids("t1", "mc1"),
            serde_json::json!({
                "request": {
                    "messages": [
                        { "role": "user", "content": [{ "type": "text", "text": "first" }] },
                        { "role": "assistant", "content": [{ "type": "text", "text": "ok" }] },
                        { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": "x", "content": "ignored", "is_error": false }] },
                        { "role": "user", "content": [{ "type": "text", "text": "latest question" }] }
                    ]
                }
            }),
        )];
        let turns = project_turns(&events);
        assert_eq!(turns[0].prompt_preview.as_deref(), Some("latest question"));
    }

    #[test]
    fn preview_fallback_skips_leading_context_block() {
        let events = vec![event(
            EVENT_LLM_REQUEST,
            1,
            turn_ids("t1", "mc1"),
            serde_json::json!({
                "request": {
                    "messages": [
                        {
                            "role": "user",
                            "content": [
                                { "type": "text", "text": "[Context]\nCurrent date: 2026-08-21" },
                                { "type": "text", "text": "66" }
                            ]
                        }
                    ]
                }
            }),
        )];
        let turns = project_turns(&events);
        assert_eq!(turns[0].prompt_preview.as_deref(), Some("66"));
        assert!(!turns[0].prompt_preview_context_only);
    }

    #[test]
    fn context_only_message_has_no_fallback_preview() {
        let events = vec![event(
            EVENT_LLM_REQUEST,
            1,
            turn_ids("t1", "mc1"),
            serde_json::json!({
                "request": {
                    "messages": [{
                        "role": "user",
                        "content": [{ "type": "text", "text": "[Context]\nCurrent date: 2026-08-21" }]
                    }]
                }
            }),
        )];
        let turns = project_turns(&events);
        assert_eq!(turns[0].prompt_preview, None);
        assert!(turns[0].prompt_preview_context_only);
    }

    #[test]
    fn preview_prefers_turn_start_prompt() {
        let events = vec![
            event(
                EVENT_TURN_START,
                1,
                ObservationIds {
                    conversation_id: Some("c1".into()),
                    root_turn_id: Some("t1".into()),
                    ..ObservationIds::default()
                },
                serde_json::json!({ "prompt_preview": "send text" }),
            ),
            event(
                EVENT_LLM_REQUEST,
                2,
                turn_ids("t1", "mc1"),
                serde_json::json!({
                    "request": {
                        "messages": [
                            { "role": "user", "content": [{ "type": "text", "text": "other" }] }
                        ]
                    }
                }),
            ),
        ];
        let turns = project_turns(&events);
        assert_eq!(turns[0].prompt_preview.as_deref(), Some("send text"));
        assert!(turns[0].has_turn_start);
        assert!(!turns[0].has_turn_end);
        assert_eq!(turns[0].status, ExecutionStatus::Running);
        assert_eq!(turns[0].max_event_seq, 2);
    }

    #[test]
    fn context_turn_start_preview_falls_back_to_real_user_text() {
        let events = vec![
            event(
                EVENT_TURN_START,
                1,
                ObservationIds {
                    conversation_id: Some("c1".into()),
                    root_turn_id: Some("t1".into()),
                    ..ObservationIds::default()
                },
                serde_json::json!({
                    "prompt_preview": "[Context]\nCurrent date: 2026-08-21"
                }),
            ),
            event(
                EVENT_LLM_REQUEST,
                2,
                turn_ids("t1", "mc1"),
                serde_json::json!({
                    "request": {
                        "messages": [{
                            "role": "user",
                            "content": [
                                { "type": "text", "text": "[Context]\nCurrent date: 2026-08-21" },
                                { "type": "text", "text": "66" }
                            ]
                        }]
                    }
                }),
            ),
        ];
        let turns = project_turns(&events);
        assert_eq!(turns[0].prompt_preview.as_deref(), Some("66"));
        assert!(!turns[0].prompt_preview_context_only);
    }

    #[test]
    fn context_turn_start_without_request_preview_stays_blank() {
        let events = vec![event(
            EVENT_TURN_START,
            1,
            ObservationIds {
                conversation_id: Some("c1".into()),
                root_turn_id: Some("t1".into()),
                ..ObservationIds::default()
            },
            serde_json::json!({
                "prompt_preview": "[Context]\nCurrent date: 2026-08-21"
            }),
        )];
        let turns = project_turns(&events);
        assert_eq!(turns[0].prompt_preview, None);
        assert!(turns[0].prompt_preview_context_only);
    }

    #[test]
    fn fold_summary_uses_turn_end_elapsed_not_wall() {
        let events = vec![
            event(
                EVENT_TURN_START,
                1,
                ObservationIds {
                    conversation_id: Some("c1".into()),
                    root_turn_id: Some("t1".into()),
                    ..ObservationIds::default()
                },
                serde_json::json!({}),
            ),
            event(EVENT_LLM_REQUEST, 2, turn_ids("t1", "mc1"), serde_json::json!({})),
            event(EVENT_LLM_RESPONSE, 3, turn_ids("t1", "mc1"), serde_json::json!({})),
            event(
                EVENT_TURN_END,
                4,
                ObservationIds {
                    conversation_id: Some("c1".into()),
                    root_turn_id: Some("t1".into()),
                    ..ObservationIds::default()
                },
                serde_json::json!({ "status": "completed", "elapsed_ms": 42 }),
            ),
        ];
        let summary = fold_observation_summary(events.iter());
        assert_eq!(summary.turn_count, 1);
        assert_eq!(summary.model_call_count, 1);
        assert_eq!(summary.active_duration_ms, 42);
        assert_eq!(summary.coverage, COVERAGE_RETAINED_OBSERVATION_HISTORY);
        assert_eq!(summary.max_event_seq, 4);
        assert_eq!(summary.integrity, Integrity::Complete);
    }

    #[test]
    fn header_package_omits_request_bodies() {
        let mut events = Vec::new();
        for index in 0..50 {
            let call = format!("mc-{index}");
            events.push(event(
                EVENT_LLM_REQUEST,
                (index * 2 + 1) as u64,
                turn_ids("t1", &call),
                serde_json::json!({
                    "request": {
                        "system": "x".repeat(4000),
                        "messages": [{ "role": "user", "content": [{ "type": "text", "text": "q" }] }],
                        "tools": [{ "name": "bash", "input_schema": { "huge": "y".repeat(4000) } }]
                    }
                }),
            ));
            events.push(event(
                EVENT_LLM_RESPONSE,
                (index * 2 + 2) as u64,
                turn_ids("t1", &call),
                serde_json::json!({
                    "text": "z".repeat(2000),
                    "usage": { "input_tokens": 3, "output_tokens": 4 }
                }),
            ));
        }
        let mut turn = project_turns(&events).remove(0);
        let full = serde_json::to_vec(&turn).unwrap();
        strip_projected_turn_payloads(&mut turn);
        let headers = serde_json::to_vec(&turn).unwrap();
        assert!(headers.len() * 8 < full.len(), "headers {} vs full {}", headers.len(), full.len());
        let encoded = String::from_utf8(headers).unwrap();
        assert!(!encoded.contains("xxxx"));
        assert!(!encoded.contains("input_schema"));
        assert_eq!(turn.model_calls.len(), 50);
        assert_eq!(
            turn.model_calls[0].usage,
            Some(ProjectedTokenUsage {
                input_tokens: Some(3),
                output_tokens: Some(4),
                cache_read_tokens: None,
                cache_creation_tokens: None,
            })
        );
        assert!(turn.model_calls[0].request.is_none());
        let detail = project_call_detail(&events, "t1", "mc-0").unwrap();
        assert!(detail.request.is_some());
        assert!(serde_json::to_string(&detail).unwrap().contains("input_schema"));
    }

    #[test]
    fn observation_usage_does_not_fold_cache_into_input() {
        let events = vec![event(
            EVENT_LLM_RESPONSE,
            1,
            turn_ids("t1", "mc1"),
            serde_json::json!({
                "usage": {
                    "input_tokens": 10,
                    "output_tokens": 2,
                    "cache_read_tokens": 4,
                    "cache_creation_tokens": 1
                }
            }),
        )];
        let usage = project_turns(&events)[0].model_calls[0]
            .usage
            .clone()
            .expect("usage");
        assert_eq!(usage.input_tokens, Some(10));
        assert_eq!(usage.cache_read_tokens, Some(4));
        assert_eq!(usage.cache_creation_tokens, Some(1));
        assert_ne!(usage.input_tokens, Some(14));
    }
}
