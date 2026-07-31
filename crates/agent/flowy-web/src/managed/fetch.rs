#![allow(dead_code)] // Wired into the managed extract coordinator in Phase 7.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use nomi_mcp::{
    protocol::{McpToolDef, McpToolResult},
    remote_peer::McpPeerError,
};
use reqwest::StatusCode;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tokio::time::{Instant, timeout_at};

use super::remote::{
    ParallelMcpClient, is_explicit_unknown_tool, is_explicit_unknown_tool_rpc,
    is_unknown_tool_message,
};

const FETCH_TOOL: &str = "web_fetch";

#[derive(Debug, Clone)]
pub struct RemoteExtractRequest {
    pub items: Vec<RemoteExtractRequestItem>,
}

#[derive(Debug, Clone)]
pub struct RemoteExtractRequestItem {
    pub index: usize,
    pub requested_url: String,
}

#[derive(Debug, Clone)]
pub struct RemoteExtractItem {
    pub index: usize,
    pub requested_url: String,
    pub final_url: Option<String>,
    pub title: Option<String>,
    pub markdown: String,
    pub source_truncated: bool,
}

#[derive(Debug)]
pub struct RemoteExtractBatch {
    pub items: Vec<RemoteExtractItem>,
    pub diagnostics: RemoteFetchDiagnostics,
}

#[derive(Debug, Clone, Default)]
pub struct RemoteFetchDiagnostics {
    pub dropped_item_count: usize,
    pub unmatched_item_count: usize,
    pub used_text_fallback: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteExtractError {
    Timeout,
    Network,
    Unauthorized,
    Forbidden,
    RateLimited(Option<Duration>),
    ToolMissing,
    SchemaMismatch,
    RpcMethodUnavailable,
    SessionExpired,
    InvalidRequest,
    MalformedResponse,
    Upstream,
}

#[async_trait]
pub trait RemoteExtractFallback: Send + Sync {
    async fn extract_batch(
        &self,
        request: RemoteExtractRequest,
        deadline: Instant,
    ) -> Result<RemoteExtractBatch, RemoteExtractError>;

    fn is_remote_warm(&self) -> bool {
        false
    }

    fn mark_remote_success(&self) {}
}

pub struct ParallelFetchAdapter {
    client: Arc<ParallelMcpClient>,
    discovery: Mutex<Option<Result<(), FetchCompatibilityError>>>,
}

#[derive(Debug, Clone, Copy)]
enum FetchCompatibilityError {
    ToolMissing,
    SchemaMismatch,
}

impl ParallelFetchAdapter {
    pub(crate) fn new(client: Arc<ParallelMcpClient>) -> Self {
        Self {
            client,
            discovery: Mutex::new(None),
        }
    }

    async fn ensure_compatible(&self, deadline: Instant) -> Result<(), RemoteExtractError> {
        let mut cache = self.discovery.lock().await;
        if let Some(result) = *cache {
            return result.map_err(map_compatibility_error);
        }
        let tools = self
            .client
            .peer()
            .discover_tools(deadline)
            .await
            .map_err(map_peer_error)?;
        let result = tools
            .iter()
            .find(|tool| tool.name == FETCH_TOOL)
            .ok_or(FetchCompatibilityError::ToolMissing)
            .and_then(validate_fetch_schema);
        *cache = Some(result);
        result.map_err(map_compatibility_error)
    }

    async fn clear_compatibility(&self) {
        *self.discovery.lock().await = None;
        self.client.invalidate_tools_cache().await;
    }
}

#[async_trait]
impl RemoteExtractFallback for ParallelFetchAdapter {
    fn is_remote_warm(&self) -> bool {
        self.client.is_remote_warm()
    }

    fn mark_remote_success(&self) {
        self.client.mark_remote_success();
    }

