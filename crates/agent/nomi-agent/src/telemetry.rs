//! Product-telemetry records derived from session observation.
//!
//! The local JSONL log keeps full payloads. Records built here leave the
//! machine, so they carry only counts, sizes, durations, opaque ids and short
//! closed-set labels. Prompt text, tool arguments, tool output and file paths
//! never enter a record.

use std::collections::BTreeMap;

use nomi_agent_trace::ObservationIds;
use nomi_coding::FinishGateFacts;
use nomi_providers::WireFacts;
use nomi_tools::phase_trace::{AttrValue, ToolAttr};
use nomi_types::llm::{LlmRequest, ThinkingConfig};
use nomi_types::message::{ContentBlock, Message, Role, TokenUsage};
use serde_json::{Value, json};

use crate::tool_execution::ToolCallTiming;

pub(crate) const EVENT_TOOL_EXECUTED: &str = "tool_executed";
pub(crate) const EVENT_LLM_REQUEST: &str = "llm_request";
pub(crate) const EVENT_LLM_RESPONSE: &str = "llm_response";
pub(crate) const EVENT_HARNESS_NUDGE: &str = "harness_nudge";
pub(crate) const EVENT_HARNESS_FINISH: &str = "harness_finish";
pub(crate) const EVENT_TURN_SUMMARY: &str = "agent_turn_summary";

/// Cloud ingest rejects events with more properties than this.
pub(crate) const MAX_PROPERTIES: usize = 24;
pub(crate) const MAX_STRING_BYTES: usize = 256;
const MAX_KEY_BYTES: usize = 64;

/// Ordered scalar property set. Insertion order is priority order: when a
/// record would exceed [`MAX_PROPERTIES`], the entries added last are dropped.
#[derive(Default)]
pub(crate) struct Props {
    entries: Vec<(String, Value)>,
}

impl Props {
    pub(crate) fn text(&mut self, key: &str, value: &str) {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return;
        }
        self.set(key, json!(clip_chars(trimmed, MAX_STRING_BYTES)));
    }

    pub(crate) fn opt_text(&mut self, key: &str, value: Option<&str>) {
        if let Some(value) = value {
            self.text(key, value);
        }
    }

    pub(crate) fn num<T: TryInto<i64>>(&mut self, key: &str, value: T) {
        self.set(key, json!(value.try_into().unwrap_or(i64::MAX)));
    }

    pub(crate) fn opt_num<T: TryInto<i64>>(&mut self, key: &str, value: Option<T>) {
        if let Some(value) = value {
            self.num(key, value);
        }
    }

    pub(crate) fn flag(&mut self, key: &str, value: bool) {
        self.set(key, json!(value));
    }

    pub(crate) fn opt_flag(&mut self, key: &str, value: Option<bool>) {
        if let Some(value) = value {
            self.flag(key, value);
        }
    }

    pub(crate) fn ratio(&mut self, key: &str, value: f64) {
        if value.is_finite() {
            self.set(key, json!((value * 10_000.0).round() / 10_000.0));
        }
    }

    pub(crate) fn opt_ratio(&mut self, key: &str, value: Option<f64>) {
        if let Some(value) = value {
            self.ratio(key, value);
        }
    }

    fn set(&mut self, key: &str, value: Value) {
        if key.len() > MAX_KEY_BYTES {
            return;
        }
        match self.entries.iter_mut().find(|(existing, _)| existing == key) {
            Some(slot) => slot.1 = value,
            None => self.entries.push((key.to_owned(), value)),
        }
    }

    pub(crate) fn finish(mut self) -> BTreeMap<String, Value> {
        self.entries.truncate(MAX_PROPERTIES);
        self.entries.into_iter().collect()
    }
}

pub(crate) fn clip_chars(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

fn base(ids: &ObservationIds) -> Props {
    let mut props = Props::default();
    props.text("feature", "conversation");
    props.opt_text("session_id", ids.conversation_id.as_deref());
    props.opt_text("session_kind", ids.session_kind.as_deref());
    props.opt_text("turn_id", ids.root_turn_id.as_deref());
    props
}

/// Counts over the canonical history one request carries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct HistoryFacts {
    pub messages: usize,
    pub user_turns: usize,
    pub assistant_messages: usize,
    /// Assistant messages that carry non-empty reasoning.
    pub reasoning_messages: usize,
    pub tool_calls: usize,
    pub tool_results: usize,
    pub tool_result_errors: usize,
    pub tool_result_bytes: usize,
    /// Assistant messages since the last user turn began: how deep the current
    /// tool loop is.
    pub loop_depth: usize,
}

impl HistoryFacts {
    pub(crate) fn of(messages: &[Message]) -> Self {
        let turn_start = messages.iter().rposition(Message::starts_user_turn);
        let mut facts = Self {
            messages: messages.len(),
            ..Self::default()
        };
        for (index, message) in messages.iter().enumerate() {
            if message.starts_user_turn() {
                facts.user_turns += 1;
            }
            if message.role == Role::Assistant {
                facts.assistant_messages += 1;
                if message.content.iter().any(|block| {
                    matches!(block, ContentBlock::Thinking { thinking, .. } if !thinking.trim().is_empty())
                }) {
                    facts.reasoning_messages += 1;
                }
                if turn_start.is_none_or(|start| index > start) {
                    facts.loop_depth += 1;
                }
            }
            for block in &message.content {
                match block {
                    ContentBlock::ToolUse { .. } => facts.tool_calls += 1,
                    ContentBlock::ToolResult {
                        content, is_error, ..
                    } => {
                        facts.tool_results += 1;
                        facts.tool_result_bytes += content.len();
                        if *is_error {
                            facts.tool_result_errors += 1;
                        }
                    }
                    _ => {}
                }
            }
        }
        facts
    }

