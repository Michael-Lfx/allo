use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use nomi_config::compat::ProviderCompat;
use nomi_providers::{LlmProvider, ProviderError};
use nomi_providers::openai::OpenAIProvider;
use nomi_types::llm::{LlmEvent, LlmRequest};
use nomi_types::message::{ContentBlock, Message, Role, StopReason};
use nomi_types::tool::ToolDef;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a minimal LlmRequest suitable for all tests.
fn make_request() -> LlmRequest {
    LlmRequest {
        model: "gpt-4o".to_string(),
        system: "You are a test assistant.".to_string(),
        messages: vec![Message::new(
            Role::User,
            vec![ContentBlock::Text {
                text: "Hello".to_string(),
            }],
        )],
        tools: vec![],
        max_tokens: Some(512),
        thinking: None,
        reasoning_effort: None,
        temperature: None,
        retain_provider_round: false,
    }
}

/// Collect all events from the receiver until the channel closes.
async fn collect_events(mut rx: tokio::sync::mpsc::Receiver<LlmEvent>) -> Vec<LlmEvent> {
    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(event);
    }
    events
}

/// Build a raw SSE body string from a slice of JSON lines.
/// Each line is wrapped in `data: ...\n\n` and a final `data: [DONE]\n\n` is appended.
fn build_sse_body(data_lines: &[&str]) -> String {
    let mut body = String::new();
    for line in data_lines {
        body.push_str("data: ");
        body.push_str(line);
        body.push_str("\n\n");
    }
    body.push_str("data: [DONE]\n\n");
    body
}

#[derive(Clone)]
struct OpenAiBedrockSchemaResponder;

impl Respond for OpenAiBedrockSchemaResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        let schema = &body["tools"][0]["function"]["parameters"];
        if schema.get("oneOf").is_some() {
            return ResponseTemplate::new(500).set_body_json(json!({
                "error": {
                    "message": "input_schema does not support oneOf, allOf, or anyOf at the top level",
                    "reason": "TOOL_SCHEMA_INVALID"
                }
            }));
        }
        let chunk = json!({
            "choices": [{ "delta": { "content": "Recovered" }, "finish_reason": null }]
        })
        .to_string();
        let finish = json!({
            "choices": [{ "delta": {}, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1 }
        })
        .to_string();
        ResponseTemplate::new(200)
            .set_body_raw(build_sse_body(&[&chunk, &finish]), "text/event-stream")
    }
}

const OPENAI_FUNCTION_SCHEMA_INCOMPATIBLE_BODY: &str = r#"{"error":{"message":"Invalid schema for function 'Read': In context=('oneOf',), schema must have type 'object' at the top level.","type":"invalid_request_error"}}"#;

#[derive(Clone)]
struct OpenAiFunctionSchemaResponder;

impl Respond for OpenAiFunctionSchemaResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        let schema = &body["tools"][0]["function"]["parameters"];
        if schema.get("oneOf").is_some() {
            return ResponseTemplate::new(500)
                .set_body_string(OPENAI_FUNCTION_SCHEMA_INCOMPATIBLE_BODY);
        }
        let chunk = json!({
            "choices": [{ "delta": { "content": "Recovered" }, "finish_reason": null }]
        })
        .to_string();
        let finish = json!({
            "choices": [{ "delta": {}, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1 }
        })
        .to_string();
        ResponseTemplate::new(200)
            .set_body_raw(build_sse_body(&[&chunk, &finish]), "text/event-stream")
    }
}

#[derive(Clone)]
struct OpenAiFailedSanitizedResendResponder {
    attempt: Arc<AtomicUsize>,
}

impl Respond for OpenAiFailedSanitizedResendResponder {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        match self.attempt.fetch_add(1, Ordering::SeqCst) {
            0 => ResponseTemplate::new(500).set_body_json(json!({
                "error": {
                    "message": "input_schema does not support oneOf at the top level",
                    "reason": "TOOL_SCHEMA_INVALID"
                }
            })),
            1..=3 => ResponseTemplate::new(503).set_body_string("sanitized resend unavailable"),
            _ => {
                let chunk = json!({
                    "choices": [{ "delta": { "content": "Recovered" }, "finish_reason": null }]
                })
                .to_string();
                let finish = json!({
                    "choices": [{ "delta": {}, "finish_reason": "stop" }],
                    "usage": { "prompt_tokens": 1, "completion_tokens": 1 }
                })
                .to_string();
                ResponseTemplate::new(200)
                    .set_body_raw(build_sse_body(&[&chunk, &finish]), "text/event-stream")
            }
        }
    }
}

fn request_with_composed_tool_schema() -> LlmRequest {
    let mut request = make_request();
    request.tools.push(ToolDef {
        name: "Read".into(),
        description: "Read one or more files".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "file_path": { "type": "string" },
                "file_paths": { "type": "array", "items": { "type": "string" } }
            },
            "oneOf": [
                { "required": ["file_path"] },
                { "required": ["file_paths"] }
            ]
        }),
        deferred: false,
    });
    request
}

#[tokio::test]
async fn openai_gateway_recovers_and_remembers_openai_function_schema_requirement() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(OpenAiFunctionSchemaResponder)
        .expect(3)
        .mount(&server)
        .await;
    let provider = OpenAIProvider::new(
        "test-key",
        &server.uri(),
        ProviderCompat::openai_defaults(),
    );
    let request = request_with_composed_tool_schema();
    for _ in 0..2 {
        let events = collect_events(provider.stream(&request).await.unwrap()).await;
        assert!(
            events
                .iter()
                .any(|event| matches!(event, LlmEvent::TextDelta(text) if text == "Recovered"))
        );
    }
    let received = server.received_requests().await.unwrap();
    let has_root_one_of: Vec<bool> = received
        .iter()
        .map(|request| {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            body["tools"][0]["function"]["parameters"]
                .get("oneOf")
                .is_some()
        })
        .collect();
    assert_eq!(has_root_one_of, vec![true, false, false]);
    server.verify().await;
}

#[tokio::test]
async fn openai_gateway_recovers_and_remembers_bedrock_schema_requirement() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(OpenAiBedrockSchemaResponder)
        .expect(3)
        .mount(&server)
        .await;
    let provider = OpenAIProvider::new(
        "test-key",
        &server.uri(),
        ProviderCompat::openai_defaults(),
    );
    let request = request_with_composed_tool_schema();
    for _ in 0..2 {
        let events = collect_events(provider.stream(&request).await.unwrap()).await;
        assert!(
            events
                .iter()
                .any(|event| matches!(event, LlmEvent::TextDelta(text) if text == "Recovered"))
        );
    }
    let received = server.received_requests().await.unwrap();
    let has_root_one_of: Vec<bool> = received
        .iter()
        .map(|request| {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            body["tools"][0]["function"]["parameters"]
                .get("oneOf")
                .is_some()
        })
        .collect();
    assert_eq!(has_root_one_of, vec![true, false, false]);
    server.verify().await;
}

