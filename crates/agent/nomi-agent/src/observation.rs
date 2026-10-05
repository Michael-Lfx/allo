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
    OMITTED_REASON_INPUT_SCHEMA,
};
use nomi_providers::{LlmProvider, ProviderError};
use nomi_types::llm::{LlmEvent, LlmRequest, ThinkingConfig};
use nomi_types::message::{ContentBlock, Message, Role, StopReason, TokenUsage};
use nomi_types::tool::{ToolDef, ToolImage};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use nomi_coding::FinishGateFacts;
use nomi_providers::WireFacts;

use crate::telemetry::{
    self, FinishFacts, HarnessView, HistoryFacts, LlmRequestFacts, LlmResponseFacts, NudgeFacts,
    ToolFacts, TurnStats, TurnSummaryFacts,
};
use crate::tool_execution::ToolCallTiming;

const TELEMETRY_MAX_EVENT_ID: usize = 128;

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
    /// Replaces the process-wide telemetry hook for this session when set.
    telemetry_sink: Mutex<Option<ObservationTelemetryHook>>,
    /// Totals for the running root turn, reported when the turn ends.
    turn_stats: Mutex<TurnStats>,
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
            telemetry_sink: Mutex::new(None),
            turn_stats: Mutex::new(TurnStats::default()),
        })
    }

    /// Route this session's telemetry records to `sink` instead of the
    /// process-wide hook. `None` restores the process-wide hook.
    pub fn set_telemetry_sink(&self, sink: Option<ObservationTelemetryHook>) {
        *self
            .telemetry_sink
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = sink;
    }

    fn telemetry_sink(&self) -> Option<ObservationTelemetryHook> {
        self.telemetry_sink
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// True when a record built now would reach a consumer.
    fn telemetry_wanted(&self, ids: &ObservationIds) -> bool {
        observation_telemetry_eligible(ids)
            && (self.telemetry_sink().is_some() || OBSERVATION_TELEMETRY_HOOK.get().is_some())
    }

    fn enqueue_telemetry(&self, record: ObservationTelemetryRecord) {
        match self.telemetry_sink() {
            Some(sink) => sink(record),
            None => enqueue_observation_telemetry(record),
        }
    }

    fn update_stats(&self, update: impl FnOnce(&mut TurnStats)) {
        update(&mut self.turn_stats.lock().unwrap_or_else(|e| e.into_inner()));
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
            self.update_stats(|stats| *stats = TurnStats::default());
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
            json!({ "prompt_preview": preview, "build": nomi_agent_trace::build_info() }),
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
        let stats = self
            .turn_stats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        observe_with_model_call(
            self,
            EVENT_TURN_END,
            json!({
                "status": status,
                "elapsed_ms": elapsed_ms,
                "stop_reason": stop_reason,
                "usage": usage,
                "error": redacted_error,
                "summary": stats.to_value(),
            }),
            None,
        );
        let status_label = serde_json::to_value(status)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned));
        self.emit_turn_summary_telemetry(
            &ids,
            status_label.as_deref(),
            elapsed_ms,
            stop_reason,
            &stats,
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
        let error_class =
            is_error.then(|| nomi_coding::ToolFailureClass::from_tool(name, result).label());
        let mut payload = json!({
            "tool_call_id": tool_call_id,
            "name": name,
            "is_error": is_error,
            "result": result,
            "result_bytes": result.len(),
        });
        if let Some(error_class) = error_class {
            payload["error_class"] = json!(error_class);
        }
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
                    attrs: Vec::new(),
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
            if let Some(attrs) = telemetry::tool_attrs_value(&timing.attrs) {
                payload["attrs"] = attrs;
            }
        }
        if let Some(round_wall_ms) = round_wall_ms {
            payload["round_wall_ms"] = json!(round_wall_ms);
        }
        observe_with_model_call(self, event_type, payload, parent.clone());
        self.mark_phase_boundary();
        self.update_stats(|stats| {
            stats.tool_calls += 1;
            if is_error {
                stats.tool_errors += 1;
            }
            if let Some(timing) = &timing {
                stats.tool_wall_ms = stats.tool_wall_ms.saturating_add(timing.duration_ms);
            }
        });
        self.emit_tool_telemetry(
            tool_call_id,
            &ToolFacts {
                name,
                outcome: if is_error { "failed" } else { "completed" },
                model_call_id: parent.as_deref(),
                result_bytes: Some(result.len()),
                error_class,
                timing: timing.as_ref(),
            },
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
            parent.clone(),
        );
        self.mark_phase_boundary();
        self.emit_tool_telemetry(
            tool_call_id,
            &ToolFacts {
                name,
                outcome: "cancelled",
                model_call_id: parent.as_deref(),
                result_bytes: None,
                error_class: None,
                timing: None,
            },
        );
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
        if let Some(view) = HarnessView::from_counters(&counters) {
            self.update_stats(|stats| stats.harness = Some(view));
        }
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
        info: NudgeInfo<'_>,
    ) {
        let preview = redact_preview(text);
        let _ = self.emit(
            EVENT_HARNESS_NUDGE,
            json!({
                "profile": profile,
                "source": source,
                "text_preview": preview,
                "hard_stop": hard_stop,
                "kind": info.kind,
                "detail": info.detail,
                "tool": info.tool,
            }),
        );
        let turn_nudge_index = self
            .turn_stats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .note_nudge(info.kind, hard_stop);
        let ids = self.ids();
        if !self.telemetry_wanted(&ids) {
            return;
        }
        let model_call_id = self.last_model_call_id();
        self.enqueue_telemetry(ObservationTelemetryRecord {
            event_id: clip_event_id(format!("obs:nudge:{}", uuid::Uuid::now_v7())),
            name: telemetry::EVENT_HARNESS_NUDGE.into(),
            occurred_at: rfc3339_now(),
            properties: telemetry::harness_nudge_props(
                &ids,
                &NudgeFacts {
                    profile,
                    source,
                    kind: info.kind,
                    detail: info.detail,
                    tool: info.tool,
                    hard_stop,
                    text_chars: text.chars().count(),
                    model_call_id: model_call_id.as_deref(),
                    turn_nudge_index,
                },
            ),
        });
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
        gate: Option<&FinishGateFacts>,
    ) {
        let mut payload = json!({
            "profile": profile,
            "decision": decision,
            "counters": counters,
        });
        if let Some(text) = nudge_preview {
            payload["nudge_preview"] = json!(redact_preview(text));
        }
        if let Some(gate) = gate {
            payload["gate"] = serde_json::to_value(gate).unwrap_or(Value::Null);
        }
        let _ = self.emit(EVENT_HARNESS_FINISH, payload);
        if nudge_preview.is_some() {
            self.update_stats(|stats| stats.finish_blocks += 1);
        }
        let ids = self.ids();
        if !self.telemetry_wanted(&ids) {
            return;
        }
        let model_call_id = self.last_model_call_id();
        self.enqueue_telemetry(ObservationTelemetryRecord {
            event_id: clip_event_id(format!("obs:finish:{}", uuid::Uuid::now_v7())),
            name: telemetry::EVENT_HARNESS_FINISH.into(),
            occurred_at: rfc3339_now(),
            properties: telemetry::harness_finish_props(
                &ids,
                &FinishFacts {
                    profile,
                    decision,
                    gate,
                    nudge_chars: nudge_preview.map(|text| text.chars().count()),
                    model_call_id: model_call_id.as_deref(),
                },
            ),
        });
    }

    fn emit_tool_telemetry(&self, tool_call_id: &str, facts: &ToolFacts<'_>) {
        let ids = self.ids();
        if !self.telemetry_wanted(&ids) {
            return;
        }
        let tool_call_id = tool_call_id.trim();
        if tool_call_id.is_empty() {
            return;
        }
        let occurred_at = facts
            .timing
            .map(|timing| rfc3339_millis(timing.completed_at_ms))
            .unwrap_or_else(rfc3339_now);
        self.enqueue_telemetry(ObservationTelemetryRecord {
            event_id: clip_event_id(format!("obs:tool:{tool_call_id}")),
            name: telemetry::EVENT_TOOL_EXECUTED.into(),
            occurred_at,
            properties: telemetry::tool_executed_props(&ids, facts),
        });
    }

    fn emit_llm_request_telemetry(
        &self,
        model_call_id: &str,
        call_kind: &str,
        scope: ObservationScope,
        request: &LlmRequest,
        fingerprint: &Value,
        history: &HistoryFacts,
        wire: Option<&WireFacts>,
    ) {
        if scope != ObservationScope::SessionWorkflow {
            return;
        }
        let ids = self.ids();
        if !self.telemetry_wanted(&ids) {
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
        self.enqueue_telemetry(ObservationTelemetryRecord {
            event_id: clip_event_id(format!("obs:llm:{model_call_id}")),
            name: telemetry::EVENT_LLM_REQUEST.into(),
            occurred_at: rfc3339_now(),
            properties: telemetry::llm_request_props(
                &ids,
                &LlmRequestFacts {
                    model_call_id,
                    call_kind,
                    request,
                    system_hash: &chain.system,
                    tools_hash: &chain.tools,
                    message_count: chain.messages.len(),
                    prefix_break,
                    prefix_break_index,
                    history,
                    wire,
                },
            ),
        });
    }

    fn emit_llm_response_telemetry(&self, scope: ObservationScope, facts: &LlmResponseFacts<'_>) {
        if scope != ObservationScope::SessionWorkflow {
            return;
        }
        let ids = self.ids();
        if !self.telemetry_wanted(&ids) {
            return;
        }
        self.enqueue_telemetry(ObservationTelemetryRecord {
            event_id: clip_event_id(format!("obs:llm_response:{}", facts.model_call_id)),
            name: telemetry::EVENT_LLM_RESPONSE.into(),
            occurred_at: rfc3339_now(),
            properties: telemetry::llm_response_props(&ids, facts),
        });
    }

    fn emit_turn_summary_telemetry(
        &self,
        ids: &ObservationIds,
        status: Option<&str>,
        elapsed_ms: u64,
        stop_reason: Option<&str>,
        stats: &TurnStats,
    ) {
        if !self.telemetry_wanted(ids) {
            return;
        }
        let Some(turn_id) = nonempty(ids.root_turn_id.as_deref()) else {
            return;
        };
        self.enqueue_telemetry(ObservationTelemetryRecord {
            event_id: clip_event_id(format!("obs:turn:{turn_id}")),
            name: telemetry::EVENT_TURN_SUMMARY.into(),
            occurred_at: rfc3339_now(),
            properties: telemetry::turn_summary_props(
                ids,
                &TurnSummaryFacts {
                    status,
                    stop_reason,
                    elapsed_ms,
                    stats,
                },
            ),
        });
    }

    /// Wire-level facts for a workflow request, built only when something
    /// will consume them.
    fn wire_facts(
        &self,
        provider: &dyn LlmProvider,
        request: &LlmRequest,
        scope: ObservationScope,
    ) -> Option<WireFacts> {
        if scope != ObservationScope::SessionWorkflow {
            return None;
        }
        let wanted = self.recorder.is_enabled() || self.telemetry_wanted(&self.ids());
        wanted.then(|| provider.describe_request(request)).flatten()
    }

    fn note_request(
        &self,
        scope: ObservationScope,
        history: &HistoryFacts,
        wire: Option<&WireFacts>,
    ) {
        if scope != ObservationScope::SessionWorkflow {
            return;
        }
        let dropped = wire.is_some_and(|wire| telemetry::reasoning_dropped(history, wire) > 0);
        if dropped {
            self.update_stats(|stats| stats.reasoning_dropped_calls += 1);
        }
    }

    fn note_response(&self, scope: ObservationScope, capture: &ResponseCapture<'_>) {
        if scope != ObservationScope::SessionWorkflow {
            return;
        }
        let thinking_chars = capture.thinking.chars().count() as u64;
        self.update_stats(|stats| {
            stats.model_calls += 1;
            if capture.error.is_some() {
                stats.model_errors += 1;
            }
            stats.llm_wall_ms = stats.llm_wall_ms.saturating_add(capture.elapsed_ms);
            stats.thinking_chars = stats.thinking_chars.saturating_add(thinking_chars);
            if let Some(split) = capture.prompt_cache {
                stats.prompt_tokens = stats.prompt_tokens.saturating_add(split.prompt_tokens);
            }
            if let Some(usage) = &capture.usage {
                stats.output_tokens = stats.output_tokens.saturating_add(usage.output_tokens);
                stats.reasoning_tokens =
                    stats.reasoning_tokens.saturating_add(usage.reasoning_tokens);
                stats.cache_read_tokens =
                    stats.cache_read_tokens.saturating_add(usage.cache_read_tokens);
                stats.cache_creation_tokens = stats
                    .cache_creation_tokens
                    .saturating_add(usage.cache_creation_tokens);
            }
        });
    }
}

