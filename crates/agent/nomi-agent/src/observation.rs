//! Explicit observation wrapper around `LlmProvider::stream`.
//!
//! Does not change `nomi-providers`. Emit failures only warn (and may write
//! `observation/gap`); they never abort an agent turn.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use nomi_agent_trace::{
    capture_borrowed, omitted_binary_payload, redact_capture, redact_preview,
    request_prefix_fingerprint,
    ExecutionStatus, ObservationEvent,
    ObservationIds, ObservationRecorder, ObservationScope, RecorderError, EVENT_LLM_REQUEST,
    EVENT_LLM_RESPONSE, EVENT_OBSERVATION_GAP, EVENT_TOOL_EXECUTION_CANCELLED,
    EVENT_TOOL_EXECUTION_COMPLETED, EVENT_TOOL_EXECUTION_FAILED, EVENT_TOOL_EXECUTION_STARTED,
    EVENT_HARNESS_FINISH, EVENT_HARNESS_HARD_STOP, EVENT_HARNESS_NUDGE, EVENT_HARNESS_PROFILE,
    EVENT_HARNESS_PROGRESS, EVENT_HARNESS_RESET, EVENT_TURN_END, EVENT_TURN_START,
    MAX_PREVIEW_CHARS, OMITTED_REASON_INPUT_SCHEMA,
};
use nomi_providers::{LlmProvider, ProviderError};
use nomi_types::llm::{LlmEvent, LlmRequest, ThinkingConfig};
use nomi_types::message::{ContentBlock, Message, Role, StopReason, TokenUsage};
use nomi_types::tool::{ToolDef, ToolImage};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::tool_execution::ToolCallTiming;

const TELEMETRY_EVENT_TOOL_EXECUTED: &str = "tool_executed";
const TELEMETRY_EVENT_LLM_REQUEST: &str = "llm_request";
const TELEMETRY_MAX_EVENT_ID: usize = 128;
const TELEMETRY_MAX_PROP: usize = 256;

/// Summarized observation event for the product telemetry warehouse.
/// Nested JSONL payloads never leave the machine; only these scalars do.
#[derive(Debug, Clone)]
pub struct ObservationTelemetryRecord {
    pub event_id: String,
    pub name: String,
    pub occurred_at: String,
    pub properties: BTreeMap<String, Value>,
}

pub type ObservationTelemetryHook =
    Arc<dyn Fn(ObservationTelemetryRecord) + Send + Sync>;

static OBSERVATION_TELEMETRY_HOOK: OnceLock<ObservationTelemetryHook> = OnceLock::new();

/// Install the cloud uploader. The composition root calls this once at boot.
pub fn set_observation_telemetry_hook(hook: ObservationTelemetryHook) {
    if OBSERVATION_TELEMETRY_HOOK.set(hook).is_err() {
        tracing::warn!("observation telemetry hook already installed");
    }
}