#[tokio::test]
async fn openai_gateway_does_not_schema_retry_an_unrelated_500() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("upstream unavailable"))
        .expect(3)
        .mount(&server)
        .await;
    let provider = OpenAIProvider::new(
        "test-key",
        &server.uri(),
        ProviderCompat::openai_defaults(),
    );
    let error = provider
        .stream(&request_with_composed_tool_schema())
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::Api { status: 500, .. }));
    server.verify().await;
}

#[tokio::test]
async fn openai_gateway_does_not_remember_a_failed_sanitized_resend() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(OpenAiFailedSanitizedResendResponder {
            attempt: Arc::new(AtomicUsize::new(0)),
        })
        .expect(5)
        .mount(&server)
        .await;
    let provider = OpenAIProvider::new(
        "test-key",
        &server.uri(),
        ProviderCompat::openai_defaults(),
    );
    let request = request_with_composed_tool_schema();

    let error = provider.stream(&request).await.unwrap_err();
    assert!(matches!(error, ProviderError::Api { status: 503, .. }));

    let events = collect_events(provider.stream(&request).await.unwrap()).await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, LlmEvent::TextDelta(text) if text == "Recovered"))
    );

    let received = server.received_requests().await.unwrap();
    let has_root_one_of: Vec<bool> = received
        .iter()
        .map(|request| {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            body["tools"][0]["function"]["parameters"]
                .get("oneOf")
                .is_some()
        })
        .collect();
    assert_eq!(
        has_root_one_of,
        vec![true, false, false, false, true]
    );
    server.verify().await;
}

async fn start_server_after_initial_connect_refusal(sse_body: String) -> String {
    let probe = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;

        let listener = TcpListener::bind(addr).await.unwrap();
        let (mut second, _) = listener.accept().await.unwrap();
        let mut buf = [0_u8; 4096];
        let _ = second.read(&mut buf).await.unwrap();

        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            sse_body.len(),
            sse_body
        );
        second.write_all(response.as_bytes()).await.unwrap();
    });

    format!("http://{addr}")
}

// ---------------------------------------------------------------------------
// test_openai_stream_text_response
// ---------------------------------------------------------------------------

/// Verify that a normal text response (multiple content deltas followed by a
/// stop chunk with usage) is parsed into the correct sequence of TextDelta
/// and Done events.
#[tokio::test]
async fn test_openai_stream_text_response() {
    let server = MockServer::start().await;

    // Chunk 1: first text delta
    let chunk1 = json!({
        "id": "chatcmpl-001",
        "object": "chat.completion.chunk",
        "choices": [{
            "index": 0,
            "delta": { "role": "assistant", "content": "Hello" },
            "finish_reason": null
        }]
    })
    .to_string();

    // Chunk 2: second text delta
    let chunk2 = json!({
        "id": "chatcmpl-001",
        "object": "chat.completion.chunk",
        "choices": [{
            "index": 0,
            "delta": { "content": ", world!" },
            "finish_reason": null
        }]
    })
    .to_string();

    // Chunk 3: finish_reason = "stop" with usage
    let chunk3 = json!({
        "id": "chatcmpl-001",
        "object": "chat.completion.chunk",
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 25,
            "completion_tokens": 10
        }
    })
    .to_string();

    let sse_body = build_sse_body(&[&chunk1, &chunk2, &chunk3]);

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer test-key"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse_body, "text/event-stream"))
        .mount(&server)
        .await;

    let provider =
        OpenAIProvider::new("test-key", &server.uri(), ProviderCompat::openai_defaults());
    let rx = provider.stream(&make_request()).await.unwrap();
    let events = collect_events(rx).await;

    // Expect: TextDelta("Hello"), TextDelta(", world!"), Done{EndTurn}
    assert_eq!(events.len(), 3, "expected 3 events, got: {:?}", events);

    match &events[0] {
        LlmEvent::TextDelta(text) => assert_eq!(text, "Hello"),
        e => panic!("expected TextDelta, got: {:?}", e),
    }

    match &events[1] {
        LlmEvent::TextDelta(text) => assert_eq!(text, ", world!"),
        e => panic!("expected TextDelta, got: {:?}", e),
    }

    match &events[2] {
        LlmEvent::Done { stop_reason, usage } => {
            assert_eq!(*stop_reason, StopReason::EndTurn);
            assert_eq!(usage.input_tokens, 25);
            assert_eq!(usage.output_tokens, 10);
        }
        e => panic!("expected Done, got: {:?}", e),
    }
}

// ---------------------------------------------------------------------------
// test_openai_initial_connect_error_is_retried
// ---------------------------------------------------------------------------

/// Verify that the provider retries when the initial HTTP request fails before
/// receiving any response. This covers transient connect/TLS failures where no
/// model output has been emitted yet.
#[tokio::test]
async fn test_openai_initial_connect_error_is_retried() {
    let chunk = json!({
        "id": "chatcmpl-retry",
        "object": "chat.completion.chunk",
        "choices": [{
            "index": 0,
            "delta": { "role": "assistant", "content": "Recovered" },
            "finish_reason": null
        }]
    })
    .to_string();
    let finish = json!({
        "id": "chatcmpl-retry",
        "object": "chat.completion.chunk",
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": "stop"
        }]
    })
    .to_string();
    let sse_body = build_sse_body(&[&chunk, &finish]);
    let base_url = start_server_after_initial_connect_refusal(sse_body).await;

    let provider = OpenAIProvider::new("test-key", &base_url, ProviderCompat::openai_defaults());
    let rx = provider.stream(&make_request()).await.unwrap();
    let events = collect_events(rx).await;

    assert_eq!(
        events.len(),
        2,
        "expected retry success events, got: {:?}",
        events
    );
    match &events[0] {
        LlmEvent::TextDelta(text) => assert_eq!(text, "Recovered"),
        e => panic!("expected TextDelta, got: {:?}", e),
    }
    match &events[1] {
        LlmEvent::Done { stop_reason, .. } => assert_eq!(*stop_reason, StopReason::EndTurn),
        e => panic!("expected Done, got: {:?}", e),
    }
}

// ---------------------------------------------------------------------------
// test_openai_stream_tool_call_aggregation
// ---------------------------------------------------------------------------

