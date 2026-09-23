mod pool;
mod protocol;
mod session;

pub use pool::McpToolCallPool;

use std::collections::HashMap;
use std::ffi::OsString;
use std::sync::Arc;
use std::time::Duration;

use nomifun_api_types::{McpConnectionTestErrorCode, McpConnectionTestResult};
use nomi_process_runtime::{ChildProcessBuilder as CmdBuilder, kill_process_tree};
use nomifun_runtime::resolve_command_path;
use serde::Serialize;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::types::McpServerTransport;
use crate::oauth_service::McpOAuthService;
use crate::error::McpError;
use protocol::{
    JsonRpcRequest, JsonRpcResponse, SseEvent, build_http_headers, build_initialize_request,
    build_initialized_notification, build_tools_call_request, build_tools_list_request, error_result,
    read_sse_events, rpc_error_result, run_stdio_protocol, spawn_error_result, success_result,
    timeout_result, tool_call_reply, wait_for_endpoint, wait_for_jsonrpc_response,
};
use session::{StdioCallError, StdioIdentity, StdioToolSession};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const CONNECTION_TIMEOUT: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------
// Tool calling (the connector call proxy's execution seam)
// ---------------------------------------------------------------------------

/// The upstream reply to one MCP tool call.
///
/// `result` is the server's own result object, verbatim (`content`,
/// `structuredContent`, …). This layer lifts exactly one field out of it —
/// [`Self::is_error`] — because a tool-level failure is something the caller
/// **branches on**, whereas a transport failure is something it catches. Those
/// two must never collapse into one another.
#[derive(Debug, Clone)]
pub struct McpToolCallOutcome {
    /// The upstream `isError` flag (`false` when the server omitted it).
    pub is_error: bool,
    /// The upstream result object, unmodified.
    pub result: serde_json::Value,
}

/// Why one MCP tool call produced no result.
///
/// Carries no credential: the messages come from the same transport layer as
/// the probe's, which reports response-side facts (status, WWW-Authenticate
/// presence, protocol errors) and never request headers or env values.
#[derive(Debug)]
pub enum McpToolCallError {
    /// The call did not finish inside the budget.
    Timeout(Duration),
    /// Transport, protocol, or server failure. Human-readable, never parsed.
    Failed(String),
}

impl std::fmt::Display for McpToolCallError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout(budget) => {
                write!(formatter, "MCP tool call timed out after {}s", budget.as_secs())
            }
            Self::Failed(message) => write!(formatter, "{message}"),
        }
    }
}

// ---------------------------------------------------------------------------
// McpConnectionTestService
// ---------------------------------------------------------------------------

/// Service for testing MCP server connectivity.
///
/// Creates a temporary MCP client, performs the protocol handshake
/// (initialize -> initialized -> tools/list), and returns the tool list
/// or an error.  Supports stdio, HTTP (Streamable HTTP), and SSE transports.
#[derive(Clone)]
pub struct McpConnectionTestService {
    http_client: HttpClientFactory,
    timeout: Duration,
    oauth_service: Option<McpOAuthService>,
}

type HttpClientFactory = Arc<dyn Fn() -> reqwest::Client + Send + Sync>;

impl McpConnectionTestService {
    pub fn new(http_client: reqwest::Client) -> Self {
        Self {
            http_client: Arc::new(move || http_client.clone()),
            timeout: CONNECTION_TIMEOUT,
            oauth_service: None,
        }
    }

    pub fn new_dynamic() -> Self {
        Self {
            http_client: Arc::new(nomifun_net::http_client),
            timeout: CONNECTION_TIMEOUT,
            oauth_service: None,
        }
    }

    fn http_client(&self) -> reqwest::Client {
        (self.http_client)()
    }

    /// Override the connection test timeout (default: 30s).
    pub fn with_timeout(self, timeout: Duration) -> Self {
        Self { timeout, ..self }
    }

    /// Use stored OAuth credentials for HTTP and SSE probes when the transport
    /// does not already provide an explicit Authorization header.
    pub fn with_oauth_service(self, oauth_service: McpOAuthService) -> Self {
        Self {
            oauth_service: Some(oauth_service),
            ..self
        }
    }

    /// Test connectivity to an MCP server.
    ///
    /// Dispatches to the appropriate transport handler.  Always returns
    /// a result (never errors) -- failures are encoded in the struct.
    pub async fn test_connection(&self, name: &str, transport: &McpServerTransport) -> McpConnectionTestResult {
        debug!(name, ?transport, "starting MCP connection test");
        match transport {
            McpServerTransport::Stdio { command, args, env } => self.test_stdio(command, args, env).await,
            McpServerTransport::Http { url, headers } => self.test_http(url, headers).await,
            McpServerTransport::Sse { url, headers } => self.test_sse(url, headers).await,
        }
    }

    // -- Tool calling -----------------------------------------------------

    /// Call one tool on a configured MCP server and return its raw result.
    ///
    /// **This opens a session and closes it again — one call, one connection.**
    /// The service is the stateless transport seam; caching a connection is a
    /// separate concern with its own lifetime rules, and lives in
    /// [`McpToolCallPool`], which wraps this method. Reusing the agent engine's
    /// `McpManager` here instead would import a Nomi session's lifetime into a
    /// surface that has no session at all.
    ///
    /// Transport and credential handling are the probe's, not a copy of it:
    /// `secret:` env resolution, OAuth bearer injection with the 401-refresh
    /// retry, and process-tree cleanup all come from the same code the
    /// connection test uses.
    pub async fn call_tool(
        &self,
        transport: &McpServerTransport,
        tool: &str,
        arguments: serde_json::Value,
    ) -> Result<McpToolCallOutcome, McpToolCallError> {
        let budget = self.timeout;
        match tokio::time::timeout(budget, self.call_tool_inner(transport, tool, arguments)).await {
            Ok(outcome) => outcome,
            Err(_) => Err(McpToolCallError::Timeout(budget)),
        }
    }