    pub(crate) fn to_value(&self) -> Value {
        json!({
            "messages": self.messages,
            "user_turns": self.user_turns,
            "assistant_messages": self.assistant_messages,
            "reasoning_messages": self.reasoning_messages,
            "tool_calls": self.tool_calls,
            "tool_results": self.tool_results,
            "tool_result_errors": self.tool_result_errors,
            "tool_result_bytes": self.tool_result_bytes,
            "loop_depth": self.loop_depth,
        })
    }
}

fn thinking_label(request: &LlmRequest) -> Option<&'static str> {
    match request.thinking {
        Some(ThinkingConfig::Enabled { .. }) => Some("enabled"),
        Some(ThinkingConfig::Disabled) => Some("disabled"),
        None => None,
    }
}

pub(crate) fn reasoning_dropped(history: &HistoryFacts, wire: &WireFacts) -> usize {
    history.reasoning_messages.saturating_sub(wire.reasoning_kept)
}

pub(crate) struct LlmRequestFacts<'a> {
    pub model_call_id: &'a str,
    pub call_kind: &'a str,
    pub request: &'a LlmRequest,
    pub system_hash: &'a str,
    pub tools_hash: &'a str,
    pub message_count: usize,
    pub prefix_break: &'a str,
    pub prefix_break_index: i64,
    pub history: &'a HistoryFacts,
    pub wire: Option<&'a WireFacts>,
}

pub(crate) fn llm_request_props(
    ids: &ObservationIds,
    facts: &LlmRequestFacts<'_>,
) -> BTreeMap<String, Value> {
    let mut props = base(ids);
    props.text("llm_model", &facts.request.model);
    props.text("system_hash", facts.system_hash);
    props.text("tools_hash", facts.tools_hash);
    props.num("message_count", facts.message_count);
    props.text("prefix_break", facts.prefix_break);
    props.num("prefix_break_index", facts.prefix_break_index);
    props.text("call_kind", facts.call_kind);
    props.text("model_call_id", facts.model_call_id);
    props.num("tool_count", facts.request.tools.len());
    props.num("loop_depth", facts.history.loop_depth);
    props.num("hist_reasoning_msgs", facts.history.reasoning_messages);
    props.num("hist_tool_results", facts.history.tool_results);
    if let Some(wire) = facts.wire {
        props.num("wire_reasoning_kept", wire.reasoning_kept);
        props.num("wire_reasoning_placeholders", wire.reasoning_placeholders);
        props.flag("wire_drop_prior_reasoning", wire.drop_prior_turn_reasoning);
        props.num("wire_bytes", wire.bytes);
        props.num("reasoning_dropped", reasoning_dropped(facts.history, wire));
    }
    props.opt_text("thinking", thinking_label(facts.request));
    props.opt_text("reasoning_effort", facts.request.reasoning_effort.as_deref());
    props.opt_num("max_tokens", facts.request.max_tokens);
    props.finish()
}

/// Local-only companion to [`llm_request_props`]; not size-limited.
pub(crate) fn request_facts_value(history: &HistoryFacts, wire: Option<&WireFacts>) -> Value {
    json!({
        "history": history.to_value(),
        "wire": wire.map(|wire| {
            let mut value = serde_json::to_value(wire).unwrap_or(Value::Null);
            value["reasoning_dropped"] = json!(reasoning_dropped(history, wire));
            value
        }),
    })
}

pub(crate) struct LlmResponseFacts<'a> {
    pub model_call_id: &'a str,
    pub call_kind: &'a str,
    pub model: &'a str,
    pub stop_reason: Option<&'a str>,
    pub error: Option<&'a str>,
    pub elapsed_ms: u64,
    pub ttft_ms: Option<u64>,
    pub pre_provider_ms: Option<u64>,
    pub usage: Option<&'a TokenUsage>,
    pub prompt_tokens: Option<u64>,
    pub cache_hit_ratio: Option<f64>,
    pub text_chars: usize,
    pub thinking_chars: usize,
    pub tool_use_count: usize,
}

pub(crate) fn llm_response_props(
    ids: &ObservationIds,
    facts: &LlmResponseFacts<'_>,
) -> BTreeMap<String, Value> {
    let mut props = base(ids);
    props.text("model_call_id", facts.model_call_id);
    props.text("call_kind", facts.call_kind);
    props.text("llm_model", facts.model);
    props.text("outcome", if facts.error.is_some() { "error" } else { "ok" });
    props.opt_text("stop_reason", facts.stop_reason);
    props.opt_text("error_class", facts.error.map(classify_llm_error));
    props.num("elapsed_ms", facts.elapsed_ms);
    props.opt_num("ttft_ms", facts.ttft_ms);
    props.opt_num(
        "generation_ms",
        facts.ttft_ms.map(|ttft| facts.elapsed_ms.saturating_sub(ttft)),
    );
    props.opt_num("pre_provider_ms", facts.pre_provider_ms);
    props.opt_num("prompt_tokens", facts.prompt_tokens);
    if let Some(usage) = facts.usage {
        props.num("output_tokens", usage.output_tokens);
        props.num("reasoning_tokens", usage.reasoning_tokens);
        props.num("cache_read_tokens", usage.cache_read_tokens);
        props.num("cache_creation_tokens", usage.cache_creation_tokens);
    }
    props.opt_ratio("cache_hit_ratio", facts.cache_hit_ratio);
    props.num("text_chars", facts.text_chars);
    props.num("thinking_chars", facts.thinking_chars);
    props.num("tool_use_count", facts.tool_use_count);
    props.finish()
}