/// Verify that a tool call streamed in multiple delta chunks (id in first chunk,
/// name in first chunk, arguments split across chunks) is correctly aggregated
/// into a single ToolUse event.
#[tokio::test]
async fn test_openai_stream_tool_call_aggregation() {
    let server = MockServer::start().await;

    // Chunk 1: tool call header — id and function name arrive first
    let chunk1 = json!({
        "id": "chatcmpl-002",
        "object": "chat.completion.chunk",
        "choices": [{
            "index": 0,
            "delta": {
                "tool_calls": [{
                    "index": 0,
                    "id": "call_abc123",
                    "type": "function",
                    "function": {
                        "name": "read_file",
                        "arguments": "{\"path\":"
                    }
                }]
            },
            "finish_reason": null
        }]
    })
    .to_string();

    // Chunk 2: arguments continuation
    let chunk2 = json!({
        "id": "chatcmpl-002",
        "object": "chat.completion.chunk",
        "choices": [{
            "index": 0,
            "delta": {
                "tool_calls": [{
                    "index": 0,
                    "function": {
                        "arguments": "\"/tmp/test.txt\"}"
                    }
                }]
            },
            "finish_reason": null
        }]
    })
    .to_string();

    // Chunk 3: finish_reason = "tool_calls" with usage
    let chunk3 = json!({
        "id": "chatcmpl-002",
        "object": "chat.completion.chunk",
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": "tool_calls"
        }],
        "usage": {
            "prompt_tokens": 40,
            "completion_tokens": 15
        }
    })
    .to_string();

    let sse_body = build_sse_body(&[&chunk1, &chunk2, &chunk3]);

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse_body, "text/event-stream"))
        .mount(&server)
        .await;

    let provider =
        OpenAIProvider::new("test-key", &server.uri(), ProviderCompat::openai_defaults());
    let rx = provider.stream(&make_request()).await.unwrap();
    let events = collect_events(rx).await;

    // Expect: early tool progress, target-bearing progress, final ToolUse,
    // Done{ToolUse}. The progress events are what keep the UI responsive while
    // long tool arguments are still streaming.
    assert_eq!(events.len(), 4, "expected 4 events, got: {:?}", events);

    match &events[0] {
        LlmEvent::ToolUseDelta { id, name, input } => {
            assert_eq!(id, "call_abc123");
            assert_eq!(name, "read_file");
            assert!(input.is_none());
        }
        e => panic!("expected first ToolUseDelta, got: {:?}", e),
    }

    match &events[1] {
        LlmEvent::ToolUseDelta { id, name, input } => {
            assert_eq!(id, "call_abc123");
            assert_eq!(name, "read_file");
            assert_eq!(input.as_ref().unwrap()["path"], "/tmp/test.txt");
        }
        e => panic!("expected second ToolUseDelta, got: {:?}", e),
    }

    match &events[2] {
        LlmEvent::ToolUse {
            id, name, input, ..
        } => {
            assert_eq!(id, "call_abc123");
            assert_eq!(name, "read_file");
            assert_eq!(input["path"], "/tmp/test.txt");
        }
        e => panic!("expected ToolUse, got: {:?}", e),
    }

    match &events[3] {
        LlmEvent::Done { stop_reason, usage } => {
            assert_eq!(*stop_reason, StopReason::ToolUse);
            assert_eq!(usage.input_tokens, 40);
            assert_eq!(usage.output_tokens, 15);
        }
        e => panic!("expected Done, got: {:?}", e),
    }
}

// ---------------------------------------------------------------------------
// test_openai_multiple_tool_calls
// ---------------------------------------------------------------------------

/// Verify that when the API streams multiple parallel tool calls (different
/// indices) they are all emitted as separate ToolUse events.
#[tokio::test]
async fn test_openai_multiple_tool_calls() {
    let server = MockServer::start().await;

    // Chunk 1: first tool call (index 0)
    let chunk1 = json!({
        "id": "chatcmpl-003",
        "object": "chat.completion.chunk",
        "choices": [{
            "index": 0,
            "delta": {
                "tool_calls": [{
                    "index": 0,
                    "id": "call_tool0",
                    "type": "function",
                    "function": {
                        "name": "list_files",
                        "arguments": "{\"dir\": \"/tmp\"}"
                    }
                }]
            },
            "finish_reason": null
        }]
    })
    .to_string();

    // Chunk 2: second tool call (index 1)
    let chunk2 = json!({
        "id": "chatcmpl-003",
        "object": "chat.completion.chunk",
        "choices": [{
            "index": 0,
            "delta": {
                "tool_calls": [{
                    "index": 1,
                    "id": "call_tool1",
                    "type": "function",
                    "function": {
                        "name": "read_file",
                        "arguments": "{\"path\": \"/etc/hosts\"}"
                    }
                }]
            },
            "finish_reason": null
        }]
    })
    .to_string();

    // Chunk 3: finish_reason = "tool_calls"
    let chunk3 = json!({
        "id": "chatcmpl-003",
        "object": "chat.completion.chunk",
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": "tool_calls"
        }],
        "usage": {
            "prompt_tokens": 60,
            "completion_tokens": 20
        }
    })
    .to_string();

    let sse_body = build_sse_body(&[&chunk1, &chunk2, &chunk3]);

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse_body, "text/event-stream"))
        .mount(&server)
        .await;

    let provider =
        OpenAIProvider::new("test-key", &server.uri(), ProviderCompat::openai_defaults());
    let rx = provider.stream(&make_request()).await.unwrap();
    let events = collect_events(rx).await;

    // Expect: progress for both tools, final ToolUse for both, Done{ToolUse}.
    assert_eq!(events.len(), 5, "expected 5 events, got: {:?}", events);

    match &events[0] {
        LlmEvent::ToolUseDelta { id, name, input } => {
            assert_eq!(id, "call_tool0");
            assert_eq!(name, "list_files");
            assert_eq!(input.as_ref().unwrap()["dir"], "/tmp");
        }
        e => panic!("expected first ToolUseDelta, got: {:?}", e),
    }

    match &events[1] {
        LlmEvent::ToolUseDelta { id, name, input } => {
            assert_eq!(id, "call_tool1");
            assert_eq!(name, "read_file");
            assert_eq!(input.as_ref().unwrap()["path"], "/etc/hosts");
        }
        e => panic!("expected second ToolUseDelta, got: {:?}", e),
    }

    match &events[2] {
        LlmEvent::ToolUse {
            id, name, input, ..
        } => {
            assert_eq!(id, "call_tool0");
            assert_eq!(name, "list_files");
            assert_eq!(input["dir"], "/tmp");
        }
        e => panic!("expected first ToolUse, got: {:?}", e),
    }

    match &events[3] {
        LlmEvent::ToolUse {
            id, name, input, ..
        } => {
            assert_eq!(id, "call_tool1");
            assert_eq!(name, "read_file");
            assert_eq!(input["path"], "/etc/hosts");
        }
        e => panic!("expected second ToolUse, got: {:?}", e),
    }

    match &events[4] {
        LlmEvent::Done { stop_reason, .. } => {
            assert_eq!(*stop_reason, StopReason::ToolUse);
        }
        e => panic!("expected Done, got: {:?}", e),
    }
}