    async fn call_tool_inner(
        &self,
        transport: &McpServerTransport,
        tool: &str,
        arguments: serde_json::Value,
    ) -> Result<McpToolCallOutcome, McpToolCallError> {
        match transport {
            McpServerTransport::Stdio { command, args, env } => {
                self.call_stdio(command, args, env, tool, arguments).await
            }
            McpServerTransport::Http { url, headers } => {
                self.call_http(url, headers, tool, arguments).await
            }
            // The legacy `sse` transport: handshake over a streamed GET plus
            // POSTed JSON-RPC, exactly as the probe does it.
            McpServerTransport::Sse { url, headers } => {
                self.call_sse(url, headers, tool, arguments).await
            }
        }
    }

    async fn call_sse(
        &self,
        url: &str,
        headers: &HashMap<String, String>,
        tool: &str,
        arguments: serde_json::Value,
    ) -> Result<McpToolCallOutcome, McpToolCallError> {
        let client = self.http_client();
        let (resolved_url, mut req_headers, oauth_managed) =
            self.request_headers(url, headers).await.map_err(call_failure)?;

        // 1. Open the stream, with the same one-shot 401 refresh the probe uses.
        let mut refreshed = false;
        let resp = loop {
            let response = client
                .get(&resolved_url)
                .headers(req_headers.clone())
                .header(reqwest::header::ACCEPT, "text/event-stream")
                .send()
                .await
                .map_err(|error| {
                    McpToolCallError::Failed(format!("SSE connection failed: {error}"))
                })?;
            if response.status() == reqwest::StatusCode::UNAUTHORIZED && oauth_managed && !refreshed {
                let Some(oauth_service) = self.oauth_service.as_ref() else {
                    break response;
                };
                let token = oauth_service
                    .refresh_access_token(url)
                    .await
                    .map_err(|error| McpToolCallError::Failed(error.to_string()))?;
                req_headers.insert(
                    reqwest::header::AUTHORIZATION,
                    reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                        .expect("OAuth access token must be a valid header value"),
                );
                refreshed = true;
                continue;
            }
            break response;
        };
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(McpToolCallError::Failed(
                "the SSE server requires authorization".to_owned(),
            ));
        }
        if !resp.status().is_success() {
            return Err(McpToolCallError::Failed(format!(
                "HTTP {} from SSE server",
                resp.status()
            )));
        }

        // 2. Reader task, aborted on every exit path below.
        let (event_tx, mut event_rx) = mpsc::channel::<SseEvent>(16);
        let reader_handle = tokio::spawn(read_sse_events(resp, event_tx));
        req_headers.insert(
            reqwest::header::CONTENT_TYPE,
            "application/json".parse().expect("valid header"),
        );

