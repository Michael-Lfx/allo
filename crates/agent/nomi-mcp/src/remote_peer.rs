//! Minimal remote MCP peer for host-managed capabilities.
//!
//! This deliberately does not participate in [`crate::manager::McpManager`]:
//! managed providers are fixed by the host and must never become user MCP
//! configuration or model-visible provider tools.

use std::time::Duration;

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
    ClientCapabilities, ClientInfo, InitializeParams, JsonRpcRequest, JsonRpcResponse, McpToolDef,
    McpToolResult, ToolsListResult,
};

const LEGACY_PROTOCOL_VERSION: &str = "2025-11-25";
const MAX_BODY_BYTES: usize = 1024 * 1024;
const MAX_SSE_EVENT_BYTES: usize = 512 * 1024;
const MAX_SSE_EVENTS: usize = 64;

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
    #[error("remote MCP response exceeded its size limit")]
    BodyTooLarge,
    #[error("remote MCP protocol error: {0}")]
    Protocol(String),
    #[error("remote MCP JSON-RPC error {code}: {message}")]
    JsonRpc { code: i64, message: String },
}

#[derive(Default)]
struct PeerState {
    session_id: Option<String>,
    tools: Option<Vec<McpToolDef>>,
}

/// A lazy, process-shareable Legacy Streamable HTTP peer.
///
/// No network request is made by [`RemoteMcpPeer::new`]. Initialization and
/// tool discovery happen on the first real search call and are cached for the
/// life of the process.
pub struct RemoteMcpPeer {
    endpoint: String,
    client: Client,
    state: Mutex<PeerState>,
    connect_gate: Mutex<()>,
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
        })
    }

    pub async fn discover_tools(
        &self,
        deadline: Instant,
    ) -> Result<Vec<McpToolDef>, McpPeerError> {
        if let Some(tools) = self.state.lock().await.tools.clone() {
            return Ok(tools);
        }
        self.ensure_initialized(deadline).await?;
        let response: ToolsListResult = self
            .request("tools/list", Some(json!({})), deadline)
            .await?;
        self.state.lock().await.tools = Some(response.tools.clone());
        Ok(response.tools)
    }

    pub async fn call_tool(
        &self,
        name: &str,
        arguments: Value,
        deadline: Instant,
    ) -> Result<McpToolResult, McpPeerError> {
        self.ensure_initialized(deadline).await?;
        let params = json!({ "name": name, "arguments": arguments });
        match self.request("tools/call", Some(params.clone()), deadline).await {
            Err(McpPeerError::Http {
                status: StatusCode::NOT_FOUND,
                ..
            }) => {
                // Some Legacy peers expire server-side sessions. Rebuild once;
                // searches are read-only and the router never repeats this
                // provider again in the same attempt.
                self.reset_session().await;
                self.ensure_initialized(deadline).await?;
                self.request("tools/call", Some(params), deadline).await
            }
            result => result,
        }
    }

    /// Best-effort Legacy session shutdown. The desktop host may await this on
    /// graceful termination; dropping the peer itself never starts async work.
    pub async fn shutdown(&self, deadline: Instant) -> Result<(), McpPeerError> {
        let session_id = self.state.lock().await.session_id.take();
        let Some(session_id) = session_id else {
            return Ok(());
        };
        let response = tokio::time::timeout_at(
            deadline,
            self.client
                .delete(&self.endpoint)
                .header("MCP-Protocol-Version", LEGACY_PROTOCOL_VERSION)
                .header("Mcp-Session-Id", session_id)
                .send(),
        )
        .await
        .map_err(|_| McpPeerError::Timeout)?
        .map_err(|error| McpPeerError::Network(error.to_string()))?;
        if response.status().is_success() || response.status() == StatusCode::NOT_FOUND {
            Ok(())
        } else {
            Err(http_error(&response))
        }
    }

    async fn ensure_initialized(&self, deadline: Instant) -> Result<(), McpPeerError> {
        if self.state.lock().await.session_id.is_some() {
            return Ok(());
        }
        let _gate = self.connect_gate.lock().await;
        if self.state.lock().await.session_id.is_some() {
            return Ok(());
        }

        let params = serde_json::to_value(InitializeParams {
            protocol_version: LEGACY_PROTOCOL_VERSION.to_owned(),
            capabilities: ClientCapabilities { tools: Some(json!({})) },
            client_info: ClientInfo {
                name: "flowy-managed-search".to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
            },
        })
        .map_err(|error| McpPeerError::Protocol(error.to_string()))?;
        let response = self
            .send(JsonRpcRequest::new(1, "initialize", Some(params)), None, deadline)
            .await?;
        let session_id = response
            .headers()
            .get("Mcp-Session-Id")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
            .ok_or_else(|| {
                McpPeerError::Protocol("Legacy initialize omitted Mcp-Session-Id".to_owned())
            })?;
        let _: Value = decode_rpc(response, deadline).await?;
        self.state.lock().await.session_id = Some(session_id.clone());

        let response = self
            .send(
                JsonRpcRequest::notification("notifications/initialized", None),
                Some(&session_id),
                deadline,
            )
            .await?;
        if !response.status().is_success() {
            return Err(http_error(&response));
        }
        Ok(())
    }

    async fn request<T: DeserializeOwned>(
        &self,
        method: &str,
        params: Option<Value>,
        deadline: Instant,
    ) -> Result<T, McpPeerError> {
        let session_id = self.state.lock().await.session_id.clone();
        let response = self
            .send(JsonRpcRequest::new(2, method, params), session_id.as_deref(), deadline)
            .await?;
        decode_rpc(response, deadline).await
    }

    async fn send(
        &self,
        request: JsonRpcRequest,
        session_id: Option<&str>,
        deadline: Instant,
    ) -> Result<Response, McpPeerError> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/json, text/event-stream"),
        );
        headers.insert(
            "MCP-Protocol-Version",
            HeaderValue::from_static(LEGACY_PROTOCOL_VERSION),
        );
        if let Some(session_id) = session_id {
            headers.insert(
                "Mcp-Session-Id",
                HeaderValue::from_str(session_id)
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
            return Err(http_error(&response));
        }
        Ok(response)
    }

    async fn reset_session(&self) {
        let mut state = self.state.lock().await;
        state.session_id = None;
        state.tools = None;
    }
}