// ---------------------------------------------------------------------------
// test_openai_stream_state_transitions
// ---------------------------------------------------------------------------

/// Verify that the stream correctly stops processing events once it encounters
/// the `[DONE]` sentinel — any data after [DONE] is ignored and the receiver
/// channel closes cleanly.
#[tokio::test]
async fn test_openai_stream_state_transitions() {
    let server = MockServer::start().await;

    // A single text delta followed by a stop chunk, then the [DONE] sentinel.
    let chunk1 = json!({
        "id": "chatcmpl-004",
        "object": "chat.completion.chunk",
        "choices": [{
            "index": 0,
            "delta": { "content": "Transition test." },
            "finish_reason": null
        }]
    })
    .to_string();

    let chunk2 = json!({
        "id": "chatcmpl-004",
        "object": "chat.completion.chunk",
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 5
        }
    })
    .to_string();

    // Build SSE body manually: two data lines, then [DONE], then a stray line
    // that must NOT produce any events.
    let mut sse_body = String::new();
    sse_body.push_str("data: ");
    sse_body.push_str(&chunk1);
    sse_body.push_str("\n\n");
    sse_body.push_str("data: ");
    sse_body.push_str(&chunk2);
    sse_body.push_str("\n\n");
    sse_body.push_str("data: [DONE]\n\n");
    // Stray chunk after [DONE] — must be ignored
    sse_body.push_str("data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ignored\"},\"finish_reason\":null}]}\n\n");

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse_body, "text/event-stream"))
        .mount(&server)
        .await;

    let provider =
        OpenAIProvider::new("test-key", &server.uri(), ProviderCompat::openai_defaults());
    let rx = provider.stream(&make_request()).await.unwrap();
    let events = collect_events(rx).await;

    // Expect exactly: TextDelta, Done — the trailing chunk after [DONE] is discarded.
    assert_eq!(events.len(), 2, "expected 2 events, got: {:?}", events);

    match &events[0] {
        LlmEvent::TextDelta(text) => assert_eq!(text, "Transition test."),
        e => panic!("expected TextDelta, got: {:?}", e),
    }

    match &events[1] {
        LlmEvent::Done { stop_reason, usage } => {
            assert_eq!(*stop_reason, StopReason::EndTurn);
            assert_eq!(usage.input_tokens, 10);
            assert_eq!(usage.output_tokens, 5);
            assert_eq!(usage.cache_creation_tokens, 0);
            assert_eq!(usage.cache_read_tokens, 0);
        }
        e => panic!("expected Done, got: {:?}", e),
    }
}

// ---------------------------------------------------------------------------
// test_openai_api_error_non_success_status
// ---------------------------------------------------------------------------

/// Verify that a non-2xx HTTP response is surfaced as a ProviderError::Api.
#[tokio::test]
async fn test_openai_api_error_non_success_status() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(401).set_body_string(
            r#"{"error":{"message":"Invalid API key","type":"invalid_request_error"}}"#,
        ))
        .mount(&server)
        .await;

    let provider = OpenAIProvider::new("bad-key", &server.uri(), ProviderCompat::openai_defaults());
    let result = provider.stream(&make_request()).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        nomi_providers::ProviderError::Api { status, .. } => {
            assert_eq!(status, 401);
        }
        e => panic!("expected Api error, got: {:?}", e),
    }
}

#[tokio::test]
async fn test_openai_multi_key_rotates_after_auth_failure() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer rejected-key"))
        .respond_with(ResponseTemplate::new(401).set_body_string(
            r#"{"error":{"message":"Invalid token","type":"invalid_request_error"}}"#,
        ))
        .expect(1)
        .mount(&server)
        .await;

    let success_chunk = json!({
        "choices": [{ "delta": { "content": "rotated" }, "finish_reason": null }]
    })
    .to_string();
    let success_finish = json!({
        "choices": [{ "delta": {}, "finish_reason": "stop" }],
        "usage": { "prompt_tokens": 1, "completion_tokens": 1 }
    })
    .to_string();
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer working-key"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(
                build_sse_body(&[&success_chunk, &success_finish]),
                "text/event-stream",
            ),
        )
        .expect(2)
        .mount(&server)
        .await;

    let provider = OpenAIProvider::new(
        " rejected-key,\n working-key ",
        &server.uri(),
        ProviderCompat::openai_defaults(),
    );
    for _ in 0..2 {
        let events = collect_events(provider.stream(&make_request()).await.unwrap()).await;
        assert!(
            events
                .iter()
                .any(|event| matches!(event, LlmEvent::TextDelta(text) if text == "rotated"))
        );
    }
    server.verify().await;
}

// ---------------------------------------------------------------------------
// test_openai_rate_limited
// ---------------------------------------------------------------------------

/// Verify that a 429 response is surfaced as ProviderError::RateLimited.
#[tokio::test]
async fn test_openai_rate_limited() {
    let server = MockServer::start().await;

    let body = r#"{"error":{"message":"You exceeded your current quota","type":"insufficient_quota","code":"insufficient_quota"}}"#;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(429).set_body_string(body))
        .mount(&server)
        .await;

    let provider =
        OpenAIProvider::new("test-key", &server.uri(), ProviderCompat::openai_defaults());
    let result = provider.stream(&make_request()).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    let display = err.to_string();
    match err {
        nomi_providers::ProviderError::RateLimited { retry_after_ms, .. } => {
            assert_eq!(retry_after_ms, 5000);
        }
        e => panic!("expected RateLimited error, got: {:?}", e),
    }

    assert!(
        display.contains("insufficient_quota"),
        "rate limit error should preserve provider body, got: {display}"
    );
}

// ---------------------------------------------------------------------------
// test_openai_stream_max_tokens_stop_reason
// ---------------------------------------------------------------------------

