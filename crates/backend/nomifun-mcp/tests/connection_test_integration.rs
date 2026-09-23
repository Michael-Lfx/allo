//! Integration tests for McpConnectionTestService.
//!
//! Tests from test-plan §2 (Connection Test):
//! - CT-3: Command not found (ENOENT)
//! - CT-4: URL not reachable
//! - CT-5: Needs OAuth authentication (401)
//! - CT-6: Timeout
//! - SSE auth probe (M-33 coverage)

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nomifun_db::{IOAuthTokenRepository, SqliteOAuthTokenRepository, UpsertOAuthTokenParams};
use nomifun_mcp::{McpConnectionTestService, McpOAuthService};
use nomifun_mcp::McpServerTransport;
use nomifun_mcp::McpError;
use nomifun_mcp::McpToolCallError;
use nomifun_mcp::McpToolCallPool;
use axum::response::IntoResponse;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

fn make_service() -> McpConnectionTestService {
    McpConnectionTestService::new(test_http_client())
}

fn make_service_with_timeout(timeout: Duration) -> McpConnectionTestService {
    McpConnectionTestService::new(test_http_client()).with_timeout(timeout)
}

#[cfg(windows)]
fn quiet_sleep_command() -> (String, Vec<String>) {
    (
        "cmd.exe".into(),
        vec!["/D".into(), "/C".into(), "ping -n 60 127.0.0.1 >NUL".into()],
    )
}

#[cfg(not(windows))]
fn quiet_sleep_command() -> (String, Vec<String>) {
    ("sleep".into(), vec!["60".into()])
}

#[cfg(windows)]
fn echo_command() -> (String, Vec<String>) {
    (
        "cmd.exe".into(),
        vec!["/D".into(), "/C".into(), "echo hello".into()],
    )
}

#[cfg(not(windows))]
fn echo_command() -> (String, Vec<String>) {
    ("echo".into(), vec!["hello".into()])
}

fn test_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("test http client")
}

// ---------------------------------------------------------------------------
// CT-3: Command not found (ENOENT)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn stdio_nonexistent_command_returns_not_found_error() {
    let svc = make_service();
    let transport = McpServerTransport::Stdio {
        command: "nonexistent-mcp-cmd-xyz-12345".into(),
        args: vec![],
        env: HashMap::new(),
    };

    let result = svc.test_connection("test-server", &transport).await;

    assert!(!result.success);
    let error = result.error.as_deref().unwrap();
    assert!(
        error.contains("Command not found"),
        "expected 'Command not found' in: {error}"
    );
    assert!(result.tools.is_none());
    assert!(result.needs_auth.is_none());
}

// ---------------------------------------------------------------------------
// CT-4: URL not reachable
// ---------------------------------------------------------------------------

#[tokio::test]
async fn http_unreachable_url_returns_connection_error() {
    let svc = make_service_with_timeout(Duration::from_secs(5));
    let transport = McpServerTransport::Http {
        url: "http://127.0.0.1:1/mcp-unreachable".into(),
        headers: HashMap::new(),
    };

    let result = svc.test_connection("test-http", &transport).await;

    assert!(!result.success);
    let error = result.error.as_deref().unwrap();
    assert!(
        error.contains("Connection failed"),
        "expected connection failure in: {error}"
    );
}

#[tokio::test]
async fn sse_unreachable_url_returns_connection_error() {
    let svc = make_service_with_timeout(Duration::from_secs(5));
    let transport = McpServerTransport::Sse {
        url: "http://127.0.0.1:1/sse-unreachable".into(),
        headers: HashMap::new(),
    };

    let result = svc.test_connection("test-sse", &transport).await;

    assert!(!result.success);
    let error = result.error.as_deref().unwrap();
    assert!(
        error.contains("Connection failed"),
        "expected connection failure in: {error}"
    );
}

// ---------------------------------------------------------------------------
// CT-5: HTTP 401 Unauthorized -> needsAuth
// ---------------------------------------------------------------------------

#[tokio::test]
async fn http_401_returns_needs_auth() {
    // Spin up a mock server that returns 401 with WWW-Authenticate
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
        let app = axum::Router::new().route(
            "/mcp",
            axum::routing::post(|| async {
                (
                    axum::http::StatusCode::UNAUTHORIZED,
                    [(axum::http::header::WWW_AUTHENTICATE, "Bearer realm=\"mcp-server\"")],
                    "",
                )
            }),
        );
        axum::serve(listener, app).await.unwrap();
    });

    let svc = make_service();
    let transport = McpServerTransport::Http {
        url: format!("http://{}/mcp", addr),
        headers: HashMap::new(),
    };

    let result = svc.test_connection("auth-server", &transport).await;

    assert!(!result.success);
    assert_eq!(result.needs_auth, Some(true));
    assert!(result.auth_method.is_some());
    assert!(result.www_authenticate.is_some());
    assert!(result.error.is_none());

    server_handle.abort();
}