    async fn extract_batch(
        &self,
        request: RemoteExtractRequest,
        deadline: Instant,
    ) -> Result<RemoteExtractBatch, RemoteExtractError> {
        let mut session_retried = false;
        let mut tool_rediscovered = false;
        loop {
            match self.ensure_compatible(deadline).await {
                Ok(()) => {}
                Err(RemoteExtractError::SessionExpired) if !session_retried => {
                    self.clear_compatibility().await;
                    session_retried = true;
                    continue;
                }
                Err(error) => return Err(error),
            }

            let urls = request
                .items
                .iter()
                .map(|item| item.requested_url.clone())
                .collect::<Vec<_>>();
            let arguments = json!({
                "urls": urls,
                "full_content": false,
            });
            let permit = match timeout_at(deadline, self.client.fetch_semaphore().acquire()).await
            {
                Ok(Ok(permit)) => permit,
                Ok(Err(_)) => return Err(RemoteExtractError::Upstream),
                Err(_) => return Err(RemoteExtractError::Timeout),
            };
            let result = self
                .client
                .call_tool(FETCH_TOOL, arguments, deadline)
                .await;
            drop(permit);

            match result {
                Ok(result) => {
                    if result.is_error && is_explicit_unknown_tool(&result) {
                        if !tool_rediscovered {
                            self.clear_compatibility().await;
                            tool_rediscovered = true;
                            continue;
                        }
                        return Err(RemoteExtractError::ToolMissing);
                    }
                    if result.is_error {
                        return Err(RemoteExtractError::Upstream);
                    }
                    return decode_fetch(&result, &request);
                }
                Err(McpPeerError::SessionExpired) if !session_retried => {
                    self.clear_compatibility().await;
                    session_retried = true;
                }
                Err(error) => {
                    if !tool_rediscovered && is_explicit_unknown_tool_rpc(&error) {
                        self.clear_compatibility().await;
                        tool_rediscovered = true;
                        continue;
                    }
                    return Err(map_peer_error(error));
                }
            }
        }
    }
}

fn validate_fetch_schema(
    tool: &McpToolDef,
) -> Result<(), FetchCompatibilityError> {
    let properties = tool
        .input_schema
        .get("properties")
        .and_then(Value::as_object)
        .ok_or(FetchCompatibilityError::SchemaMismatch)?;
    if properties
        .get("urls")
        .and_then(|property| property.get("type"))
        .and_then(Value::as_str)
        != Some("array")
    {
        return Err(FetchCompatibilityError::SchemaMismatch);
    }
    let required = tool
        .input_schema
        .get("required")
        .and_then(Value::as_array)
        .ok_or(FetchCompatibilityError::SchemaMismatch)?;
    if !required.iter().any(|field| field.as_str() == Some("urls")) {
        return Err(FetchCompatibilityError::SchemaMismatch);
    }
    if let Some(output_schema) = tool.output_schema.as_ref() {
        let properties = output_schema
            .get("properties")
            .and_then(Value::as_object)
            .ok_or(FetchCompatibilityError::SchemaMismatch)?;
        if !properties.contains_key("results") || !properties.contains_key("errors") {
            return Err(FetchCompatibilityError::SchemaMismatch);
        }
    }
    Ok(())
}

fn decode_fetch(
    result: &McpToolResult,
    request: &RemoteExtractRequest,
) -> Result<RemoteExtractBatch, RemoteExtractError> {
    if let Some(structured) = result.structured_content.as_ref()
        && let Ok(payload) = decode_payload(structured)
    {
        return build_batch(payload, request, false);
    }
    if let Some(text) = text_json_copy(result)
        && let Ok(payload) = decode_payload(&text)
    {
        return build_batch(payload, request, true);
    }
    Err(RemoteExtractError::MalformedResponse)
}

fn text_json_copy(result: &McpToolResult) -> Option<Value> {
    result.content.iter().find_map(|content| {
        let nomi_mcp::protocol::McpContent::Text { text } = content else {
            return None;
        };
        serde_json::from_str(text).ok()
    })
}

struct DecodedPayload {
    results: Vec<DecodedResult>,
    errors: Vec<DecodedError>,
    malformed_result_count: usize,
}

struct DecodedResult {
    url: String,
    final_url: Option<String>,
    title: Option<String>,
    markdown: String,
}

struct DecodedError {
    url: String,
    http_status: Option<u16>,
}

fn decode_payload(value: &Value) -> Result<DecodedPayload, ()> {
    let results = value
        .get("results")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let errors = value
        .get("errors")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if results.is_empty() && errors.is_empty() {
        return Err(());
    }
    let mut decoded_results = Vec::new();
    let mut malformed_result_count = 0usize;
    for item in results {
        if let Some(decoded) = decode_result_item(item) {
            decoded_results.push(decoded);
        } else {
            malformed_result_count += 1;
        }
    }
    let decoded_errors = errors.iter().filter_map(decode_error_item).collect();
    Ok(DecodedPayload {
        results: decoded_results,
        errors: decoded_errors,
        malformed_result_count,
    })
}

fn decode_result_item(item: &Value) -> Option<DecodedResult> {
    let url = item.get("url")?.as_str()?.to_owned();
    let title = item
        .get("title")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let final_url = item
        .get("final_url")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let full_content = item
        .get("full_content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let excerpts = item
        .get("excerpts")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let markdown = if !excerpts.is_empty() {
        excerpts.join("\n\n")
    } else if !full_content.is_empty() {
        full_content.to_owned()
    } else {
        return None;
    };
    if markdown.trim().is_empty() {
        return None;
    }
    Some(DecodedResult {
        url,
        final_url,
        title,
        markdown,
    })
}

fn decode_error_item(item: &Value) -> Option<DecodedError> {
    Some(DecodedError {
        url: item.get("url")?.as_str()?.to_owned(),
        http_status: item
            .get("http_status_code")
            .and_then(Value::as_u64)
            .and_then(|status| u16::try_from(status).ok()),
    })
}

fn build_batch(
    payload: DecodedPayload,
    request: &RemoteExtractRequest,
    used_text_fallback: bool,
) -> Result<RemoteExtractBatch, RemoteExtractError> {
    let mut assignments: Vec<Option<usize>> = vec![None; request.items.len()];
    let mut assignment_counts = vec![0usize; payload.results.len()];
    let failed_urls = payload
        .errors
        .iter()
        .map(|error| canonical_url(&error.url))
        .collect::<HashSet<_>>();

    for (request_index, request_item) in request.items.iter().enumerate() {
        let matching = payload
            .results
            .iter()
            .enumerate()
            .filter(|(_, result)| canonical_url(&result.url) == canonical_url(&request_item.requested_url))
            .map(|(result_index, _)| result_index)
            .collect::<Vec<_>>();
        let selected = matching
            .iter()
            .copied()
            .find(|result_index| assignment_counts[*result_index] == 0)
            .or_else(|| matching.first().copied());
        if let Some(result_index) = selected {
            assignments[request_index] = Some(result_index);
            assignment_counts[result_index] += 1;
        }
    }

    let unmatched_requests = request
        .items
        .iter()
        .enumerate()
        .filter(|(index, item)| {
            assignments[*index].is_none()
                && !failed_urls.contains(&canonical_url(&item.requested_url))
        })
        .collect::<Vec<_>>();
    let unused_results = payload
        .results
        .iter()
        .enumerate()
        .filter(|(index, _)| assignment_counts[*index] == 0)
        .collect::<Vec<_>>();
    if unmatched_requests.len() == unused_results.len() {
        for ((request_index, _), (result_index, _)) in
            unmatched_requests.iter().zip(unused_results.iter())
        {
            assignments[*request_index] = Some(*result_index);
            assignment_counts[*result_index] += 1;
        }
    }

    let mut items = Vec::new();
    let mut unmatched_item_count = 0usize;
    for (request_index, request_item) in request.items.iter().enumerate() {
        let Some(result_index) = assignments[request_index] else {
            unmatched_item_count += 1;
            continue;
        };
        let result = &payload.results[result_index];
        items.push(RemoteExtractItem {
            index: request_item.index,
            requested_url: request_item.requested_url.clone(),
            final_url: result.final_url.clone(),
            title: result.title.clone(),
            markdown: result.markdown.clone(),
            source_truncated: false,
        });
    }

    let dropped_item_count = payload
        .results
        .iter()
        .enumerate()
        .filter(|(result_index, _)| assignment_counts[*result_index] == 0)
        .count()
        + payload.malformed_result_count;
    Ok(RemoteExtractBatch {
        items,
        diagnostics: RemoteFetchDiagnostics {
            dropped_item_count,
            unmatched_item_count,
            used_text_fallback,
        },
    })
}

fn canonical_url(value: &str) -> String {
    let value = value.trim();
    let without_fragment = value.split('#').next().unwrap_or(value);
    without_fragment.trim_end_matches('/').to_owned()
}

fn map_compatibility_error(error: FetchCompatibilityError) -> RemoteExtractError {
    match error {
        FetchCompatibilityError::ToolMissing => RemoteExtractError::ToolMissing,
        FetchCompatibilityError::SchemaMismatch => RemoteExtractError::SchemaMismatch,
    }
}

fn map_peer_error(error: McpPeerError) -> RemoteExtractError {
    match error {
        McpPeerError::Timeout => RemoteExtractError::Timeout,
        McpPeerError::Network(_) => RemoteExtractError::Network,
        McpPeerError::Http {
            status: StatusCode::UNAUTHORIZED,
            ..
        } => RemoteExtractError::Unauthorized,
        McpPeerError::Http {
            status: StatusCode::FORBIDDEN,
            ..
        } => RemoteExtractError::Forbidden,
        McpPeerError::Http {
            status: StatusCode::TOO_MANY_REQUESTS,
            retry_after,
        } => RemoteExtractError::RateLimited(retry_after),
        McpPeerError::Http { status, .. } if status.is_server_error() => {
            RemoteExtractError::Upstream
        }
        McpPeerError::JsonRpc {
            code: -32602,
            message,
            data,
        } => {
            if is_unknown_tool_message(&message, data.as_ref()) {
                RemoteExtractError::ToolMissing
            } else {
                RemoteExtractError::InvalidRequest
            }
        }
        McpPeerError::JsonRpc { code: -32601, .. } => {
            RemoteExtractError::RpcMethodUnavailable
        }
        McpPeerError::Http { .. } | McpPeerError::JsonRpc { .. } => RemoteExtractError::Upstream,
        McpPeerError::BodyTooLarge
        | McpPeerError::Protocol(_)
        | McpPeerError::UnsupportedServerRequest { .. }
        | McpPeerError::ResponseIdMismatch { .. }
        | McpPeerError::DuplicateResponseId => RemoteExtractError::MalformedResponse,
        McpPeerError::UnsupportedProtocolVersion { .. } => RemoteExtractError::SchemaMismatch,
        McpPeerError::SessionExpired => RemoteExtractError::SessionExpired,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomi_mcp::protocol::{McpContent, McpToolResult};
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_partial_json, method},
    };

    fn request(urls: &[&str]) -> RemoteExtractRequest {
        RemoteExtractRequest {
            items: urls
                .iter()
                .enumerate()
                .map(|(index, url)| RemoteExtractRequestItem {
                    index,
                    requested_url: (*url).to_owned(),
                })
                .collect(),
        }
    }

    fn result(structured: Value, text: Option<&str>) -> McpToolResult {
        McpToolResult {
            content: text
                .map(|text| {
                    vec![McpContent::Text {
                        text: text.to_owned(),
                    }]
                })
                .unwrap_or_default(),
            structured_content: Some(structured),
            is_error: false,
        }
    }

    #[test]
    fn decodes_structured_success() {
        let remote = result(
            json!({
                "results": [{
                    "url": "https://example.com/a",
                    "title": "A",
                    "excerpts": ["# Hello"]
                }],
                "errors": []
            }),
            None,
        );
        let batch = decode_fetch(&remote, &request(&["https://example.com/a"])).unwrap();
        assert_eq!(batch.items.len(), 1);
        assert_eq!(batch.items[0].markdown, "# Hello");
        assert!(!batch.diagnostics.used_text_fallback);
    }

    #[test]
    fn falls_back_to_text_json_when_structured_is_malformed() {
        let remote = result(
            json!({"unexpected": true}),
            Some(
                r#"{"results":[{"url":"https://example.com/a","excerpts":["text fallback"]}],"errors":[]}"#,
            ),
        );
        let batch = decode_fetch(&remote, &request(&["https://example.com/a"])).unwrap();
        assert_eq!(batch.items[0].markdown, "text fallback");
        assert!(batch.diagnostics.used_text_fallback);
    }

    #[test]
    fn rejects_malformed_structured_and_text() {
        let remote = result(json!({"unexpected": true}), Some("not json"));
        assert!(matches!(
            decode_fetch(&remote, &request(&["https://example.com/a"])),
            Err(RemoteExtractError::MalformedResponse)
        ));
    }

    #[test]
    fn maps_out_of_order_results_by_requested_url() {
        let remote = result(
            json!({
                "results": [
                    {"url": "https://example.com/b", "excerpts": ["B"]},
                    {"url": "https://example.com/a", "excerpts": ["A"]}
                ],
                "errors": []
            }),
            None,
        );
        let batch = decode_fetch(
            &remote,
            &request(&["https://example.com/a", "https://example.com/b"]),
        )
        .unwrap();
        assert_eq!(batch.items[0].requested_url, "https://example.com/a");
        assert_eq!(batch.items[0].markdown, "A");
        assert_eq!(batch.items[1].markdown, "B");
    }

    #[test]
    fn drops_extra_results_and_counts_missing_requests() {
        let remote = result(
            json!({
                "results": [
                    {"url": "https://example.com/a", "excerpts": ["A"]},
                    {"url": "https://example.com/extra", "excerpts": ["Extra"]}
                ],
                "errors": [
                    {"url": "https://example.com/missing", "http_status_code": 404}
                ]
            }),
            None,
        );
        let batch = decode_fetch(&remote, &request(&["https://example.com/a", "https://example.com/missing"])).unwrap();
        assert_eq!(batch.items.len(), 1);
        assert_eq!(batch.diagnostics.dropped_item_count, 1);
        assert_eq!(batch.diagnostics.unmatched_item_count, 1);
    }

    #[test]
    fn fans_out_duplicate_urls_to_original_indexes() {
        let remote = result(
            json!({
                "results": [{"url": "https://example.com/a", "excerpts": ["A"]}],
                "errors": []
            }),
            None,
        );
        let batch = decode_fetch(
            &remote,
            &request(&["https://example.com/a", "https://example.com/a"]),
        )
        .unwrap();
        assert_eq!(batch.items.len(), 2);
        assert_eq!(batch.items[0].markdown, "A");
        assert_eq!(batch.items[1].markdown, "A");
    }

    #[test]
    fn supports_unicode_and_long_markdown() {
        let long = "中".repeat(5000);
        let remote = result(
            json!({
                "results": [{"url": "https://example.com/zh", "excerpts": [long]}],
                "errors": []
            }),
            None,
        );
        let batch = decode_fetch(&remote, &request(&["https://example.com/zh"])).unwrap();
        assert_eq!(batch.items[0].markdown.chars().count(), 5000);
    }

    #[test]
    fn schema_requires_urls_and_stable_output() {
        let tool = McpToolDef {
            name: FETCH_TOOL.to_owned(),
            description: None,
            input_schema: json!({
                "type": "object",
                "properties": {
                    "urls": {"type": "array"}
                },
                "required": ["urls"]
            }),
            output_schema: Some(json!({
                "type": "object",
                "properties": {
                    "results": {"type": "array"},
                    "errors": {"type": "array"}
                }
            })),
            annotations: None,
        };
        assert!(validate_fetch_schema(&tool).is_ok());

        let mut bad = tool.clone();
        bad.input_schema = json!({"type": "object", "properties": {}});
        assert!(validate_fetch_schema(&bad).is_err());
    }

    #[test]
    fn maps_http_errors_to_fetch_errors() {
        assert_eq!(
            map_peer_error(McpPeerError::Http {
                status: StatusCode::TOO_MANY_REQUESTS,
                retry_after: Some(Duration::from_secs(30)),
            }),
            RemoteExtractError::RateLimited(Some(Duration::from_secs(30)))
        );
        assert_eq!(
            map_peer_error(McpPeerError::JsonRpc {
                code: -32602,
                message: "unknown tool: web_fetch".to_owned(),
                data: None,
            }),
            RemoteExtractError::ToolMissing
        );
    }

    fn tools_list_response() -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "result": {
                "tools": [{
                    "name": "web_fetch",
                    "inputSchema": {
                        "type": "object",
                        "properties": {"urls": {"type": "array"}},
                        "required": ["urls"]
                    },
                    "outputSchema": {
                        "type": "object",
                        "properties": {
                            "results": {"type": "array"},
                            "errors": {"type": "array"}
                        }
                    }
                }]
            }
        })
    }

    async fn mount_peer(server: &MockServer) {
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "initialize"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "serverInfo": {"name": "mock", "version": "1"}
                }
            })))
            .expect(1)
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(
                json!({"method": "notifications/initialized"}),
            ))
            .respond_with(ResponseTemplate::new(202))
            .expect(1)
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "tools/list", "id": 2})))
            .respond_with(ResponseTemplate::new(200).set_body_json(tools_list_response()))
            .expect(1)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn parallel_fetch_adapter_decodes_remote_batch() {
        let server = MockServer::start().await;
        mount_peer(&server).await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "tools/call"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 3,
                "result": {
                    "structuredContent": {
                        "results": [{
                            "url": "https://example.com/a",
                            "title": "A",
                            "excerpts": ["# Hello"]
                        }],
                        "errors": []
                    }
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = Arc::new(
            ParallelMcpClient::new_at_endpoint(server.uri()).expect("offline construction"),
        );
        let adapter = ParallelFetchAdapter::new(client);
        let batch = adapter
            .extract_batch(
                request(&["https://example.com/a"]),
                Instant::now() + Duration::from_secs(5),
            )
            .await
            .expect("remote batch");
        assert_eq!(batch.items.len(), 1);
        assert_eq!(batch.items[0].markdown, "# Hello");
    }

    #[tokio::test]
    async fn parallel_fetch_adapter_rediscovers_unknown_tool() {
        let server = MockServer::start().await;
        mount_peer(&server).await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "tools/list", "id": 4})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 4,
                "result": {"tools": [{
                    "name": "web_fetch",
                    "inputSchema": {
                        "type": "object",
                        "properties": {"urls": {"type": "array"}},
                        "required": ["urls"]
                    },
                    "outputSchema": {
                        "type": "object",
                        "properties": {
                            "results": {"type": "array"},
                            "errors": {"type": "array"}
                        }
                    }
                }]}
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "tools/call", "id": 3})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 3,
                "result": {
                    "content": [{"type": "text", "text": "unknown tool: web_fetch"}],
                    "isError": true
                }
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "tools/call", "id": 5})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 5,
                "result": {
                    "structuredContent": {
                        "results": [{
                            "url": "https://example.com/a",
                            "excerpts": ["recovered"]
                        }],
                        "errors": []
                    }
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = Arc::new(
            ParallelMcpClient::new_at_endpoint(server.uri()).expect("offline construction"),
        );
        let adapter = ParallelFetchAdapter::new(client);
        let batch = adapter
            .extract_batch(
                request(&["https://example.com/a"]),
                Instant::now() + Duration::from_secs(5),
            )
            .await
            .expect("unknown tool recovery");
        assert_eq!(batch.items[0].markdown, "recovered");
    }
}