/// Verify that finish_reason "length" maps to StopReason::MaxTokens.
#[tokio::test]
async fn test_openai_stream_max_tokens_stop_reason() {
    let server = MockServer::start().await;

    let chunk1 = json!({
        "id": "chatcmpl-005",
        "object": "chat.completion.chunk",
        "choices": [{
            "index": 0,
            "delta": { "content": "Truncated" },
            "finish_reason": null
        }]
    })
    .to_string();

    let chunk2 = json!({
        "id": "chatcmpl-005",
        "object": "chat.completion.chunk",
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": "length"
        }],
        "usage": {
            "prompt_tokens": 100,
            "completion_tokens": 512
        }
    })
    .to_string();

    let sse_body = build_sse_body(&[&chunk1, &chunk2]);

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse_body, "text/event-stream"))
        .mount(&server)
        .await;

    let provider =
        OpenAIProvider::new("test-key", &server.uri(), ProviderCompat::openai_defaults());
    let rx = provider.stream(&make_request()).await.unwrap();
    let events = collect_events(rx).await;

    assert_eq!(events.len(), 2);

    match &events[1] {
        LlmEvent::Done { stop_reason, usage } => {
            assert_eq!(*stop_reason, StopReason::MaxTokens);
            assert_eq!(usage.input_tokens, 100);
            assert_eq!(usage.output_tokens, 512);
        }
        e => panic!("expected Done with MaxTokens, got: {:?}", e),
    }
}

// ---------------------------------------------------------------------------
// test_openai_stream_empty_content_delta_skipped
// ---------------------------------------------------------------------------

/// Verify that empty content strings in deltas do NOT produce TextDelta events
/// (the provider filters them out).
#[tokio::test]
async fn test_openai_stream_empty_content_delta_skipped() {
    let server = MockServer::start().await;

    // Chunk with empty content — should be silently skipped
    let chunk_empty = json!({
        "id": "chatcmpl-006",
        "object": "chat.completion.chunk",
        "choices": [{
            "index": 0,
            "delta": { "content": "" },
            "finish_reason": null
        }]
    })
    .to_string();

    let chunk_text = json!({
        "id": "chatcmpl-006",
        "object": "chat.completion.chunk",
        "choices": [{
            "index": 0,
            "delta": { "content": "actual content" },
            "finish_reason": null
        }]
    })
    .to_string();

    let chunk_done = json!({
        "id": "chatcmpl-006",
        "object": "chat.completion.chunk",
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": "stop"
        }],
        "usage": { "prompt_tokens": 5, "completion_tokens": 3 }
    })
    .to_string();

    let sse_body = build_sse_body(&[&chunk_empty, &chunk_text, &chunk_done]);

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse_body, "text/event-stream"))
        .mount(&server)
        .await;

    let provider =
        OpenAIProvider::new("test-key", &server.uri(), ProviderCompat::openai_defaults());
    let rx = provider.stream(&make_request()).await.unwrap();
    let events = collect_events(rx).await;

    // Expect only TextDelta("actual content") and Done — no empty TextDelta
    assert_eq!(events.len(), 2, "expected 2 events, got: {:?}", events);

    match &events[0] {
        LlmEvent::TextDelta(text) => assert_eq!(text, "actual content"),
        e => panic!("expected TextDelta with actual content, got: {:?}", e),
    }

    match &events[1] {
        LlmEvent::Done { stop_reason, .. } => assert_eq!(*stop_reason, StopReason::EndTurn),
        e => panic!("expected Done, got: {:?}", e),
    }
}

// ---------------------------------------------------------------------------
// tools + reasoning_effort negotiation
// ---------------------------------------------------------------------------

/// Verbatim gateway rejection observed on 2026-09-12 (Flowy Cloud hardware user).
const TOOLS_EFFORT_INCOMPATIBLE_BODY: &str = r#"{"code":500,"msg":"Model call failed. Please try again later: Function tools with reasoning_effort are not supported for gpt-5.6-sol-tec-do in /v1/chat/completions. To use function tools, use /v1/responses or set reasoning_effort to 'none'.","error_key":"error.all_channel_models_failed"}"#;

/// Rejects tool-bearing requests that carry an effort other than `none`,
/// mirroring the gateway that rejected `AIPC-GPT5.6-Sol`.
#[derive(Clone)]
struct ToolsEffortIncompatResponder;

impl Respond for ToolsEffortIncompatResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        let has_tools = body
            .get("tools")
            .and_then(|value| value.as_array())
            .is_some_and(|tools| !tools.is_empty());
        let effort = body.get("reasoning_effort").and_then(|value| value.as_str());
        if has_tools && effort.is_some() && effort != Some("none") {
            return ResponseTemplate::new(500).set_body_string(TOOLS_EFFORT_INCOMPATIBLE_BODY);
        }
        let chunk = json!({
            "choices": [{ "delta": { "content": "Recovered" }, "finish_reason": null }]
        })
        .to_string();
        let finish = json!({
            "choices": [{ "delta": {}, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1 }
        })
        .to_string();
        ResponseTemplate::new(200)
            .set_body_raw(build_sse_body(&[&chunk, &finish]), "text/event-stream")
    }
}

fn request_with_tool_and_effort(model: &str, effort: Option<&str>) -> LlmRequest {
    let mut request = make_request();
    request.model = model.to_string();
    request.reasoning_effort = effort.map(str::to_owned);
    request.tools.push(ToolDef {
        name: "Read".into(),
        description: "Read one file".into(),
        input_schema: json!({
            "type": "object",
            "properties": { "file_path": { "type": "string" } },
            "required": ["file_path"]
        }),
        deferred: false,
    });
    request
}

fn recorded_efforts(received: &[Request]) -> Vec<Option<String>> {
    received
        .iter()
        .map(|request| {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            body.get("reasoning_effort")
                .and_then(|value| value.as_str())
                .map(str::to_owned)
        })
        .collect()
}

#[tokio::test]
async fn openai_gateway_negotiates_reasoning_effort_none_for_tool_requests() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ToolsEffortIncompatResponder)
        .expect(3)
        .mount(&server)
        .await;
    let provider = OpenAIProvider::new(
        "test-key",
        &server.uri(),
        ProviderCompat::openai_defaults(),
    );
    let request = request_with_tool_and_effort("gpt-5.6-sol", Some("medium"));

    for _ in 0..2 {
        let events = collect_events(provider.stream(&request).await.unwrap()).await;
        assert!(
            events
                .iter()
                .any(|event| matches!(event, LlmEvent::TextDelta(text) if text == "Recovered"))
        );
    }

    let received = server.received_requests().await.unwrap();
    assert_eq!(
        recorded_efforts(&received),
        vec![
            Some("medium".to_string()),
            Some("none".to_string()),
            Some("none".to_string())
        ],
        "first request keeps the requested effort, the retry uses none, and the second turn remembers it"
    );
    let authorization: Vec<Option<String>> = received
        .iter()
        .map(|request| {
            request
                .headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        })
        .collect();
    assert!(
        authorization.windows(2).all(|pair| pair[0] == pair[1]),
        "negotiation must keep the same attribution headers: {authorization:?}"
    );
    server.verify().await;
}