/// Classification of a harness nudge, recorded next to its text.
#[derive(Debug, Clone, Copy)]
pub struct NudgeInfo<'a> {
    pub kind: &'a str,
    pub detail: Option<&'a str>,
    pub tool: Option<&'a str>,
}

impl<'a> NudgeInfo<'a> {
    /// A nudge with no finer classification than where it came from.
    pub fn source(kind: &'a str) -> Self {
        Self {
            kind,
            detail: None,
            tool: None,
        }
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
    let history = HistoryFacts::of(&request.messages);
    let wire = session.wire_facts(provider, request, scope);
    session.note_request(scope, &history, wire.as_ref());
    let request_payload = json!({
        "call_kind": call_kind,
        "observation_scope": scope,
        "fidelity": "canonical",
        "capture": ["redacted"],
        "request": llm_request_to_value(request),
        "prefix_fingerprint": prefix_fingerprint,
        "pre_provider_ms": pre_provider_ms,
        "request_facts": telemetry::request_facts_value(&history, wire.as_ref()),
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
        &history,
        wire.as_ref(),
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
        StreamMeta {
            call_kind: call_kind.to_owned(),
            model: request.model.clone(),
            scope,
            model_call_id: Some(model_call_id),
            pre_provider_ms,
            input_includes_cache: provider.input_tokens_include_cache(),
        },
        started,
    ))
}

struct StreamMeta {
    call_kind: String,
    model: String,
    scope: ObservationScope,
    model_call_id: Option<String>,
    pre_provider_ms: Option<u64>,
    input_includes_cache: bool,
}

fn wrap_stream(
    mut rx: mpsc::Receiver<LlmEvent>,
    session: Arc<ObservationSession>,
    meta: StreamMeta,
    started: Instant,
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
            let capture = match &event {
                LlmEvent::Done { stop_reason, usage } => Some(ResponseCapture {
                    text: &text,
                    thinking: &thinking,
                    tool_use: &tool_use,
                    stop_reason: Some(stop_reason_name(*stop_reason)),
                    usage: Some(usage.clone()),
                    prompt_cache: prompt_cache_split(usage, meta.input_includes_cache),
                    error: None,
                    elapsed_ms: elapsed_millis(started),
                    ttft_ms,
                }),
                LlmEvent::Error(message) => Some(ResponseCapture {
                    text: &text,
                    thinking: &thinking,
                    tool_use: &tool_use,
                    stop_reason: Some("error"),
                    usage: None,
                    prompt_cache: None,
                    error: Some(message.clone()),
                    elapsed_ms: elapsed_millis(started),
                    ttft_ms,
                }),
                _ => None,
            };
            if let Some(capture) = &capture {
                session.note_response(meta.scope, capture);
            }
            // Deliver to the live consumer first. Compact/judge timeouts drop
            // this receiver; recording a complete llm/response after that
            // would mark an abandoned call as intact.
            if tx.send(event).await.is_err() {
                return;
            }
            if let Some(capture) = &capture {
                saw_terminal = true;
                emit_response(&session, &meta, capture);
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
        isolate_malformed_tool_calls: false,
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
    let history = HistoryFacts::of(&request.messages);
    session.note_request(scope, &history, None);
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
            "request_facts": telemetry::request_facts_value(&history, None),
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
        &history,
        None,
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

struct ResponseCapture<'a> {
    text: &'a str,
    thinking: &'a str,
    tool_use: &'a [Value],
    stop_reason: Option<&'static str>,
    usage: Option<TokenUsage>,
    prompt_cache: Option<PromptCacheSplit>,
    error: Option<String>,
    elapsed_ms: u64,
    ttft_ms: Option<u64>,
}

fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(0)
}

fn emit_response(session: &ObservationSession, meta: &StreamMeta, capture: &ResponseCapture<'_>) {
    let elapsed_ms = capture.elapsed_ms;
    let ttft_ms = capture.ttft_ms;
    let prompt_cache = capture.prompt_cache;
    observe_with_model_call(
        session,
        EVENT_LLM_RESPONSE,
        json!({
            "call_kind": meta.call_kind,
            "observation_scope": meta.scope,
            "fidelity": "canonical",
            "text": redact_capture(capture.text),
            "thinking": redact_capture(capture.thinking),
            "tool_use": capture.tool_use,
            "stop_reason": capture.stop_reason,
            "usage": capture.usage.as_ref().and_then(|usage| serde_json::to_value(usage).ok()),
            "error": capture.error,
            "elapsed_ms": elapsed_ms,
            "ttft_ms": ttft_ms,
            "generation_ms": ttft_ms.map(|ttft| elapsed_ms.saturating_sub(ttft)),
            "prompt_tokens": prompt_cache.map(|split| split.prompt_tokens),
            "cache_hit_ratio": prompt_cache.map(|split| split.cache_hit_ratio),
        }),
        meta.model_call_id.clone(),
    );
    session.mark_phase_boundary();
    if let Some(model_call_id) = meta.model_call_id.as_deref() {
        session.emit_llm_response_telemetry(
            meta.scope,
            &LlmResponseFacts {
                model_call_id,
                call_kind: &meta.call_kind,
                model: &meta.model,
                stop_reason: capture.stop_reason,
                error: capture.error.as_deref(),
                elapsed_ms,
                ttft_ms,
                pre_provider_ms: meta.pre_provider_ms,
                usage: capture.usage.as_ref(),
                prompt_tokens: prompt_cache.map(|split| split.prompt_tokens),
                cache_hit_ratio: prompt_cache.map(|split| split.cache_hit_ratio),
                text_chars: capture.text.chars().count(),
                thinking_chars: capture.thinking.chars().count(),
                tool_use_count: capture.tool_use.len(),
            },
        );
    }
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
        EVENT_TOOL_EXECUTION_STARTED, EVENT_TURN_END, EVENT_TURN_START, MAX_PREVIEW_CHARS,
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
                isolate_malformed_tool_calls: false,
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
                isolate_malformed_tool_calls: false,
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
                isolate_malformed_tool_calls: false,
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
                isolate_malformed_tool_calls: false,
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
                isolate_malformed_tool_calls: false,
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
                attrs: Vec::new(),
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
        assert_eq!(
            starts[0].payload["build"]["app_version"],
            env!("CARGO_PKG_VERSION")
        );
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

    type Captured = Arc<Mutex<Vec<ObservationTelemetryRecord>>>;

    fn dialogue_ids(turn: &str) -> ObservationIds {
        ObservationIds {
            conversation_id: Some("c-tel".into()),
            root_turn_id: Some(turn.into()),
            session_kind: Some("session_dialogue".into()),
            ..ObservationIds::default()
        }
    }

    fn capturing_session(
        recorder: Arc<ObservationRecorder>,
        ids: ObservationIds,
    ) -> (Arc<ObservationSession>, Captured) {
        let captured: Captured = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&captured);
        let session = ObservationSession::new(recorder);
        session.set_telemetry_sink(Some(Arc::new(move |record| {
            sink.lock().unwrap().push(record);
        })));
        session.bind_ids(ids);
        (session, captured)
    }

