//! Incremental builder for a single [`TurnTrace`].

use std::collections::{BTreeMap, HashMap};

use uuid::Uuid;

use crate::redact::redact_preview;
use crate::types::{
    SpanKind, SpanStatus, TraceArtifactMeta, TraceSpan, TurnSummary, TurnTrace, SCHEMA_VERSION,
};

/// Identity / routing metadata supplied when a turn starts.
#[derive(Debug, Clone)]
pub struct TurnTraceMeta {
    pub conversation_id: String,
    pub msg_id: String,
    pub root_turn_id: String,
    pub session_kind: String,
    pub origin: Option<String>,
    pub companion: bool,
    pub channel_platform: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
}

/// Token / context counters applied at turn completion.
#[derive(Debug, Clone, Copy, Default)]
pub struct TokenCounts {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub context_tokens: u64,
    pub context_window: u64,
}

/// Accumulates spans and summary fields for one agent turn.
#[derive(Debug)]
pub struct TurnTraceBuilder {
    trace: TurnTrace,
    /// span_id → index in `trace.spans`
    open_spans: HashMap<String, usize>,
    /// tool call_id → span_id
    tool_spans: HashMap<String, String>,
    current_llm_span: Option<String>,
    current_thinking_span: Option<String>,
    /// Characters seen via [`Self::note_text_delta`] (never stores full text).
    text_chars: u64,
    parent_stack: Vec<String>,
}

impl TurnTraceBuilder {
    /// Start a new turn with a fresh `trace_id` and `started_at_ms = now`.
    pub fn new(meta: TurnTraceMeta) -> Self {
        let started_at_ms = now_ms();
        let trace = TurnTrace {
            schema_version: SCHEMA_VERSION,
            trace_id: Uuid::now_v7().to_string(),
            conversation_id: meta.conversation_id,
            msg_id: meta.msg_id,
            root_turn_id: meta.root_turn_id,
            session_kind: meta.session_kind,
            origin: meta.origin,
            companion: meta.companion,
            channel_platform: meta.channel_platform,
            provider: meta.provider,
            model: meta.model,
            started_at_ms,
            ended_at_ms: None,
            spans: Vec::new(),
            summary: TurnSummary::default(),
        };
        Self {
            trace,
            open_spans: HashMap::new(),
            tool_spans: HashMap::new(),
            current_llm_span: None,
            current_thinking_span: None,
            text_chars: 0,
            parent_stack: Vec::new(),
        }
    }

    pub fn trace_id(&self) -> &str {
        &self.trace.trace_id
    }

    pub fn started_at_ms(&self) -> u64 {
        self.trace.started_at_ms
    }

    /// Open a new span; returns its `span_id`.
    pub fn start_span(&mut self, kind: SpanKind, name: impl Into<String>) -> String {
        let span_id = Uuid::now_v7().to_string();
        let parent_span_id = self.parent_stack.last().cloned();
        let span = TraceSpan {
            span_id: span_id.clone(),
            parent_span_id,
            kind,
            name: name.into(),
            started_at_ms: now_ms(),
            ended_at_ms: None,
            status: SpanStatus::Running,
            attributes: BTreeMap::new(),
            preview: None,
        };
        let idx = self.trace.spans.len();
        self.trace.spans.push(span);
        self.open_spans.insert(span_id.clone(), idx);
        self.parent_stack.push(span_id.clone());
        span_id
    }