#[tokio::test]
async fn sse_401_returns_needs_auth() {
    // Spin up a mock server that returns 401 for GET
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
        let app = axum::Router::new().route(
            "/sse",
            axum::routing::get(|| async {
                (
                    axum::http::StatusCode::UNAUTHORIZED,
                    [(axum::http::header::WWW_AUTHENTICATE, "Bearer realm=\"mcp-sse\"")],
                    "",
                )
            }),
        );
        axum::serve(listener, app).await.unwrap();
    });

    let svc = make_service();
    let transport = McpServerTransport::Sse {
        url: format!("http://{}/sse", addr),
        headers: HashMap::new(),
    };

    let result = svc.test_connection("sse-auth", &transport).await;

    assert!(!result.success);
    assert_eq!(result.needs_auth, Some(true));
    assert!(result.www_authenticate.is_some());

    server_handle.abort();
}

#[tokio::test]
async fn sse_connection_test_uses_string_jsonrpc_ids() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (event_tx, event_rx) = mpsc::unbounded_channel::<String>();

    let server_handle = tokio::spawn(async move {
        let mut event_rx = Some(event_rx);
        while let Ok((stream, _)) = listener.accept().await {
            let event_tx = event_tx.clone();
            let rx = event_rx.take();
            tokio::spawn(async move {
                let _ = handle_string_id_sse_connection(stream, event_tx, rx).await;
            });
        }
    });

    let svc = make_service_with_timeout(Duration::from_secs(5));
    let transport = McpServerTransport::Sse {
        url: format!("http://{}/sse", addr),
        headers: HashMap::new(),
    };

    let result = svc.test_connection("string-id-sse", &transport).await;

    assert!(result.success, "expected string-id SSE server to connect: {result:?}");
    let tools = result.tools.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "strict_string_id_tool");

    server_handle.abort();
}

/// The connector call proxy over the **legacy SSE transport**.
///
/// One call per test: the fixture hands its event receiver to the first
/// `GET /sse`, so a second concurrent stream would have nothing to read from.
#[tokio::test]
async fn sse_tool_call_returns_the_upstream_result() {
    let (addr, server_handle) = spawn_string_id_sse_fixture().await;
    let svc = make_service_with_timeout(Duration::from_secs(5));
    let transport = McpServerTransport::Sse {
        url: format!("http://{addr}/sse"),
        headers: HashMap::new(),
    };

    let outcome = svc
        .call_tool(&transport, "echo", serde_json::json!({ "n": 1 }))
        .await
        .expect("the SSE call succeeds");
    assert!(!outcome.is_error);
    assert_eq!(outcome.result["content"][0]["text"], "pong");

    server_handle.abort();
}

#[tokio::test]
async fn sse_tool_level_failure_is_a_result_and_a_server_error_is_not() {
    let (addr, server_handle) = spawn_string_id_sse_fixture().await;
    let svc = make_service_with_timeout(Duration::from_secs(5));
    let transport = McpServerTransport::Sse {
        url: format!("http://{addr}/sse"),
        headers: HashMap::new(),
    };

    // A tool-level failure resolves, with `is_error` set.
    let failed = svc
        .call_tool(&transport, "fail", serde_json::json!({}))
        .await
        .expect("a tool-level failure is a completed call");
    assert!(failed.is_error);
    assert_eq!(failed.result["content"][0]["text"], "boom");

    server_handle.abort();
}

#[tokio::test]
async fn sse_unknown_tool_is_a_call_failure() {
    let (addr, server_handle) = spawn_string_id_sse_fixture().await;
    let svc = make_service_with_timeout(Duration::from_secs(5));
    let transport = McpServerTransport::Sse {
        url: format!("http://{addr}/sse"),
        headers: HashMap::new(),
    };

    let error = svc
        .call_tool(&transport, "no-such-tool", serde_json::json!({}))
        .await
        .expect_err("a JSON-RPC error is a call failure");
    match error {
        McpToolCallError::Failed(message) => {
            assert!(message.contains("tools/call rejected"), "got {message}");
        }
        other => panic!("expected Failed, got {other:?}"),
    }

    server_handle.abort();
}

