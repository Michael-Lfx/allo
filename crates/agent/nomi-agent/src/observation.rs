//! Explicit observation wrapper around `LlmProvider::stream`.
//!
//! Does not change `nomi-providers`. Emit failures only warn (and may write
//! `observation/gap`); they never abort an agent turn.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use nomi_agent_trace::{
    redact_preview, ExecutionStatus, ObservationEvent, ObservationIds, ObservationRecorder,
    ObservationScope, RecorderError, EVENT_LLM_REQUEST, EVENT_LLM_RESPONSE, EVENT_OBSERVATION_GAP,
    EVENT_TOOL_EXECUTION_CANCELLED, EVENT_TOOL_EXECUTION_COMPLETED, EVENT_TOOL_EXECUTION_FAILED,
    EVENT_TOOL_EXECUTION_STARTED, EVENT_TURN_END, EVENT_TURN_START, MAX_PREVIEW_CHARS,
};
use nomi_providers::{LlmProvider, ProviderError};
use nomi_types::llm::{LlmEvent, LlmRequest, ThinkingConfig};
use nomi_types::message::StopReason;
use serde_json::{json, Value};
use tokio::sync::mpsc;

pub struct ObservationSession {
    recorder: Arc<ObservationRecorder>,
    ids: Mutex<ObservationIds>,
    last_model_call_id: Mutex<Option<String>>,
    /// `tool_call_id` → the model call that issued the tool, captured at start.
    tool_parents: Mutex<HashMap<String, String>>,
    started_root_turns: Mutex<HashSet<String>>,
    ended_root_turns: Mutex<HashSet<String>>,
}

impl ObservationSession {
    pub fn new(recorder: Arc<ObservationRecorder>) -> Arc<Self> {
        Arc::new(Self {
            recorder,
            ids: Mutex::new(ObservationIds::default()),
            last_model_call_id: Mutex::new(None),
            tool_parents: Mutex::new(HashMap::new()),
            started_root_turns: Mutex::new(HashSet::new()),
            ended_root_turns: Mutex::new(HashSet::new()),
        })
    }

    pub fn recorder(&self) -> &Arc<ObservationRecorder> {
        &self.recorder
    }

    pub fn bind_ids(&self, ids: ObservationIds) {
        self.bind_ids_with_preview(ids, None);
    }

    pub fn bind_ids_with_preview(&self, mut ids: ObservationIds, prompt_preview: Option<&str>) {
        ids.model_call_id = None;
        *self.ids.lock().unwrap_or_else(|e| e.into_inner()) = ids;
        *self
            .last_model_call_id
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
        self.tool_parents
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.emit_turn_start_once(prompt_preview);
    }