        let result = self
            .run_sse_tool_call(
                &client,
                &resolved_url,
                &mut req_headers,
                oauth_managed,
                &mut event_rx,
                tool,
                &arguments,
            )
            .await;
        reader_handle.abort();
        result
    }

    /// `initialize → initialized → tools/call` over an already-open SSE stream.
    ///
    /// A sibling of `run_sse_protocol` for the same reason
    /// [`run_stdio_tool_call`] is a sibling of `run_stdio_protocol`: the probe's
    /// per-stage result details are its existing behaviour.
    #[allow(clippy::too_many_arguments)]
    async fn run_sse_tool_call(
        &self,
        client: &reqwest::Client,
        base_url: &str,
        headers: &mut reqwest::header::HeaderMap,
        oauth_managed: bool,
        event_rx: &mut mpsc::Receiver<SseEvent>,
        tool: &str,
        arguments: &serde_json::Value,
    ) -> Result<McpToolCallOutcome, McpToolCallError> {
        let endpoint = wait_for_endpoint(event_rx, base_url)
            .await
            .map_err(|error| McpToolCallError::Failed(format!("SSE endpoint: {error}")))?;

        self.sse_post_with_auth(
            client,
            base_url,
            &endpoint,
            headers,
            oauth_managed,
            &build_initialize_request(1),
            "initialize_send",
        )
        .await
        .map_err(call_failure)?;
        let init_resp = wait_for_jsonrpc_response(event_rx)
            .await
            .map_err(|error| McpToolCallError::Failed(format!("initialize response: {error}")))?;
        if let Some(error) = init_resp.error {
            return Err(McpToolCallError::Failed(format!(
                "initialize rejected: {} (code {})",
                error.message, error.code
            )));
        }

        let _ = self
            .sse_post_with_auth(
                client,
                base_url,
                &endpoint,
                headers,
                oauth_managed,
                &build_initialized_notification(),
                "initialized_send",
            )
            .await;

        self.sse_post_with_auth(
            client,
            base_url,
            &endpoint,
            headers,
            oauth_managed,
            &build_tools_call_request(2, tool, arguments),
            "tools_call_send",
        )
        .await
        .map_err(call_failure)?;
        let call_resp = wait_for_jsonrpc_response(event_rx)
            .await
            .map_err(|error| McpToolCallError::Failed(format!("tools/call response: {error}")))?;
        if let Some(error) = call_resp.error {
            return Err(McpToolCallError::Failed(format!(
                "tools/call rejected: {} (code {})",
                error.message, error.code
            )));
        }

        let reply = tool_call_reply(call_resp.result.unwrap_or(serde_json::Value::Null));
        Ok(McpToolCallOutcome {
            is_error: reply.is_error,
            result: reply.result,
        })
    }

    /// One stdio call on a session that is opened and closed around it.
    ///
    /// This is the non-pooled path: it is what the pool falls back to when it
    /// cannot (or should not) keep a session, and it is the path the pool's
    /// `AtCapacity` branch takes. Sharing [`StdioToolSession`] with the pool
    /// means the handshake, the framing and the process cleanup cannot drift
    /// between the two.
    async fn call_stdio(
        &self,
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
        tool: &str,
        arguments: serde_json::Value,
    ) -> Result<McpToolCallOutcome, McpToolCallError> {
        let identity = StdioIdentity::new(command, args, env);
        let mut session = StdioToolSession::connect(identity)
            .await
            .map_err(McpToolCallError::Failed)?;
        let reply = session.call(tool, &arguments).await;
        // Always reaped, success or failure: a stdio MCP server is a child
        // process, and leaving one behind per call would leak a process tree.
        session.close().await;

        let reply = reply.map_err(StdioCallError::into_message).map_err(McpToolCallError::Failed)?;
        Ok(McpToolCallOutcome { is_error: reply.is_error, result: reply.result })
    }

    async fn call_http(
        &self,
        url: &str,
        headers: &HashMap<String, String>,
        tool: &str,
        arguments: serde_json::Value,
    ) -> Result<McpToolCallOutcome, McpToolCallError> {
        let client = self.http_client();
        let (resolved_url, mut req_headers, oauth_managed) =
            self.request_headers(url, headers).await.map_err(call_failure)?;
        req_headers.insert(
            reqwest::header::CONTENT_TYPE,
            "application/json".parse().expect("valid header"),
        );
        req_headers.insert(
            reqwest::header::ACCEPT,
            "application/json, text/event-stream".parse().expect("valid header"),
        );

        let init_resp = self
            .http_post_mcp_with_auth(&client, &resolved_url, &mut req_headers, oauth_managed, &build_initialize_request(1))
            .await
            .map_err(call_failure)?;
        if let Some(error) = init_resp.rpc.error {
            return Err(McpToolCallError::Failed(format!(
                "initialize rejected: {} (code {})",
                error.message, error.code
            )));
        }
        if let Some(session_id) = init_resp.session_id
            && let Ok(value) = reqwest::header::HeaderValue::from_str(&session_id)
        {
            req_headers.insert("mcp-session-id", value);
        }

        // Fire-and-forget, exactly as the probe does.
        let _ = client
            .post(&resolved_url)
            .headers(req_headers.clone())
            .json(&build_initialized_notification())
            .send()
            .await;

        let call_resp = self
            .http_post_mcp_with_auth(
                &client,
                &resolved_url,
                &mut req_headers,
                oauth_managed,
                &build_tools_call_request(2, tool, &arguments),
            )
            .await
            .map_err(call_failure)?;
        if let Some(error) = call_resp.rpc.error {
            return Err(McpToolCallError::Failed(format!(
                "tools/call rejected: {} (code {})",
                error.message, error.code
            )));
        }

        let reply = tool_call_reply(call_resp.rpc.result.unwrap_or(serde_json::Value::Null));
        Ok(McpToolCallOutcome {
            is_error: reply.is_error,
            result: reply.result,
        })
    }

    // -- Stdio transport --------------------------------------------------

    async fn test_stdio(
        &self,
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> McpConnectionTestResult {
        self.test_stdio_inner(command, args, env).await
    }

    async fn test_stdio_inner(
        &self,
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> McpConnectionTestResult {
        let program = resolve_stdio_command(command);
        // `17` §6 / `21` D5=C: an imported env value is a `secret:NAME`
        // reference, resolved here in memory against the host's credentials. A
        // reference with no credential is dropped (fail-closed) and named in the
        // log; the literal `secret:NAME` string is never handed to the child.
        let resolved = nomifun_common::secret_ref::resolve_env(env);
        if !resolved.missing.is_empty() {
            warn!(
                command = %command,
                missing = ?resolved.missing,
                "MCP stdio env has unresolved credential references; omitting them"
            );
        }
        let mut cmd = CmdBuilder::new(&program);
        cmd.args(args)
            .envs(resolved.env.iter())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return spawn_error_result(command, &e),
        };

        let stdin = child.stdin.take().expect("stdin was piped");
        let stdout = child.stdout.take().expect("stdout was piped");
        let result = match tokio::time::timeout(self.timeout, run_stdio_protocol(stdin, stdout)).await {
            Ok(r) => r,
            Err(_) => timeout_result(self.timeout),
        };
        if let Err(error) = kill_process_tree(&mut child).await {
            warn!(%error, "failed to clean up MCP stdio connection test process tree");
        }
        result
    }

    // -- HTTP (Streamable HTTP) transport ---------------------------------

    async fn test_http(&self, url: &str, headers: &HashMap<String, String>) -> McpConnectionTestResult {
        match tokio::time::timeout(self.timeout, self.test_http_inner(url, headers)).await {
            Ok(r) => r,
            Err(_) => timeout_result(self.timeout),
        }
    }

    async fn test_http_inner(&self, url: &str, headers: &HashMap<String, String>) -> McpConnectionTestResult {
        let client = self.http_client();
        let (resolved_url, mut req_headers, oauth_managed) = match self.request_headers(url, headers).await {
            Ok(value) => value,
            Err(result) => return result,
        };
        req_headers.insert(
            reqwest::header::CONTENT_TYPE,
            "application/json".parse().expect("valid header"),
        );
        req_headers.insert(
            reqwest::header::ACCEPT,
            "application/json, text/event-stream".parse().expect("valid header"),
        );

        // 1. initialize
        let init_resp = match self
            .http_post_mcp_with_auth(
                &client,
                &resolved_url,
                &mut req_headers,
                oauth_managed,
                &build_initialize_request(1),
            )
            .await
        {
            Ok(r) => r,
            Err(result) => return result,
        };
        if let Some(err) = init_resp.rpc.error {
            return rpc_error_result("initialize", &err);
        }

        // Extract session ID for subsequent requests
        if let Some(sid) = init_resp.session_id
            && let Ok(val) = reqwest::header::HeaderValue::from_str(&sid)
        {
            req_headers.insert("mcp-session-id", val);
        }

        // 2. initialized notification (fire-and-forget)
        let _ = client
            .post(&resolved_url)
            .headers(req_headers.clone())
            .json(&build_initialized_notification())
            .send()
            .await;

        // 3. tools/list
        let tools_resp = match self
            .http_post_mcp_with_auth(
                &client,
                &resolved_url,
                &mut req_headers,
                oauth_managed,
                &build_tools_list_request(2),
            )
            .await
        {
            Ok(r) => r,
            Err(result) => return result,
        };
        if let Some(err) = tools_resp.rpc.error {
            return rpc_error_result("tools/list", &err);
        }

        success_result(tools_resp.rpc.result)
    }

    /// POST a JSON-RPC message and parse the response.
    ///
    /// Returns `Err(McpConnectionTestResult)` for HTTP-level failures
    /// (connection error, 401, non-success status).
    async fn http_post_mcp(
        &self,
        client: &reqwest::Client,
        url: &str,
        headers: &reqwest::header::HeaderMap,
        body: &JsonRpcRequest,
    ) -> Result<HttpMcpResponse, McpConnectionTestResult> {
        let resp = client
            .post(url)
            .headers(headers.clone())
            .json(body)
            .send()
            .await
            .map_err(|e| {
                error_result(
                    McpConnectionTestErrorCode::ConnectionFailed,
                    format!("Connection failed: {e}"),
                    Some(serde_json::json!({ "transport": "http" })),
                )
            })?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(protocol::auth_result(resp.headers()));
        }
        if !resp.status().is_success() {
            return Err(error_result(
                McpConnectionTestErrorCode::HttpError,
                format!("HTTP {} from server", resp.status()),
                Some(serde_json::json!({ "status": resp.status().as_u16() })),
            ));
        }

        let session_id = resp
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .map(String::from);

        let rpc = protocol::parse_http_response(resp).await.map_err(|error| {
            error_result(
                McpConnectionTestErrorCode::ProtocolError,
                error,
                Some(serde_json::json!({ "transport": "http" })),
            )
        })?;
        Ok(HttpMcpResponse { rpc, session_id })
    }

    /// Resolve the credential references carried by a remote transport, then
    /// build the request headers (an explicit `Authorization` header wins over
    /// the stored OAuth token, as before).
    ///
    /// Returns the **resolved URL** alongside the headers: a marketplace
    /// `mcp.json` may put the credential in the query string, and the reference
    /// must never be sent as literal text.
    async fn request_headers(
        &self,
        url: &str,
        headers: &HashMap<String, String>,
    ) -> Result<(String, reqwest::header::HeaderMap, bool), McpConnectionTestResult> {
        let (resolved_url, resolved_headers) = resolve_remote_auth(url, headers)?;
        let mut request_headers = build_http_headers(&resolved_headers);
        if request_headers.contains_key(reqwest::header::AUTHORIZATION) {
            return Ok((resolved_url, request_headers, false));
        }
        let Some(oauth_service) = self.oauth_service.as_ref() else {
            return Ok((resolved_url, request_headers, false));
        };
        // The OAuth token is keyed by the URL the connector was registered with,
        // so look it up with the unresolved one — only the request itself goes to
        // the resolved URL.
        let token = match oauth_service.get_token(url).await {
            Ok(token) => token,
            Err(McpError::ReauthorizationRequired) => {
                return Err(protocol::reauthorization_result());
            }
            Err(error) => {
                return Err(protocol::oauth_error_result(error.to_string()));
            }
        };
        let Some(token) = token else {
            return Ok((resolved_url, request_headers, false));
        };
        request_headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                .expect("OAuth access token must be a valid header value"),
        );
        Ok((resolved_url, request_headers, true))
    }

    async fn http_post_mcp_with_auth(
        &self,
        client: &reqwest::Client,
        url: &str,
        headers: &mut reqwest::header::HeaderMap,
        oauth_managed: bool,
        body: &JsonRpcRequest,
    ) -> Result<HttpMcpResponse, McpConnectionTestResult> {
        let mut refreshed = false;
        loop {
            match self.http_post_mcp(client, url, headers, body).await {
                Err(result)
                    if result.needs_auth == Some(true) && oauth_managed && !refreshed =>
                {
                    let Some(oauth_service) = self.oauth_service.as_ref() else {
                        return Err(result);
                    };
                    let token = match oauth_service.refresh_access_token(url).await {
                        Ok(token) => token,
                        Err(McpError::ReauthorizationRequired) => {
                            return Err(protocol::reauthorization_result());
                        }
                        Err(error) => {
                            return Err(protocol::oauth_error_result(error.to_string()));
                        }
                    };
                    headers.insert(
                        reqwest::header::AUTHORIZATION,
                        reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                            .expect("OAuth access token must be a valid header value"),
                    );
                    refreshed = true;
                }
                result => return result,
            }
        }
    }

    // -- SSE transport ----------------------------------------------------

    async fn test_sse(&self, url: &str, headers: &HashMap<String, String>) -> McpConnectionTestResult {
        match tokio::time::timeout(self.timeout, self.test_sse_inner(url, headers)).await {
            Ok(r) => r,
            Err(_) => timeout_result(self.timeout),
        }
    }

    async fn test_sse_inner(&self, url: &str, headers: &HashMap<String, String>) -> McpConnectionTestResult {
        let client = self.http_client();
        let (resolved_url, mut req_headers, oauth_managed) = match self.request_headers(url, headers).await {
            Ok(value) => value,
            Err(result) => return result,
        };

        // 1. Open SSE connection
        let mut refreshed = false;
        let resp = loop {
            let response = match client
                .get(&resolved_url)
                .headers(req_headers.clone())
                .header(reqwest::header::ACCEPT, "text/event-stream")
                .send()
                .await
            {
                Ok(response) => response,
                Err(error) => {
                    return error_result(
                        McpConnectionTestErrorCode::ConnectionFailed,
                        format!("Connection failed: {error}"),
                        Some(serde_json::json!({ "transport": "sse" })),
                    );
                }
            };
            if response.status() == reqwest::StatusCode::UNAUTHORIZED && oauth_managed && !refreshed {
                let Some(oauth_service) = self.oauth_service.as_ref() else {
                    break response;
                };
                let token = match oauth_service.refresh_access_token(url).await {
                    Ok(token) => token,
                    Err(McpError::ReauthorizationRequired) => {
                        return protocol::reauthorization_result();
                    }
                    Err(error) => return protocol::oauth_error_result(error.to_string()),
                };
                req_headers.insert(
                    reqwest::header::AUTHORIZATION,
                    reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                        .expect("OAuth access token must be a valid header value"),
                );
                refreshed = true;
                continue;
            }
            break response;
        };
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return protocol::auth_result(resp.headers());
        }
        if !resp.status().is_success() {
            return error_result(
                McpConnectionTestErrorCode::HttpError,
                format!("HTTP {} from server", resp.status()),
                Some(serde_json::json!({ "status": resp.status().as_u16() })),
            );
        }

        // 2. Start SSE reader task
        let (event_tx, mut event_rx) = mpsc::channel::<SseEvent>(16);
        let reader_handle = tokio::spawn(read_sse_events(resp, event_tx));

        req_headers.insert(
            reqwest::header::CONTENT_TYPE,
            "application/json".parse().expect("valid header"),
        );

        let result = self
            .run_sse_protocol(&client, url, &mut req_headers, oauth_managed, &mut event_rx)
            .await;
        reader_handle.abort();
        result
    }

    async fn run_sse_protocol(
        &self,
        client: &reqwest::Client,
        base_url: &str,
        headers: &mut reqwest::header::HeaderMap,
        oauth_managed: bool,
        event_rx: &mut mpsc::Receiver<SseEvent>,
    ) -> McpConnectionTestResult {
        // 3. Wait for endpoint event
        let endpoint = match wait_for_endpoint(event_rx, base_url).await {
            Ok(ep) => ep,
            Err(e) => {
                return error_result(
                    McpConnectionTestErrorCode::ProtocolError,
                    e,
                    Some(serde_json::json!({ "transport": "sse", "stage": "endpoint" })),
                );
            }
        };

        // 4. initialize
        if let Err(result) = self
            .sse_post_with_auth(
                client,
                base_url,
                &endpoint,
                headers,
                oauth_managed,
                &build_initialize_request(1),
                "initialize_send",
            )
            .await
        {
            return result;
        }
        let init_resp = match wait_for_jsonrpc_response(event_rx).await {
            Ok(r) => r,
            Err(e) => {
                return error_result(
                    McpConnectionTestErrorCode::ProtocolError,
                    format!("initialize response: {e}"),
                    Some(serde_json::json!({ "transport": "sse", "stage": "initialize_response" })),
                );
            }
        };
        if let Some(err) = init_resp.error {
            return rpc_error_result("initialize", &err);
        }

        // 5. initialized notification
        let _ = self
            .sse_post_with_auth(
                client,
                base_url,
                &endpoint,
                headers,
                oauth_managed,
                &build_initialized_notification(),
                "initialized_send",
            )
            .await;

        // 6. tools/list
        if let Err(result) = self
            .sse_post_with_auth(
                client,
                base_url,
                &endpoint,
                headers,
                oauth_managed,
                &build_tools_list_request(2),
                "tools_list_send",
            )
            .await
        {
            return result;
        }
        let tools_resp = match wait_for_jsonrpc_response(event_rx).await {
            Ok(r) => r,
            Err(e) => {
                return error_result(
                    McpConnectionTestErrorCode::ProtocolError,
                    format!("tools/list response: {e}"),
                    Some(serde_json::json!({ "transport": "sse", "stage": "tools_list_response" })),
                );
            }
        };
        if let Some(err) = tools_resp.error {
            return rpc_error_result("tools/list", &err);
        }

        success_result(tools_resp.result)
    }

    /// POST a JSON-RPC message to an SSE endpoint (fire-and-forget semantics).
    async fn sse_post<T: Serialize>(
        &self,
        client: &reqwest::Client,
        endpoint: &str,
        headers: &reqwest::header::HeaderMap,
        body: &T,
    ) -> Result<SsePostResponse, String> {
        let response = client
            .post(endpoint)
            .headers(headers.clone())
            .json(body)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        Ok(SsePostResponse {
            status: response.status(),
            www_authenticate: response
                .headers()
                .get(reqwest::header::WWW_AUTHENTICATE)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
        })
    }

    async fn sse_post_with_auth<T: Serialize>(
        &self,
        client: &reqwest::Client,
        oauth_url: &str,
        endpoint: &str,
        headers: &mut reqwest::header::HeaderMap,
        oauth_managed: bool,
        body: &T,
        stage: &str,
    ) -> Result<(), McpConnectionTestResult> {
        let mut refreshed = false;
        loop {
            let response = self
                .sse_post(client, endpoint, headers, body)
                .await
                .map_err(|error| {
                    error_result(
                        McpConnectionTestErrorCode::ConnectionFailed,
                        format!("Failed to send {stage}: {error}"),
                        Some(serde_json::json!({ "transport": "sse", "stage": stage })),
                    )
                })?;
            if response.status == reqwest::StatusCode::UNAUTHORIZED && oauth_managed && !refreshed {
                let Some(oauth_service) = self.oauth_service.as_ref() else {
                    return Err(protocol::auth_result_from_www_authenticate(
                        response.www_authenticate,
                    ));
                };
                let token = match oauth_service.refresh_access_token(oauth_url).await {
                    Ok(token) => token,
                    Err(McpError::ReauthorizationRequired) => {
                        return Err(protocol::reauthorization_result());
                    }
                    Err(error) => return Err(protocol::oauth_error_result(error.to_string())),
                };
                headers.insert(
                    reqwest::header::AUTHORIZATION,
                    reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                        .expect("OAuth access token must be a valid header value"),
                );
                refreshed = true;
                continue;
            }
            if response.status == reqwest::StatusCode::UNAUTHORIZED {
                return Err(protocol::auth_result_from_www_authenticate(
                    response.www_authenticate,
                ));
            }
            if !response.status.is_success() {
                return Err(error_result(
                    McpConnectionTestErrorCode::HttpError,
                    format!("HTTP {} from server", response.status),
                    Some(serde_json::json!({ "transport": "sse", "stage": stage })),
                ));
            }
            return Ok(());
        }
    }
}

