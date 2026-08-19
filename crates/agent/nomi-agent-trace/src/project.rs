//! Project JSONL events into per-turn workflow views. Sort only by `event_seq`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::event::{
    ids_from_payload, ExecutionStatus, Integrity, ObservationEvent, ObservationScope,
    EVENT_LLM_REQUEST, EVENT_LLM_RESPONSE, EVENT_OBSERVATION_GAP, EVENT_TOOL_EXECUTION_CANCELLED,
    EVENT_TOOL_EXECUTION_COMPLETED, EVENT_TOOL_EXECUTION_FAILED, EVENT_TOOL_EXECUTION_STARTED,
    EVENT_TURN_END, EVENT_TURN_START,
};
use crate::redact::redact_preview;

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
                let tool_call_id = event
                    .payload
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
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
    let prompt_preview = turn_start_preview.or_else(|| {
        latest_request.and_then(|event| last_user_text_preview(&event.payload))
    });
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

/// Last user Text in `messages`, walking from the tail. Skips pure ToolResult.
pub fn last_user_text_preview(payload: &Value) -> Option<String> {
    let request = payload.get("request").unwrap_or(payload);
    let messages = request.get("messages")?.as_array()?;
    for message in messages.iter().rev() {
        let role = message.get("role").and_then(Value::as_str)?;
        if role != "user" {
            continue;
        }
        if let Some(text) = message.get("content").and_then(Value::as_str) {
            if !text.trim().is_empty() {
                return Some(redact_preview(text));
            }
            continue;
        }
        let Some(blocks) = message.get("content").and_then(Value::as_array) else {
            continue;
        };
        let mut texts = Vec::new();
        let mut has_text = false;
        for block in blocks {
            let ty = block.get("type").and_then(Value::as_str).unwrap_or("");
            if ty == "tool_result" || ty == "tool_use" {
                continue;
            }
            if ty == "text" {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    if !text.trim().is_empty() {
                        texts.push(text);
                        has_text = true;
                    }
                }
            }
        }
        if has_text {
            return Some(redact_preview(&texts.join("\n")));
        }
    }
    None
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
                if let Some(tool) = event
                    .payload
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
                {
                    self.tools.insert(tool.clone());
                    self.open_tools.insert((turn, tool));
                }
            }
            EVENT_TOOL_EXECUTION_COMPLETED
            | EVENT_TOOL_EXECUTION_FAILED
            | EVENT_TOOL_EXECUTION_CANCELLED => {
                if let Some(tool) = event
                    .payload
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
                {
                    self.tools.insert(tool.clone());
                    self.open_tools.remove(&(turn, tool));
                }
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