    /// End an open span. Unknown `span_id` is a no-op.
    pub fn end_span(
        &mut self,
        span_id: &str,
        status: SpanStatus,
        preview: Option<&str>,
        attrs: BTreeMap<String, serde_json::Value>,
    ) {
        let Some(&idx) = self.open_spans.get(span_id) else {
            // Already closed or unknown — still allow attribute merge on closed spans.
            if let Some(span) = self.trace.spans.iter_mut().find(|s| s.span_id == span_id) {
                for (k, v) in attrs {
                    span.attributes.insert(k, v);
                }
                if let Some(p) = preview {
                    span.preview = Some(redact_preview(p));
                }
                if span.ended_at_ms.is_none() {
                    span.ended_at_ms = Some(now_ms());
                    span.status = status;
                }
            }
            return;
        };
        let span = &mut self.trace.spans[idx];
        span.ended_at_ms = Some(now_ms());
        span.status = status;
        for (k, v) in attrs {
            span.attributes.insert(k, v);
        }
        if let Some(p) = preview {
            span.preview = Some(redact_preview(p));
        }
        self.open_spans.remove(span_id);
        if self.parent_stack.last().map(String::as_str) == Some(span_id) {
            self.parent_stack.pop();
        } else {
            self.parent_stack.retain(|id| id != span_id);
        }
        if self.current_llm_span.as_deref() == Some(span_id) {
            self.current_llm_span = None;
        }
        if self.current_thinking_span.as_deref() == Some(span_id) {
            self.current_thinking_span = None;
        }
    }

    /// Record a tool call start (`call_id` is the provider tool-call id).
    pub fn note_tool_start(
        &mut self,
        call_id: impl Into<String>,
        name: impl Into<String>,
        args_preview: Option<&str>,
    ) -> String {
        let call_id = call_id.into();
        let name = name.into();
        let span_id = self.start_span(SpanKind::Tool, name.clone());
        self.tool_spans.insert(call_id.clone(), span_id.clone());
        self.trace.summary.tool_call_count =
            self.trace.summary.tool_call_count.saturating_add(1);

        if let Some(&idx) = self.open_spans.get(&span_id) {
            let span = &mut self.trace.spans[idx];
            span.attributes
                .insert("call_id".into(), serde_json::Value::String(call_id));
            span.attributes
                .insert("tool_name".into(), serde_json::Value::String(name));
            if let Some(args) = args_preview {
                span.preview = Some(redact_preview(args));
            }
        }
        span_id
    }

    /// Record a tool call end by `call_id`.
    ///
    /// `artifacts` is metadata-only (id / kind / relative_path / size / sha).
    /// Absolute paths and binary payloads must never be passed here.
    pub fn note_tool_end(
        &mut self,
        call_id: &str,
        status: SpanStatus,
        output_preview: Option<&str>,
        artifacts: &[TraceArtifactMeta],
    ) {
        if matches!(status, SpanStatus::Error) {
            self.trace.summary.tool_error_count =
                self.trace.summary.tool_error_count.saturating_add(1);
        }
        let Some(span_id) = self.tool_spans.remove(call_id) else {
            return;
        };
        let mut attrs = BTreeMap::new();
        attrs.insert(
            "call_id".into(),
            serde_json::Value::String(call_id.to_owned()),
        );
        if !artifacts.is_empty() {
            let count = artifacts.len() as u32;
            attrs.insert("artifact_count".into(), serde_json::json!(count));
            attrs.insert(
                "artifacts".into(),
                serde_json::to_value(artifacts).unwrap_or_else(|_| serde_json::json!([])),
            );
            self.trace.summary.artifact_count =
                self.trace.summary.artifact_count.saturating_add(count);
            self.trace.summary.artifacts.extend(artifacts.iter().cloned());
        }
        self.end_span(&span_id, status, output_preview, attrs);
    }

    /// Start an LLM round span and mark it current for text-delta accumulation.
    pub fn note_llm_round_start(&mut self, name: impl Into<String>) -> String {
        let span_id = self.start_span(SpanKind::Llm, name);
        self.current_llm_span = Some(span_id.clone());
        self.trace.summary.llm_round_count =
            self.trace.summary.llm_round_count.saturating_add(1);
        span_id
    }

    /// Accumulate assistant text length only (never stores the full text).
    pub fn note_text_delta(&mut self, delta: &str) {
        let added = delta.chars().count() as u64;
        self.text_chars = self.text_chars.saturating_add(added);
        if let Some(span_id) = self.current_llm_span.clone() {
            if let Some(&idx) = self.open_spans.get(&span_id) {
                let span = &mut self.trace.spans[idx];
                span.attributes.insert(
                    "text_chars".into(),
                    serde_json::json!(self.text_chars),
                );
            } else if let Some(span) = self
                .trace
                .spans
                .iter_mut()
                .find(|s| s.span_id == span_id)
            {
                span.attributes.insert(
                    "text_chars".into(),
                    serde_json::json!(self.text_chars),
                );
            }
        }
    }