/// Spawn the SSE fixture and return its address plus the accept-loop handle.
async fn spawn_string_id_sse_fixture() -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (event_tx, event_rx) = mpsc::unbounded_channel::<String>();
    let handle = tokio::spawn(async move {
        let mut event_rx = Some(event_rx);
        while let Ok((stream, _)) = listener.accept().await {
            let event_tx = event_tx.clone();
            let rx = event_rx.take();
            tokio::spawn(async move {
                let _ = handle_string_id_sse_connection(stream, event_tx, rx).await;
            });
        }
    });
    (addr, handle)
}

async fn handle_string_id_sse_connection(
    mut stream: tokio::net::TcpStream,
    event_tx: mpsc::UnboundedSender<String>,
    event_rx: Option<mpsc::UnboundedReceiver<String>>,
) -> std::io::Result<()> {
    let (request, body) = read_http_request(&mut stream).await?;
    if request.starts_with("GET /sse ") {
        let mut event_rx = event_rx.expect("SSE GET should be the first connection");
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\n\r\n",
            )
            .await?;
        stream
            .write_all(b"event: endpoint\ndata: /messages\n\n")
            .await?;
        stream.flush().await?;
        while let Some(message) = event_rx.recv().await {
            stream
                .write_all(format!("event: message\ndata: {message}\n\n").as_bytes())
                .await?;
            stream.flush().await?;
        }
        return Ok(());
    }

    if request.starts_with("POST /messages ") {
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let method = body["method"].as_str().unwrap_or_default();
        match method {
            "initialize" | "tools/list" => {
                let Some(id) = body["id"].as_str() else {
                    write_http_response(
                        &mut stream,
                        "400 Bad Request",
                        "Bad request: id expected a string",
                    )
                    .await?;
                    return Ok(());
                };
                let response = match method {
                    "initialize" => serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "protocolVersion": "2024-11-05",
                            "capabilities": {},
                            "serverInfo": { "name": "strict-string-id", "version": "1.0.0" }
                        }
                    }),
                    _ => serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "tools": [
                                { "name": "strict_string_id_tool", "description": "Requires string JSON-RPC ids" }
                            ]
                        }
                    }),
                };
                event_tx.send(response.to_string()).unwrap();
                write_http_response(&mut stream, "202 Accepted", "").await?;
            }
            "notifications/initialized" => {
                write_http_response(&mut stream, "202 Accepted", "").await?;
            }
            // The connector call proxy's request, answered over the same stream.
            "tools/call" => {
                let Some(id) = body["id"].as_str() else {
                    write_http_response(
                        &mut stream,
                        "400 Bad Request",
                        "Bad request: id expected a string",
                    )
                    .await?;
                    return Ok(());
                };
                let tool = body["params"]["name"].as_str().unwrap_or_default();
                let response = match tool {
                    "echo" => serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [{ "type": "text", "text": "pong" }],
                            "isError": false
                        }
                    }),
                    "fail" => serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [{ "type": "text", "text": "boom" }],
                            "isError": true
                        }
                    }),
                    other => serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32602, "message": format!("unknown tool {other}") }
                    }),
                };
                event_tx.send(response.to_string()).unwrap();
                write_http_response(&mut stream, "202 Accepted", "").await?;
            }
            _ => {
                write_http_response(&mut stream, "400 Bad Request", "unknown method").await?;
            }
        }
        return Ok(());
    }

    write_http_response(&mut stream, "404 Not Found", "").await
}

async fn read_http_request(stream: &mut tokio::net::TcpStream) -> std::io::Result<(String, Vec<u8>)> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    let header_end = loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "connection closed before headers",
            ));
        }
        buffer.extend_from_slice(&chunk[..n]);
        if let Some(pos) = find_header_end(&buffer) {
            break pos;
        }
    };

    let header = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let content_length = header
        .lines()
        .find_map(|line| line.strip_prefix("content-length:").or_else(|| line.strip_prefix("Content-Length:")))
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);

    let body_start = header_end + 4;
    let mut body = buffer[body_start..].to_vec();
    while body.len() < content_length {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(content_length);

    Ok((header, body))
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

async fn write_http_response(
    stream: &mut tokio::net::TcpStream,
    status: &str,
    body: &str,
) -> std::io::Result<()> {
    let response = format!(
        "HTTP/1.1 {status}\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await
}

// ---------------------------------------------------------------------------
// CT-6: Timeout
// ---------------------------------------------------------------------------

#[tokio::test]
async fn stdio_timeout_returns_timeout_error() {
    // Use a platform-native command that produces no stdout so the protocol read blocks.
    let svc = make_service_with_timeout(Duration::from_secs(1));
    let (command, args) = quiet_sleep_command();
    let transport = McpServerTransport::Stdio {
        command,
        args,
        env: HashMap::new(),
    };

    let result = svc.test_connection("timeout-server", &transport).await;

    assert!(!result.success);
    let error = result.error.as_deref().unwrap();
    assert!(error.contains("timed out"), "expected timeout in: {error}");
}

// ---------------------------------------------------------------------------
// HTTP non-success status
// ---------------------------------------------------------------------------

#[tokio::test]
async fn http_500_returns_error_with_status() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
        let app = axum::Router::new().route(
            "/mcp",
            axum::routing::post(|| async { axum::http::StatusCode::INTERNAL_SERVER_ERROR }),
        );
        axum::serve(listener, app).await.unwrap();
    });

    let svc = make_service();
    let transport = McpServerTransport::Http {
        url: format!("http://{}/mcp", addr),
        headers: HashMap::new(),
    };

    let result = svc.test_connection("error-server", &transport).await;

    assert!(!result.success);
    let error = result.error.as_deref().unwrap();
    assert!(error.contains("500"), "expected HTTP 500 in: {error}");

    server_handle.abort();
}