    fn captured_named(captured: &Captured, name: &str) -> Vec<ObservationTelemetryRecord> {
        captured
            .lock()
            .unwrap()
            .iter()
            .filter(|record| record.name == name)
            .cloned()
            .collect()
    }

    fn assert_uploadable(record: &ObservationTelemetryRecord) {
        assert!(record.event_id.len() <= 128, "{}", record.event_id);
        assert!(
            record.properties.len() <= 24,
            "{}: {} properties",
            record.name,
            record.properties.len()
        );
        for (key, value) in &record.properties {
            assert!(key.len() <= 64, "key too long: {key}");
            match value {
                Value::String(text) => assert!(text.len() <= 256, "{key} too long"),
                Value::Number(_) | Value::Bool(_) => {}
                other => panic!("{}: non-scalar property {key}: {other:?}", record.name),
            }
        }
    }

    struct WireProvider {
        wire: WireFacts,
        fail: bool,
    }

    #[async_trait::async_trait]
    impl LlmProvider for WireProvider {
        fn describe_request(&self, _: &LlmRequest) -> Option<WireFacts> {
            Some(self.wire.clone())
        }

        async fn stream(
            &self,
            _: &LlmRequest,
        ) -> Result<mpsc::Receiver<LlmEvent>, ProviderError> {
            let (tx, rx) = mpsc::channel(8);
            tx.send(LlmEvent::ThinkingDelta("ponder".into())).await.ok();
            if self.fail {
                tx.send(LlmEvent::Error("API error 429: slow down for secret-token".into()))
                    .await
                    .ok();
            } else {
                tx.send(LlmEvent::TextDelta("hello".into())).await.ok();
                tx.send(LlmEvent::Done {
                    stop_reason: StopReason::EndTurn,
                    usage: TokenUsage {
                        input_tokens: 1000,
                        output_tokens: 50,
                        reasoning_tokens: 20,
                        cache_creation_tokens: 100,
                        cache_read_tokens: 600,
                    },
                })
                .await
                .ok();
            }
            Ok(rx)
        }
    }