/// Collapse a probe-shaped failure into a call failure.
///
/// The transport layer reports failures as an `McpConnectionTestResult`; a call
/// only needs the reason. Reusing that shape (rather than re-deriving one) is
/// what keeps the two paths' error reporting from drifting apart.
fn call_failure(result: McpConnectionTestResult) -> McpToolCallError {
    McpToolCallError::Failed(
        result
            .error
            .unwrap_or_else(|| "MCP transport failure with no message".to_owned()),
    )
}

/// Resolve the credential references a remote transport carries, in both forms.
///
/// The persisted row holds **references**, never values: the importer writes
/// `secret:NAME` for a whole-value slot and `${secret:NAME}` inside a longer
/// string, which is exactly how a marketplace `mcp.json` templates a header
/// (`Authorization: Bearer ${...}`) or puts a token in the URL query. Both the
/// probe and the tool-call path resolve here, so neither can send the literal
/// reference — that shape fails at the server with no local signal.
///
/// Fail-closed: if **any** reference is unresolvable nothing is sent, and the
/// error names the missing credential names. A partially substituted
/// Authorization header is worse than no request at all, because it looks
/// configured.
fn resolve_remote_auth(
    url: &str,
    headers: &HashMap<String, String>,
) -> Result<(String, HashMap<String, String>), McpConnectionTestResult> {
    resolve_remote_auth_with(url, headers, &nomifun_common::secret_ref::credentials())
}

