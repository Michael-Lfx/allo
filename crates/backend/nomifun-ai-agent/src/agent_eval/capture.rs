//! Isolated eval capture sink. Records a redacted trajectory without emitting
//! into the conversation websocket or runtime registry.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use nomi_agent::output::{OutputSink, ToolMediaDelivery};
use nomi_agent_eval::{EvalCaseTrace, EvalTrajectoryEvent};
use nomi_types::tool::ToolImage;

const PREVIEW_CHARS: usize = 4000;

#[derive(Default)]
pub struct EvalCaptureSink {
    text: Mutex<String>,
    tools: Mutex<Vec<String>>,
    events: Mutex<Vec<EvalTrajectoryEvent>>,
    tool_errors: AtomicU32,
}

impl EvalCaptureSink {
    pub fn snapshot(&self) -> (String, Vec<String>, u32) {
        let text = self.text.lock().map(|g| g.clone()).unwrap_or_default();
        let tools = self.tools.lock().map(|g| g.clone()).unwrap_or_default();
        (text, tools, self.tool_errors.load(Ordering::Relaxed))
    }

    pub fn snapshot_trace(&self, case_id: &str, live: bool) -> EvalCaseTrace {
        let (assistant_text, _, _) = self.snapshot();
        let events = self.events.lock().map(|g| g.clone()).unwrap_or_default();
        EvalCaseTrace {
            case_id: case_id.to_owned(),
            live,
            assistant_text,
            events,
            artifacts: Vec::new(),
        }
    }
}

impl OutputSink for EvalCaptureSink {
    fn emit_text_delta(&self, text: &str, _msg_id: &str) {
        if let Ok(mut guard) = self.text.lock() {
            guard.push_str(text);
        }
        push_event(
            &self.events,
            EvalTrajectoryEvent {
                kind: "text".into(),
                ts_ms: now_ms(),
                content: Some(truncate(text)),
                ..EvalTrajectoryEvent::default()
            },
        );
    }
    fn emit_thinking(&self, text: &str, _msg_id: &str) {
        if text.trim().is_empty() {
            return;
        }
        push_event(
            &self.events,
            EvalTrajectoryEvent {
                kind: "thinking".into(),
                ts_ms: now_ms(),
                content: Some(truncate(text)),
                ..EvalTrajectoryEvent::default()
            },
        );
    }
    fn emit_tool_call(&self, tool_use_id: &str, name: &str, input: &str) {
        if let Ok(mut guard) = self.tools.lock() {
            guard.push(name.to_owned());
        }
        push_event(
            &self.events,
            EvalTrajectoryEvent {
                kind: "tool_call".into(),
                ts_ms: now_ms(),
                tool_use_id: Some(tool_use_id.to_owned()),
                name: Some(name.to_owned()),
                input: Some(truncate(&nomi_redact::redact_secrets_owned(input.to_owned()))),
                ..EvalTrajectoryEvent::default()
            },
        );
    }
    fn emit_tool_result(&self, tool_use_id: &str, name: &str, is_error: bool, content: &str) {
        if is_error {
            self.tool_errors.fetch_add(1, Ordering::Relaxed);
        }
        push_event(
            &self.events,
            EvalTrajectoryEvent {
                kind: "tool_result".into(),
                ts_ms: now_ms(),
                tool_use_id: Some(tool_use_id.to_owned()),
                name: Some(name.to_owned()),
                content: Some(truncate(&nomi_redact::redact_secrets_owned(content.to_owned()))),
                is_error: Some(is_error),
                ..EvalTrajectoryEvent::default()
            },
        );
    }
    fn emit_tool_result_with_images(
        &self,
        tool_use_id: &str,
        name: &str,
        is_error: bool,
        content: &str,
        _images: &[ToolImage],
    ) -> ToolMediaDelivery {
        self.emit_tool_result(tool_use_id, name, is_error, content);
        ToolMediaDelivery::Unmanaged
    }
    fn emit_stream_start(&self, _msg_id: &str) {}
    fn emit_stream_end(
        &self,
        _msg_id: &str,
        _turns: usize,
        _input_tokens: u64,
        _output_tokens: u64,
        _cache_creation_tokens: u64,
        _cache_read_tokens: u64,
    ) {
    }
    fn emit_error(&self, msg: &str) {
        push_event(
            &self.events,
            EvalTrajectoryEvent {
                kind: "error".into(),
                ts_ms: now_ms(),
                content: Some(truncate(msg)),
                is_error: Some(true),
                ..EvalTrajectoryEvent::default()
            },
        );
    }
    fn emit_info(&self, msg: &str) {
        push_event(
            &self.events,
            EvalTrajectoryEvent {
                kind: "info".into(),
                ts_ms: now_ms(),
                content: Some(truncate(msg)),
                ..EvalTrajectoryEvent::default()
            },
        );
    }
}

fn push_event(events: &Mutex<Vec<EvalTrajectoryEvent>>, event: EvalTrajectoryEvent) {
    if let Ok(mut guard) = events.lock() {
        if matches!(event.kind.as_str(), "text" | "thinking") {
            if let Some(last) = guard.last_mut() {
                if last.kind == event.kind {
                    match (&mut last.content, &event.content) {
                        (Some(existing), Some(chunk)) => existing.push_str(chunk),
                        (empty, Some(chunk)) if empty.is_none() => *empty = Some(chunk.clone()),
                        _ => {}
                    }
                    last.ts_ms = event.ts_ms;
                    return;
                }
            }
        }
        if guard.len() < 400 {
            guard.push(event);
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn truncate(text: &str) -> String {
    if text.chars().count() <= PREVIEW_CHARS {
        text.to_owned()
    } else {
        let clipped: String = text.chars().take(PREVIEW_CHARS).collect();
        format!("{clipped}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_tools_and_errors() {
        let sink = EvalCaptureSink::default();
        sink.emit_text_delta("hi", "m");
        sink.emit_tool_call("1", "Write", r#"{"path":"a.md"}"#);
        sink.emit_tool_result("1", "Write", true, "no");
        let (text, tools, errors) = sink.snapshot();
        assert_eq!(text, "hi");
        assert_eq!(tools, vec!["Write".to_string()]);
        assert_eq!(errors, 1);
        let trace = sink.snapshot_trace("memo-write", true);
        assert_eq!(trace.events.len(), 3);
        assert_eq!(trace.events[1].kind, "tool_call");
        assert!(trace.events[1].input.as_deref().unwrap().contains("a.md"));
    }

    #[test]
    fn coalesces_consecutive_text_deltas() {
        let sink = EvalCaptureSink::default();
        sink.emit_text_delta("hel", "m");
        sink.emit_text_delta("lo", "m");
        sink.emit_thinking("a", "m");
        sink.emit_thinking("b", "m");
        let trace = sink.snapshot_trace("memo-write", true);
        assert_eq!(trace.events.len(), 2);
        assert_eq!(trace.events[0].content.as_deref(), Some("hello"));
        assert_eq!(trace.events[1].content.as_deref(), Some("ab"));
    }
}