// ---------------------------------------------------------------------------
// HTTP transport with custom headers
// ---------------------------------------------------------------------------

#[tokio::test]
async fn http_custom_headers_are_sent() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
        let app = axum::Router::new().route(
            "/mcp",
            axum::routing::post(|headers: axum::http::HeaderMap| async move {
                // Verify the custom header was received
                if headers.get("x-api-key").and_then(|v| v.to_str().ok()) == Some("secret") {
                    // Return a valid initialize response
                    axum::Json(serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "result": {
                            "protocolVersion": "2024-11-05",
                            "capabilities": {},
                            "serverInfo": { "name": "test", "version": "1.0" }
                        }
                    }))
                } else {
                    // Return error if header missing
                    axum::Json(serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "error": { "code": -1, "message": "Missing API key" }
                    }))
                }
            }),
        );
        axum::serve(listener, app).await.unwrap();
    });

    let svc = make_service();
    let mut headers = HashMap::new();
    headers.insert("X-Api-Key".into(), "secret".into());
    let transport = McpServerTransport::Http {
        url: format!("http://{}/mcp", addr),
        headers,
    };

    let result = svc.test_connection("header-server", &transport).await;

    // The server returns a valid initialize response for request id=1,
    // but the subsequent tools/list (id=2) will also hit the same handler.
    // Either way, the first request should succeed (no initialize error).
    // The tools/list might succeed or fail depending on how the mock handles id=2.
    // For this test, we just verify the custom header was sent (no "Missing API key" error).
    if let Some(ref error) = result.error {
        assert!(
            !error.contains("Missing API key"),
            "Custom header should have been sent"
        );
    }

    server_handle.abort();
}

// ---------------------------------------------------------------------------
// Credential references: resolved, or not sent at all
// ---------------------------------------------------------------------------

/// What the mock MCP server observed, so the assertions are about the wire
/// rather than about a helper's return value.
#[derive(Default)]
struct ObservedRequest {
    authorization: Option<String>,
    query: Option<String>,
}

/// A minimal Streamable-HTTP MCP server that records the auth-bearing parts of
/// every request it receives.
async fn spawn_recording_mcp_server() -> (
    std::net::SocketAddr,
    Arc<Mutex<Vec<ObservedRequest>>>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen: Arc<Mutex<Vec<ObservedRequest>>> = Arc::new(Mutex::new(Vec::new()));
    let recorder = Arc::clone(&seen);

    let handle = tokio::spawn(async move {
        let app = axum::Router::new().route(
            "/mcp",
            axum::routing::post(
                move |uri: axum::http::Uri, headers: axum::http::HeaderMap, body: axum::Json<serde_json::Value>| {
                    let recorder = Arc::clone(&recorder);
                    async move {
                        recorder.lock().unwrap().push(ObservedRequest {
                            authorization: headers
                                .get(axum::http::header::AUTHORIZATION)
                                .and_then(|value| value.to_str().ok())
                                .map(str::to_owned),
                            query: uri.query().map(str::to_owned),
                        });

                        // Answer the handshake by method so the probe reaches
                        // `tools/list` and can succeed.
                        let id = body.get("id").cloned().unwrap_or(serde_json::Value::Null);
                        let result = match body.get("method").and_then(|m| m.as_str()) {
                            Some("initialize") => serde_json::json!({
                                "protocolVersion": "2024-11-05",
                                "capabilities": {},
                                "serverInfo": { "name": "recording", "version": "1.0" }
                            }),
                            Some("tools/list") => serde_json::json!({ "tools": [] }),
                            _ => serde_json::json!({}),
                        };
                        axum::Json(serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": result,
                        }))
                    }
                },
            ),
        );
        axum::serve(listener, app).await.unwrap();
    });

    (addr, seen, handle)
}