fn enqueue_observation_telemetry(record: ObservationTelemetryRecord) {
    if let Some(hook) = OBSERVATION_TELEMETRY_HOOK.get() {
        hook(record);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PrefixChain {
    system: String,
    tools: String,
    messages: Vec<String>,
}

pub struct ObservationSession {
    recorder: Arc<ObservationRecorder>,
    ids: Mutex<ObservationIds>,
    last_model_call_id: Mutex<Option<String>>,
    /// `tool_call_id` → the model call that issued the tool, captured at start.
    tool_parents: Mutex<HashMap<String, String>>,
    /// Wall-clock start for tools that finish without an execution timing entry.
    tool_started_at: Mutex<HashMap<String, u64>>,
    /// Last SessionWorkflow request chain, used to classify prefix-cache breaks.
    last_prefix: Mutex<Option<PrefixChain>>,
    /// End of the last phase (turn start, model response, tool round), so the
    /// next `llm/request` can report how long local preparation took.
    phase_boundary: Mutex<Option<Instant>>,
}

impl ObservationSession {
    pub fn new(recorder: Arc<ObservationRecorder>) -> Arc<Self> {
        Arc::new(Self {
            recorder,
            ids: Mutex::new(ObservationIds::default()),
            last_model_call_id: Mutex::new(None),
            tool_parents: Mutex::new(HashMap::new()),
            tool_started_at: Mutex::new(HashMap::new()),
            last_prefix: Mutex::new(None),
            phase_boundary: Mutex::new(None),
        })
    }

    fn wall_clock_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0)
    }

    fn mark_phase_boundary(&self) {
        *self
            .phase_boundary
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
    }

    fn pre_provider_ms(&self) -> Option<u64> {
        self.phase_boundary
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .map(|at| u64::try_from(at.elapsed().as_millis()).unwrap_or(u64::MAX))
    }

    pub fn recorder(&self) -> &Arc<ObservationRecorder> {
        &self.recorder
    }

    pub fn bind_ids(&self, ids: ObservationIds) {
        self.bind_ids_with_preview(ids, None);
    }

    pub fn bind_ids_with_preview(&self, mut ids: ObservationIds, prompt_preview: Option<&str>) {
        ids.model_call_id = None;
        let (same_turn, conversation_changed) = {
            let mut current = self.ids.lock().unwrap_or_else(|e| e.into_inner());
            let conversation_changed = current.conversation_id != ids.conversation_id;
            let same_turn = current.root_turn_id.is_some()
                && current.root_turn_id == ids.root_turn_id
                && current.conversation_id == ids.conversation_id;
            *current = ids;
            (same_turn, conversation_changed)
        };
        if !same_turn {
            self.mark_phase_boundary();
            *self
                .last_model_call_id
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = None;
            self.tool_parents
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clear();
        }
        if conversation_changed {
            *self
                .last_prefix
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = None;
        }
        self.emit_turn_start_once(prompt_preview);
    }

    fn emit_turn_start_once(&self, prompt_preview: Option<&str>) {
        let ids = self.ids();
        if !self.recorder.claim_turn_start(&ids) {
            return;
        }
        let preview = prompt_preview
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(redact_preview);
        observe_with_model_call(
            self,
            EVENT_TURN_START,
            json!({ "prompt_preview": preview }),
            None,
        );
    }

    pub fn emit_turn_end(
        &self,
        status: ExecutionStatus,
        elapsed_ms: u64,
        stop_reason: Option<&str>,
        usage: Option<Value>,
        error: Option<&str>,
    ) {
        let ids = self.ids();
        if !self.recorder.claim_turn_end(&ids) {
            return;
        }
        let redacted_error = error.map(|e| {
            nomi_agent_trace::truncate_chars(&redact_preview(e), 1000)
        });
        observe_with_model_call(
            self,
            EVENT_TURN_END,
            json!({
                "status": status,
                "elapsed_ms": elapsed_ms,
                "stop_reason": stop_reason,
                "usage": usage,
                "error": redacted_error,
            }),
            None,
        );
    }

    pub fn ids(&self) -> ObservationIds {
        self.ids.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn last_model_call_id(&self) -> Option<String> {
        self.last_model_call_id
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn begin_model_call(&self) -> String {
        let id = format!("mc-{}", uuid::Uuid::now_v7());
        *self
            .last_model_call_id
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(id.clone());
        let mut ids = self.ids.lock().unwrap_or_else(|e| e.into_inner());
        ids.model_call_id = Some(id.clone());
        id
    }

    pub fn emit(
        &self,
        event_type: &str,
        payload: Value,
    ) -> Result<Option<ObservationEvent>, RecorderError> {
        self.emit_with_model_call(event_type, payload, None)
    }

    fn emit_with_model_call(
        &self,
        event_type: &str,
        payload: Value,
        model_call_id: Option<String>,
    ) -> Result<Option<ObservationEvent>, RecorderError> {
        let mut ids = self.ids();
        if let Some(model_call_id) = model_call_id {
            ids.model_call_id = Some(model_call_id);
        }
        self.recorder.emit(event_type, &ids, payload)
    }

    pub fn emit_gap(
        &self,
        reason: &str,
        from_seq: Option<u64>,
        to_seq: Option<u64>,
        lost_count: Option<u64>,
    ) -> Result<Option<ObservationEvent>, RecorderError> {
        let ids = self.ids();
        self.recorder
            .emit_gap(&ids, reason, from_seq, to_seq, lost_count)
    }

    pub fn emit_tool_started(&self, tool_call_id: &str, name: &str, arguments: &Value) {
        let parent = self.last_model_call_id();
        if let Some(model_call_id) = parent.clone() {
            self.tool_parents
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(tool_call_id.to_owned(), model_call_id);
        }
        let started_at_ms = Self::wall_clock_ms();
        self.tool_started_at
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(tool_call_id.to_owned(), started_at_ms);
        observe_with_model_call(
            self,
            EVENT_TOOL_EXECUTION_STARTED,
            json!({
                "tool_call_id": tool_call_id,
                "name": name,
                "arguments": arguments,
                "started_at_ms": started_at_ms,
            }),
            parent,
        );
    }

    pub fn emit_tool_finished(
        &self,
        tool_call_id: &str,
        name: &str,
        is_error: bool,
        result: &str,
        timing: Option<ToolCallTiming>,
        round_wall_ms: Option<u64>,
    ) {
        let event_type = if is_error {
            EVENT_TOOL_EXECUTION_FAILED
        } else {
            EVENT_TOOL_EXECUTION_COMPLETED
        };
        let parent = self
            .tool_parents
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(tool_call_id);
        let mut payload = json!({
            "tool_call_id": tool_call_id,
            "name": name,
            "is_error": is_error,
            "result": result,
        });
        let mut timing = timing;
        {
            let mut starts = self
                .tool_started_at
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if timing.is_some() {
                starts.remove(tool_call_id);
            } else if let Some(started_at_ms) = starts.remove(tool_call_id) {
                let completed_at_ms = Self::wall_clock_ms();
                let duration_ms = completed_at_ms.saturating_sub(started_at_ms);
                timing = Some(ToolCallTiming {
                    duration_ms,
                    duration_us: duration_ms.saturating_mul(1000),
                    started_at_ms,
                    completed_at_ms,
                    phases: Vec::new(),
                });
            }
        }
        if let Some(timing) = &timing {
            payload["duration_ms"] = json!(timing.duration_ms);
            payload["duration_us"] = json!(timing.duration_us);
            payload["started_at_ms"] = json!(timing.started_at_ms);
            payload["completed_at_ms"] = json!(timing.completed_at_ms);
            if !timing.phases.is_empty() {
                payload["phases"] = timing
                    .phases
                    .iter()
                    .map(|phase| json!({ "name": phase.name, "us": phase.micros }))
                    .collect();
            }
        }
        if let Some(round_wall_ms) = round_wall_ms {
            payload["round_wall_ms"] = json!(round_wall_ms);
        }
        observe_with_model_call(self, event_type, payload, parent);
        self.mark_phase_boundary();
        self.emit_tool_telemetry(
            tool_call_id,
            name,
            if is_error { "failed" } else { "completed" },
            timing,
        );
    }

    pub fn emit_tool_cancelled(&self, tool_call_id: &str, name: &str) {
        self.tool_started_at
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(tool_call_id);
        let parent = self
            .tool_parents
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(tool_call_id);
        observe_with_model_call(
            self,
            EVENT_TOOL_EXECUTION_CANCELLED,
            json!({
                "tool_call_id": tool_call_id,
                "name": name,
            }),
            parent,
        );
        self.mark_phase_boundary();
        self.emit_tool_telemetry(tool_call_id, name, "cancelled", None);
    }

    pub fn emit_harness_profile(
        &self,
        profile: &str,
        active: bool,
        config: Option<Value>,
    ) {
        let mut payload = json!({
            "profile": profile,
            "active": active,
        });
        if let Some(config) = config {
            payload["config"] = config;
        }
        let _ = self.emit(EVENT_HARNESS_PROFILE, payload);
    }

    pub fn emit_harness_reset(&self, profile: &str, reason: &str) {
        let _ = self.emit(
            EVENT_HARNESS_RESET,
            json!({
                "profile": profile,
                "reason": reason,
            }),
        );
    }

    pub fn emit_harness_progress(
        &self,
        profile: &str,
        progress_action: &str,
        recon_only: bool,
        parent_tool_count: usize,
        counters: Value,
    ) {
        let _ = self.emit(
            EVENT_HARNESS_PROGRESS,
            json!({
                "profile": profile,
                "progress_action": progress_action,
                "recon_only": recon_only,
                "parent_tool_count": parent_tool_count,
                "counters": counters,
            }),
        );
    }

    pub fn emit_harness_nudge(
        &self,
        profile: &str,
        source: &str,
        text: &str,
        hard_stop: bool,
    ) {
        let preview = redact_preview(text);
        let _ = self.emit(
            EVENT_HARNESS_NUDGE,
            json!({
                "profile": profile,
                "source": source,
                "text_preview": preview,
                "hard_stop": hard_stop,
            }),
        );
    }

    pub fn emit_harness_hard_stop(&self, profile: &str, reason: &str, counters: Value) {
        let preview = redact_preview(reason);
        let _ = self.emit(
            EVENT_HARNESS_HARD_STOP,
            json!({
                "profile": profile,
                "reason_preview": preview,
                "counters": counters,
            }),
        );
    }

    pub fn emit_harness_finish(
        &self,
        profile: &str,
        decision: &str,
        counters: Value,
        nudge_preview: Option<&str>,
    ) {
        let mut payload = json!({
            "profile": profile,
            "decision": decision,
            "counters": counters,
        });
        if let Some(text) = nudge_preview {
            payload["nudge_preview"] = json!(redact_preview(text));
        }
        let _ = self.emit(EVENT_HARNESS_FINISH, payload);
    }

    fn emit_tool_telemetry(
        &self,
        tool_call_id: &str,
        name: &str,
        outcome: &str,
        timing: Option<ToolCallTiming>,
    ) {
        let ids = self.ids();
        if !observation_telemetry_eligible(&ids) {
            return;
        }
        let tool_call_id = tool_call_id.trim();
        if tool_call_id.is_empty() {
            return;
        }
        let mut properties = BTreeMap::new();
        insert_telemetry_str(&mut properties, "feature", "conversation");
        if let Some(session_id) = nonempty(ids.conversation_id.as_deref()) {
            insert_telemetry_str(&mut properties, "session_id", session_id);
        }
        insert_telemetry_str(&mut properties, "tool_name", name);
        insert_telemetry_str(&mut properties, "outcome", outcome);
        if let Some(kind) = nonempty(ids.session_kind.as_deref()) {
            insert_telemetry_str(&mut properties, "session_kind", kind);
        }
        if let Some(timing) = &timing {
            properties.insert("duration_ms".into(), json!(timing.duration_ms));
            properties.insert("duration_us".into(), json!(timing.duration_us));
        }
        let occurred_at = timing
            .as_ref()
            .map(|timing| rfc3339_millis(timing.completed_at_ms))
            .unwrap_or_else(rfc3339_now);
        enqueue_observation_telemetry(ObservationTelemetryRecord {
            event_id: clip_event_id(format!("obs:tool:{tool_call_id}")),
            name: TELEMETRY_EVENT_TOOL_EXECUTED.into(),
            occurred_at,
            properties,
        });
    }

    fn emit_llm_request_telemetry(
        &self,
        model_call_id: &str,
        call_kind: &str,
        scope: ObservationScope,
        request: &LlmRequest,
        fingerprint: &Value,
    ) {
        if scope != ObservationScope::SessionWorkflow {
            return;
        }
        let ids = self.ids();
        if !observation_telemetry_eligible(&ids) {
            return;
        }
        let Some(chain) = parse_prefix_chain(fingerprint) else {
            return;
        };
        let (prefix_break, prefix_break_index) = {
            let mut last = self
                .last_prefix
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let classified = classify_prefix_break(last.as_ref(), &chain);
            *last = Some(chain.clone());
            classified
        };
        let mut properties = BTreeMap::new();
        insert_telemetry_str(&mut properties, "feature", "conversation");
        if let Some(session_id) = nonempty(ids.conversation_id.as_deref()) {
            insert_telemetry_str(&mut properties, "session_id", session_id);
        }
        insert_telemetry_str(&mut properties, "llm_model", &request.model);
        insert_telemetry_str(&mut properties, "system_hash", &chain.system);
        insert_telemetry_str(&mut properties, "tools_hash", &chain.tools);
        properties.insert("message_count".into(), json!(chain.messages.len() as i64));
        insert_telemetry_str(&mut properties, "prefix_break", prefix_break);
        properties.insert("prefix_break_index".into(), json!(prefix_break_index));
        insert_telemetry_str(&mut properties, "call_kind", call_kind);
        if let Some(kind) = nonempty(ids.session_kind.as_deref()) {
            insert_telemetry_str(&mut properties, "session_kind", kind);
        }
        enqueue_observation_telemetry(ObservationTelemetryRecord {
            event_id: clip_event_id(format!("obs:llm:{model_call_id}")),
            name: TELEMETRY_EVENT_LLM_REQUEST.into(),
            occurred_at: rfc3339_now(),
            properties,
        });
    }
}

/// Stream a canonical request and record `llm/request` + `llm/response`.
pub async fn stream_llm(
    provider: &dyn LlmProvider,
    request: &LlmRequest,
    observer: Option<Arc<ObservationSession>>,
    call_kind: &str,
    scope: ObservationScope,
) -> Result<mpsc::Receiver<LlmEvent>, ProviderError> {
    let Some(session) = observer else {
        return provider.stream(request).await;
    };

    let model_call_id = session.begin_model_call();
    let prefix_fingerprint = request_prefix_fingerprint(
        &request.system,
        &request.tools.iter().map(ToolFingerprint::from).collect::<Vec<_>>(),
        &request.messages,
    );
    let pre_provider_ms = session.pre_provider_ms();
    let request_payload = json!({
        "call_kind": call_kind,
        "observation_scope": scope,
        "fidelity": "canonical",
        "capture": ["redacted"],
        "request": llm_request_to_value(request),
        "prefix_fingerprint": prefix_fingerprint,
        "pre_provider_ms": pre_provider_ms,
    });
    observe_with_model_call(
        &session,
        EVENT_LLM_REQUEST,
        request_payload,
        Some(model_call_id.clone()),
    );
    session.emit_llm_request_telemetry(
        &model_call_id,
        call_kind,
        scope,
        request,
        &prefix_fingerprint,
    );

    let started = Instant::now();
    let rx = match provider.stream(request).await {
        Ok(rx) => rx,
        Err(error) => {
            observe_with_model_call(
                &session,
                EVENT_OBSERVATION_GAP,
                json!({ "reason": "provider_stream_failed", "error": error.to_string() }),
                Some(model_call_id),
            );
            return Err(error);
        }
    };

    Ok(wrap_stream(
        rx,
        session,
        call_kind.to_owned(),
        scope,
        started,
        Some(model_call_id),
        provider.input_tokens_include_cache(),
    ))
}

fn wrap_stream(
    mut rx: mpsc::Receiver<LlmEvent>,
    session: Arc<ObservationSession>,
    call_kind: String,
    scope: ObservationScope,
    started: Instant,
    model_call_id: Option<String>,
    input_includes_cache: bool,
) -> mpsc::Receiver<LlmEvent> {
    let (tx, out_rx) = mpsc::channel(32);
    tokio::spawn(async move {
        let mut ttft_ms: Option<u64> = None;
        let mut text = String::new();
        let mut thinking = String::new();
        let mut tool_use: Vec<Value> = Vec::new();
        let mut saw_terminal = false;

        while let Some(event) = rx.recv().await {
            if ttft_ms.is_none()
                && matches!(
                    event,
                    LlmEvent::TextDelta(_)
                        | LlmEvent::ThinkingDelta(_)
                        | LlmEvent::ToolUse { .. }
                        | LlmEvent::ToolUseDelta { .. }
                )
            {
                ttft_ms = Some(u64::try_from(started.elapsed().as_millis()).unwrap_or(0));
            }
            match &event {
                LlmEvent::TextDelta(delta) => {
                    append_stream_capture(&mut text, delta);
                }
                LlmEvent::ThinkingDelta(delta) => {
                    append_stream_capture(&mut thinking, delta);
                }
                LlmEvent::ToolUse {
                    id,
                    name,
                    input,
                    extra,
                } => {
                    tool_use.push(json!({
                        "id": id,
                        "name": name,
                        "input": input,
                        "extra": extra,
                    }));
                }
                _ => {}
            }
            let pending_response = match &event {
                LlmEvent::Done { stop_reason, usage } => Some((
                    stop_reason_name(*stop_reason),
                    serde_json::to_value(usage).ok(),
                    prompt_cache_split(usage, input_includes_cache),
                    None::<String>,
                )),
                LlmEvent::Error(message) => {
                    Some(("error", None, None, Some(message.clone())))
                }
                _ => None,
            };
            // Deliver to the live consumer first. Compact/judge timeouts drop
            // this receiver; recording a complete llm/response after that
            // would mark an abandoned call as intact.
            if tx.send(event).await.is_err() {
                return;
            }
            if let Some((stop_reason, usage, prompt_cache, error)) = pending_response {
                saw_terminal = true;
                emit_response(
                    &session,
                    &call_kind,
                    scope,
                    &text,
                    &thinking,
                    &tool_use,
                    Some(stop_reason),
                    usage,
                    prompt_cache,
                    error.as_deref(),
                    started,
                    ttft_ms,
                    model_call_id.clone(),
                );
            }
        }
        if !saw_terminal {
            // Leave without llm/response so projection marks interrupted.
        }
    });
    out_rx
}

fn append_stream_capture(buf: &mut String, delta: &str) {
    let started = Instant::now();
    buf.push_str(delta);
    crate::profiler::record(
        crate::profiler::HotPath::ObservationAccumulate,
        started.elapsed(),
    );
}

/// Build a minimal canonical request for side-model / auxiliary chat completions.
pub fn messages_to_llm_request(model: &str, messages: &[Message]) -> LlmRequest {
    let mut system = String::new();
    let mut out = Vec::new();
    for message in messages {
        if message.role == Role::System && system.is_empty() {
            for block in &message.content {
                if let ContentBlock::Text { text } = block {
                    system = text.clone();
                }
            }
            continue;
        }
        out.push(message.clone());
    }
    LlmRequest {
        model: model.to_string(),
        system,
        messages: out,
        tools: Vec::new(),
        max_tokens: None,
        thinking: None,
        reasoning_effort: None,
        temperature: None,
        retain_provider_round: false,
    }
}

/// Record `llm/request` for a non-streaming chat completion.
pub fn begin_chat_llm(
    session: &ObservationSession,
    call_kind: &str,
    scope: ObservationScope,
    request: &LlmRequest,
) -> String {
    let model_call_id = session.begin_model_call();
    let prefix_fingerprint = request_prefix_fingerprint(
        &request.system,
        &request.tools.iter().map(ToolFingerprint::from).collect::<Vec<_>>(),
        &request.messages,
    );
    let pre_provider_ms = session.pre_provider_ms();
    observe_with_model_call(
        session,
        EVENT_LLM_REQUEST,
        json!({
            "call_kind": call_kind,
            "observation_scope": scope,
            "fidelity": "canonical",
            "capture": ["redacted"],
            "request": llm_request_to_value(request),
            "prefix_fingerprint": prefix_fingerprint,
            "pre_provider_ms": pre_provider_ms,
            "observation_type": "generation",
        }),
        Some(model_call_id.clone()),
    );
    session.emit_llm_request_telemetry(
        &model_call_id,
        call_kind,
        scope,
        request,
        &prefix_fingerprint,
    );
    model_call_id
}

/// Record `llm/response` for a non-streaming chat completion.
pub fn finish_chat_llm(
    session: &ObservationSession,
    model_call_id: &str,
    call_kind: &str,
    scope: ObservationScope,
    text: &str,
    elapsed_ms: u64,
    error: Option<&str>,
) {
    observe_with_model_call(
        session,
        EVENT_LLM_RESPONSE,
        json!({
            "call_kind": call_kind,
            "observation_scope": scope,
            "fidelity": "canonical",
            "text": redact_capture(text),
            "thinking": "",
            "tool_use": [],
            "stop_reason": if error.is_some() { Value::Null } else { json!("end_turn") },
            "usage": Value::Null,
            "error": error,
            "elapsed_ms": elapsed_ms,
            "observation_type": "generation",
        }),
        Some(model_call_id.to_string()),
    );
    session.mark_phase_boundary();
}

fn emit_response(
    session: &ObservationSession,
    call_kind: &str,
    scope: ObservationScope,
    text: &str,
    thinking: &str,
    tool_use: &[Value],
    stop_reason: Option<&str>,
    usage: Option<Value>,
    prompt_cache: Option<PromptCacheSplit>,
    error: Option<&str>,
    started: Instant,
    ttft_ms: Option<u64>,
    model_call_id: Option<String>,
) {
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(0);
    observe_with_model_call(
        session,
        EVENT_LLM_RESPONSE,
        json!({
            "call_kind": call_kind,
            "observation_scope": scope,
            "fidelity": "canonical",
            "text": redact_capture(text),
            "thinking": redact_capture(thinking),
            "tool_use": tool_use,
            "stop_reason": stop_reason,
            "usage": usage,
            "error": error,
            "elapsed_ms": elapsed_ms,
            "ttft_ms": ttft_ms,
            "generation_ms": ttft_ms.map(|ttft| elapsed_ms.saturating_sub(ttft)),
            "prompt_tokens": prompt_cache.map(|split| split.prompt_tokens),
            "cache_hit_ratio": prompt_cache.map(|split| split.cache_hit_ratio),
        }),
        model_call_id,
    );
    session.mark_phase_boundary();
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct PromptCacheSplit {
    prompt_tokens: u64,
    cache_hit_ratio: f64,
}

fn prompt_cache_split(usage: &TokenUsage, input_includes_cache: bool) -> Option<PromptCacheSplit> {
    let prompt_tokens = if input_includes_cache {
        usage.input_tokens
    } else {
        usage
            .input_tokens
            .saturating_add(usage.cache_read_tokens)
            .saturating_add(usage.cache_creation_tokens)
    };
    if prompt_tokens == 0 {
        return None;
    }
    let ratio = usage.cache_read_tokens.min(prompt_tokens) as f64 / prompt_tokens as f64;
    Some(PromptCacheSplit {
        prompt_tokens,
        cache_hit_ratio: (ratio * 10_000.0).round() / 10_000.0,
    })
}

fn observe_with_model_call(
    session: &ObservationSession,
    event_type: &str,
    payload: Value,
    model_call_id: Option<String>,
) {
    match session.emit_with_model_call(event_type, payload, model_call_id) {
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(%error, event_type, "observation emit failed");
            if event_type != EVENT_OBSERVATION_GAP {
                if let Err(gap_error) = session.emit_gap("emit_failed", None, None, None) {
                    tracing::warn!(error = %gap_error, "observation gap emit failed");
                }
            }
        }
    }
}

fn observation_telemetry_eligible(ids: &ObservationIds) -> bool {
    nonempty(ids.conversation_id.as_deref()).is_some()
        && nonempty(ids.session_kind.as_deref()) != Some("eval")
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn clip_event_id(mut raw: String) -> String {
    if raw.len() > TELEMETRY_MAX_EVENT_ID {
        raw.truncate(TELEMETRY_MAX_EVENT_ID);
    }
    raw
}

fn insert_telemetry_str(properties: &mut BTreeMap<String, Value>, key: &str, value: &str) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return;
    }
    properties.insert(key.to_string(), json!(clip_chars(trimmed, TELEMETRY_MAX_PROP)));
}

fn clip_chars(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

fn rfc3339_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn rfc3339_millis(ms: u64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(i64::try_from(ms).unwrap_or(i64::MAX))
        .unwrap_or_else(chrono::Utc::now)
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn parse_prefix_chain(value: &Value) -> Option<PrefixChain> {
    let system = value.get("system")?.as_str()?.trim().to_string();
    let tools = value.get("tools")?.as_str()?.trim().to_string();
    if system.is_empty() && tools.is_empty() && value.get("messages").is_none() {
        return None;
    }
    let messages = value
        .get("messages")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Some(PrefixChain {
        system,
        tools,
        messages,
    })
}

fn classify_prefix_break(prev: Option<&PrefixChain>, next: &PrefixChain) -> (&'static str, i64) {
    let Some(prev) = prev else {
        return ("first", 0);
    };
    if prev.system != next.system {
        return ("system", 0);
    }
    if prev.tools != next.tools {
        return ("tools", 0);
    }
    let shared = prev.messages.len().min(next.messages.len());
    for index in 0..shared {
        if prev.messages[index] != next.messages[index] {
            return ("message", i64::try_from(index).unwrap_or(i64::MAX));
        }
    }
    if prev.messages.len() != next.messages.len() {
        return ("message", i64::try_from(shared).unwrap_or(i64::MAX));
    }
    ("none", 0)
}

pub(crate) fn llm_request_to_value(request: &LlmRequest) -> Value {
    json!({
        "model": request.model,
        "system": redact_capture(&request.system),
        "messages": request.messages.iter().map(observation_message).collect::<Vec<_>>(),
        "tools": request.tools.iter().map(observation_tool).collect::<Vec<_>>(),
        "max_tokens": request.max_tokens,
        "thinking": match &request.thinking {
            Some(ThinkingConfig::Enabled { budget_tokens }) => json!({
                "enabled": true,
                "budget_tokens": budget_tokens,
            }),
            Some(ThinkingConfig::Disabled) => json!({ "enabled": false }),
            None => Value::Null,
        },
        "reasoning_effort": request.reasoning_effort,
        "temperature": request.temperature,
    })
}

#[derive(serde::Serialize)]
struct ToolFingerprint<'a> {
    name: &'a str,
    description: &'a str,
    input_schema: &'a Value,
    deferred: bool,
}

impl<'a> From<&'a ToolDef> for ToolFingerprint<'a> {
    fn from(tool: &'a ToolDef) -> Self {
        Self {
            name: &tool.name,
            description: &tool.description,
            input_schema: &tool.input_schema,
            deferred: tool.deferred,
        }
    }
}

fn observation_tool(tool: &ToolDef) -> Value {
    json!({
        "name": tool.name,
        "description": redact_capture(&tool.description),
        "input_schema": json!({
            "omitted_reason": OMITTED_REASON_INPUT_SCHEMA,
        }),
        "deferred": tool.deferred,
    })
}

fn observation_message(message: &Message) -> Value {
    let mut object = serde_json::Map::new();
    object.insert("role".into(), json!(message.role));
    object.insert(
        "content".into(),
        Value::Array(
            message
                .content
                .iter()
                .map(observation_content_block)
                .collect(),
        ),
    );
    if let Some(timestamp) = message.timestamp {
        object.insert("timestamp".into(), json!(timestamp));
    }
    Value::Object(object)
}

fn observation_content_block(block: &ContentBlock) -> Value {
    match block {
        ContentBlock::Text { text } => json!({
            "type": "text",
            "text": redact_capture(text),
        }),
        ContentBlock::ToolUse {
            id,
            name,
            input,
            extra,
        } => {
            let mut object = json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": capture_borrowed(input),
            });
            if let Some(extra) = extra {
                object["extra"] = capture_borrowed(extra);
            }
            object
        }
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
            images,
        } => {
            let mut object = json!({
                "type": "tool_result",
                "tool_use_id": tool_use_id,
                "content": redact_capture(content),
                "is_error": is_error,
            });
            if !images.is_empty() {
                object["images"] = Value::Array(
                    images.iter().map(observation_tool_image).collect(),
                );
            }
            object
        }
        ContentBlock::Thinking {
            thinking,
            signature,
        } => {
            let mut object = json!({
                "type": "thinking",
                "thinking": redact_capture(thinking),
            });
            if let Some(signature) = signature {
                object["signature"] = Value::String(redact_capture(signature));
            }
            object
        }
        ContentBlock::Image { media_type, data } => json!({
            "type": "image",
            "media_type": media_type,
            "data": omitted_binary_payload(media_type, data.len() as u64),
        }),
    }
}