/// [`resolve_remote_auth`] against an explicit credential map (pure; for tests).
fn resolve_remote_auth_with(
    url: &str,
    headers: &HashMap<String, String>,
    credentials: &HashMap<String, String>,
) -> Result<(String, HashMap<String, String>), McpConnectionTestResult> {
    let mut missing: Vec<String> = Vec::new();
    let mut resolved_headers = HashMap::with_capacity(headers.len());
    for (key, value) in headers {
        let resolution = nomifun_common::secret_ref::resolve_template_with(value, credentials);
        match resolution.value {
            Some(value) => {
                resolved_headers.insert(key.clone(), value);
            }
            None => missing.extend(resolution.missing),
        }
    }
    let resolved_url = nomifun_common::secret_ref::resolve_template_with(url, credentials);
    missing.extend(resolved_url.missing.iter().cloned());

    if !missing.is_empty() {
        // Key names only — never the value, and never the URL.
        warn!(
            missing = ?missing,
            "MCP remote transport has unresolved credential references; not sending the request"
        );
        return Err(protocol::missing_credential_result(&missing));
    }

    Ok((
        resolved_url.value.unwrap_or_else(|| url.to_owned()),
        resolved_headers,
    ))
}

pub(super) fn resolve_stdio_command(command: &str) -> OsString {
    if !command.is_empty()
        && !command.contains('/')
        && !command.contains('\\')
        && let Some(path) = resolve_command_path(command)
    {
        return path.into_os_string();
    }

    OsString::from(command)
}

