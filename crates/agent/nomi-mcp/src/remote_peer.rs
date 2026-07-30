//! Minimal remote MCP peer for host-managed capabilities.
//!
//! This deliberately does not participate in [`crate::manager::McpManager`]:
//! managed providers are fixed by the host and must never become user MCP
//! configuration or model-visible provider tools.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use futures::StreamExt;
use reqwest::{
    Client, Response, StatusCode,
    header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderValue},
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use thiserror::Error;
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::protocol::{
    InitializeResult, JsonRpcRequest, JsonRpcResponse, McpToolDef, McpToolResult,
    ToolsListResult,
};

const CLIENT_PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::V2025_11_25;
const LEGACY_PROTOCOL_VERSION: &str = "2025-11-25";
const MAX_BODY_BYTES: usize = 1024 * 1024;
const MAX_SSE_EVENT_BYTES: usize = 512 * 1024;
const MAX_SSE_EVENTS: usize = 64;
const MAX_SESSION_ID_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProtocolVersion {
    V2025_11_25,
}

impl ProtocolVersion {
    fn as_str(self) -> &'static str {
        match self {
            Self::V2025_11_25 => LEGACY_PROTOCOL_VERSION,
        }
    }

    fn negotiate(value: &str) -> Result<Self, McpPeerError> {
        match value {
            LEGACY_PROTOCOL_VERSION => Ok(Self::V2025_11_25),
            other => Err(McpPeerError::UnsupportedProtocolVersion {
                version: other.to_owned(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionId(String);

impl SessionId {
    fn parse(value: &str) -> Result<Self, McpPeerError> {
        if value.is_empty() || value.len() > MAX_SESSION_ID_BYTES {
            return Err(McpPeerError::Protocol(
                "MCP Session ID has an invalid length".to_owned(),
            ));
        }
        if !value.bytes().all(|byte| (0x21..=0x7e).contains(&byte)) {
            return Err(McpPeerError::Protocol(
                "MCP Session ID contains non-visible ASCII".to_owned(),
            ));
        }
        HeaderValue::from_str(value)
            .map_err(|error| McpPeerError::Protocol(format!("invalid MCP Session ID: {error}")))?;
        Ok(Self(value.to_owned()))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SessionMode {
    Sessionless,
    Stateful(SessionId),
}

impl SessionMode {
    fn session_id(&self) -> Option<&SessionId> {
        match self {
            Self::Sessionless => None,
            Self::Stateful(session_id) => Some(session_id),
        }
    }
}

#[derive(Debug)]
enum PeerState {
    Uninitialized,
    Ready {
        protocol_version: ProtocolVersion,
        session: SessionMode,
        tools: Option<Vec<McpToolDef>>,
    },
}

impl Default for PeerState {
    fn default() -> Self {
        Self::Uninitialized
    }
}

#[derive(Debug, Clone)]
struct ReadyTransport {
    protocol_version: ProtocolVersion,
    session: SessionMode,
}

#[derive(Debug, Error)]
pub enum McpPeerError {
    #[error("remote MCP request timed out")]
    Timeout,
    #[error("remote MCP network error: {0}")]
    Network(String),
    #[error("remote MCP returned HTTP {status}")]
    Http {
        status: StatusCode,
        retry_after: Option<Duration>,
    },
    #[error("remote MCP session expired")]
    SessionExpired,
    #[error("remote MCP does not support protocol version {version}")]
    UnsupportedProtocolVersion { version: String },
    #[error("remote MCP server request is unsupported: {method}")]
    UnsupportedServerRequest { method: String },
    #[error("remote MCP response ID mismatch: expected {expected}, got {actual:?}")]
    ResponseIdMismatch {
        expected: u64,
        actual: Option<u64>,
    },
    #[error("remote MCP response ID was duplicated")]
    DuplicateResponseId,
    #[error("remote MCP response exceeded its size limit")]
    BodyTooLarge,
    #[error("remote MCP protocol error: {0}")]
    Protocol(String),
    #[error("remote MCP JSON-RPC error {code}: {message}")]
    JsonRpc {
        code: i64,
        message: String,
        data: Option<Value>,
    },
}

/// A lazy, process-shareable Streamable HTTP peer.
///
/// No network request is made by [`RemoteMcpPeer::new`]. Initialization and
/// tool discovery happen on the first real search call and are cached for the
/// life of the process, until a Stateful session expires.
pub struct RemoteMcpPeer {
    endpoint: String,
    client: Client,
    state: Mutex<PeerState>,
    connect_gate: Mutex<()>,
    next_request_id: AtomicU64,
}

impl RemoteMcpPeer {
    pub fn new(endpoint: impl Into<String>) -> Result<Self, McpPeerError> {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| McpPeerError::Network(error.to_string()))?;
        Ok(Self {
            endpoint: endpoint.into(),
            client,
            state: Mutex::new(PeerState::default()),
            connect_gate: Mutex::new(()),
            next_request_id: AtomicU64::new(1),
        })
    }

    pub async fn discover_tools(
        &self,
        deadline: Instant,
    ) -> Result<Vec<McpToolDef>, McpPeerError> {
        if let Some(tools) = self.cached_tools().await {
            return Ok(tools);
        }
        self.ensure_initialized(deadline).await?;
        let transport = self.ready_transport().await?;
        let response: ToolsListResult = self
            .request("tools/list", Some(json!({})), &transport, deadline)
            .await?;
        let tools = response.tools;
        let mut state = self.state.lock().await;
        if let PeerState::Ready {
            protocol_version,
            session,
            tools: cached,
        } = &mut *state
            && *protocol_version == transport.protocol_version
            && *session == transport.session
        {
            *cached = Some(tools.clone());
        }
        Ok(tools)
    }

    pub async fn call_tool(
        &self,
        name: &str,
        arguments: Value,
        deadline: Instant,
    ) -> Result<McpToolResult, McpPeerError> {
        self.ensure_initialized(deadline).await?;
        let transport = self.ready_transport().await?;
        let params = json!({ "name": name, "arguments": arguments });
        self.request("tools/call", Some(params), &transport, deadline)
            .await
    }

    /// Best-effort session shutdown. The desktop host awaits this explicitly;
    /// dropping the peer itself never starts asynchronous work.
    pub async fn shutdown(&self, deadline: Instant) -> Result<(), McpPeerError> {
        let transport = {
            let mut state = self.state.lock().await;
            match std::mem::replace(&mut *state, PeerState::Uninitialized) {
                PeerState::Ready {
                    protocol_version,
                    session: SessionMode::Stateful(session),
                    ..
                } => Some(ReadyTransport {
                    protocol_version,
                    session: SessionMode::Stateful(session),
                }),
                PeerState::Ready { .. } | PeerState::Uninitialized => None,
            }
        };
        let Some(ReadyTransport {
            protocol_version,
            session: SessionMode::Stateful(session),
        }) = transport
        else {
            return Ok(());
        };

        let mut headers = standard_headers(protocol_version);
        headers.insert(
            "Mcp-Session-Id",
            HeaderValue::from_str(session.as_str())
                .map_err(|error| McpPeerError::Protocol(error.to_string()))?,
        );
        let response = tokio::time::timeout_at(
            deadline,
            self.client.delete(&self.endpoint).headers(headers).send(),
        )
        .await
        .map_err(|_| McpPeerError::Timeout)?
        .map_err(|error| McpPeerError::Network(error.to_string()))?;
        if response.status().is_success()
            || response.status() == StatusCode::NOT_FOUND
            || response.status() == StatusCode::METHOD_NOT_ALLOWED
        {
            Ok(())
        } else {
            Err(http_error(&response))
        }
    }

    async fn ensure_initialized(&self, deadline: Instant) -> Result<(), McpPeerError> {
        if self.is_ready().await {
            return Ok(());
        }
        let _gate = self.connect_gate.lock().await;
        if self.is_ready().await {
            return Ok(());
        }

        let request_id = self.next_request_id()?;
        let initialize = JsonRpcRequest::new(
            request_id,
            "initialize",
            Some(json!({
                "protocolVersion": CLIENT_PROTOCOL_VERSION.as_str(),
                "capabilities": {},
                "clientInfo": {
                    "name": "flowy-managed-search",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
        );
        let response = self
            .send(initialize, CLIENT_PROTOCOL_VERSION, None, deadline)
            .await?;
        let session = response
            .headers()
            .get("Mcp-Session-Id")
            .and_then(|value| value.to_str().ok())
            .map(SessionId::parse)
            .transpose()?;
        let initialize_result: InitializeResult =
            decode_rpc(response, request_id, deadline).await?;
        let protocol_version = ProtocolVersion::negotiate(&initialize_result.protocol_version)?;
        let session = session.map_or(SessionMode::Sessionless, SessionMode::Stateful);

        let notification = JsonRpcRequest::notification("notifications/initialized", None);
        let notification_response = self
            .send(
                notification,
                protocol_version,
                session.session_id(),
                deadline,
            )
            .await?;
        if !notification_response.status().is_success() {
            return Err(http_error(&notification_response));
        }

        *self.state.lock().await = PeerState::Ready {
            protocol_version,
            session,
            tools: None,
        };
        Ok(())
    }

    async fn request<T: DeserializeOwned>(
        &self,
        method: &str,
        params: Option<Value>,
        transport: &ReadyTransport,
        deadline: Instant,
    ) -> Result<T, McpPeerError> {
        let request_id = self.next_request_id()?;
        let response = self
            .send(
                JsonRpcRequest::new(request_id, method, params),
                transport.protocol_version,
                transport.session.session_id(),
                deadline,
            )
            .await?;
        decode_rpc(response, request_id, deadline).await
    }

    async fn send(
        &self,
        request: JsonRpcRequest,
        protocol_version: ProtocolVersion,
        session: Option<&SessionId>,
        deadline: Instant,
    ) -> Result<Response, McpPeerError> {
        let mut headers = standard_headers(protocol_version);
        if let Some(session) = session {
            headers.insert(
                "Mcp-Session-Id",
                HeaderValue::from_str(session.as_str())
                    .map_err(|error| McpPeerError::Protocol(error.to_string()))?,
            );
        }
        let response = tokio::time::timeout_at(
            deadline,
            self.client
                .post(&self.endpoint)
                .headers(headers)
                .json(&request)
                .send(),
        )
        .await
        .map_err(|_| McpPeerError::Timeout)?
        .map_err(|error| McpPeerError::Network(error.to_string()))?;
        if !response.status().is_success() {
            if response.status() == StatusCode::NOT_FOUND && session.is_some() {
                self.reset_state().await;
                return Err(McpPeerError::SessionExpired);
            }
            return Err(http_error(&response));
        }
        Ok(response)
    }

    async fn ready_transport(&self) -> Result<ReadyTransport, McpPeerError> {
        match &*self.state.lock().await {
            PeerState::Ready {
                protocol_version,
                session,
                ..
            } => Ok(ReadyTransport {
                protocol_version: *protocol_version,
                session: session.clone(),
            }),
            PeerState::Uninitialized => Err(McpPeerError::Protocol(
                "remote MCP peer is not initialized".to_owned(),
            )),
        }
    }

    async fn cached_tools(&self) -> Option<Vec<McpToolDef>> {
        match &*self.state.lock().await {
            PeerState::Ready {
                tools: Some(tools),
                ..
            } => Some(tools.clone()),
            _ => None,
        }
    }

    async fn is_ready(&self) -> bool {
        matches!(*self.state.lock().await, PeerState::Ready { .. })
    }

    async fn reset_state(&self) {
        *self.state.lock().await = PeerState::Uninitialized;
    }

    fn next_request_id(&self) -> Result<u64, McpPeerError> {
        self.next_request_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .map_err(|_| McpPeerError::Protocol("JSON-RPC request ID exhausted".to_owned()))
    }
}

fn standard_headers(protocol_version: ProtocolVersion) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/json, text/event-stream"),
    );
    headers.insert(
        "MCP-Protocol-Version",
        HeaderValue::from_static(protocol_version.as_str()),
    );
    headers
}

fn http_error(response: &Response) -> McpPeerError {
    let retry_after = parse_retry_after(response.headers(), SystemTime::now());
    McpPeerError::Http {
        status: response.status(),
        retry_after,
    }
}

fn parse_retry_after(headers: &HeaderMap, local_now: SystemTime) -> Option<Duration> {
    let value = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?;
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let retry_at = httpdate::parse_http_date(value).ok()?;
    let base = headers
        .get(reqwest::header::DATE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| httpdate::parse_http_date(value).ok())
        .unwrap_or(local_now);
    retry_at.duration_since(base).ok()
}

async fn decode_rpc<T: DeserializeOwned>(
    response: Response,
    expected_id: u64,
    deadline: Instant,
) -> Result<T, McpPeerError> {
    tokio::time::timeout_at(deadline, decode_rpc_inner(response, expected_id))
        .await
        .map_err(|_| McpPeerError::Timeout)?
}

async fn decode_rpc_inner<T: DeserializeOwned>(
    response: Response,
    expected_id: u64,
) -> Result<T, McpPeerError> {
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let bytes = read_limited(response).await?;
    let payload = if content_type.contains("text/event-stream") {
        decode_sse(&bytes, expected_id)?
    } else {
        bytes
    };
    let rpc: JsonRpcResponse = serde_json::from_slice(&payload)
        .map_err(|error| McpPeerError::Protocol(format!("invalid JSON-RPC response: {error}")))?;
    if rpc.id != Some(expected_id) {
        return Err(McpPeerError::ResponseIdMismatch {
            expected: expected_id,
            actual: rpc.id,
        });
    }
    if let Some(error) = rpc.error {
        return Err(McpPeerError::JsonRpc {
            code: error.code,
            message: error.message,
            data: error.data,
        });
    }
    let result = rpc
        .result
        .ok_or_else(|| McpPeerError::Protocol("JSON-RPC response omitted result".to_owned()))?;
    serde_json::from_value(result)
        .map_err(|error| McpPeerError::Protocol(format!("invalid MCP result: {error}")))
}

async fn read_limited(response: Response) -> Result<Vec<u8>, McpPeerError> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| McpPeerError::Network(error.to_string()))?;
        if body.len().saturating_add(chunk.len()) > MAX_BODY_BYTES {
            return Err(McpPeerError::BodyTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn decode_sse(body: &[u8], expected_id: u64) -> Result<Vec<u8>, McpPeerError> {
    let text = std::str::from_utf8(body)
        .map_err(|error| McpPeerError::Protocol(format!("SSE was not UTF-8: {error}")))?;
    let normalized = text.replace("\r\n", "\n");
    let mut events = 0usize;
    let mut selected: Option<Value> = None;
    for block in normalized.split("\n\n") {
        if block.trim().is_empty() {
            continue;
        }
        events += 1;
        if events > MAX_SSE_EVENTS || block.len() > MAX_SSE_EVENT_BYTES {
            return Err(McpPeerError::BodyTooLarge);
        }
        let data = block
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(str::trim_start)
            .collect::<Vec<_>>()
            .join("\n");
        if data.is_empty() {
            continue;
        }
        let message: Value = serde_json::from_str(&data)
            .map_err(|error| McpPeerError::Protocol(format!("invalid SSE JSON: {error}")))?;
        if message.get("id").is_none() {
            if let Some(method) = message.get("method").and_then(Value::as_str) {
                if method == "notifications/message" || method.starts_with("notifications/") {
                    continue;
                }
                return Err(McpPeerError::UnsupportedServerRequest {
                    method: method.to_owned(),
                });
            }
            continue;
        }
        if message.get("method").is_some() {
            return Err(McpPeerError::UnsupportedServerRequest {
                method: message
                    .get("method")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_owned(),
            });
        }
        let actual = message
            .get("id")
            .and_then(Value::as_u64)
            .ok_or_else(|| McpPeerError::Protocol("SSE response ID was not an integer".to_owned()))?;
        if actual != expected_id {
            return Err(McpPeerError::ResponseIdMismatch {
                expected: expected_id,
                actual: Some(actual),
            });
        }
        if selected.replace(message).is_some() {
            return Err(McpPeerError::DuplicateResponseId);
        }
    }
    selected
        .map(|value| serde_json::to_vec(&value).expect("JSON value must serialize"))
        .ok_or_else(|| McpPeerError::Protocol("SSE contained no matching response".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_partial_json, method},
    };

    #[test]
    fn sse_matches_response_after_notification() {
        let decoded = decode_sse(
            b"data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/message\"}\n\ndata: {\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"ok\":true}}\n\n",
            7,
        )
        .expect("matching response");
        assert_eq!(serde_json::from_slice::<Value>(&decoded).unwrap()["id"], 7);
    }

    #[test]
    fn sse_rejects_server_requests() {
        let result = decode_sse(
            b"data: {\"jsonrpc\":\"2.0\",\"id\":8,\"method\":\"ping\"}\n\n",
            7,
        );
        assert!(matches!(
            result,
            Err(McpPeerError::UnsupportedServerRequest { .. })
        ));
    }

    #[test]
    fn sse_rejects_duplicate_response_ids() {
        let result = decode_sse(
            b"data: {\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{}}\n\ndata: {\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{}}\n\n",
            7,
        );
        assert!(matches!(result, Err(McpPeerError::DuplicateResponseId)));
    }

    #[test]
    fn sse_rejects_oversized_event() {
        let oversized = format!("data: {}\n\n", "x".repeat(MAX_SSE_EVENT_BYTES));
        let result = decode_sse(oversized.as_bytes(), 7);
        assert!(matches!(result, Err(McpPeerError::BodyTooLarge)));
    }

    #[test]
    fn retry_after_supports_seconds_and_http_date() {
        let mut headers = HeaderMap::new();
        headers.insert("Retry-After", HeaderValue::from_static("120"));
        assert_eq!(
            parse_retry_after(&headers, SystemTime::UNIX_EPOCH),
            Some(Duration::from_secs(120))
        );

        let mut headers = HeaderMap::new();
        headers.insert("Date", HeaderValue::from_static("Thu, 01 Jan 1970 00:00:00 GMT"));
        headers.insert(
            "Retry-After",
            HeaderValue::from_static("Thu, 01 Jan 1970 00:02:00 GMT"),
        );
        assert_eq!(
            parse_retry_after(&headers, SystemTime::UNIX_EPOCH),
            Some(Duration::from_secs(120))
        );
    }

    #[tokio::test]
    async fn legacy_peer_initializes_once_then_lists_calls_and_shuts_down() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "initialize"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "protocolVersion": LEGACY_PROTOCOL_VERSION,
                    "capabilities": {},
                    "serverInfo": {"name": "mock", "version": "1"}
                }
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(
                json!({"method": "notifications/initialized"}),
            ))
            .respond_with(ResponseTemplate::new(202))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "tools/list"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 2,
                "result": {"tools": [{"name": "web_search", "inputSchema": {}}]}
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "tools/call"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 3,
                "result": {"content": [{"type": "text", "text": "ok"}]}
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;

        let peer = RemoteMcpPeer::new(server.uri()).expect("peer");
        let deadline = Instant::now() + Duration::from_secs(2);
        let tools = peer.discover_tools(deadline).await.expect("tools");
        assert_eq!(tools.len(), 1);
        let result = peer
            .call_tool("web_search", json!({"query": "offline"}), deadline)
            .await
            .expect("call");
        assert!(!result.is_error);
        peer.shutdown(deadline).await.expect("shutdown");
    }

    #[tokio::test]
    async fn stateful_peer_rejects_expired_session_without_internal_retry() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "initialize"})))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Mcp-Session-Id", "session-1")
                    .set_body_json(json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "result": {
                            "protocolVersion": LEGACY_PROTOCOL_VERSION,
                            "capabilities": {}
                        }
                    })),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "notifications/initialized"})))
            .respond_with(ResponseTemplate::new(202))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "tools/call"})))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;

        let peer = RemoteMcpPeer::new(server.uri()).expect("peer");
        peer.ensure_initialized(Instant::now() + Duration::from_secs(2))
            .await
            .expect("initialize");
        let result = peer
            .call_tool(
                "web_search",
                json!({"query": "offline"}),
                Instant::now() + Duration::from_secs(2),
            )
            .await;
        assert!(matches!(result, Err(McpPeerError::SessionExpired)));
        assert!(!peer.is_ready().await);
    }

    #[tokio::test]
    async fn constructing_peer_does_not_contact_endpoint() {
        let server = MockServer::start().await;
        let _peer = RemoteMcpPeer::new(server.uri()).expect("offline peer construction");
        assert!(
            server
                .received_requests()
                .await
                .expect("request log")
                .is_empty()
        );
    }
}