#[tokio::test]
async fn an_unresolved_reference_sends_nothing_at_all() {
    let (addr, seen, server_handle) = spawn_recording_mcp_server().await;

    let svc = make_service();
    let mut headers = HashMap::new();
    // Namespaced so this test cannot collide with another test's credentials.
    headers.insert("x-api-key".into(), "${secret:__STEP1_UNSET__}".into());
    let transport = McpServerTransport::Http {
        url: format!("http://{}/mcp", addr),
        headers,
    };

    let result = svc.test_connection("unresolved-server", &transport).await;

    assert!(!result.success, "an unresolvable reference must not probe successfully");
    assert_eq!(
        result.code,
        Some(nomifun_api_types::McpConnectionTestErrorCode::MissingCredential),
    );
    let error = result.error.unwrap_or_default();
    assert!(error.contains("__STEP1_UNSET__"), "the error names the credential: {error}");
    assert!(
        !error.contains("${secret:"),
        "the error must not echo the unresolved template: {error}"
    );
    assert!(
        seen.lock().unwrap().is_empty(),
        "fail-closed means the request is never attempted"
    );

    server_handle.abort();
}

#[tokio::test]
async fn resolved_references_reach_the_wire_in_headers_and_url() {
    let (addr, seen, server_handle) = spawn_recording_mcp_server().await;

    // The process-wide map is what the real hosts install at startup. The name is
    // namespaced so a parallel test cannot observe it by accident.
    nomifun_common::secret_ref::set_credentials(HashMap::from([(
        "__STEP1_TOKEN__".to_owned(),
        "resolved-token".to_owned(),
    )]));

    let svc = make_service();
    let mut headers = HashMap::new();
    // Embedded form, with the literal prefix the marketplace writes.
    headers.insert(
        "Authorization".into(),
        "Bearer ${secret:__STEP1_TOKEN__}".into(),
    );
    // Whole-value form, to prove both survive the same path.
    headers.insert("x-api-key".into(), "secret:__STEP1_TOKEN__".into());
    let url = format!("http://{}/mcp?token=${{secret:__STEP1_TOKEN__}}", addr);
    let transport = McpServerTransport::Http { url, headers };

    let result = svc.test_connection("resolved-server", &transport).await;
    assert!(result.success, "probe should succeed: {:?}", result.error);

    let seen = seen.lock().unwrap();
    assert!(!seen.is_empty(), "the request must have been sent");
    let first = &seen[0];
    assert_eq!(
        first.authorization.as_deref(),
        Some("Bearer resolved-token"),
        "the `Bearer ` prefix comes from the template text, not from the header name"
    );
    assert_eq!(
        first.query.as_deref(),
        Some("token=resolved-token"),
        "a credential in the URL query is resolved too"
    );

    // Leave no credential behind for another test in this binary.
    nomifun_common::secret_ref::set_credentials(HashMap::new());
    server_handle.abort();
}

// ---------------------------------------------------------------------------
// Stdio with args and env
// ---------------------------------------------------------------------------

#[tokio::test]
async fn stdio_with_args_spawns_correctly() {
    // Use a platform-native echo command that exits immediately. Since it doesn't
    // speak MCP, we expect a protocol error rather than a spawn error.
    let svc = make_service_with_timeout(Duration::from_secs(3));
    let (command, args) = echo_command();
    let transport = McpServerTransport::Stdio {
        command,
        args,
        env: HashMap::new(),
    };

    let result = svc.test_connection("echo-server", &transport).await;

    // The command outputs "hello\n" then exits — not valid JSON-RPC.
    assert!(!result.success);
    let error = result.error.as_deref().unwrap();
    // Should be a protocol error, not a spawn error
    assert!(!error.contains("Command not found"), "echo command should be found");
}

// ---------------------------------------------------------------------------
// Pooled stdio sessions (doc 24 §5)
//
// These run against `tests/fixtures/fake_stdio_mcp.mjs`, a real MCP server over
// stdio, because the pooling claim is *"the server process is reused"* — which
// can only be shown by asking the server. It appends one line per event to
// `$FAKE_MCP_LOG`, so `initialize` appears once per spawned process and the
// tests can count sessions instead of trusting the implementation.
// ---------------------------------------------------------------------------

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/fake_stdio_mcp.mjs");