    /// Start (or continue) a thinking span with a redacted preview snippet.
    pub fn note_thinking(&mut self, preview: Option<&str>) -> String {
        if let Some(existing) = self.current_thinking_span.clone() {
            if self.open_spans.contains_key(&existing) {
                if let Some(p) = preview {
                    if let Some(&idx) = self.open_spans.get(&existing) {
                        self.trace.spans[idx].preview = Some(redact_preview(p));
                    }
                }
                return existing;
            }
        }
        let span_id = self.start_span(SpanKind::Thinking, "thinking");
        self.current_thinking_span = Some(span_id.clone());
        if let Some(p) = preview {
            if let Some(&idx) = self.open_spans.get(&span_id) {
                self.trace.spans[idx].preview = Some(redact_preview(p));
            }
        }
        span_id
    }

    /// Apply successful turn-completion stats.
    pub fn apply_turn_completed(
        &mut self,
        elapsed_ms: Option<i64>,
        tokens: TokenCounts,
        stop_reason: Option<String>,
    ) {
        self.trace.summary.elapsed_ms = elapsed_ms;
        self.trace.summary.input_tokens = tokens.input_tokens;
        self.trace.summary.output_tokens = tokens.output_tokens;
        self.trace.summary.cache_creation_tokens = tokens.cache_creation_tokens;
        self.trace.summary.cache_read_tokens = tokens.cache_read_tokens;
        self.trace.summary.context_tokens = tokens.context_tokens;
        self.trace.summary.context_window = tokens.context_window;
        self.trace.summary.stop_reason = stop_reason;
        self.trace.summary.success = Some(true);
        self.trace.summary.error_code = None;
        self.trace.summary.error_message = None;
    }

    /// Mark the turn as failed.
    pub fn apply_error(&mut self, code: impl Into<String>, message: impl Into<String>) {
        let message = message.into();
        self.trace.summary.success = Some(false);
        self.trace.summary.error_code = Some(code.into());
        self.trace.summary.error_message = Some(redact_preview(&message));
    }

    /// Close remaining open spans and return the finished [`TurnTrace`].
    ///
    /// Running spans are closed as [`SpanStatus::Ok`] when `success == Some(true)`,
    /// otherwise [`SpanStatus::Cancelled`].
    pub fn finalize(mut self) -> TurnTrace {
        let end_ms = now_ms();
        self.trace.ended_at_ms = Some(end_ms);
        if self.trace.summary.elapsed_ms.is_none() {
            let elapsed = end_ms.saturating_sub(self.trace.started_at_ms) as i64;
            self.trace.summary.elapsed_ms = Some(elapsed);
        }

        let close_status = if self.trace.summary.success == Some(true) {
            SpanStatus::Ok
        } else {
            SpanStatus::Cancelled
        };

        let open_ids: Vec<String> = self.open_spans.keys().cloned().collect();
        for span_id in open_ids {
            self.end_span(&span_id, close_status, None, BTreeMap::new());
        }

        self.trace
    }
}