/// Intermediate struct for HTTP transport response parsing.
struct HttpMcpResponse {
    rpc: JsonRpcResponse,
    session_id: Option<String>,
}

struct SsePostResponse {
    status: reqwest::StatusCode,
    www_authenticate: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_clone() {
        let svc = McpConnectionTestService::new(reqwest::Client::new());
        let _cloned = svc.clone();
    }

    #[test]
    fn service_with_timeout() {
        let svc = McpConnectionTestService::new(reqwest::Client::new()).with_timeout(Duration::from_secs(5));
        assert_eq!(svc.timeout, Duration::from_secs(5));
    }

    // ---- Credential references in a remote transport ---------------------

    #[test]
    fn remote_auth_resolution_reports_every_missing_name_once() {
        let credentials = HashMap::from([("PRESENT".to_owned(), "v".to_owned())]);
        let headers = HashMap::from([
            ("Authorization".to_owned(), "Bearer ${secret:MISSING_B}".to_owned()),
            ("x-api-key".to_owned(), "${secret:MISSING_A}".to_owned()),
            ("x-ok".to_owned(), "secret:PRESENT".to_owned()),
        ]);
        let url = "https://h/mcp?token=${secret:MISSING_B}";

        let error = resolve_remote_auth_with(url, &headers, &credentials)
            .expect_err("an unresolvable reference must stop the request");

        assert_eq!(
            error.code,
            Some(McpConnectionTestErrorCode::MissingCredential),
        );
        let message = error.error.unwrap_or_default();
        // Sorted, de-duplicated, names only: the same name appears in two places
        // and is reported once.
        assert!(message.contains("MISSING_A, MISSING_B"), "{message}");
        assert!(!message.contains("PRESENT"), "a resolved name is not missing: {message}");
        assert!(!message.contains("https://"), "the URL carries the credential: {message}");
    }