    fn emit_turn_start_once(&self, prompt_preview: Option<&str>) {
        let ids = self.ids();
        let Some(root) = ids
            .root_turn_id
            .clone()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
        else {
            return;
        };
        {
            let mut started = self
                .started_root_turns
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if !started.insert(root) {
                return;
            }
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
    ) {
        let ids = self.ids();
        let Some(root) = ids
            .root_turn_id
            .clone()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
        else {
            return;
        };
        {
            let mut ended = self
                .ended_root_turns
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if !ended.insert(root) {
                return;
            }
        }
        observe_with_model_call(
            self,
            EVENT_TURN_END,
            json!({
                "status": status,
                "elapsed_ms": elapsed_ms,
                "stop_reason": stop_reason,
                "usage": usage,
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
        observe_with_model_call(
            self,
            EVENT_TOOL_EXECUTION_STARTED,
            json!({
                "tool_call_id": tool_call_id,
                "name": name,
                "arguments": arguments,
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
        observe_with_model_call(
            self,
            event_type,
            json!({
                "tool_call_id": tool_call_id,
                "name": name,
                "is_error": is_error,
                "result": result,
            }),
            parent,
        );
    }

    pub fn emit_tool_cancelled(&self, tool_call_id: &str, name: &str) {
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
    let request_payload = json!({
        "call_kind": call_kind,
        "observation_scope": scope,
        "fidelity": "canonical",
        "capture": ["truncated", "redacted"],
        "request": llm_request_to_value(request),
    });
    observe_with_model_call(
        &session,
        EVENT_LLM_REQUEST,
        request_payload,
        Some(model_call_id.clone()),
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
    ))
}

fn wrap_stream(
    mut rx: mpsc::Receiver<LlmEvent>,
    session: Arc<ObservationSession>,
    call_kind: String,
    scope: ObservationScope,
    started: Instant,
    model_call_id: Option<String>,
) -> mpsc::Receiver<LlmEvent> {
    let (tx, out_rx) = mpsc::channel(32);
    tokio::spawn(async move {
        let mut ttft_ms: Option<u64> = None;
        let mut text = String::new();
        let mut text_chars = 0usize;
        let mut text_truncated = false;
        let mut thinking = String::new();
        let mut thinking_chars = 0usize;
        let mut thinking_truncated = false;
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
                    push_bounded(
                        &mut text,
                        &mut text_chars,
                        &mut text_truncated,
                        delta,
                    );
                }
                LlmEvent::ThinkingDelta(delta) => {
                    push_bounded(
                        &mut thinking,
                        &mut thinking_chars,
                        &mut thinking_truncated,
                        delta,
                    );
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
                    None::<String>,
                )),
                LlmEvent::Error(message) => Some(("error", None, Some(message.clone()))),
                _ => None,
            };
            // Deliver to the live consumer first. Compact/judge timeouts drop
            // this receiver; recording a complete llm/response after that
            // would mark an abandoned call as intact.
            if tx.send(event).await.is_err() {
                return;
            }
            if let Some((stop_reason, usage, error)) = pending_response {
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

fn push_bounded(buf: &mut String, chars: &mut usize, truncated: &mut bool, delta: &str) {
    if *truncated {
        return;
    }
    for ch in delta.chars() {
        if *chars >= MAX_PREVIEW_CHARS {
            buf.push_str("…(truncated)");
            *truncated = true;
            return;
        }
        buf.push(ch);
        *chars += 1;
    }
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
    error: Option<&str>,
    started: Instant,
    ttft_ms: Option<u64>,
    model_call_id: Option<String>,
) {
    observe_with_model_call(
        session,
        EVENT_LLM_RESPONSE,
        json!({
            "call_kind": call_kind,
            "observation_scope": scope,
            "fidelity": "canonical",
            "text": text,
            "thinking": thinking,
            "tool_use": tool_use,
            "stop_reason": stop_reason,
            "usage": usage,
            "error": error,
            "elapsed_ms": u64::try_from(started.elapsed().as_millis()).unwrap_or(0),
            "ttft_ms": ttft_ms,
        }),
        model_call_id,
    );
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

pub(crate) fn llm_request_to_value(request: &LlmRequest) -> Value {
    json!({
        "model": request.model,
        "system": request.system,
        "messages": request.messages,
        "tools": request.tools.iter().map(|tool| json!({
            "name": tool.name,
            "description": tool.description,
            "input_schema": tool.input_schema,
            "deferred": tool.deferred,
        })).collect::<Vec<_>>(),
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

fn stop_reason_name(reason: StopReason) -> &'static str {
    match reason {
        StopReason::EndTurn => "end_turn",
        StopReason::ToolUse => "tool_use",
        StopReason::MaxTokens => "max_tokens",
        StopReason::MaxTurns => "max_turns",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomi_agent_trace::{
        EVENT_LLM_REQUEST, EVENT_LLM_RESPONSE, EVENT_TOOL_EXECUTION_COMPLETED,
        EVENT_TOOL_EXECUTION_STARTED,
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
            max_tokens: 32,
            thinking: None,
            reasoning_effort: None,
            temperature: None,
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
    async fn wrap_stream_bounds_observation_text_without_clipping_live_events() {
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
            max_tokens: 32,
            thinking: None,
            reasoning_effort: None,
            temperature: None,
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
        assert!(text.contains("…(truncated)"));
        assert!(text.chars().count() < live_chars);
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
            max_tokens: 32,
            thinking: None,
            reasoning_effort: None,
            temperature: None,
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
            max_tokens: 16,
            thinking: None,
            reasoning_effort: None,
            temperature: None,
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

        session.emit_tool_finished("call-echo", "echo", false, "pong");

        let events = recorder.read_events(Some("c-nested")).unwrap();
        let started = events
            .iter()
            .find(|event| event.event_type == EVENT_TOOL_EXECUTION_STARTED)
            .expect("tool started");
        let completed = events
            .iter()
            .find(|event| event.event_type == EVENT_TOOL_EXECUTION_COMPLETED)
            .expect("tool completed");
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
}