/// Closed-set class of a provider failure, for grouping without keeping the
/// message (which can echo request content).
pub(crate) fn classify_llm_error(message: &str) -> &'static str {
    let message = message.trim();
    if let Some(rest) = message.strip_prefix("API error ") {
        let status: u16 = rest
            .split(|c: char| !c.is_ascii_digit())
            .next()
            .and_then(|digits| digits.parse().ok())
            .unwrap_or(0);
        return match status {
            401 | 403 => "auth",
            408 => "timeout",
            429 => "rate_limited",
            400..=499 => "client_error",
            500..=599 => "server_error",
            _ => "api_error",
        };
    }
    const PREFIXES: &[(&str, &str)] = &[
        ("Rate limited", "rate_limited"),
        ("Prompt too long", "context_overflow"),
        ("Initial request timeout", "timeout"),
        ("Provider stream truncated", "stream_truncated"),
        ("Connection error", "network"),
        ("HTTP error", "network"),
        ("SSE parse error", "protocol"),
        ("Provider configuration error", "config"),
    ];
    if let Some((_, class)) = PREFIXES
        .iter()
        .find(|(prefix, _)| message.starts_with(prefix))
    {
        return class;
    }
    let lower = message.to_ascii_lowercase();
    if lower.contains("timed out") || lower.contains("timeout") {
        "timeout"
    } else {
        "other"
    }
}

pub(crate) struct ToolFacts<'a> {
    pub name: &'a str,
    pub outcome: &'a str,
    pub model_call_id: Option<&'a str>,
    pub result_bytes: Option<usize>,
    pub error_class: Option<&'a str>,
    pub timing: Option<&'a ToolCallTiming>,
}

pub(crate) fn tool_executed_props(
    ids: &ObservationIds,
    facts: &ToolFacts<'_>,
) -> BTreeMap<String, Value> {
    let mut props = base(ids);
    props.text("tool_name", facts.name);
    props.text("outcome", facts.outcome);
    if let Some(timing) = facts.timing {
        props.num("duration_ms", timing.duration_ms);
        props.num("duration_us", timing.duration_us);
    }
    props.opt_text("model_call_id", facts.model_call_id);
    props.opt_num("result_bytes", facts.result_bytes);
    props.opt_text("error_class", facts.error_class);
    if let Some((name, micros)) = facts.timing.and_then(slowest_phase) {
        props.text("slowest_phase", name);
        props.num("slowest_phase_ms", micros / 1000);
    }
    if let Some(timing) = facts.timing {
        for attr in &timing.attrs {
            set_attr(&mut props, attr);
        }
    }
    props.finish()
}

/// The slowest recorded phase other than the enclosing `tool.execute`, which
/// by construction spans the tool's own phases.
fn slowest_phase(timing: &ToolCallTiming) -> Option<(&'static str, u64)> {
    timing
        .phases
        .iter()
        .filter(|phase| phase.name != "tool.execute")
        .max_by_key(|phase| phase.micros)
        .map(|phase| (phase.name, phase.micros))
}

fn attr_key(key: &str) -> String {
    key.replace('.', "_")
}

fn set_attr(props: &mut Props, attr: &ToolAttr) {
    let key = attr_key(attr.key);
    match attr.value {
        AttrValue::Int(value) => props.num(&key, value),
        AttrValue::Bool(value) => props.flag(&key, value),
        AttrValue::Label(value) => props.text(&key, value),
    }
}

pub(crate) fn tool_attrs_value(attrs: &[ToolAttr]) -> Option<Value> {
    if attrs.is_empty() {
        return None;
    }
    let mut object = serde_json::Map::new();
    for attr in attrs {
        let value = match attr.value {
            AttrValue::Int(value) => json!(value),
            AttrValue::Bool(value) => json!(value),
            AttrValue::Label(value) => json!(value),
        };
        object.insert(attr.key.to_owned(), value);
    }
    Some(Value::Object(object))
}

pub(crate) struct NudgeFacts<'a> {
    pub profile: &'a str,
    pub source: &'a str,
    pub kind: &'a str,
    pub detail: Option<&'a str>,
    pub tool: Option<&'a str>,
    pub hard_stop: bool,
    pub text_chars: usize,
    pub model_call_id: Option<&'a str>,
    pub turn_nudge_index: u32,
}

pub(crate) fn harness_nudge_props(
    ids: &ObservationIds,
    facts: &NudgeFacts<'_>,
) -> BTreeMap<String, Value> {
    let mut props = base(ids);
    props.text("profile", facts.profile);
    props.text("source", facts.source);
    props.text("nudge_kind", facts.kind);
    props.opt_text("nudge_detail", facts.detail);
    props.opt_text("nudge_tool", facts.tool);
    props.flag("hard_stop", facts.hard_stop);
    props.num("text_chars", facts.text_chars);
    props.opt_text("model_call_id", facts.model_call_id);
    props.num("turn_nudge_index", facts.turn_nudge_index);
    props.finish()
}

pub(crate) struct FinishFacts<'a> {
    pub profile: &'a str,
    pub decision: &'a str,
    pub gate: Option<&'a FinishGateFacts>,
    pub nudge_chars: Option<usize>,
    pub model_call_id: Option<&'a str>,
}

pub(crate) fn harness_finish_props(
    ids: &ObservationIds,
    facts: &FinishFacts<'_>,
) -> BTreeMap<String, Value> {
    let mut props = base(ids);
    props.text("profile", facts.profile);
    props.text("decision", facts.decision);
    if let Some(gate) = facts.gate {
        props.text("reason", gate.reason);
        props.text("verification_mode", gate.verification_mode);
        props.flag("needs_verification", gate.needs_verification);
        props.flag("mutated_files", gate.mutated_files);
        props.flag("verified_after_mutation", gate.verified_after_mutation);
        props.flag("trivial_mutation", gate.trivial_mutation);
        props.opt_text("trivial_mutation_ext", gate.trivial_mutation_ext.as_deref());
    }
    props.opt_num("nudge_chars", facts.nudge_chars);
    props.opt_text("model_call_id", facts.model_call_id);
    props.finish()
}