    #[test]
    fn remote_auth_resolution_leaves_a_plain_transport_alone() {
        let headers = HashMap::from([("x-static".to_owned(), "WorkBuddy".to_owned())]);
        let (url, resolved) = resolve_remote_auth_with(
            "https://h/mcp",
            &headers,
            &HashMap::new(),
        )
        .expect("no references, nothing to resolve");
        assert_eq!(url, "https://h/mcp");
        assert_eq!(resolved.get("x-static").map(String::as_str), Some("WorkBuddy"));
    }

    // ---- Tool calling ----------------------------------------------------

    #[test]
    fn tools_call_request_names_the_tool_and_passes_arguments_through() {
        let request = super::protocol::build_tools_call_request(
            2,
            "create_issue",
            &serde_json::json!({ "title": "t", "nested": { "n": 1 } }),
        );
        assert_eq!(request.method, "tools/call");
        assert_eq!(request.id, "2");
        let params = request.params.expect("tools/call always carries params");
        assert_eq!(params["name"], "create_issue");
        // Arguments are opaque: whatever the tool's schema says, this layer
        // forwards it untouched.
        assert_eq!(params["arguments"]["title"], "t");
        assert_eq!(params["arguments"]["nested"]["n"], 1);
    }

    #[test]
    fn tool_call_reply_lifts_is_error_and_keeps_the_rest_verbatim() {
        let reply = super::protocol::tool_call_reply(
            serde_json::json!({ "content": [], "isError": true, "structuredContent": { "k": 1 } }),
        );
        assert!(reply.is_error);
        assert_eq!(reply.result["structuredContent"]["k"], 1, "the result is not reshaped");

        // Omitting `isError` means "not an error" (the MCP default).
        assert!(!super::protocol::tool_call_reply(serde_json::json!({ "content": [] })).is_error);
        // A server that returns nothing must not panic the lift.
        assert!(!super::protocol::tool_call_reply(serde_json::Value::Null).is_error);
    }