fn observation_tool_image(image: &ToolImage) -> Value {
    json!({
        "media_type": image.media_type,
        "data": omitted_binary_payload(&image.media_type, image.data.len() as u64),
    })
}

fn stop_reason_name(reason: StopReason) -> &'static str {
    match reason {
        StopReason::EndTurn => "end_turn",
        StopReason::ToolUse => "tool_use",
        StopReason::MaxTokens => "max_tokens",
        StopReason::MaxTurns => "max_turns",
        StopReason::Refusal => "refusal",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomi_agent_trace::{
        ExecutionStatus, EVENT_LLM_REQUEST, EVENT_LLM_RESPONSE, EVENT_TOOL_EXECUTION_COMPLETED,
        EVENT_TOOL_EXECUTION_STARTED, EVENT_TURN_END, EVENT_TURN_START,
    };
    use nomi_types::message::{Message, Role, TokenUsage};
    use serde_json::json;

    struct ScriptedProvider;

    #[async_trait::async_trait]
    impl LlmProvider for ScriptedProvider {
        async fn stream(
            &self,
            _: &LlmRequest,
        ) -> Result<mpsc::Receiver<LlmEvent>, ProviderError> {
            let (tx, rx) = mpsc::channel(8);
            tx.send(LlmEvent::TextDelta("hello".into())).await.ok();
            tx.send(LlmEvent::Done {
                stop_reason: StopReason::EndTurn,
                usage: TokenUsage::default(),
            })
            .await
            .ok();
            Ok(rx)
        }
    }

    fn sample_request(
        system: impl Into<String>,
        messages: Vec<Message>,
        tools: Vec<ToolDef>,
    ) -> LlmRequest {
        LlmRequest {
            model: "test-model".into(),
            system: system.into(),
            messages,
            tools,
            max_tokens: Some(32),
            thinking: None,
            reasoning_effort: None,
            temperature: None,
                retain_provider_round: false,
        }
    }

    #[test]
    fn llm_request_to_value_stubs_input_schema_without_cloning_body() {
        let marker = "SCHEMA_MARKER_DO_NOT_COPY_9f3a";
        let request = sample_request(
            "sys",
            vec![Message::new(
                Role::User,
                vec![ContentBlock::Text {
                    text: "hi".into(),
                }],
            )],
            vec![ToolDef {
                name: "bash".into(),
                description: "run a command".into(),
                input_schema: json!({
                    "type": "object",
                    "description": marker.repeat(64),
                }),
                deferred: false,
            }],
        );
        let value = llm_request_to_value(&request);
        let encoded = value.to_string();
        assert!(
            !encoded.contains(marker),
            "observation copy must not clone schema body: {encoded}"
        );
        assert_eq!(value["tools"][0]["name"], "bash");
        assert_eq!(
            value["tools"][0]["input_schema"]["omitted_reason"],
            OMITTED_REASON_INPUT_SCHEMA
        );
        assert!(
            value["tools"][0]["input_schema"].get("captured_bytes").is_none(),
            "schema elision must not pretend a size-budget omit ran"
        );
        assert!(
            request.tools[0].input_schema.to_string().contains(marker),
            "live request schema must stay intact"
        );
    }

    #[test]
    fn llm_request_to_value_preserves_full_system_and_tool_result_without_mutating_live() {
        let long_system = "S".repeat(MAX_PREVIEW_CHARS + 50);
        let long_result = "R".repeat(MAX_PREVIEW_CHARS + 80);
        let request = sample_request(
            long_system.clone(),
            vec![
                Message::new(
                    Role::User,
                    vec![ContentBlock::Text {
                        text: "q".into(),
                    }],
                ),
                Message::new(
                    Role::User,
                    vec![ContentBlock::ToolResult {
                        tool_use_id: "t1".into(),
                        content: long_result.clone(),
                        is_error: false,
                        images: Vec::new(),
                    }],
                ),
            ],
            Vec::new(),
        );
        let value = llm_request_to_value(&request);
        let system = value["system"].as_str().expect("system");
        assert!(!system.contains("…(truncated)"));
        assert_eq!(system.chars().count(), request.system.chars().count());
        assert_eq!(request.system, long_system);

        let content = value["messages"][1]["content"][0]["content"]
            .as_str()
            .expect("tool result");
        assert!(!content.contains("…(truncated)"));
        assert_eq!(content.chars().count(), long_result.chars().count());
        match &request.messages[1].content[0] {
            ContentBlock::ToolResult { content, .. } => {
                assert_eq!(content.as_str(), long_result);
            }
            other => panic!("expected tool result, got {other:?}"),
        }
    }

    #[test]
    fn llm_request_to_value_omits_image_bytes() {
        let blob = format!("IMAGE_MARKER_BASE64_{}", "A".repeat(5000));
        let request = sample_request(
            "sys",
            vec![Message::new(
                Role::User,
                vec![ContentBlock::Image {
                    media_type: "image/png".into(),
                    data: blob.clone(),
                }],
            )],
            Vec::new(),
        );
        let value = llm_request_to_value(&request);
        let encoded = value.to_string();
        assert!(
            !encoded.contains("IMAGE_MARKER_BASE64_"),
            "observation copy must not include image bytes: {encoded}"
        );
        assert_eq!(
            value["messages"][0]["content"][0]["data"]["omitted_reason"],
            nomi_agent_trace::OMITTED_REASON_BINARY_PAYLOAD
        );
        assert!(
            value["messages"][0]["content"][0]["data"]
                .get("sha256")
                .is_none(),
            "emit path must not invent a media digest"
        );
        assert_eq!(
            value["messages"][0]["content"][0]["data"]["byte_length"],
            blob.len() as u64
        );
        match &request.messages[0].content[0] {
            ContentBlock::Image { data, .. } => assert_eq!(data.as_str(), blob),
            other => panic!("expected image, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn stream_llm_writes_request_and_response_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        let session = ObservationSession::new(recorder.clone());
        session.bind_ids(ObservationIds {
            conversation_id: Some("c-obs".into()),
            root_turn_id: Some("t-obs".into()),
            msg_id: Some("m-obs".into()),
            session_kind: Some("session_dialogue".into()),
            ..ObservationIds::default()
        });

        let request = LlmRequest {
            model: "test-model".into(),
            system: "sys".into(),
            messages: vec![Message::new(
                Role::User,
                vec![nomi_types::message::ContentBlock::Text {
                    text: "hi".into(),
                }],
            )],
            tools: Vec::new(),
            max_tokens: Some(32),
            thinking: None,
            reasoning_effort: None,
            temperature: None,
                retain_provider_round: false,
        };

        let mut rx = stream_llm(
            &ScriptedProvider,
            &request,
            Some(Arc::clone(&session)),
            "agent_turn",
            ObservationScope::SessionWorkflow,
        )
        .await
        .unwrap();
        while rx.recv().await.is_some() {}

        let events = recorder.read_events(Some("c-obs")).unwrap();
        assert!(
            events.iter().any(|event| event.event_type == EVENT_LLM_REQUEST),
            "expected llm/request in {events:?}"
        );
        assert!(
            events
                .iter()
                .any(|event| event.event_type == EVENT_LLM_RESPONSE),
            "expected llm/response in {events:?}"
        );
        let request_event = events
            .iter()
            .find(|event| event.event_type == EVENT_LLM_REQUEST)
            .unwrap();
        assert_eq!(request_event.payload["request"]["model"], "test-model");
        assert_eq!(request_event.payload["call_kind"], "agent_turn");
    }

    #[tokio::test]
    async fn prefix_fingerprint_survives_when_request_body_exceeds_size_budget() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        let session = ObservationSession::new(recorder.clone());
        session.bind_ids(ObservationIds {
            conversation_id: Some("c-fp".into()),
            root_turn_id: Some("t-fp".into()),
            ..ObservationIds::default()
        });
        let messages: Vec<Message> = (0..100)
            .map(|i| {
                Message::new(
                    Role::User,
                    vec![ContentBlock::Text {
                        text: format!("{i}:{}", "x".repeat(MAX_PREVIEW_CHARS)),
                    }],
                )
            })
            .collect();
        let request = sample_request("sys", messages, Vec::new());

        let mut rx = stream_llm(
            &ScriptedProvider,
            &request,
            Some(Arc::clone(&session)),
            "agent_turn",
            ObservationScope::SessionWorkflow,
        )
        .await
        .unwrap();
        while rx.recv().await.is_some() {}

        let events = recorder.read_events(Some("c-fp")).unwrap();
        let payload = &events
            .iter()
            .find(|event| event.event_type == EVENT_LLM_REQUEST)
            .expect("llm/request")
            .payload;
        assert!(
            payload["request"]["messages"].is_array(),
            "full request messages should be retained in observation"
        );
        let expected = request_prefix_fingerprint(
            &request.system,
            &Vec::<ToolFingerprint>::new(),
            &request.messages,
        );
        assert_eq!(payload["prefix_fingerprint"], expected);
        assert_eq!(payload["prefix_fingerprint"]["messages"].as_array().unwrap().len(), 100);
    }

    struct LongTextProvider;

    #[async_trait::async_trait]
    impl LlmProvider for LongTextProvider {
        async fn stream(
            &self,
            _: &LlmRequest,
        ) -> Result<mpsc::Receiver<LlmEvent>, ProviderError> {
            let (tx, rx) = mpsc::channel(8);
            tx.send(LlmEvent::TextDelta("字".repeat(MAX_PREVIEW_CHARS + 80)))
                .await
                .ok();
            tx.send(LlmEvent::Done {
                stop_reason: StopReason::EndTurn,
                usage: TokenUsage::default(),
            })
            .await
            .ok();
            Ok(rx)
        }
    }

    #[tokio::test]
    async fn wrap_stream_records_full_observation_text_without_clipping_live_events() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        let session = ObservationSession::new(recorder.clone());
        session.bind_ids(ObservationIds {
            conversation_id: Some("c-long".into()),
            root_turn_id: Some("t-long".into()),
            ..ObservationIds::default()
        });
        let request = LlmRequest {
            model: "test-model".into(),
            system: String::new(),
            messages: vec![Message::new(
                Role::User,
                vec![nomi_types::message::ContentBlock::Text {
                    text: "hi".into(),
                }],
            )],
            tools: Vec::new(),
            max_tokens: Some(32),
            thinking: None,
            reasoning_effort: None,
            temperature: None,
                retain_provider_round: false,
        };
        let mut rx = stream_llm(
            &LongTextProvider,
            &request,
            Some(Arc::clone(&session)),
            "agent_turn",
            ObservationScope::SessionWorkflow,
        )
        .await
        .unwrap();
        let mut live_chars = 0usize;
        while let Some(event) = rx.recv().await {
            if let LlmEvent::TextDelta(delta) = event {
                live_chars += delta.chars().count();
            }
        }
        assert_eq!(live_chars, MAX_PREVIEW_CHARS + 80);

        let events = recorder.read_events(Some("c-long")).unwrap();
        let response = events
            .iter()
            .find(|event| event.event_type == EVENT_LLM_RESPONSE)
            .expect("llm/response");
        let text = response.payload["text"].as_str().expect("text");
        assert!(!text.contains("…(truncated)"));
        assert_eq!(text.chars().count(), live_chars);
    }

    struct HandshakeDoneProvider {
        release_done: std::sync::Arc<tokio::sync::Notify>,
    }

    #[async_trait::async_trait]
    impl LlmProvider for HandshakeDoneProvider {
        async fn stream(
            &self,
            _: &LlmRequest,
        ) -> Result<mpsc::Receiver<LlmEvent>, ProviderError> {
            let (tx, rx) = mpsc::channel(8);
            let release_done = std::sync::Arc::clone(&self.release_done);
            tokio::spawn(async move {
                let _ = tx.send(LlmEvent::TextDelta("partial".into())).await;
                release_done.notified().await;
                let _ = tx
                    .send(LlmEvent::Done {
                        stop_reason: StopReason::EndTurn,
                        usage: TokenUsage::default(),
                    })
                    .await;
            });
            Ok(rx)
        }
    }

    #[tokio::test]
    async fn wrap_stream_skips_response_when_consumer_drops() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        let session = ObservationSession::new(recorder.clone());
        session.bind_ids(ObservationIds {
            conversation_id: Some("c-drop".into()),
            root_turn_id: Some("t-drop".into()),
            ..ObservationIds::default()
        });
        let request = LlmRequest {
            model: "test-model".into(),
            system: String::new(),
            messages: vec![Message::new(
                Role::User,
                vec![nomi_types::message::ContentBlock::Text {
                    text: "hi".into(),
                }],
            )],
            tools: Vec::new(),
            max_tokens: Some(32),
            thinking: None,
            reasoning_effort: None,
            temperature: None,
                retain_provider_round: false,
        };
        let release_done = std::sync::Arc::new(tokio::sync::Notify::new());
        let mut rx = stream_llm(
            &HandshakeDoneProvider {
                release_done: std::sync::Arc::clone(&release_done),
            },
            &request,
            Some(Arc::clone(&session)),
            "compaction",
            ObservationScope::SessionWorkflow,
        )
        .await
        .unwrap();
        assert!(matches!(rx.recv().await, Some(LlmEvent::TextDelta(_))));
        drop(rx);
        release_done.notify_one();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let events = recorder.read_events(Some("c-drop")).unwrap();
        assert!(
            events.iter().any(|event| event.event_type == EVENT_LLM_REQUEST),
            "expected llm/request in {events:?}"
        );
        assert!(
            events
                .iter()
                .all(|event| event.event_type != EVENT_LLM_RESPONSE),
            "abandoned consumer must not record llm/response: {events:?}"
        );
    }

    #[tokio::test]
    async fn nested_stream_does_not_reattach_tool_events() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        let session = ObservationSession::new(recorder.clone());
        session.bind_ids(ObservationIds {
            conversation_id: Some("c-nested".into()),
            root_turn_id: Some("t-nested".into()),
            ..ObservationIds::default()
        });

        let parent = session.begin_model_call();
        session.emit_tool_started("call-echo", "echo", &json!({ "text": "ping" }));

        let request = LlmRequest {
            model: "extract".into(),
            system: String::new(),
            messages: vec![Message::new(
                Role::User,
                vec![nomi_types::message::ContentBlock::Text {
                    text: "extract".into(),
                }],
            )],
            tools: Vec::new(),
            max_tokens: Some(16),
            thinking: None,
            reasoning_effort: None,
            temperature: None,
                retain_provider_round: false,
        };
        let mut rx = stream_llm(
            &ScriptedProvider,
            &request,
            Some(Arc::clone(&session)),
            "browser_extract",
            ObservationScope::SessionWorkflow,
        )
        .await
        .unwrap();
        while rx.recv().await.is_some() {}

        session.emit_tool_finished(
            "call-echo",
            "echo",
            false,
            "pong",
            Some(ToolCallTiming {
                duration_ms: 1234,
                duration_us: 1_234_000,
                started_at_ms: 1_790_000_000_000 - 1234,
                completed_at_ms: 1_790_000_000_000,
                phases: vec![
                    nomi_tools::phase_trace::ToolPhase { name: "tool.execute", micros: 1_200_000 },
                    nomi_tools::phase_trace::ToolPhase { name: "bash.spawn", micros: 900_000 },
                ],
            }),
            Some(4321),
        );

        let events = recorder.read_events(Some("c-nested")).unwrap();
        let started = events
            .iter()
            .find(|event| event.event_type == EVENT_TOOL_EXECUTION_STARTED)
            .expect("tool started");
        let completed = events
            .iter()
            .find(|event| event.event_type == EVENT_TOOL_EXECUTION_COMPLETED)
            .expect("tool completed");
        assert_eq!(completed.payload["duration_ms"], 1234);
        assert_eq!(completed.payload["duration_us"], 1_234_000);
        assert_eq!(completed.payload["completed_at_ms"], 1_790_000_000_000u64);
        assert_eq!(completed.payload["round_wall_ms"], 4321);
        assert_eq!(
            completed.payload["phases"],
            json!([
                { "name": "tool.execute", "us": 1_200_000 },
                { "name": "bash.spawn", "us": 900_000 },
            ])
        );
        assert_eq!(
            nomi_agent_trace::ids_from_payload(&started.payload)
                .model_call_id
                .as_deref(),
            Some(parent.as_str())
        );
        assert_eq!(
            nomi_agent_trace::ids_from_payload(&completed.payload)
                .model_call_id
                .as_deref(),
            Some(parent.as_str())
        );
        let extract = events
            .iter()
            .find(|event| {
                event.event_type == EVENT_LLM_REQUEST
                    && event.payload["call_kind"] == "browser_extract"
            })
            .expect("nested extract request");
        let extract_id = nomi_agent_trace::ids_from_payload(&extract.payload)
            .model_call_id
            .expect("extract model_call_id");
        assert_ne!(extract_id.as_str(), parent.as_str());
        let extract_response = events
            .iter()
            .find(|event| {
                event.event_type == EVENT_LLM_RESPONSE
                    && event.payload["call_kind"] == "browser_extract"
            })
            .expect("nested extract response");
        assert_eq!(
            nomi_agent_trace::ids_from_payload(&extract_response.payload)
                .model_call_id
                .as_deref(),
            Some(extract_id.as_str())
        );
    }

    #[test]
    fn turn_start_and_end_are_first_write_wins_across_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        let ids = ObservationIds {
            conversation_id: Some("c-share".into()),
            root_turn_id: Some("t-share".into()),
            msg_id: Some("m-share".into()),
            ..ObservationIds::default()
        };
        let first = ObservationSession::new(recorder.clone());
        let rebuilt = ObservationSession::new(recorder.clone());
        first.bind_ids_with_preview(ids.clone(), Some("raw user message"));
        rebuilt.bind_ids_with_preview(ids.clone(), Some("enriched provider context"));
        first.emit_turn_end(
            ExecutionStatus::Completed,
            11,
            Some("end_turn"),
            Some(json!({ "input_tokens": 3 })),
            None,
        );
        rebuilt.emit_turn_end(
            ExecutionStatus::Cancelled,
            99,
            Some("cancelled"),
            None,
            None,
        );

        let events = recorder.read_events(Some("c-share")).unwrap();
        let starts: Vec<_> = events
            .iter()
            .filter(|event| event.event_type == EVENT_TURN_START)
            .collect();
        let ends: Vec<_> = events
            .iter()
            .filter(|event| event.event_type == EVENT_TURN_END)
            .collect();
        assert_eq!(starts.len(), 1, "failover rebuild must not emit a second turn/start: {events:?}");
        assert_eq!(ends.len(), 1, "failover rebuild must not emit a second turn/end: {events:?}");
        assert_eq!(starts[0].payload["prompt_preview"], "raw user message");
        assert_eq!(ends[0].payload["status"], "completed");
        assert_eq!(ends[0].payload["elapsed_ms"], 11);

        recorder.clear_conversation("c-share").unwrap();
        rebuilt.bind_ids(ids);
        rebuilt.emit_turn_end(ExecutionStatus::Completed, 4, Some("end_turn"), None, None);
        let after = recorder.read_events(Some("c-share")).unwrap();
        assert!(
            after
                .iter()
                .any(|event| event.event_type == EVENT_TURN_START),
            "clear must allow a later turn/start, got {after:?}"
        );
        assert!(
            after.iter().any(|event| event.event_type == EVENT_TURN_END),
            "clear must allow a later turn/end, got {after:?}"
        );
    }

    #[test]
    fn same_turn_rebind_keeps_in_flight_tool_parents() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        let session = ObservationSession::new(recorder.clone());
        let ids = ObservationIds {
            conversation_id: Some("c-rebind".into()),
            root_turn_id: Some("t-rebind".into()),
            msg_id: Some("m-1".into()),
            ..ObservationIds::default()
        };
        session.bind_ids(ids.clone());
        let model_call_id = session.begin_model_call();
        session.emit_tool_started("tool-1", "bash", &json!({ "cmd": "ls" }));
        session.bind_ids_with_preview(
            ObservationIds {
                msg_id: Some("m-2".into()),
                ..ids
            },
            Some("retry"),
        );
        session.emit_tool_finished("tool-1", "bash", false, "ok", None, None);

        let events = recorder.read_events(Some("c-rebind")).unwrap();
        let finished = events
            .iter()
            .find(|event| event.event_type == EVENT_TOOL_EXECUTION_COMPLETED)
            .expect("tool completed");
        assert_eq!(
            nomi_agent_trace::ids_from_payload(&finished.payload)
                .model_call_id
                .as_deref(),
            Some(model_call_id.as_str()),
            "continuation bind must not drop in-flight tool parents: {events:?}"
        );
    }

    struct CachedUsageProvider {
        input_includes_cache: bool,
    }

    #[async_trait::async_trait]
    impl LlmProvider for CachedUsageProvider {
        fn input_tokens_include_cache(&self) -> bool {
            self.input_includes_cache
        }

        async fn stream(
            &self,
            _: &LlmRequest,
        ) -> Result<mpsc::Receiver<LlmEvent>, ProviderError> {
            let (tx, rx) = mpsc::channel(8);
            tokio::spawn(async move {
                tx.send(LlmEvent::TextDelta("hi".into())).await.ok();
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                tx.send(LlmEvent::Done {
                    stop_reason: StopReason::EndTurn,
                    usage: TokenUsage {
                        input_tokens: 1000,
                        output_tokens: 5,
                        reasoning_tokens: 0,
                        cache_creation_tokens: 100,
                        cache_read_tokens: 600,
                    },
                })
                .await
                .ok();
            });
            Ok(rx)
        }
    }

    #[test]
    fn prompt_cache_split_normalizes_both_usage_conventions() {
        let usage = TokenUsage {
            input_tokens: 1000,
            output_tokens: 0,
            reasoning_tokens: 0,
            cache_creation_tokens: 100,
            cache_read_tokens: 600,
        };
        assert_eq!(
            prompt_cache_split(&usage, true),
            Some(PromptCacheSplit {
                prompt_tokens: 1000,
                cache_hit_ratio: 0.6,
            })
        );
        assert_eq!(
            prompt_cache_split(&usage, false),
            Some(PromptCacheSplit {
                prompt_tokens: 1700,
                cache_hit_ratio: 0.3529,
            })
        );
        assert_eq!(prompt_cache_split(&TokenUsage::default(), true), None);
    }

    #[tokio::test]
    async fn llm_events_split_local_prep_first_token_and_generation() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        let session = ObservationSession::new(recorder.clone());
        session.bind_ids(ObservationIds {
            conversation_id: Some("c-phase".into()),
            root_turn_id: Some("t-phase".into()),
            ..ObservationIds::default()
        });
        let request = sample_request("sys", Vec::new(), Vec::new());
        let provider = CachedUsageProvider {
            input_includes_cache: false,
        };

        let mut rx = stream_llm(
            &provider,
            &request,
            Some(Arc::clone(&session)),
            "turn",
            ObservationScope::SessionWorkflow,
        )
        .await
        .unwrap();
        while rx.recv().await.is_some() {}
        session.emit_tool_finished("tool-1", "bash", false, "ok", None, Some(7));
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        let mut rx = stream_llm(
            &provider,
            &request,
            Some(Arc::clone(&session)),
            "turn",
            ObservationScope::SessionWorkflow,
        )
        .await
        .unwrap();
        while rx.recv().await.is_some() {}

        let events = recorder.read_events(Some("c-phase")).unwrap();
        let requests: Vec<_> = events
            .iter()
            .filter(|event| event.event_type == EVENT_LLM_REQUEST)
            .collect();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].payload["pre_provider_ms"].is_u64());
        assert!(
            requests[1].payload["pre_provider_ms"].as_u64().unwrap() >= 40,
            "pre-provider time counts from the tool round end: {:?}",
            requests[1].payload
        );

        let response = events
            .iter()
            .find(|event| event.event_type == EVENT_LLM_RESPONSE)
            .expect("llm response");
        let elapsed = response.payload["elapsed_ms"].as_u64().unwrap();
        let ttft = response.payload["ttft_ms"].as_u64().unwrap();
        let generation = response.payload["generation_ms"].as_u64().unwrap();
        assert_eq!(generation, elapsed - ttft);
        assert!(generation >= 25, "generation spans first token to done: {generation}");
        assert_eq!(response.payload["prompt_tokens"], 1700);
        assert_eq!(response.payload["cache_hit_ratio"], 0.3529);
    }

    #[test]
    fn prefix_break_classifies_first_none_system_tools_and_message() {
        let base = PrefixChain {
            system: "sys-a".into(),
            tools: "tools-a".into(),
            messages: vec!["m0".into(), "m1".into()],
        };
        assert_eq!(classify_prefix_break(None, &base), ("first", 0));
        assert_eq!(classify_prefix_break(Some(&base), &base), ("none", 0));
        assert_eq!(
            classify_prefix_break(
                Some(&base),
                &PrefixChain {
                    system: "sys-b".into(),
                    ..base.clone()
                }
            ),
            ("system", 0)
        );
        assert_eq!(
            classify_prefix_break(
                Some(&base),
                &PrefixChain {
                    tools: "tools-b".into(),
                    ..base.clone()
                }
            ),
            ("tools", 0)
        );
        assert_eq!(
            classify_prefix_break(
                Some(&base),
                &PrefixChain {
                    messages: vec!["m0".into(), "mX".into()],
                    ..base.clone()
                }
            ),
            ("message", 1)
        );
        assert_eq!(
            classify_prefix_break(
                Some(&base),
                &PrefixChain {
                    messages: vec!["m0".into(), "m1".into(), "m2".into()],
                    ..base.clone()
                }
            ),
            ("message", 2)
        );
    }

    #[test]
    fn observation_telemetry_skips_eval_and_missing_conversation() {
        assert!(!observation_telemetry_eligible(&ObservationIds::default()));
        assert!(!observation_telemetry_eligible(&ObservationIds {
            conversation_id: Some("c-eval".into()),
            session_kind: Some("eval".into()),
            ..ObservationIds::default()
        }));
        assert!(observation_telemetry_eligible(&ObservationIds {
            conversation_id: Some("c-ok".into()),
            session_kind: Some("session_dialogue".into()),
            ..ObservationIds::default()
        }));
    }

    #[test]
    fn parse_prefix_chain_reads_sha256_digest_fields() {
        let chain = parse_prefix_chain(&json!({
            "algorithm": "sha256-chain",
            "system": "aaaaaaaaaaaaaaaa",
            "tools": "bbbbbbbbbbbbbbbb",
            "messages": ["cccccccccccccccc"]
        }))
        .expect("chain");
        assert_eq!(chain.system, "aaaaaaaaaaaaaaaa");
        assert_eq!(chain.tools, "bbbbbbbbbbbbbbbb");
        assert_eq!(chain.messages, vec!["cccccccccccccccc"]);
    }

    #[test]
    fn clip_chars_stays_on_char_boundary() {
        let raw = "工具名称".repeat(80);
        let clipped = clip_chars(&raw, TELEMETRY_MAX_PROP);
        assert!(clipped.len() <= TELEMETRY_MAX_PROP);
        assert!(raw.is_char_boundary(clipped.len()) || clipped.is_empty());
        assert_eq!(clipped, &raw[..clipped.len()]);
    }

    #[test]
    fn tool_finished_without_timing_uses_started_at() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        let session = ObservationSession::new(recorder.clone());
        session.bind_ids(ObservationIds {
            conversation_id: Some("c-timing".into()),
            root_turn_id: Some("t-timing".into()),
            ..Default::default()
        });
        session.emit_tool_started("call-read", "Read", &json!({ "file_path": "a.rs" }));
        session.emit_tool_finished("call-read", "Read", false, "ok", None, None);
        let events = recorder.read_events(Some("c-timing")).unwrap();
        let completed = events
            .iter()
            .find(|e| e.event_type == EVENT_TOOL_EXECUTION_COMPLETED)
            .expect("completed");
        assert!(completed.payload.get("started_at_ms").is_some());
        assert!(completed.payload.get("duration_ms").is_some());
        assert!(completed.payload.get("duration_us").is_some());
    }

    #[test]
    fn harness_profile_event_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        let session = ObservationSession::new(recorder.clone());
        session.bind_ids(ObservationIds {
            conversation_id: Some("c-harness".into()),
            ..Default::default()
        });
        session.emit_harness_profile("office", false, None);
        let events = recorder.read_events(Some("c-harness")).unwrap();
        assert_eq!(events[0].event_type, EVENT_HARNESS_PROFILE);
        assert_eq!(events[0].payload["profile"], "office");
        assert_eq!(events[0].payload["active"], false);
    }
}