/// Latest harness KPI numbers for the running turn, read from the counters the
/// harness already serializes for `harness/progress`.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct HarnessView {
    pub reread_rate: Option<f64>,
    pub recon_only_turns: Option<u64>,
    pub serial_recon_turns: Option<u64>,
    pub time_to_first_edit_ms: Option<u64>,
    pub verify_before_end: Option<bool>,
    pub failure_nudges_suppressed: Option<u64>,
}

impl HarnessView {
    pub(crate) fn from_counters(counters: &Value) -> Option<Self> {
        let kpi = counters.get("kpi").or_else(|| counters.get("profiler"))?;
        Some(Self {
            reread_rate: kpi.get("unique_path_reread_rate").and_then(Value::as_f64),
            recon_only_turns: kpi.get("recon_only_turns").and_then(Value::as_u64),
            serial_recon_turns: kpi.get("serial_recon_turns").and_then(Value::as_u64),
            time_to_first_edit_ms: kpi.get("time_to_first_edit_ms").and_then(Value::as_u64),
            verify_before_end: kpi.get("verify_before_end").and_then(Value::as_bool),
            failure_nudges_suppressed: kpi
                .get("failure_nudges_suppressed")
                .and_then(Value::as_u64),
        })
    }
}

/// Running totals for one root turn, folded from the observation hooks and
/// reported once when the turn ends.
#[derive(Debug, Clone, Default)]
pub(crate) struct TurnStats {
    pub model_calls: u32,
    pub model_errors: u32,
    pub llm_wall_ms: u64,
    pub prompt_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub thinking_chars: u64,
    pub reasoning_dropped_calls: u32,
    pub tool_calls: u32,
    pub tool_errors: u32,
    pub tool_wall_ms: u64,
    pub nudges: u32,
    pub hard_stops: u32,
    pub finish_blocks: u32,
    pub nudges_by_kind: BTreeMap<String, u32>,
    pub harness: Option<HarnessView>,
}

impl TurnStats {
    /// A hard-stop record repeats a nudge already counted when its text was
    /// injected, so it only bumps `hard_stops`.
    pub(crate) fn note_nudge(&mut self, kind: &str, hard_stop: bool) -> u32 {
        if hard_stop {
            self.hard_stops += 1;
        } else {
            self.nudges += 1;
            *self.nudges_by_kind.entry(kind.to_owned()).or_default() += 1;
        }
        self.nudges
    }

    pub(crate) fn to_value(&self) -> Value {
        let harness = self.harness.as_ref().map(|view| {
            json!({
                "reread_rate": view.reread_rate,
                "recon_only_turns": view.recon_only_turns,
                "serial_recon_turns": view.serial_recon_turns,
                "time_to_first_edit_ms": view.time_to_first_edit_ms,
                "verify_before_end": view.verify_before_end,
                "failure_nudges_suppressed": view.failure_nudges_suppressed,
            })
        });
        json!({
            "model_calls": self.model_calls,
            "model_errors": self.model_errors,
            "llm_wall_ms": self.llm_wall_ms,
            "prompt_tokens": self.prompt_tokens,
            "output_tokens": self.output_tokens,
            "reasoning_tokens": self.reasoning_tokens,
            "cache_read_tokens": self.cache_read_tokens,
            "cache_creation_tokens": self.cache_creation_tokens,
            "thinking_chars": self.thinking_chars,
            "reasoning_dropped_calls": self.reasoning_dropped_calls,
            "tool_calls": self.tool_calls,
            "tool_errors": self.tool_errors,
            "tool_wall_ms": self.tool_wall_ms,
            "nudges": self.nudges,
            "hard_stops": self.hard_stops,
            "finish_blocks": self.finish_blocks,
            "nudges_by_kind": self.nudges_by_kind,
            "harness": harness,
        })
    }
}

pub(crate) struct TurnSummaryFacts<'a> {
    pub status: Option<&'a str>,
    pub stop_reason: Option<&'a str>,
    pub elapsed_ms: u64,
    pub stats: &'a TurnStats,
}