fn now_ms() -> u64 {
    let ms = chrono::Utc::now().timestamp_millis();
    if ms < 0 {
        0
    } else {
        ms as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> TurnTraceMeta {
        TurnTraceMeta {
            conversation_id: "conv-1".into(),
            msg_id: "msg-1".into(),
            root_turn_id: "root-1".into(),
            session_kind: "session_dialogue".into(),
            origin: None,
            companion: false,
            channel_platform: None,
            provider: Some("test".into()),
            model: Some("m".into()),
        }
    }

    #[test]
    fn span_lifecycle_ok_on_success() {
        let mut b = TurnTraceBuilder::new(meta());
        let llm = b.note_llm_round_start("round-1");
        b.note_text_delta("hello ");
        b.note_text_delta("world");
        let tool = b.note_tool_start("call_1", "bash", Some(r#"{"cmd":"echo hi"}"#));
        assert_ne!(llm, tool);
        b.note_tool_end("call_1", SpanStatus::Ok, Some("hi"), &[]);
        b.end_span(&llm, SpanStatus::Ok, None, BTreeMap::new());
        b.apply_turn_completed(
            Some(42),
            TokenCounts {
                input_tokens: 10,
                output_tokens: 5,
                ..Default::default()
            },
            Some("end_turn".into()),
        );
        let trace = b.finalize();
        assert_eq!(trace.schema_version, SCHEMA_VERSION);
        assert_eq!(trace.summary.success, Some(true));
        assert_eq!(trace.summary.tool_call_count, 1);
        assert_eq!(trace.summary.tool_error_count, 0);
        assert_eq!(trace.summary.llm_round_count, 1);
        assert_eq!(trace.summary.elapsed_ms, Some(42));
        assert!(trace.ended_at_ms.is_some());
        assert!(trace.spans.iter().all(|s| s.status != SpanStatus::Running));
        let llm_span = trace.spans.iter().find(|s| s.kind == SpanKind::Llm).unwrap();
        assert_eq!(llm_span.attributes.get("text_chars"), Some(&serde_json::json!(11)));
        let tool_span = trace.spans.iter().find(|s| s.kind == SpanKind::Tool).unwrap();
        assert_eq!(tool_span.status, SpanStatus::Ok);
        assert!(tool_span.preview.is_some());
    }

    #[test]
    fn tool_artifacts_attach_to_span_and_summary() {
        let mut b = TurnTraceBuilder::new(meta());
        b.note_tool_start("call_a", "generate_image", Some("{}"));
        let artifacts = vec![TraceArtifactMeta {
            id: "art_1".into(),
            kind: "image".into(),
            mime_type: "image/png".into(),
            relative_path: "nomifun-artifacts/out.png".into(),
            size_bytes: 128,
            sha256: "abc".into(),
            call_id: Some("call_a".into()),
            tool_name: Some("generate_image".into()),
        }];
        b.note_tool_end("call_a", SpanStatus::Ok, Some("ok"), &artifacts);
        let trace = b.finalize();
        assert_eq!(trace.summary.artifact_count, 1);
        assert_eq!(trace.summary.artifacts.len(), 1);
        assert_eq!(
            trace.summary.artifacts[0].relative_path,
            "nomifun-artifacts/out.png"
        );
        let tool_span = trace.spans.iter().find(|s| s.kind == SpanKind::Tool).unwrap();
        assert_eq!(
            tool_span.attributes.get("artifact_count"),
            Some(&serde_json::json!(1))
        );
        assert!(tool_span.attributes.get("artifacts").is_some());
    }

    #[test]
    fn finalize_cancels_open_spans_on_error() {
        let mut b = TurnTraceBuilder::new(meta());
        let _ = b.start_span(SpanKind::System, "work");
        let _ = b.note_thinking(Some("sk-ABCDEFGHIJ0123456789xyz plan"));
        b.apply_error("E_FAIL", "boom sk-ABCDEFGHIJ0123456789xyz");
        let trace = b.finalize();
        assert_eq!(trace.summary.success, Some(false));
        assert!(trace
            .summary
            .error_message
            .as_deref()
            .unwrap()
            .contains("[REDACTED_SECRET]"));
        assert!(trace.spans.iter().all(|s| s.status == SpanStatus::Cancelled));
        let thinking = trace
            .spans
            .iter()
            .find(|s| s.kind == SpanKind::Thinking)
            .unwrap();
        assert!(thinking
            .preview
            .as_deref()
            .unwrap()
            .contains("[REDACTED_SECRET]"));
    }

    #[test]
    fn tool_error_increments_counter() {
        let mut b = TurnTraceBuilder::new(meta());
        b.note_tool_start("c1", "x", None);
        b.note_tool_end("c1", SpanStatus::Error, Some("fail"), &[]);
        assert_eq!(b.trace.summary.tool_call_count, 1);
        assert_eq!(b.trace.summary.tool_error_count, 1);
    }
}