    /// Behaviour the fake MCP server should exhibit for `tools/call`.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum FakeMcp {
        Ok,
        ToolError,
        RpcError,
        Slow,
    }

    /// Spawn a fake Streamable-HTTP MCP server and return its endpoint.
    async fn spawn_fake_mcp(behaviour: FakeMcp) -> String {
        use axum::response::IntoResponse;

        async fn handler(
            axum::extract::State(behaviour): axum::extract::State<FakeMcp>,
            axum::Json(body): axum::Json<serde_json::Value>,
        ) -> axum::response::Response {
            let id = body.get("id").cloned().unwrap_or(serde_json::Value::Null);
            let method = body.get("method").and_then(|m| m.as_str()).unwrap_or_default();
            match method {
                "initialize" => axum::Json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "protocolVersion": "2025-11-25",
                        "capabilities": {},
                        "serverInfo": { "name": "fake-mcp", "version": "1" }
                    }
                }))
                .into_response(),
                "notifications/initialized" => axum::http::StatusCode::ACCEPTED.into_response(),
                "tools/call" => match behaviour {
                    FakeMcp::Ok => axum::Json(serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [{ "type": "text", "text": "pong" }],
                            "isError": false,
                            "structuredContent": { "echo": 42 }
                        }
                    }))
                    .into_response(),
                    FakeMcp::ToolError => axum::Json(serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [{ "type": "text", "text": "boom" }],
                            "isError": true
                        }
                    }))
                    .into_response(),
                    FakeMcp::RpcError => axum::Json(serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32602, "message": "bad arguments" }
                    }))
                    .into_response(),
                    FakeMcp::Slow => {
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        axum::Json(serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": {} }))
                            .into_response()
                    }
                },
                other => axum::Json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": -32601, "message": format!("unexpected method {other}") }
                }))
                .into_response(),
            }
        }

        let app = axum::Router::new()
            .route("/mcp", axum::routing::post(handler))
            .with_state(behaviour);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind loopback");
        let address = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        format!("http://{address}/mcp")
    }

    /// `no_proxy()` on purpose: this host runs a system HTTP proxy, and a
    /// loopback test must never be routed through it.
    fn test_service() -> McpConnectionTestService {
        let client = reqwest::Client::builder().no_proxy().build().expect("test client");
        McpConnectionTestService::new(client)
    }

    fn http_transport(url: &str) -> McpServerTransport {
        McpServerTransport::Http { url: url.to_owned(), headers: HashMap::new() }
    }

    #[tokio::test]
    async fn http_tool_call_returns_the_upstream_result_verbatim() {
        let url = spawn_fake_mcp(FakeMcp::Ok).await;
        let outcome = test_service()
            .call_tool(&http_transport(&url), "echo", serde_json::json!({ "n": 1 }))
            .await
            .expect("the call succeeds");

        assert!(!outcome.is_error);
        assert_eq!(outcome.result["content"][0]["text"], "pong");
        // Not just `content`: a field this layer knows nothing about survives,
        // which is the difference between proxying and reinterpreting.
        assert_eq!(outcome.result["structuredContent"]["echo"], 42);
    }

    #[tokio::test]
    async fn http_tool_level_failure_is_a_result_not_a_transport_error() {
        let url = spawn_fake_mcp(FakeMcp::ToolError).await;
        let outcome = test_service()
            .call_tool(&http_transport(&url), "echo", serde_json::json!({}))
            .await
            .expect("a tool-level failure is still a completed call");

        assert!(outcome.is_error, "`isError` must be lifted, never swallowed");
        assert_eq!(outcome.result["content"][0]["text"], "boom");
    }

    #[tokio::test]
    async fn http_jsonrpc_error_becomes_a_failed_call() {
        let url = spawn_fake_mcp(FakeMcp::RpcError).await;
        let error = test_service()
            .call_tool(&http_transport(&url), "echo", serde_json::json!({}))
            .await
            .expect_err("a JSON-RPC error is a call failure");

        match error {
            McpToolCallError::Failed(message) => {
                assert!(message.contains("tools/call rejected"), "got {message}");
                assert!(message.contains("bad arguments"), "got {message}");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn http_tool_call_hits_its_budget() {
        let url = spawn_fake_mcp(FakeMcp::Slow).await;
        let service = test_service().with_timeout(Duration::from_millis(200));
        let error = service
            .call_tool(&http_transport(&url), "echo", serde_json::json!({}))
            .await
            .expect_err("a hung server must hit the budget");

        assert!(matches!(error, McpToolCallError::Timeout(_)), "got {error:?}");
    }

    #[tokio::test]
    async fn sse_calls_reach_the_transport_rather_than_being_refused() {
        // SSE is supported for calls (see the fixture tests in
        // `tests/connection_test_integration.rs`). This only pins that the
        // dispatch does not refuse it up front: an unreachable endpoint must
        // fail as a *connection* failure, which is a different thing from a
        // transport that this layer declines to speak.
        let error = test_service()
            .call_tool(
                &McpServerTransport::Sse {
                    url: "http://127.0.0.1:1/sse".to_owned(),
                    headers: HashMap::new(),
                },
                "echo",
                serde_json::json!({}),
            )
            .await
            .expect_err("an unreachable SSE endpoint cannot succeed");

        match error {
            McpToolCallError::Failed(message) => assert!(
                !message.contains("not supported"),
                "SSE must not be refused by name any more: {message}"
            ),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stdio_timeout_cleans_up_process_group() {
        let marker_path = std::env::temp_dir().join(format!(
            "nomifun-mcp-timeout-pid-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let transport = McpServerTransport::Stdio {
            command: "sh".into(),
            args: vec![
                "-c".into(),
                "printf '%s\n' \"$$\" > \"$1\"; sleep 30".into(),
                "mcp-timeout-child".into(),
                marker_path.to_string_lossy().into_owned(),
            ],
            env: HashMap::new(),
        };
        let svc = McpConnectionTestService::new(reqwest::Client::new()).with_timeout(Duration::from_millis(100));

        let result = svc.test_connection("timeout-cleanup", &transport).await;
        assert!(!result.success);
        assert!(
            result.error.as_deref().unwrap_or_default().contains("timed out"),
            "expected timeout result, got {result:?}"
        );

        let pid: i32 = std::fs::read_to_string(&marker_path)
            .expect("stdio child should write its pid")
            .trim()
            .parse()
            .expect("pid marker should be numeric");

        let group_alive = wait_for_process_group_exit(pid, Duration::from_secs(1)).await;
        if group_alive {
            let _ = kill_process_group(pid, libc_sigkill());
        }
        let _ = std::fs::remove_file(marker_path);

        assert!(
            !group_alive,
            "stdio timeout should terminate the spawned process group for pid={pid}"
        );
    }

    #[cfg(unix)]
    async fn wait_for_process_group_exit(pid: i32, timeout: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        while tokio::time::Instant::now() < deadline {
            if !is_process_group_alive(pid) {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        is_process_group_alive(pid)
    }

    #[cfg(unix)]
    fn is_process_group_alive(pid: i32) -> bool {
        kill_process_group(pid, 0)
    }

    #[cfg(unix)]
    fn kill_process_group(pid: i32, signal: i32) -> bool {
        unsafe extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
        }
        unsafe { kill(-pid, signal) == 0 }
    }

    #[cfg(unix)]
    fn libc_sigkill() -> i32 {
        9
    }
}
