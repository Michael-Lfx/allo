//! Trace models persisted as JSON / JSONL (`schema_version = 1`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Current on-disk schema version for [`TurnTrace`] and [`TraceIndexEntry`].
pub const SCHEMA_VERSION: u32 = 1;

/// One completed (or finalized) agent turn with nested spans and a summary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TurnTrace {
    pub schema_version: u32,
    /// UUID string identifying this trace document.
    pub trace_id: String,
    pub conversation_id: String,
    pub msg_id: String,
    pub root_turn_id: String,
    /// `"session_dialogue"` | `"companion"` | `"cron"` | `"autowork"` | `"idmm"` |
    /// `"agent_execution"` | `"channel"` | `"other"`
    pub session_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    pub companion: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_platform: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub started_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at_ms: Option<u64>,
    #[serde(default)]
    pub spans: Vec<TraceSpan>,
    pub summary: TurnSummary,
}

/// A single timed span inside a turn (LLM round, tool call, thinking, …).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TraceSpan {
    pub span_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_span_id: Option<String>,
    pub kind: SpanKind,
    pub name: String,
    pub started_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at_ms: Option<u64>,
    pub status: SpanStatus,
    #[serde(default)]
    pub attributes: BTreeMap<String, serde_json::Value>,
    /// Truncated/redacted preview text (never full secrets).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

/// Aggregate counters and outcome for a turn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TurnSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<i64>,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_creation_tokens: u64,
    #[serde(default)]
    pub cache_read_tokens: u64,
    #[serde(default)]
    pub context_tokens: u64,
    #[serde(default)]
    pub context_window: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub tool_call_count: u32,
    #[serde(default)]
    pub tool_error_count: u32,
    #[serde(default)]
    pub llm_round_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

/// One line in the append-only `index.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TraceIndexEntry {
    pub schema_version: u32,
    pub trace_id: String,
    pub conversation_id: String,
    pub msg_id: String,
    pub root_turn_id: String,
    pub session_kind: String,
    pub started_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<i64>,
    pub tool_call_count: u32,
    pub tool_error_count: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,
    /// Path relative to the agent-traces root, e.g. `turns/{conv}/{trace}.json`.
    pub relative_path: String,
}

impl TraceIndexEntry {
    pub fn from_trace(trace: &TurnTrace, relative_path: String) -> Self {
        Self {
            schema_version: trace.schema_version,
            trace_id: trace.trace_id.clone(),
            conversation_id: trace.conversation_id.clone(),
            msg_id: trace.msg_id.clone(),
            root_turn_id: trace.root_turn_id.clone(),
            session_kind: trace.session_kind.clone(),
            started_at_ms: trace.started_at_ms,
            ended_at_ms: trace.ended_at_ms,
            elapsed_ms: trace.summary.elapsed_ms,
            tool_call_count: trace.summary.tool_call_count,
            tool_error_count: trace.summary.tool_error_count,
            input_tokens: trace.summary.input_tokens,
            output_tokens: trace.summary.output_tokens,
            stop_reason: trace.summary.stop_reason.clone(),
            success: trace.summary.success,
            relative_path,
        }
    }
}

/// Kind of work a span represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanKind {
    Llm,
    Tool,
    Thinking,
    Text,
    Moa,
    System,
    Compact,
    Goal,
    Error,
}

/// Lifecycle status of a span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanStatus {
    Ok,
    Error,
    Cancelled,
    Running,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_version_is_one() {
        assert_eq!(SCHEMA_VERSION, 1);
    }

    #[test]
    fn turn_trace_roundtrips_json() {
        let mut attrs = BTreeMap::new();
        attrs.insert("k".into(), serde_json::json!("v"));
        let trace = TurnTrace {
            schema_version: SCHEMA_VERSION,
            trace_id: "t1".into(),
            conversation_id: "c1".into(),
            msg_id: "m1".into(),
            root_turn_id: "r1".into(),
            session_kind: "session_dialogue".into(),
            origin: None,
            companion: false,
            channel_platform: None,
            provider: Some("openai".into()),
            model: Some("gpt".into()),
            started_at_ms: 100,
            ended_at_ms: Some(200),
            spans: vec![TraceSpan {
                span_id: "s1".into(),
                parent_span_id: None,
                kind: SpanKind::Llm,
                name: "round".into(),
                started_at_ms: 100,
                ended_at_ms: Some(150),
                status: SpanStatus::Ok,
                attributes: attrs,
                preview: Some("hi".into()),
            }],
            summary: TurnSummary {
                elapsed_ms: Some(100),
                input_tokens: 1,
                output_tokens: 2,
                success: Some(true),
                ..Default::default()
            },
        };
        let json = serde_json::to_string(&trace).unwrap();
        let back: TurnTrace = serde_json::from_str(&json).unwrap();
        assert_eq!(back, trace);
    }

    #[test]
    fn index_entry_from_trace() {
        let trace = TurnTrace {
            schema_version: SCHEMA_VERSION,
            trace_id: "t1".into(),
            conversation_id: "c1".into(),
            msg_id: "m1".into(),
            root_turn_id: "r1".into(),
            session_kind: "cron".into(),
            origin: Some("cron".into()),
            companion: false,
            channel_platform: None,
            provider: None,
            model: None,
            started_at_ms: 1,
            ended_at_ms: Some(2),
            spans: vec![],
            summary: TurnSummary {
                elapsed_ms: Some(1),
                tool_call_count: 3,
                tool_error_count: 1,
                input_tokens: 10,
                output_tokens: 20,
                stop_reason: Some("end_turn".into()),
                success: Some(false),
                ..Default::default()
            },
        };
        let entry = TraceIndexEntry::from_trace(&trace, "turns/c1/t1.json".into());
        assert_eq!(entry.relative_path, "turns/c1/t1.json");
        assert_eq!(entry.tool_call_count, 3);
        assert_eq!(entry.success, Some(false));
    }
}