/// A temp directory that removes itself, so repeated runs do not silt up `%TEMP%`.
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after the epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "nomifun-mcp-pool-{tag}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    fn join(&self, name: &str) -> std::path::PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn stdio_transport(log: &std::path::Path) -> McpServerTransport {
    McpServerTransport::Stdio {
        command: "bun".into(),
        args: vec![FIXTURE.into()],
        env: HashMap::from([("FAKE_MCP_LOG".to_owned(), log.to_string_lossy().into_owned())]),
    }
}

fn log_lines(path: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn count(lines: &[String], prefix: &str) -> usize {
    lines.iter().filter(|line| line.starts_with(prefix)).count()
}

#[tokio::test]
async fn a_pooled_stdio_session_is_reused_across_calls() {
    let dir = TempDir::new("reuse");
    let log = dir.join("events.log");
    let transport = stdio_transport(&log);
    let pool = McpToolCallPool::new(make_service_with_timeout(Duration::from_secs(20)));

    for n in [1, 2] {
        let outcome = pool
            .call("connector-1", &transport, "echo", serde_json::json!({ "n": n }))
            .await
            .expect("pooled call should succeed");
        assert!(!outcome.is_error);
    }

    let lines = log_lines(&log);
    assert_eq!(count(&lines, "initialize:"), 1, "the server should be handshaken once: {lines:?}");
    assert_eq!(count(&lines, "call:"), 2, "both calls should reach the same server: {lines:?}");

    // Same process, not merely the same count.
    let pids: std::collections::HashSet<&str> = lines
        .iter()
        .filter_map(|line| line.strip_prefix("call:"))
        .filter_map(|rest| rest.split(':').next())
        .collect();
    assert_eq!(pids.len(), 1, "both calls should hit one process: {lines:?}");

    pool.close_all().await;
}

#[tokio::test]
async fn each_pooled_call_gets_its_own_reply() {
    // The failure this guards against is a pooled session handing a caller the
    // *previous* call's answer — the reason a session that breaks mid-call is
    // discarded rather than reused.
    let dir = TempDir::new("attribution");
    let log = dir.join("events.log");
    let transport = stdio_transport(&log);
    let pool = McpToolCallPool::new(make_service_with_timeout(Duration::from_secs(20)));

    let first = pool
        .call("connector-1", &transport, "echo", serde_json::json!({ "n": 1 }))
        .await
        .expect("first call");
    let second = pool
        .call("connector-1", &transport, "echo", serde_json::json!({ "n": 2 }))
        .await
        .expect("second call");

    assert_eq!(first.result["structuredContent"]["saw"]["n"], 1);
    assert_eq!(second.result["structuredContent"]["saw"]["n"], 2);

    pool.close_all().await;
}

#[tokio::test]
async fn an_idle_session_expires_and_is_replaced() {
    let dir = TempDir::new("ttl");
    let log = dir.join("events.log");
    let transport = stdio_transport(&log);
    let pool = McpToolCallPool::new(make_service_with_timeout(Duration::from_secs(20)))
        .with_idle_ttl(Duration::from_millis(80));

    pool.call("connector-1", &transport, "echo", serde_json::json!({}))
        .await
        .expect("first call");
    tokio::time::sleep(Duration::from_millis(200)).await;
    pool.call("connector-1", &transport, "echo", serde_json::json!({}))
        .await
        .expect("second call");

    let lines = log_lines(&log);
    assert_eq!(
        count(&lines, "initialize:"),
        2,
        "an expired session must be reopened, not reused: {lines:?}"
    );

    pool.close_all().await;
}

#[tokio::test]
async fn editing_the_connector_replaces_the_session() {
    let dir = TempDir::new("edited");
    let log = dir.join("events.log");
    let original = stdio_transport(&log);
    let McpServerTransport::Stdio { command, args, env } = original.clone() else {
        unreachable!("fixture is stdio");
    };
    let edited = McpServerTransport::Stdio {
        command,
        // A different argument is a different server as far as a session is
        // concerned: reusing the old process would run the pre-edit config.
        args: [args.clone(), vec!["--edited".to_owned()]].concat(),
        env,
    };
    let pool = McpToolCallPool::new(make_service_with_timeout(Duration::from_secs(20)));

    pool.call("connector-1", &original, "echo", serde_json::json!({}))
        .await
        .expect("call before the edit");
    pool.call("connector-1", &edited, "echo", serde_json::json!({}))
        .await
        .expect("call after the edit");

    let lines = log_lines(&log);
    assert_eq!(
        count(&lines, "initialize:"),
        2,
        "an edited connector must not reuse the old session: {lines:?}"
    );

    pool.close_all().await;
}

#[tokio::test]
async fn a_rotated_credential_replaces_the_session() {
    // The security-relevant half of the identity rule: the env is compared
    // *after* `secret:` resolution, so a rotated credential cannot keep serving
    // calls through a session opened with the old one.
    let dir = TempDir::new("rotated");
    let log = dir.join("events.log");
    let with_mark = |mark: &str| {
        let McpServerTransport::Stdio { command, args, env } = stdio_transport(&log) else {
            unreachable!("fixture is stdio");
        };
        let mut env = env;
        env.insert("FAKE_MCP_MARK".to_owned(), mark.to_owned());
        McpServerTransport::Stdio { command, args, env }
    };
    let before = with_mark("first-credential");
    let after = with_mark("rotated-credential");
    let pool = McpToolCallPool::new(make_service_with_timeout(Duration::from_secs(20)));

    pool.call("connector-1", &before, "echo", serde_json::json!({}))
        .await
        .expect("call before rotation");
    pool.call("connector-1", &after, "echo", serde_json::json!({}))
        .await
        .expect("call after rotation");

    let lines = log_lines(&log);
    assert_eq!(
        count(&lines, "initialize:"),
        2,
        "a rotated credential must not reuse the old session: {lines:?}"
    );

    pool.close_all().await;
}

#[tokio::test]
async fn a_tool_level_failure_keeps_the_session() {
    let dir = TempDir::new("toolfail");
    let log = dir.join("events.log");
    let transport = stdio_transport(&log);
    let pool = McpToolCallPool::new(make_service_with_timeout(Duration::from_secs(20)));

    let failed = pool
        .call("connector-1", &transport, "boom", serde_json::json!({}))
        .await
        .expect("a tool-level failure is a result, not a transport error");
    assert!(failed.is_error, "the upstream isError flag should survive pooling");

    // The session is still in step, so the next call must not pay for a new one.
    pool.call("connector-1", &transport, "echo", serde_json::json!({}))
        .await
        .expect("call after a tool-level failure");

    let lines = log_lines(&log);
    assert_eq!(count(&lines, "initialize:"), 1, "a tool-level failure is not a broken pipe: {lines:?}");

    pool.close_all().await;
}

#[tokio::test]
async fn a_rejected_call_keeps_the_session() {
    let dir = TempDir::new("rejected");
    let log = dir.join("events.log");
    let transport = stdio_transport(&log);
    let pool = McpToolCallPool::new(make_service_with_timeout(Duration::from_secs(20)));

    let error = pool
        .call("connector-1", &transport, "rpc-error", serde_json::json!({}))
        .await
        .expect_err("a JSON-RPC error is reported as a failed call");
    assert!(
        matches!(error, McpToolCallError::Failed(ref message) if message.contains("bad tool")),
        "expected the server's message, got {error:?}"
    );

    pool.call("connector-1", &transport, "echo", serde_json::json!({}))
        .await
        .expect("call after a rejection");

    let lines = log_lines(&log);
    assert_eq!(
        count(&lines, "initialize:"),
        1,
        "a well-framed rejection leaves the session in step: {lines:?}"
    );

    pool.close_all().await;
}

#[tokio::test]
async fn a_timed_out_call_evicts_the_session() {
    // The subtle hazard pooling introduces: a call that ran out of budget may
    // leave a reply in flight. The session's request/response pairing is then
    // unknown, so it must be killed — otherwise the next call can read the
    // timed-out call's answer and attribute it to the wrong request.
    //
    // The budget covers the *cold start* as well as the deliberate hang, so it
    // is set well above a `bun` start-up (tens of ms, measured) to stay honest
    // under a loaded, parallel test run.
    let dir = TempDir::new("timeout");
    let log = dir.join("events.log");
    let transport = stdio_transport(&log);
    let pool = McpToolCallPool::new(make_service_with_timeout(Duration::from_secs(4)));

    let error = pool
        .call("connector-1", &transport, "hang", serde_json::json!({}))
        .await
        .expect_err("a server that never replies must time out");
    assert!(matches!(error, McpToolCallError::Timeout(_)), "expected a timeout, got {error:?}");

    let recovered = pool
        .call("connector-1", &transport, "echo", serde_json::json!({ "after": true }))
        .await
        .expect("the next call should get a fresh session");
    assert_eq!(recovered.result["structuredContent"]["saw"]["after"], true);

    let lines = log_lines(&log);
    assert_eq!(
        count(&lines, "initialize:"),
        2,
        "a timed-out session must be discarded, not reused: {lines:?}"
    );

    pool.close_all().await;
}

#[tokio::test]
async fn a_broken_session_is_discarded() {
    // Same rule as the timeout case, reached without any timing: the server
    // exits mid-call, so the pipe's state is unknown and the session must go.
    let dir = TempDir::new("broken");
    let log = dir.join("events.log");
    let transport = stdio_transport(&log);
    let pool = McpToolCallPool::new(make_service_with_timeout(Duration::from_secs(20)));

    let error = pool
        .call("connector-1", &transport, "die", serde_json::json!({}))
        .await
        .expect_err("a server that exits mid-call cannot answer");
    assert!(
        matches!(error, McpToolCallError::Failed(ref message) if message.contains("closed stdout")),
        "expected an EOF failure, got {error:?}"
    );

    pool.call("connector-1", &transport, "echo", serde_json::json!({}))
        .await
        .expect("the next call should get a fresh session");

    let lines = log_lines(&log);
    assert_eq!(
        count(&lines, "initialize:"),
        2,
        "a session whose pipe broke must be discarded: {lines:?}"
    );

    pool.close_all().await;
}

#[tokio::test]
async fn a_full_pool_falls_back_to_a_one_shot_call() {
    // Pooling is an optimisation, never a requirement: with no room to keep a
    // second session the call still has to be answered.
    let dir = TempDir::new("capacity");
    let log = dir.join("events.log");
    let transport = stdio_transport(&log);
    let pool = McpToolCallPool::new(make_service_with_timeout(Duration::from_secs(20)))
        .with_capacity(1);

    pool.call("connector-1", &transport, "echo", serde_json::json!({}))
        .await
        .expect("first connector");
    pool.call("connector-2", &transport, "echo", serde_json::json!({}))
        .await
        .expect("second connector must still be answered");

    let lines = log_lines(&log);
    assert_eq!(count(&lines, "initialize:"), 2, "the second call is not pooled: {lines:?}");
    assert_eq!(count(&lines, "call:"), 2);

    // The first connector's session survived the fallback.
    pool.call("connector-1", &transport, "echo", serde_json::json!({}))
        .await
        .expect("first connector again");
    assert_eq!(
        count(&log_lines(&log), "initialize:"),
        2,
        "the pooled connector should still be reused"
    );

    pool.close_all().await;
}

#[tokio::test]
async fn closing_the_pool_kills_the_server_process() {
    // The leak-critical claim, separated from "the process happened to exit".
    // The fixture's `stubborn` tool stops reading stdin and then appends to a
    // heartbeat file forever, so closing its pipes cannot end it: the heartbeat
    // only stops if the process is actually killed. Asserting that it was alive
    // first is what makes the silence afterwards mean something.
    let dir = TempDir::new("kill");
    let log = dir.join("events.log");
    let heartbeat = dir.join("heartbeat");
    let McpServerTransport::Stdio { command, args, mut env } = stdio_transport(&log) else {
        unreachable!("fixture is stdio");
    };
    env.insert("FAKE_MCP_HB".to_owned(), heartbeat.to_string_lossy().into_owned());
    let transport = McpServerTransport::Stdio { command, args, env };
    let pool = McpToolCallPool::new(make_service_with_timeout(Duration::from_secs(20)));

    pool.call("connector-1", &transport, "stubborn", serde_json::json!({}))
        .await
        .expect("the stubborn server answers once");

    // It is alive: the heartbeat is growing even though we are not calling it.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let alive = std::fs::metadata(&heartbeat).map(|meta| meta.len()).unwrap_or(0);
    assert!(alive > 0, "the fixture should be heartbeating before the pool is closed");

    pool.close_all().await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let after = std::fs::metadata(&heartbeat).map(|meta| meta.len()).unwrap_or(0);
    assert_eq!(
        after, alive,
        "close_all must kill the server process, not just close its pipes"
    );
}

#[tokio::test]
async fn closing_the_pool_drops_every_session() {
    let dir = TempDir::new("closeall");
    let log = dir.join("events.log");
    let transport = stdio_transport(&log);
    let pool = McpToolCallPool::new(make_service_with_timeout(Duration::from_secs(20)));

    pool.call("connector-1", &transport, "echo", serde_json::json!({}))
        .await
        .expect("first call");
    pool.close_all().await;

    pool.call("connector-1", &transport, "echo", serde_json::json!({}))
        .await
        .expect("call after close_all");

    let lines = log_lines(&log);
    assert_eq!(
        count(&lines, "initialize:"),
        2,
        "close_all must empty the pool: {lines:?}"
    );

    pool.close_all().await;
}