pub(crate) fn turn_summary_props(
    ids: &ObservationIds,
    facts: &TurnSummaryFacts<'_>,
) -> BTreeMap<String, Value> {
    let stats = facts.stats;
    let mut props = base(ids);
    props.opt_text("status", facts.status);
    props.opt_text("stop_reason", facts.stop_reason);
    props.num("elapsed_ms", facts.elapsed_ms);
    props.num("model_calls", stats.model_calls);
    props.num("tool_calls", stats.tool_calls);
    props.num("tool_errors", stats.tool_errors);
    props.num("nudges", stats.nudges);
    props.num("finish_blocks", stats.finish_blocks);
    props.num("prompt_tokens", stats.prompt_tokens);
    props.num("output_tokens", stats.output_tokens);
    props.num("cache_read_tokens", stats.cache_read_tokens);
    props.num("thinking_chars", stats.thinking_chars);
    props.num("llm_wall_ms", stats.llm_wall_ms);
    props.num("tool_wall_ms", stats.tool_wall_ms);
    if let Some(view) = &stats.harness {
        props.opt_ratio("reread_rate", view.reread_rate);
        props.opt_num("recon_only_turns", view.recon_only_turns);
        props.opt_num("time_to_first_edit_ms", view.time_to_first_edit_ms);
        props.opt_flag("verify_before_end", view.verify_before_end);
        props.opt_num("failure_nudges_suppressed", view.failure_nudges_suppressed);
        props.opt_num("serial_recon_turns", view.serial_recon_turns);
    }
    props.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomi_tools::phase_trace::ToolPhase;

    fn ids() -> ObservationIds {
        ObservationIds {
            conversation_id: Some("conv-1".into()),
            root_turn_id: Some("turn-1".into()),
            session_kind: Some("session_dialogue".into()),
            ..ObservationIds::default()
        }
    }

    fn assert_wire_safe(props: &BTreeMap<String, Value>) {
        assert!(props.len() <= MAX_PROPERTIES, "{} props: {props:?}", props.len());
        for (key, value) in props {
            assert!(key.len() <= MAX_KEY_BYTES, "key too long: {key}");
            match value {
                Value::String(text) => assert!(text.len() <= MAX_STRING_BYTES, "{key} too long"),
                Value::Number(_) | Value::Bool(_) => {}
                other => panic!("{key} is not a scalar: {other:?}"),
            }
        }
    }

    fn user(text: &str) -> Message {
        Message::new(Role::User, vec![ContentBlock::Text { text: text.into() }])
    }

    fn assistant_call(reasoning: &str, id: &str) -> Message {
        let mut content = Vec::new();
        if !reasoning.is_empty() {
            content.push(ContentBlock::Thinking {
                thinking: reasoning.into(),
                signature: None,
            });
        }
        content.push(ContentBlock::ToolUse {
            id: id.into(),
            name: "Read".into(),
            input: json!({ "file_path": "secret/path.rs" }),
            extra: None,
        });
        Message::new(Role::Assistant, content)
    }

    fn tool_result(id: &str, body: &str, is_error: bool) -> Message {
        Message::new(
            Role::User,
            vec![ContentBlock::ToolResult {
                tool_use_id: id.into(),
                content: body.into(),
                is_error,
                images: Vec::new(),
            }],
        )
    }

    fn request(messages: Vec<Message>) -> LlmRequest {
        LlmRequest {
            model: "deepseek-v4".into(),
            system: "sys".into(),
            messages,
            tools: Vec::new(),
            max_tokens: Some(4096),
            thinking: Some(ThinkingConfig::Enabled { budget_tokens: 1000 }),
            reasoning_effort: Some("high".into()),
            temperature: None,
            retain_provider_round: false,
            isolate_malformed_tool_calls: false,
        }
    }

    #[test]
    fn props_keep_insertion_priority_and_cap_the_count() {
        let mut props = Props::default();
        for index in 0..40 {
            props.num(&format!("k{index:02}"), index);
        }
        let out = props.finish();
        assert_eq!(out.len(), MAX_PROPERTIES);
        assert!(out.contains_key("k00"));
        assert!(out.contains_key("k23"));
        assert!(!out.contains_key("k24"));
    }

    #[test]
    fn props_skip_blank_text_and_oversized_keys_and_replace_duplicates() {
        let mut props = Props::default();
        props.text("blank", "   ");
        props.opt_text("none", None);
        props.num(&"k".repeat(MAX_KEY_BYTES + 1), 1);
        props.num("n", 1);
        props.num("n", 2);
        props.ratio("nan", f64::NAN);
        props.ratio("r", 0.123_456);
        let out = props.finish();
        assert_eq!(out.len(), 2, "{out:?}");
        assert_eq!(out["n"], json!(2));
        assert_eq!(out["r"], json!(0.1235));
    }

    #[test]
    fn props_clip_long_text_on_a_char_boundary() {
        let raw = "工具名称".repeat(80);
        let mut props = Props::default();
        props.text("name", &raw);
        let out = props.finish();
        let clipped = out["name"].as_str().unwrap();
        assert!(clipped.len() <= MAX_STRING_BYTES);
        assert_eq!(clipped, &raw[..clipped.len()]);
    }

    #[test]
    fn num_saturates_instead_of_wrapping() {
        let mut props = Props::default();
        props.num("big", u64::MAX);
        assert_eq!(props.finish()["big"], json!(i64::MAX));
    }

    #[test]
    fn history_facts_count_reasoning_results_and_loop_depth() {
        let messages = vec![
            user("first question"),
            assistant_call("think-a", "c1"),
            tool_result("c1", "12345", false),
            Message::new(
                Role::Assistant,
                vec![ContentBlock::Text { text: "answer".into() }],
            ),
            user("second question"),
            assistant_call("think-b", "c2"),
            tool_result("c2", "boom", true),
            assistant_call("", "c3"),
            tool_result("c3", "ok", false),
        ];

        let facts = HistoryFacts::of(&messages);

        assert_eq!(facts.messages, 9);
        assert_eq!(facts.user_turns, 2);
        assert_eq!(facts.assistant_messages, 4);
        assert_eq!(facts.reasoning_messages, 2);
        assert_eq!(facts.tool_calls, 3);
        assert_eq!(facts.tool_results, 3);
        assert_eq!(facts.tool_result_errors, 1);
        assert_eq!(facts.tool_result_bytes, 5 + 4 + 2);
        assert_eq!(facts.loop_depth, 2, "assistant steps since 'second question'");
    }

    #[test]
    fn context_only_user_messages_do_not_reset_loop_depth() {
        let carrier = Message::new(
            Role::User,
            vec![
                ContentBlock::Text {
                    text: format!("{}date: today", nomi_types::message::TURN_TAIL_CONTEXT_PREFIX),
                },
                ContentBlock::ToolResult {
                    tool_use_id: "c1".into(),
                    content: "ok".into(),
                    is_error: false,
                    images: Vec::new(),
                },
            ],
        );
        let messages = vec![user("go"), assistant_call("a", "c1"), carrier, assistant_call("b", "c2")];

        assert_eq!(HistoryFacts::of(&messages).loop_depth, 2);
        assert_eq!(HistoryFacts::of(&messages).user_turns, 1);
    }

    #[test]
    fn history_facts_of_empty_history_is_all_zero() {
        assert_eq!(HistoryFacts::of(&[]), HistoryFacts::default());
    }

    #[test]
    fn llm_request_props_report_reasoning_exposure_without_content() {
        let messages = vec![
            user("secret prompt text"),
            assistant_call("private chain of thought", "c1"),
            tool_result("c1", "file body", false),
            assistant_call("more thoughts", "c2"),
        ];
        let history = HistoryFacts::of(&messages);
        let req = request(messages);
        let wire = WireFacts {
            protocol: "openai_chat",
            messages: 5,
            bytes: 2048,
            assistant_messages: 2,
            tool_messages: 1,
            reasoning_kept: 1,
            reasoning_placeholders: 1,
            drop_prior_turn_reasoning: true,
            require_reasoning_content: true,
        };

        let props = llm_request_props(
            &ids(),
            &LlmRequestFacts {
                model_call_id: "mc-1",
                call_kind: "turn",
                request: &req,
                system_hash: "aaaa",
                tools_hash: "bbbb",
                message_count: 4,
                prefix_break: "message",
                prefix_break_index: 2,
                history: &history,
                wire: Some(&wire),
            },
        );

        assert_wire_safe(&props);
        assert_eq!(props["feature"], json!("conversation"));
        assert_eq!(props["session_id"], json!("conv-1"));
        assert_eq!(props["turn_id"], json!("turn-1"));
        assert_eq!(props["model_call_id"], json!("mc-1"));
        assert_eq!(props["loop_depth"], json!(2));
        assert_eq!(props["hist_reasoning_msgs"], json!(2));
        assert_eq!(props["wire_reasoning_kept"], json!(1));
        assert_eq!(props["wire_reasoning_placeholders"], json!(1));
        assert_eq!(props["reasoning_dropped"], json!(1));
        assert_eq!(props["wire_drop_prior_reasoning"], json!(true));
        assert_eq!(props["wire_bytes"], json!(2048));
        assert_eq!(props["thinking"], json!("enabled"));
        assert_eq!(props["reasoning_effort"], json!("high"));
        assert_eq!(props["max_tokens"], json!(4096));
        let encoded = serde_json::to_string(&props).unwrap();
        assert!(!encoded.contains("secret"), "{encoded}");
        assert!(!encoded.contains("chain of thought"), "{encoded}");
    }

    #[test]
    fn llm_request_props_without_wire_facts_omit_the_wire_fields() {
        let history = HistoryFacts::default();
        let mut req = request(Vec::new());
        req.thinking = None;
        req.reasoning_effort = None;
        req.max_tokens = None;

        let props = llm_request_props(
            &ids(),
            &LlmRequestFacts {
                model_call_id: "mc-1",
                call_kind: "turn",
                request: &req,
                system_hash: "aaaa",
                tools_hash: "bbbb",
                message_count: 0,
                prefix_break: "first",
                prefix_break_index: 0,
                history: &history,
                wire: None,
            },
        );

        assert_wire_safe(&props);
        for key in [
            "wire_bytes",
            "reasoning_dropped",
            "thinking",
            "reasoning_effort",
            "max_tokens",
        ] {
            assert!(!props.contains_key(key), "{key} must be absent: {props:?}");
        }
    }

    #[test]
    fn request_facts_value_reports_dropped_reasoning_for_local_analysis() {
        let history = HistoryFacts {
            reasoning_messages: 3,
            ..HistoryFacts::default()
        };
        let wire = WireFacts {
            reasoning_kept: 1,
            ..WireFacts::default()
        };

        let value = request_facts_value(&history, Some(&wire));

        assert_eq!(value["history"]["reasoning_messages"], 3);
        assert_eq!(value["wire"]["reasoning_kept"], 1);
        assert_eq!(value["wire"]["reasoning_dropped"], 2);
        assert!(request_facts_value(&history, None)["wire"].is_null());
    }

    #[test]
    fn llm_response_props_split_timing_usage_and_output_shape() {
        let usage = TokenUsage {
            input_tokens: 900,
            output_tokens: 120,
            reasoning_tokens: 80,
            cache_creation_tokens: 10,
            cache_read_tokens: 600,
        };

        let props = llm_response_props(
            &ids(),
            &LlmResponseFacts {
                model_call_id: "mc-2",
                call_kind: "turn",
                model: "deepseek-v4",
                stop_reason: Some("tool_use"),
                error: None,
                elapsed_ms: 5000,
                ttft_ms: Some(1200),
                pre_provider_ms: Some(40),
                usage: Some(&usage),
                prompt_tokens: Some(1000),
                cache_hit_ratio: Some(0.6),
                text_chars: 0,
                thinking_chars: 4000,
                tool_use_count: 2,
            },
        );

        assert_wire_safe(&props);
        assert_eq!(props["outcome"], json!("ok"));
        assert_eq!(props["stop_reason"], json!("tool_use"));
        assert_eq!(props["generation_ms"], json!(3800));
        assert_eq!(props["pre_provider_ms"], json!(40));
        assert_eq!(props["prompt_tokens"], json!(1000));
        assert_eq!(props["output_tokens"], json!(120));
        assert_eq!(props["reasoning_tokens"], json!(80));
        assert_eq!(props["cache_read_tokens"], json!(600));
        assert_eq!(props["cache_hit_ratio"], json!(0.6));
        assert_eq!(props["thinking_chars"], json!(4000));
        assert_eq!(props["tool_use_count"], json!(2));
        assert!(!props.contains_key("error_class"));
    }

    #[test]
    fn llm_response_props_classify_errors_without_keeping_the_message() {
        let props = llm_response_props(
            &ids(),
            &LlmResponseFacts {
                model_call_id: "mc-3",
                call_kind: "turn",
                model: "m",
                stop_reason: None,
                error: Some("Rate limited, retry after 500ms: key sk-secret-token exhausted"),
                elapsed_ms: 10,
                ttft_ms: None,
                pre_provider_ms: None,
                usage: None,
                prompt_tokens: None,
                cache_hit_ratio: None,
                text_chars: 0,
                thinking_chars: 0,
                tool_use_count: 0,
            },
        );

        assert_wire_safe(&props);
        assert_eq!(props["outcome"], json!("error"));
        assert_eq!(props["error_class"], json!("rate_limited"));
        assert!(!serde_json::to_string(&props).unwrap().contains("sk-secret"));
        assert!(!props.contains_key("output_tokens"));
        assert!(!props.contains_key("ttft_ms"));
    }

    #[test]
    fn llm_error_classes_follow_the_provider_error_display_forms() {
        for (message, class) in [
            ("API error 429: slow down", "rate_limited"),
            ("API error 401: bad key", "auth"),
            ("API error 403: nope", "auth"),
            ("API error 408: slow", "timeout"),
            ("API error 400: bad request", "client_error"),
            ("API error 503: overloaded", "server_error"),
            ("API error 999: ?", "api_error"),
            ("Rate limited, retry after 1ms: x", "rate_limited"),
            ("Prompt too long: 300k tokens", "context_overflow"),
            ("Initial request timeout: deadline exceeded", "timeout"),
            ("Provider stream truncated: no terminal marker", "stream_truncated"),
            ("Connection error: reset by peer", "network"),
            ("HTTP error: error sending request", "network"),
            ("SSE parse error: bad json", "protocol"),
            ("Provider configuration error: no key", "config"),
            ("operation timed out after 30s", "timeout"),
            ("something unexpected", "other"),
        ] {
            assert_eq!(classify_llm_error(message), class, "{message}");
        }
    }

    fn timing(phases: &[(&'static str, u64)], attrs: Vec<ToolAttr>) -> ToolCallTiming {
        ToolCallTiming {
            duration_ms: 1500,
            duration_us: 1_500_000,
            started_at_ms: 1,
            completed_at_ms: 1501,
            phases: phases
                .iter()
                .map(|(name, micros)| ToolPhase { name, micros: *micros })
                .collect(),
            attrs,
        }
    }

    #[test]
    fn tool_executed_props_surface_the_slowest_inner_phase_and_tool_attrs() {
        let timing = timing(
            &[
                ("hooks.pre", 2_000),
                ("tool.execute", 1_400_000),
                ("bash.spawn", 1_300_000),
                ("tool.post_process", 5_000),
            ],
            vec![
                ToolAttr {
                    key: "glob.matched",
                    value: AttrValue::Int(7),
                },
                ToolAttr {
                    key: "glob.walk_stop",
                    value: AttrValue::Label("completed"),
                },
                ToolAttr {
                    key: "glob.prefix_narrowed",
                    value: AttrValue::Bool(true),
                },
            ],
        );

        let props = tool_executed_props(
            &ids(),
            &ToolFacts {
                name: "Glob",
                outcome: "completed",
                model_call_id: Some("mc-9"),
                result_bytes: Some(321),
                error_class: None,
                timing: Some(&timing),
            },
        );

        assert_wire_safe(&props);
        assert_eq!(props["tool_name"], json!("Glob"));
        assert_eq!(props["duration_ms"], json!(1500));
        assert_eq!(props["slowest_phase"], json!("bash.spawn"));
        assert_eq!(props["slowest_phase_ms"], json!(1300));
        assert_eq!(props["result_bytes"], json!(321));
        assert_eq!(props["model_call_id"], json!("mc-9"));
        assert_eq!(props["glob_matched"], json!(7));
        assert_eq!(props["glob_walk_stop"], json!("completed"));
        assert_eq!(props["glob_prefix_narrowed"], json!(true));
    }

    #[test]
    fn tool_executed_props_without_timing_keep_the_legacy_fields_only() {
        let props = tool_executed_props(
            &ids(),
            &ToolFacts {
                name: "Read",
                outcome: "cancelled",
                model_call_id: None,
                result_bytes: None,
                error_class: None,
                timing: None,
            },
        );

        assert_wire_safe(&props);
        assert_eq!(props["outcome"], json!("cancelled"));
        for key in ["duration_ms", "slowest_phase", "result_bytes", "model_call_id"] {
            assert!(!props.contains_key(key), "{key}: {props:?}");
        }
    }

    #[test]
    fn tool_attrs_never_push_a_record_over_the_property_cap() {
        let attrs = (0..40)
            .map(|index| ToolAttr {
                key: Box::leak(format!("tool.attr{index}").into_boxed_str()),
                value: AttrValue::Int(index),
            })
            .collect();
        let timing = timing(&[("tool.execute", 10)], attrs);

        let props = tool_executed_props(
            &ids(),
            &ToolFacts {
                name: "Wide",
                outcome: "completed",
                model_call_id: Some("mc"),
                result_bytes: Some(1),
                error_class: None,
                timing: Some(&timing),
            },
        );

        assert_wire_safe(&props);
        assert_eq!(props.len(), MAX_PROPERTIES);
        assert!(props.contains_key("duration_ms"), "core fields outrank attrs");
    }

    #[test]
    fn tool_attrs_value_is_a_flat_object_or_absent() {
        assert!(tool_attrs_value(&[]).is_none());
        let value = tool_attrs_value(&[
            ToolAttr {
                key: "glob.matched",
                value: AttrValue::Int(3),
            },
            ToolAttr {
                key: "glob.outcome",
                value: AttrValue::Label("matched"),
            },
        ])
        .unwrap();
        assert_eq!(value, json!({ "glob.matched": 3, "glob.outcome": "matched" }));
    }

    #[test]
    fn nudge_props_carry_the_classification_but_not_the_text() {
        let props = harness_nudge_props(
            &ids(),
            &NudgeFacts {
                profile: "coding",
                source: "tool_turn",
                kind: "tool_failure",
                detail: Some("not_found"),
                tool: Some("Read"),
                hard_stop: false,
                text_chars: 321,
                model_call_id: Some("mc-4"),
                turn_nudge_index: 3,
            },
        );

        assert_wire_safe(&props);
        assert_eq!(props["nudge_kind"], json!("tool_failure"));
        assert_eq!(props["nudge_detail"], json!("not_found"));
        assert_eq!(props["nudge_tool"], json!("Read"));
        assert_eq!(props["text_chars"], json!(321));
        assert_eq!(props["turn_nudge_index"], json!(3));
        assert_eq!(props["hard_stop"], json!(false));
    }

    #[test]
    fn finish_props_include_the_gate_facts() {
        let gate = FinishGateFacts {
            reason: "unverified_end",
            verification_mode: "hard_gate",
            needs_verification: false,
            mutated_files: true,
            verified_after_mutation: false,
            trivial_mutation: true,
            trivial_mutation_ext: Some("md".into()),
        };

        let props = harness_finish_props(
            &ids(),
            &FinishFacts {
                profile: "coding",
                decision: "allow",
                gate: Some(&gate),
                nudge_chars: None,
                model_call_id: Some("mc-5"),
            },
        );

        assert_wire_safe(&props);
        assert_eq!(props["reason"], json!("unverified_end"));
        assert_eq!(props["verification_mode"], json!("hard_gate"));
        assert_eq!(props["mutated_files"], json!(true));
        assert_eq!(props["verified_after_mutation"], json!(false));
        assert_eq!(props["trivial_mutation_ext"], json!("md"));
        assert!(!props.contains_key("nudge_chars"));
    }

    #[test]
    fn harness_view_reads_coding_and_office_counter_shapes() {
        let coding = json!({
            "progress": {},
            "kpi": {
                "unique_path_reread_rate": 0.25,
                "recon_only_turns": 4,
                "serial_recon_turns": 2,
                "time_to_first_edit_ms": 9000,
                "verify_before_end": true,
                "failure_nudges_suppressed": 3
            },
            "verify_fail_streak": 0
        });
        let office = json!({ "profiler": { "recon_only_turns": 1 }, "office": {} });

        let view = HarnessView::from_counters(&coding).unwrap();
        assert_eq!(view.reread_rate, Some(0.25));
        assert_eq!(view.recon_only_turns, Some(4));
        assert_eq!(view.time_to_first_edit_ms, Some(9000));
        assert_eq!(view.verify_before_end, Some(true));
        assert_eq!(view.failure_nudges_suppressed, Some(3));
        assert_eq!(HarnessView::from_counters(&office).unwrap().recon_only_turns, Some(1));
        assert!(HarnessView::from_counters(&json!({})).is_none());
    }

    #[test]
    fn turn_summary_props_fit_the_cap_even_with_every_field_present() {
        let mut stats = TurnStats {
            model_calls: 12,
            tool_calls: 30,
            tool_errors: 4,
            prompt_tokens: 100_000,
            output_tokens: 8_000,
            reasoning_tokens: 3_000,
            cache_read_tokens: 60_000,
            thinking_chars: 12_345,
            llm_wall_ms: 80_000,
            tool_wall_ms: 20_000,
            finish_blocks: 1,
            harness: Some(HarnessView {
                reread_rate: Some(0.4),
                recon_only_turns: Some(5),
                serial_recon_turns: Some(2),
                time_to_first_edit_ms: Some(42_000),
                verify_before_end: Some(false),
                failure_nudges_suppressed: Some(2),
            }),
            ..TurnStats::default()
        };
        stats.note_nudge("tool_failure", false);
        stats.note_nudge("explore_hard_stop", false);
        stats.note_nudge("hard_stop", true);

        let props = turn_summary_props(
            &ids(),
            &TurnSummaryFacts {
                status: Some("completed"),
                stop_reason: Some("end_turn"),
                elapsed_ms: 100_000,
                stats: &stats,
            },
        );

        assert_wire_safe(&props);
        assert_eq!(props.len(), MAX_PROPERTIES);
        assert_eq!(props["nudges"], json!(2));
        assert_eq!(props["model_calls"], json!(12));
        assert_eq!(props["reread_rate"], json!(0.4));
        assert_eq!(props["verify_before_end"], json!(false));
        assert_eq!(props["serial_recon_turns"], json!(2));
    }

    #[test]
    fn turn_stats_value_groups_nudges_by_kind_and_counts_hard_stops() {
        let mut stats = TurnStats::default();
        assert_eq!(stats.note_nudge("read_repeat", false), 1);
        assert_eq!(stats.note_nudge("read_repeat", false), 2);
        assert_eq!(stats.note_nudge("explore_hard_stop", false), 3);
        assert_eq!(stats.note_nudge("hard_stop", true), 3);

        let value = stats.to_value();

        assert_eq!(value["nudges"], 3);
        assert_eq!(value["hard_stops"], 1);
        assert_eq!(value["nudges_by_kind"], json!({ "read_repeat": 2, "explore_hard_stop": 1 }));
        assert!(value["harness"].is_null());
    }

    #[test]
    fn clip_chars_stays_on_char_boundary() {
        let raw = "工具名称".repeat(80);
        let clipped = clip_chars(&raw, MAX_STRING_BYTES);
        assert!(clipped.len() <= MAX_STRING_BYTES);
        assert!(raw.is_char_boundary(clipped.len()) || clipped.is_empty());
        assert_eq!(clipped, &raw[..clipped.len()]);
    }
}