#[tokio::test]
async fn openai_effort_fallback_keeps_effort_for_requests_without_tools() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ToolsEffortIncompatResponder)
        .expect(3)
        .mount(&server)
        .await;
    let provider = OpenAIProvider::new(
        "test-key",
        &server.uri(),
        ProviderCompat::openai_defaults(),
    );

    let with_tools = request_with_tool_and_effort("gpt-5.6-sol", Some("medium"));
    let events = collect_events(provider.stream(&with_tools).await.unwrap()).await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, LlmEvent::TextDelta(text) if text == "Recovered"))
    );

    let mut without_tools = request_with_tool_and_effort("gpt-5.6-sol", Some("medium"));
    without_tools.tools.clear();
    let events = collect_events(provider.stream(&without_tools).await.unwrap()).await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, LlmEvent::TextDelta(text) if text == "Recovered"))
    );

    let received = server.received_requests().await.unwrap();
    assert_eq!(
        recorded_efforts(&received),
        vec![
            Some("medium".to_string()),
            Some("none".to_string()),
            Some("medium".to_string())
        ],
        "requests without tools must keep the user-selected effort"
    );
    server.verify().await;
}

/// Models a gateway that defaults reasoning for tool calls and therefore
/// rejects any tool-bearing request without an explicit `reasoning_effort:
/// "none"`, even when the client never configured an effort.
#[derive(Clone)]
struct ToolsRequireEffortNoneResponder;

impl Respond for ToolsRequireEffortNoneResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        let has_tools = body
            .get("tools")
            .and_then(|value| value.as_array())
            .is_some_and(|tools| !tools.is_empty());
        let effort = body.get("reasoning_effort").and_then(|value| value.as_str());
        if has_tools && effort != Some("none") {
            return ResponseTemplate::new(500).set_body_string(TOOLS_EFFORT_INCOMPATIBLE_BODY);
        }
        let chunk = json!({
            "choices": [{ "delta": { "content": "Recovered" }, "finish_reason": null }]
        })
        .to_string();
        let finish = json!({
            "choices": [{ "delta": {}, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1 }
        })
        .to_string();
        ResponseTemplate::new(200)
            .set_body_raw(build_sse_body(&[&chunk, &finish]), "text/event-stream")
    }
}

#[tokio::test]
async fn openai_explicit_none_heals_tool_requests_without_configured_effort() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ToolsRequireEffortNoneResponder)
        .expect(2)
        .mount(&server)
        .await;
    let provider = OpenAIProvider::new(
        "test-key",
        &server.uri(),
        ProviderCompat::openai_defaults(),
    );
    let request = request_with_tool_and_effort("gpt-5.6-sol", None);
    assert!(
        request.reasoning_effort.is_none(),
        "this case models a gateway that defaults reasoning server-side"
    );

    let events = collect_events(provider.stream(&request).await.unwrap()).await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, LlmEvent::TextDelta(text) if text == "Recovered"))
    );

    let received = server.received_requests().await.unwrap();
    assert_eq!(
        recorded_efforts(&received),
        vec![None, Some("none".to_string())],
        "the retry must carry an explicit none instead of resending the same body"
    );
    server.verify().await;
}

#[tokio::test]
async fn openai_effort_fallback_is_isolated_per_model() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ToolsEffortIncompatResponder)
        .expect(4)
        .mount(&server)
        .await;
    let provider = OpenAIProvider::new(
        "test-key",
        &server.uri(),
        ProviderCompat::openai_defaults(),
    );

    for model in ["gpt-5.6-sol", "gpt-5.6-other"] {
        let request = request_with_tool_and_effort(model, Some("medium"));
        let events = collect_events(provider.stream(&request).await.unwrap()).await;
        assert!(
            events
                .iter()
                .any(|event| matches!(event, LlmEvent::TextDelta(text) if text == "Recovered"))
        );
    }

    let received = server.received_requests().await.unwrap();
    assert_eq!(
        recorded_efforts(&received),
        vec![
            Some("medium".to_string()),
            Some("none".to_string()),
            Some("medium".to_string()),
            Some("none".to_string())
        ],
        "the learned effort policy must not leak to another model"
    );
    server.verify().await;
}

#[tokio::test]
async fn openai_gateway_does_not_effort_retry_an_unrelated_500() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("upstream unavailable"))
        .expect(3)
        .mount(&server)
        .await;
    let provider = OpenAIProvider::new(
        "test-key",
        &server.uri(),
        ProviderCompat::openai_defaults(),
    );
    let error = provider
        .stream(&request_with_tool_and_effort("gpt-5.6-sol", Some("medium")))
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::Api { status: 500, .. }));
    server.verify().await;
}

/// Negotiates usage options, tool schemas, and reasoning_effort in one bounded
/// loop: each incompatible extension is removed once and the request succeeds.
#[derive(Clone)]
struct LayeredNegotiationResponder;

impl Respond for LayeredNegotiationResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        if body.get("stream_options").is_some() {
            return ResponseTemplate::new(400).set_body_json(json!({
                "error": { "message": "unknown parameter: stream_options" }
            }));
        }
        let schema = &body["tools"][0]["function"]["parameters"];
        if schema.get("oneOf").is_some() {
            return ResponseTemplate::new(500).set_body_string(
                r#"{"error":{"message":"Invalid schema for function 'Read': In context=('oneOf',), schema must have type 'object' at the top level.","type":"invalid_request_error"}}"#,
            );
        }
        let effort = body.get("reasoning_effort").and_then(|value| value.as_str());
        if effort.is_some() && effort != Some("none") {
            return ResponseTemplate::new(500).set_body_string(TOOLS_EFFORT_INCOMPATIBLE_BODY);
        }
        let chunk = json!({
            "choices": [{ "delta": { "content": "Recovered" }, "finish_reason": null }]
        })
        .to_string();
        let finish = json!({
            "choices": [{ "delta": {}, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1 }
        })
        .to_string();
        ResponseTemplate::new(200)
            .set_body_raw(build_sse_body(&[&chunk, &finish]), "text/event-stream")
    }
}