    fn placeholder_wire() -> WireFacts {
        WireFacts {
            protocol: "openai_chat",
            messages: 3,
            bytes: 900,
            assistant_messages: 1,
            tool_messages: 0,
            reasoning_kept: 0,
            reasoning_placeholders: 1,
            drop_prior_turn_reasoning: true,
            require_reasoning_content: true,
        }
    }

    fn reasoning_history() -> Vec<Message> {
        vec![
            Message::new(
                Role::User,
                vec![ContentBlock::Text { text: "first".into() }],
            ),
            Message::new(
                Role::Assistant,
                vec![
                    ContentBlock::Thinking {
                        thinking: "plan".into(),
                        signature: None,
                    },
                    ContentBlock::Text { text: "done".into() },
                ],
            ),
            Message::new(
                Role::User,
                vec![ContentBlock::Text { text: "second".into() }],
            ),
        ]
    }

    #[tokio::test]
    async fn workflow_call_reports_request_and_response_telemetry() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        let (session, captured) = capturing_session(recorder.clone(), dialogue_ids("t-1"));
        let provider = WireProvider {
            wire: placeholder_wire(),
            fail: false,
        };
        let request = sample_request("sys", reasoning_history(), Vec::new());

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

        let requests = captured_named(&captured, "llm_request");
        assert_eq!(requests.len(), 1);
        let props = &requests[0].properties;
        assert_eq!(props["turn_id"], json!("t-1"));
        assert_eq!(props["call_kind"], json!("turn"));
        assert_eq!(props["hist_reasoning_msgs"], json!(1));
        assert_eq!(props["wire_reasoning_kept"], json!(0));
        assert_eq!(props["wire_reasoning_placeholders"], json!(1));
        assert_eq!(props["wire_drop_prior_reasoning"], json!(true));
        assert_eq!(props["reasoning_dropped"], json!(1));
        assert_eq!(props["wire_bytes"], json!(900));

