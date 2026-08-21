//! Explicit observation wrapper around `LlmProvider::stream`.
//!
//! Does not change `nomi-providers`. Emit failures only warn (and may write
//! `observation/gap`); they never abort an agent turn.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use nomi_agent_trace::{
    capture_borrowed, omitted_binary_payload, redact_preview, ExecutionStatus, ObservationEvent,
    ObservationIds, ObservationRecorder, ObservationScope, RecorderError, EVENT_LLM_REQUEST,
    EVENT_LLM_RESPONSE, EVENT_OBSERVATION_GAP, EVENT_TOOL_EXECUTION_CANCELLED,
    EVENT_TOOL_EXECUTION_COMPLETED, EVENT_TOOL_EXECUTION_FAILED, EVENT_TOOL_EXECUTION_STARTED,
    EVENT_TURN_END, EVENT_TURN_START, MAX_PREVIEW_CHARS, OMITTED_REASON_INPUT_SCHEMA,
};
use nomi_providers::{LlmProvider, ProviderError};
use nomi_types::llm::{LlmEvent, LlmRequest, ThinkingConfig};
use nomi_types::message::{ContentBlock, Message, StopReason};
use nomi_types::tool::{ToolDef, ToolImage};
use serde_json::{json, Value};
use tokio::sync::mpsc;

pub struct ObservationSession {
    recorder: Arc<ObservationRecorder>,
    ids: Mutex<ObservationIds>,
    last_model_call_id: Mutex<Option<String>>,
    /// `tool_call_id` → the model call that issued the tool, captured at start.
    tool_parents: Mutex<HashMap<String, String>>,
}

impl ObservationSession {
    pub fn new(recorder: Arc<ObservationRecorder>) -> Arc<Self> {
        Arc::new(Self {
            recorder,
            ids: Mutex::new(ObservationIds::default()),
            last_model_call_id: Mutex::new(None),
            tool_parents: Mutex::new(HashMap::new()),
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
        let same_turn = {
            let mut current = self.ids.lock().unwrap_or_else(|e| e.into_inner());
            let same_turn = current.root_turn_id.is_some()
                && current.root_turn_id == ids.root_turn_id
                && current.conversation_id == ids.conversation_id;
            *current = ids;
            same_turn
        };
        if !same_turn {
            *self
                .last_model_call_id
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = None;
            self.tool_parents
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clear();
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
    ) {
        let ids = self.ids();
        if !self.recorder.claim_turn_end(&ids) {
            return;
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
        "system": redact_preview(&request.system),
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

fn observation_tool(tool: &ToolDef) -> Value {
    json!({
        "name": tool.name,
        "description": redact_preview(&tool.description),
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
            "text": redact_preview(text),
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
                "content": redact_preview(content),
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
                "thinking": redact_preview(thinking),
            });
            if let Some(signature) = signature {
                object["signature"] = Value::String(redact_preview(signature));
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
    fn llm_request_to_value_truncates_system_and_tool_result_without_mutating_live() {
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
        assert!(system.contains("…(truncated)"));
        assert!(system.chars().count() < request.system.chars().count());
        assert_eq!(request.system, long_system);

        let content = value["messages"][1]["content"][0]["content"]
            .as_str()
            .expect("tool result");
        assert!(content.contains("…(truncated)"));
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
        first.bind_ids(ids.clone());
        rebuilt.bind_ids(ids.clone());
        first.emit_turn_end(
            ExecutionStatus::Completed,
            11,
            Some("end_turn"),
            Some(json!({ "input_tokens": 3 })),
        );
        rebuilt.emit_turn_end(
            ExecutionStatus::Cancelled,
            99,
            Some("cancelled"),
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
        assert_eq!(ends[0].payload["status"], "completed");
        assert_eq!(ends[0].payload["elapsed_ms"], 11);

        recorder.clear_conversation("c-share").unwrap();
        rebuilt.bind_ids(ids);
        rebuilt.emit_turn_end(ExecutionStatus::Completed, 4, Some("end_turn"), None);
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
        session.emit_tool_finished("tool-1", "bash", false, "ok");

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
}