#[tokio::test]
async fn openai_negotiates_usage_schema_and_effort_exactly_once_each() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(LayeredNegotiationResponder)
        .expect(4)
        .mount(&server)
        .await;
    let provider = OpenAIProvider::new(
        "test-key",
        &server.uri(),
        ProviderCompat::openai_defaults(),
    );
    let mut request = request_with_composed_tool_schema();
    request.reasoning_effort = Some("medium".to_string());

    let events = collect_events(provider.stream(&request).await.unwrap()).await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, LlmEvent::TextDelta(text) if text == "Recovered"))
    );

    let received = server.received_requests().await.unwrap();
    assert_eq!(received.len(), 4, "one request per negotiation step");
    let shapes: Vec<(bool, bool, Option<String>)> = received
        .iter()
        .map(|request| {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            (
                body.get("stream_options").is_some(),
                body["tools"][0]["function"]["parameters"]
                    .get("oneOf")
                    .is_some(),
                body.get("reasoning_effort")
                    .and_then(|value| value.as_str())
                    .map(str::to_owned),
            )
        })
        .collect();
    assert_eq!(
        shapes,
        vec![
            (true, true, Some("medium".to_string())),
            (false, true, Some("medium".to_string())),
            (false, false, Some("medium".to_string())),
            (false, false, Some("none".to_string())),
        ],
        "each extension is removed once, in order, and the loop stays bounded"
    );
    server.verify().await;
}

#[tokio::test]
async fn openai_effort_rejection_never_rewrites_requests_without_tools() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string(TOOLS_EFFORT_INCOMPATIBLE_BODY))
        .mount(&server)
        .await;
    let provider = OpenAIProvider::new(
        "test-key",
        &server.uri(),
        ProviderCompat::openai_defaults(),
    );
    let mut request = request_with_tool_and_effort("gpt-5.6-sol", Some("medium"));
    request.tools.clear();

    let error = provider.stream(&request).await.unwrap_err();
    assert!(matches!(error, ProviderError::Api { status: 500, .. }));

    let received = server.received_requests().await.unwrap();
    let efforts = recorded_efforts(&received);
    assert!(!efforts.is_empty());
    assert!(
        efforts
            .iter()
            .all(|effort| effort.as_deref() == Some("medium")),
        "a request without tools must never be rewritten to reasoning_effort=none: {efforts:?}"
    );
}

// ---------------------------------------------------------------------------
// output token ceiling negotiation
// ---------------------------------------------------------------------------

/// Verbatim gateway rejection observed on 2026-09-12 (Gemini 3.5 Flash).
const OUTPUT_RANGE_REJECTION_BODY: &str = r#"{"code":500,"msg":"Unable to submit request because it has a maxOutputTokens value of 128000 but the supported range is from 1 (inclusive) to 65537 (exclusive). Update the value and try again.","error_key":"error.all_channel_models_failed"}"#;

const SUPPORTED_OUTPUT_CEILING: u64 = 65_536;

#[derive(Clone)]
struct OutputLimitResponder;

impl Respond for OutputLimitResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        let requested = body
            .get("max_tokens")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        if requested > SUPPORTED_OUTPUT_CEILING {
            return ResponseTemplate::new(500).set_body_string(OUTPUT_RANGE_REJECTION_BODY);
        }
        let chunk = json!({
            "choices": [{ "delta": { "content": "Recovered" }, "finish_reason": null }]
        })
        .to_string();
        let finish = json!({
            "choices": [{ "delta": {}, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1 }
        })
        .to_string();
        ResponseTemplate::new(200)
            .set_body_raw(build_sse_body(&[&chunk, &finish]), "text/event-stream")
    }
}

fn request_with_max_tokens(model: &str, max_tokens: u32) -> LlmRequest {
    let mut request = make_request();
    request.model = model.to_string();
    request.max_tokens = Some(max_tokens);
    request
}

fn recorded_max_tokens(received: &[Request]) -> Vec<Option<u64>> {
    received
        .iter()
        .map(|request| {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            body.get("max_tokens").and_then(|value| value.as_u64())
        })
        .collect()
}

#[tokio::test]
async fn openai_gateway_negotiates_output_limit_from_supported_range() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(OutputLimitResponder)
        .expect(3)
        .mount(&server)
        .await;
    let provider = OpenAIProvider::new(
        "test-key",
        &server.uri(),
        ProviderCompat::openai_defaults(),
    );
    let request = request_with_max_tokens("gemini-3.5-flash", 128_000);

    for _ in 0..2 {
        let events = collect_events(provider.stream(&request).await.unwrap()).await;
        assert!(
            events
                .iter()
                .any(|event| matches!(event, LlmEvent::TextDelta(text) if text == "Recovered"))
        );
    }

    let received = server.received_requests().await.unwrap();
    assert_eq!(
        recorded_max_tokens(&received),
        vec![
            Some(128_000),
            Some(SUPPORTED_OUTPUT_CEILING),
            Some(SUPPORTED_OUTPUT_CEILING)
        ],
        "exclusive 65537 becomes 65536, and later turns reuse the learned ceiling"
    );
    server.verify().await;
}

#[tokio::test]
async fn openai_output_limit_is_isolated_per_model() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(OutputLimitResponder)
        .expect(4)
        .mount(&server)
        .await;
    let provider = OpenAIProvider::new(
        "test-key",
        &server.uri(),
        ProviderCompat::openai_defaults(),
    );

    for model in ["gemini-3.5-flash", "gemini-other"] {
        let request = request_with_max_tokens(model, 128_000);
        let events = collect_events(provider.stream(&request).await.unwrap()).await;
        assert!(
            events
                .iter()
                .any(|event| matches!(event, LlmEvent::TextDelta(text) if text == "Recovered"))
        );
    }

    let received = server.received_requests().await.unwrap();
    assert_eq!(
        recorded_max_tokens(&received),
        vec![
            Some(128_000),
            Some(SUPPORTED_OUTPUT_CEILING),
            Some(128_000),
            Some(SUPPORTED_OUTPUT_CEILING)
        ],
        "the learned ceiling must not leak to another model"
    );
    server.verify().await;
}

#[tokio::test]
async fn openai_output_limit_at_supported_max_is_accepted_without_negotiation() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(OutputLimitResponder)
        .expect(1)
        .mount(&server)
        .await;
    let provider = OpenAIProvider::new(
        "test-key",
        &server.uri(),
        ProviderCompat::openai_defaults(),
    );
    let request = request_with_max_tokens("gemini-3.5-flash", SUPPORTED_OUTPUT_CEILING as u32);

    let events = collect_events(provider.stream(&request).await.unwrap()).await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, LlmEvent::TextDelta(text) if text == "Recovered"))
    );
    assert_eq!(
        recorded_max_tokens(&server.received_requests().await.unwrap()),
        vec![Some(SUPPORTED_OUTPUT_CEILING)]
    );
    server.verify().await;
}