        let responses = captured_named(&captured, "llm_response");
        assert_eq!(responses.len(), 1);
        let props = &responses[0].properties;
        assert_eq!(props["model_call_id"], requests[0].properties["model_call_id"]);
        assert_eq!(props["outcome"], json!("ok"));
        assert_eq!(props["stop_reason"], json!("end_turn"));
        assert_eq!(props["prompt_tokens"], json!(1000));
        assert_eq!(props["cache_hit_ratio"], json!(0.6));
        assert_eq!(props["cache_read_tokens"], json!(600));
        assert_eq!(props["cache_creation_tokens"], json!(100));
        assert_eq!(props["output_tokens"], json!(50));
        assert_eq!(props["reasoning_tokens"], json!(20));
        assert_eq!(props["thinking_chars"], json!(6));
        assert_eq!(props["text_chars"], json!(5));
        assert_eq!(props["tool_use_count"], json!(0));
        for record in captured.lock().unwrap().iter() {
            assert_uploadable(record);
        }

        let events = recorder.read_events(Some("c-tel")).unwrap();
        let local = events
            .iter()
            .find(|event| event.event_type == EVENT_LLM_REQUEST)
            .expect("llm request");
        assert_eq!(local.payload["request_facts"]["history"]["reasoning_messages"], 1);
        assert_eq!(local.payload["request_facts"]["wire"]["reasoning_dropped"], 1);
        assert_eq!(local.payload["request_facts"]["wire"]["reasoning_placeholders"], 1);
    }

    #[tokio::test]
    async fn failed_stream_reports_an_error_class_without_the_message() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        let (session, captured) = capturing_session(recorder, dialogue_ids("t-err"));
        let provider = WireProvider {
            wire: placeholder_wire(),
            fail: true,
        };
        let request = sample_request("sys", reasoning_history(), Vec::new());

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

        let responses = captured_named(&captured, "llm_response");
        assert_eq!(responses.len(), 1);
        let props = &responses[0].properties;
        assert_eq!(props["outcome"], json!("error"));
        assert_eq!(props["error_class"], json!("rate_limited"));
        assert!(
            !serde_json::to_string(props).unwrap().contains("secret-token"),
            "provider message must not leave the machine: {props:?}"
        );
    }

    #[tokio::test]
    async fn auxiliary_calls_stay_out_of_cloud_telemetry_and_turn_totals() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        let (session, captured) = capturing_session(recorder, dialogue_ids("t-aux"));
        let provider = WireProvider {
            wire: placeholder_wire(),
            fail: false,
        };
        let request = sample_request("sys", reasoning_history(), Vec::new());

        let mut rx = stream_llm(
            &provider,
            &request,
            Some(Arc::clone(&session)),
            "compact",
            ObservationScope::SessionAuxiliary,
        )
        .await
        .unwrap();
        while rx.recv().await.is_some() {}
        session.emit_turn_end(ExecutionStatus::Completed, 5, Some("end_turn"), None, None);

        assert!(captured_named(&captured, "llm_request").is_empty());
        assert!(captured_named(&captured, "llm_response").is_empty());
        let summaries = captured_named(&captured, "agent_turn_summary");
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].properties["model_calls"], json!(0));
    }

    #[test]
    fn tool_calls_report_outcome_error_class_result_size_and_tool_attrs() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        let (session, captured) = capturing_session(recorder.clone(), dialogue_ids("t-tool"));

        session.emit_tool_started("call-glob", "Glob", &json!({ "pattern": "src/**/*.{rs,toml}" }));
        session.emit_tool_finished(
            "call-glob",
            "Glob",
            true,
            "No such file: src",
            Some(ToolCallTiming {
                duration_ms: 12,
                duration_us: 12_000,
                started_at_ms: 1_790_000_000_000 - 12,
                completed_at_ms: 1_790_000_000_000,
                phases: Vec::new(),
                attrs: vec![nomi_tools::phase_trace::ToolAttr {
                    key: "glob.outcome",
                    value: nomi_tools::phase_trace::AttrValue::Label("root_missing"),
                }],
            }),
            None,
        );
        session.emit_tool_started("call-cancel", "Bash", &json!({ "command": "sleep 9" }));
        session.emit_tool_cancelled("call-cancel", "Bash");

        let tools = captured_named(&captured, "tool_executed");
        assert_eq!(tools.len(), 2);
        let failed = &tools[0].properties;
        assert_eq!(failed["tool_name"], json!("Glob"));
        assert_eq!(failed["outcome"], json!("failed"));
        assert_eq!(failed["error_class"], json!("not_found"));
        assert_eq!(failed["result_bytes"], json!("No such file: src".len()));
        assert_eq!(failed["glob_outcome"], json!("root_missing"));
        assert_eq!(failed["duration_ms"], json!(12));
        assert_eq!(tools[1].properties["outcome"], json!("cancelled"));
        assert!(tools[1].properties.get("error_class").is_none());
        for record in &tools {
            assert_uploadable(record);
        }

        let events = recorder.read_events(Some("c-tel")).unwrap();
        let local = events
            .iter()
            .find(|event| event.event_type == EVENT_TOOL_EXECUTION_FAILED)
            .expect("tool failed");
        assert_eq!(local.payload["error_class"], "not_found");
        assert_eq!(local.payload["result_bytes"], "No such file: src".len());
        assert_eq!(local.payload["attrs"]["glob.outcome"], "root_missing");
    }

    #[test]
    fn nudges_and_finish_decisions_are_classified_locally_and_in_the_cloud() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        let (session, captured) = capturing_session(recorder.clone(), dialogue_ids("t-nudge"));

        session.emit_harness_nudge(
            "coding",
            "tool_turn",
            "The file does not exist; list the directory first.",
            false,
            NudgeInfo {
                kind: "tool_failure",
                detail: Some("not_found"),
                tool: Some("Read"),
            },
        );
        session.emit_harness_nudge(
            "coding",
            "hard_stop",
            "stop reading",
            true,
            NudgeInfo {
                kind: "hard_stop",
                detail: Some("read_repeat"),
                tool: None,
            },
        );
        let gate = FinishGateFacts {
            reason: "verify_gate",
            verification_mode: "hard_gate",
            needs_verification: true,
            mutated_files: true,
            verified_after_mutation: false,
            trivial_mutation: false,
            trivial_mutation_ext: None,
        };
        session.emit_harness_finish(
            "coding",
            "continue_with_nudge",
            json!({}),
            Some("run the tests before finishing"),
            Some(&gate),
        );

        let nudges = captured_named(&captured, "harness_nudge");
        assert_eq!(nudges.len(), 2);
        assert_eq!(nudges[0].properties["nudge_kind"], json!("tool_failure"));
        assert_eq!(nudges[0].properties["nudge_detail"], json!("not_found"));
        assert_eq!(nudges[0].properties["nudge_tool"], json!("Read"));
        assert_eq!(nudges[0].properties["turn_nudge_index"], json!(1));
        assert_eq!(nudges[1].properties["hard_stop"], json!(true));
        assert_eq!(nudges[1].properties["turn_nudge_index"], json!(1));
        assert_ne!(nudges[0].event_id, nudges[1].event_id);

        let finishes = captured_named(&captured, "harness_finish");
        assert_eq!(finishes.len(), 1);
        let props = &finishes[0].properties;
        assert_eq!(props["decision"], json!("continue_with_nudge"));
        assert_eq!(props["reason"], json!("verify_gate"));
        assert_eq!(props["verification_mode"], json!("hard_gate"));
        assert_eq!(props["needs_verification"], json!(true));
        assert_eq!(props["verified_after_mutation"], json!(false));
        for record in captured.lock().unwrap().iter() {
            assert_uploadable(record);
        }

        let events = recorder.read_events(Some("c-tel")).unwrap();
        let nudge = events
            .iter()
            .find(|event| event.event_type == EVENT_HARNESS_NUDGE)
            .expect("harness nudge");
        assert_eq!(nudge.payload["kind"], "tool_failure");
        assert_eq!(nudge.payload["detail"], "not_found");
        assert_eq!(nudge.payload["tool"], "Read");
        let finish = events
            .iter()
            .find(|event| event.event_type == EVENT_HARNESS_FINISH)
            .expect("harness finish");
        assert_eq!(finish.payload["gate"]["reason"], "verify_gate");
        assert_eq!(finish.payload["gate"]["needs_verification"], true);
    }

    #[tokio::test]
    async fn turn_summary_folds_the_turn_and_resets_for_the_next_one() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        let (session, captured) = capturing_session(recorder.clone(), dialogue_ids("t-1"));
        let provider = WireProvider {
            wire: placeholder_wire(),
            fail: false,
        };
        let request = sample_request("sys", reasoning_history(), Vec::new());

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
        session.emit_tool_started("call-1", "Read", &json!({ "file_path": "a.rs" }));
        session.emit_tool_finished("call-1", "Read", true, "No such file: a.rs", None, None);
        session.emit_harness_progress(
            "coding",
            "continue",
            false,
            1,
            json!({ "kpi": {
                "unique_path_reread_rate": 0.25,
                "recon_only_turns": 2,
                "serial_recon_turns": 1,
                "time_to_first_edit_ms": 4200,
                "verify_before_end": false,
                "failure_nudges_suppressed": 3,
            } }),
        );
        session.emit_harness_nudge(
            "coding",
            "tool_turn",
            "nudge text",
            false,
            NudgeInfo::source("tool_failure"),
        );
        session.emit_harness_finish("coding", "continue_with_nudge", json!({}), Some("again"), None);
        session.emit_turn_end(ExecutionStatus::Completed, 1234, Some("end_turn"), None, None);

        let summaries = captured_named(&captured, "agent_turn_summary");
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].event_id, "obs:turn:t-1");
        assert_uploadable(&summaries[0]);
        let props = &summaries[0].properties;
        assert_eq!(props["status"], json!("completed"));
        assert_eq!(props["stop_reason"], json!("end_turn"));
        assert_eq!(props["elapsed_ms"], json!(1234));
        assert_eq!(props["model_calls"], json!(1));
        assert_eq!(props["tool_calls"], json!(1));
        assert_eq!(props["tool_errors"], json!(1));
        assert_eq!(props["nudges"], json!(1));
        assert_eq!(props["finish_blocks"], json!(1));
        assert_eq!(props["prompt_tokens"], json!(1000));
        assert_eq!(props["output_tokens"], json!(50));
        assert_eq!(props["cache_read_tokens"], json!(600));
        assert_eq!(props["thinking_chars"], json!(6));
        assert_eq!(props["reread_rate"], json!(0.25));
        assert_eq!(props["recon_only_turns"], json!(2));
        assert_eq!(props["time_to_first_edit_ms"], json!(4200));
        assert_eq!(props["verify_before_end"], json!(false));
        assert_eq!(props["failure_nudges_suppressed"], json!(3));

        let events = recorder.read_events(Some("c-tel")).unwrap();
        let local = events
            .iter()
            .find(|event| event.event_type == EVENT_TURN_END)
            .expect("turn end");
        assert_eq!(local.payload["summary"]["model_calls"], 1);
        assert_eq!(local.payload["summary"]["reasoning_dropped_calls"], 1);
        assert_eq!(local.payload["summary"]["nudges_by_kind"]["tool_failure"], 1);

        session.bind_ids(dialogue_ids("t-2"));
        session.emit_turn_end(ExecutionStatus::Completed, 9, Some("end_turn"), None, None);
        let summaries = captured_named(&captured, "agent_turn_summary");
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[1].event_id, "obs:turn:t-2");
        assert_eq!(summaries[1].properties["model_calls"], json!(0));
        assert_eq!(summaries[1].properties["tool_calls"], json!(0));
        assert!(summaries[1].properties.get("reread_rate").is_none());
    }

    #[test]
    fn eval_sessions_emit_no_cloud_telemetry() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = ObservationRecorder::isolated(dir.path());
        let (session, captured) = capturing_session(
            recorder,
            ObservationIds {
                session_kind: Some("eval".into()),
                ..dialogue_ids("t-eval")
            },
        );

        session.emit_tool_started("call-1", "Read", &json!({}));
        session.emit_tool_finished("call-1", "Read", false, "ok", None, None);
        session.emit_harness_nudge("coding", "tool_turn", "x", false, NudgeInfo::source("x"));
        session.emit_turn_end(ExecutionStatus::Completed, 1, None, None, None);

        assert!(captured.lock().unwrap().is_empty());
    }
}