fn http_error(response: &Response) -> McpPeerError {
    let retry_after = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs);
    McpPeerError::Http {
        status: response.status(),
        retry_after,
    }
}

async fn decode_rpc<T: DeserializeOwned>(
    response: Response,
    deadline: Instant,
) -> Result<T, McpPeerError> {
    tokio::time::timeout_at(deadline, decode_rpc_inner(response))
        .await
        .map_err(|_| McpPeerError::Timeout)?
}

async fn decode_rpc_inner<T: DeserializeOwned>(response: Response) -> Result<T, McpPeerError> {
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let bytes = read_limited(response).await?;
    let payload = if content_type.contains("text/event-stream") {
        decode_sse(&bytes)?
    } else {
        bytes
    };
    let rpc: JsonRpcResponse = serde_json::from_slice(&payload)
        .map_err(|error| McpPeerError::Protocol(format!("invalid JSON-RPC response: {error}")))?;
    if let Some(error) = rpc.error {
        return Err(McpPeerError::JsonRpc {
            code: error.code,
            message: error.message,
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

fn decode_sse(body: &[u8]) -> Result<Vec<u8>, McpPeerError> {
    let text = std::str::from_utf8(body)
        .map_err(|error| McpPeerError::Protocol(format!("SSE was not UTF-8: {error}")))?;
    let normalized = text.replace("\r\n", "\n");
    let mut events = 0usize;
    let mut selected = None;
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
        if !data.is_empty() {
            selected = Some(data.into_bytes());
        }
    }
    selected.ok_or_else(|| McpPeerError::Protocol("SSE contained no data event".to_owned()))
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
    fn sse_limits_and_extracts_last_data_event() {
        let decoded = decode_sse(b"event: message\ndata: {\"one\":1}\n\ndata: {\"two\":2}\n\n")
            .expect("valid SSE");
        assert_eq!(decoded, br#"{"two":2}"#);
    }

    #[test]
    fn empty_sse_is_rejected() {
        assert!(matches!(
            decode_sse(b": keepalive\n\n"),
            Err(McpPeerError::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn legacy_peer_initializes_once_then_lists_calls_and_shuts_down() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "initialize"})))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Mcp-Session-Id", "test-session")
                    .set_body_json(json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "result": {
                            "protocolVersion": LEGACY_PROTOCOL_VERSION,
                            "capabilities": {},
                            "serverInfo": {"name": "mock", "version": "1"}
                        }
                    })),
            )
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
                "result": {
                    "tools": [{
                        "name": "web_search",
                        "description": "search",
                        "inputSchema": {"type": "object", "properties": {"query": {"type": "string"}}}
                    }]
                }
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "tools/call"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 2,
                "result": {"content": [{"type": "text", "text": "ok"}]}
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
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