#[tokio::test]
async fn openai_output_limit_never_increases_a_smaller_request() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(OutputLimitResponder)
        .expect(3)
        .mount(&server)
        .await;
    let provider = OpenAIProvider::new(
        "test-key",
        &server.uri(),
        ProviderCompat::openai_defaults(),
    );

    let large = request_with_max_tokens("gemini-3.5-flash", 128_000);
    let events = collect_events(provider.stream(&large).await.unwrap()).await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, LlmEvent::TextDelta(text) if text == "Recovered"))
    );

    let small = request_with_max_tokens("gemini-3.5-flash", 4_096);
    let events = collect_events(provider.stream(&small).await.unwrap()).await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, LlmEvent::TextDelta(text) if text == "Recovered"))
    );

    assert_eq!(
        recorded_max_tokens(&server.received_requests().await.unwrap()),
        vec![Some(128_000), Some(SUPPORTED_OUTPUT_CEILING), Some(4_096)],
        "a learned ceiling must clamp only downward and never rewrite a smaller request"
    );
    server.verify().await;
}

#[tokio::test]
async fn openai_gateway_does_not_output_limit_retry_an_unrelated_500() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("upstream unavailable"))
        .expect(3)
        .mount(&server)
        .await;
    let provider = OpenAIProvider::new(
        "test-key",
        &server.uri(),
        ProviderCompat::openai_defaults(),
    );
    let error = provider
        .stream(&request_with_max_tokens("gemini-3.5-flash", 128_000))
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::Api { status: 500, .. }));
    server.verify().await;
}

// ---------------------------------------------------------------------------
// Gemini object-only composition branches
// ---------------------------------------------------------------------------

/// Verbatim Gemini wording observed on 2026-09-12.
const GEMINI_ANY_OF_REJECTION_BODY: &str = r#"{"code":500,"msg":"Model call failed. Please try again later: * GenerateContentRequest.tools[0].function_declarations[10].parameters.any_of[0].required: only allowed for OBJECT type","error_key":"error.all_channel_models_failed"}"#;

/// Rejects tool schemas that still carry object-only keywords on branches
/// without `type: object`, mirroring Gemini's validation.
fn gemini_unsafe_schema(schema: &serde_json::Value) -> bool {
    match schema {
        serde_json::Value::Object(map) => {
            let has_object_only =
                map.contains_key("required") || map.contains_key("properties");
            let allows_object = match map.get("type") {
                Some(serde_json::Value::String(kind)) => kind == "object",
                Some(serde_json::Value::Array(kinds)) => {
                    kinds.iter().any(|kind| kind.as_str() == Some("object"))
                }
                _ => false,
            };
            if has_object_only && !allows_object {
                return true;
            }
            map.values().any(gemini_unsafe_schema)
        }
        serde_json::Value::Array(items) => items.iter().any(gemini_unsafe_schema),
        _ => false,
    }
}

#[derive(Clone)]
struct GeminiAnyOfResponder;

impl Respond for GeminiAnyOfResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        let parameters = &body["tools"][0]["function"]["parameters"];
        if gemini_unsafe_schema(parameters) {
            return ResponseTemplate::new(500).set_body_string(GEMINI_ANY_OF_REJECTION_BODY);
        }
        let chunk = json!({
            "choices": [{ "delta": { "content": "Recovered" }, "finish_reason": null }]
        })
        .to_string();
        let finish = json!({
            "choices": [{ "delta": {}, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1 }
        })
        .to_string();
        ResponseTemplate::new(200)
            .set_body_raw(build_sse_body(&[&chunk, &finish]), "text/event-stream")
    }
}

#[tokio::test]
async fn openai_gateway_sanitizes_gemini_object_only_branches() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(GeminiAnyOfResponder)
        .expect(2)
        .mount(&server)
        .await;
    let provider = OpenAIProvider::new(
        "test-key",
        &server.uri(),
        ProviderCompat::openai_defaults(),
    );

    let request = request_with_composed_tool_schema();
    let events = collect_events(provider.stream(&request).await.unwrap()).await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, LlmEvent::TextDelta(text) if text == "Recovered"))
    );
    assert!(
        request.tools[0].input_schema.get("oneOf").is_some(),
        "the local execution schema must stay composed after provider-facing sanitizing"
    );

    let received = server.received_requests().await.unwrap();
    assert_eq!(
        received.len(),
        2,
        "the Gemini schema rejection must skip the transient 500 retries"
    );
    let first: serde_json::Value = serde_json::from_slice(&received[0].body).unwrap();
    assert!(
        first["tools"][0]["function"]["parameters"]
            .get("oneOf")
            .is_some()
    );
    let second: serde_json::Value = serde_json::from_slice(&received[1].body).unwrap();
    let sanitized = &second["tools"][0]["function"]["parameters"];
    assert!(sanitized.get("oneOf").is_none());
    assert!(sanitized.get("anyOf").is_none());
    assert!(!gemini_unsafe_schema(sanitized));
    server.verify().await;
}

// ---------------------------------------------------------------------------
// Manual: real 90s initial-negotiation deadline (network-real, not mocked)
// ---------------------------------------------------------------------------

/// Run with:
/// `cargo test -p nomi-providers --test provider_openai_test -- --ignored initial_request_deadline`
///
/// A black-hole listener accepts the connection and never responds. The shared
/// per-stream deadline must fire once at ~90s instead of stacking retries
/// (30s connect / 120s idle-read must not be the bound here).
#[tokio::test]
#[ignore = "manual verification: waits out the real 90s initial-negotiation deadline"]
async fn initial_request_deadline_against_blackhole_listener() {
    use std::time::Instant as StdInstant;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            held.push(socket);
        }
    });

    let provider = OpenAIProvider::new(
        "test-key",
        &format!("http://{addr}"),
        ProviderCompat::openai_defaults(),
    );

    let started = StdInstant::now();
    let error = provider.stream(&make_request()).await.unwrap_err();
    let elapsed = started.elapsed();

    assert!(
        matches!(error, ProviderError::InitialRequestTimeout(_)),
        "expected the deadline error, got: {error:?}"
    );
    assert!(
        elapsed >= Duration::from_secs(90),
        "deadline fired too early: {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(120),
        "wait must not stack retries on top of one deadline: {elapsed:?}"
    );
}
