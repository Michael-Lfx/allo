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

use crate::provider::extract_policy::{
    CanonicalRequestedUrl, PreparedRemoteUrl, canonical_requested_url, prepare_remote_url,
};

use super::remote::{
    EndpointFailureKind, FetchToolFailureKind, ManagedMcpCallError, ParallelMcpClient,
    is_explicit_unknown_tool, is_explicit_unknown_tool_rpc, is_unknown_tool_message,
};

#[cfg(test)]
use super::remote::{ManagedMcpCallGate, ManagedMcpTool};

const FETCH_TOOL: &str = "web_fetch";

#[derive(Debug, Clone)]
pub struct RemoteExtractRequest {
    pub items: Vec<RemoteExtractRequestItem>,
}

#[derive(Debug, Clone)]
pub struct RemoteExtractRequestItem {
    pub index: usize,
    pub prepared: PreparedRemoteUrl,
}

impl RemoteExtractRequestItem {
    pub fn new(
        index: usize,
        requested_url: String,
        allow_private: bool,
    ) -> Result<Self, crate::provider::extract_policy::RemoteForbiddenReason> {
        Ok(Self {
            index,
            prepared: prepare_remote_url(&requested_url, allow_private)?,
        })
    }

    pub fn requested_url(&self) -> &str {
        &self.prepared.requested_url
    }

    pub fn canonical_url(&self) -> &CanonicalRequestedUrl {
        &self.prepared.canonical_url
    }
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
    pub source_truncated_count: usize,
    pub used_text_fallback: bool,
    pub queue_ms: Option<u128>,
    pub call_ms: Option<u128>,
    /// Additional `web_fetch` calls caused by session recovery or tool
    /// rediscovery. Discovery/list calls are not included.
    pub recovery_call_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteExtractError {
    Timeout {
        kind: RemoteTimeoutKind,
    },
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
    SourceContractViolation {
        unmatched_items: usize,
        dropped_items: usize,
    },
    Upstream,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteTimeoutKind {
    QueueDeadline,
    CallDeadline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteExtractReadiness {
    ColdTransport,
    WarmTransportToolUnknown,
    Ready {
        generation: u64,
    },
}

#[async_trait]
pub(crate) trait RemoteExtractProvider: Send + Sync {
    async fn extract_batch(
        &self,
        request: RemoteExtractRequest,
        deadline: Instant,
    ) -> Result<RemoteExtractBatch, RemoteExtractError>;

    async fn readiness(&self) -> RemoteExtractReadiness {
        RemoteExtractReadiness::ColdTransport
    }
}

// Transitional crate-private aliases keep the feature-gated evaluation and
// legacy adapter tests source-compatible while the provider seam is migrated.
// They are intentionally not exported from the crate root.
#[cfg(any(test, feature = "fetch-eval"))]
pub(crate) use RemoteExtractProvider as RemoteExtractFallback;
#[cfg(any(test, feature = "fetch-eval"))]
pub(crate) type FetchReadiness = RemoteExtractReadiness;

pub struct ParallelFetchAdapter {
    client: Arc<ParallelMcpClient>,
    discovery: Mutex<Option<FetchCompatibilityCache>>,
}

#[derive(Debug, Clone, Copy)]
enum FetchCompatibilityError {
    ToolMissing,
    SchemaMismatch,
}

#[derive(Debug, Clone, Copy)]
struct FetchCompatibilityCache {
    generation: Option<u64>,
    result: Result<(), FetchCompatibilityError>,
}

impl ParallelFetchAdapter {
    pub(crate) fn new(client: Arc<ParallelMcpClient>) -> Self {
        Self {
            client,
            discovery: Mutex::new(None),
        }
    }

    async fn ensure_compatible(&self, deadline: Instant) -> Result<(), RemoteExtractError> {
        let peer_readiness = self.client.peer().readiness().await;
        {
            let cache = self.discovery.lock().await;
            if let Some(cached) = cache.as_ref()
                && cached.generation == peer_readiness.generation
            {
                return cached.result.map_err(map_compatibility_error);
            }
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
        let peer_readiness = self.client.peer().readiness().await;
        *self.discovery.lock().await = Some(FetchCompatibilityCache {
            generation: peer_readiness.generation,
            result,
        });
        result.map_err(map_compatibility_error)
    }

    async fn clear_compatibility(&self) {
        *self.discovery.lock().await = None;
        self.client.invalidate_tools_cache().await;
    }

    #[cfg(any(test, feature = "fetch-eval"))]
    pub(crate) async fn fetch_readiness(&self) -> RemoteExtractReadiness {
        self.readiness().await
    }

    #[cfg(feature = "fetch-eval")]
    pub(crate) async fn warm_compatibility(&self, deadline: Instant) -> Result<(), RemoteExtractError> {
        self.ensure_compatible(deadline).await
    }
}

#[async_trait]
impl RemoteExtractProvider for ParallelFetchAdapter {
    async fn readiness(&self) -> RemoteExtractReadiness {
        let peer_readiness = self.client.peer().readiness().await;
        if !peer_readiness.initialized {
            return RemoteExtractReadiness::ColdTransport;
        }
        let cache = self.discovery.lock().await;
        match (peer_readiness.generation, cache.as_ref()) {
            (
                Some(generation),
                Some(cached),
            ) if cached.generation == Some(generation) && cached.result.is_ok() => {
                RemoteExtractReadiness::Ready { generation }
            }
            _ => RemoteExtractReadiness::WarmTransportToolUnknown,
        }
    }

    async fn extract_batch(
        &self,
        request: RemoteExtractRequest,
        deadline: Instant,
    ) -> Result<RemoteExtractBatch, RemoteExtractError> {
        let mut session_retried = false;
        let mut tool_rediscovered = false;
        let mut tool_attempt = 0_u8;
        let mut recovery_call_count = 0usize;
        loop {
            match self.ensure_compatible(deadline).await {
                Ok(()) => {}
                Err(RemoteExtractError::SessionExpired) if !session_retried => {
                    self.clear_compatibility().await;
                    session_retried = true;
                    recovery_call_count += 1;
                    continue;
                }
                Err(error) => {
                    if matches!(error, RemoteExtractError::MalformedResponse) {
                        self.client
                            .record_fetch_error(
                                EndpointFailureKind::MalformedResponse,
                                Instant::now(),
                            )
                            .await;
                    } else {
                        self.record_extract_error(&error).await;
                    }
                    return Err(error);
                }
            }

            if !self.client.endpoint_available(Instant::now()).await {
                return Err(RemoteExtractError::Upstream);
            }
            if !self.client.fetch_tool_available(Instant::now()).await {
                return Err(RemoteExtractError::Upstream);
            }
            let urls = request
                .items
                .iter()
                .map(|item| {
                    let prepared = prepare_remote_url(&item.prepared.requested_url, false)
                        .map_err(|_| RemoteExtractError::InvalidRequest)?;
                    if prepared.canonical_url != item.prepared.canonical_url
                        || prepared.outbound_url != item.prepared.outbound_url
                    {
                        return Err(RemoteExtractError::InvalidRequest);
                    }
                    Ok(prepared.outbound_url)
                })
                .collect::<Vec<_>>();
            let mut prepared_urls = Vec::with_capacity(urls.len());
            for url in urls {
                prepared_urls.push(url?);
            }
            let arguments = json!({
                "urls": prepared_urls,
                "full_content": false,
            });
            let attempt_started = Instant::now();
            let permit = match timeout_at(deadline, self.client.fetch_semaphore().acquire()).await
            {
                Ok(Ok(permit)) => permit,
                Ok(Err(_)) => return Err(RemoteExtractError::Upstream),
                Err(_) => {
                    return Err(RemoteExtractError::Timeout {
                        kind: RemoteTimeoutKind::QueueDeadline,
                    });
                }
            };
            let queue_ms = Some(attempt_started.elapsed().as_millis());
            let call_started = Instant::now();
            tool_attempt = tool_attempt.saturating_add(1);
            let result = self
                .client
                .call_tool(FETCH_TOOL, arguments, deadline, tool_attempt)
                .await;
            let call_ms = Some(call_started.elapsed().as_millis());
            drop(permit);

            match result {
                Ok(result) => {
                    if result.is_error && is_explicit_unknown_tool(&result) {
                        if !tool_rediscovered {
                            self.clear_compatibility().await;
                            tool_rediscovered = true;
                            recovery_call_count += 1;
                            continue;
                        }
                        return Err(RemoteExtractError::ToolMissing);
                    }
                    if result.is_error {
                        self.client
                            .record_fetch_error(
                                FetchToolFailureKind::Upstream,
                                Instant::now(),
                            )
                            .await;
                        return Err(RemoteExtractError::Upstream);
                    }
                    match decode_fetch(&result, &request) {
                        Ok(mut batch) => {
                            batch.diagnostics.queue_ms = queue_ms;
                            batch.diagnostics.call_ms = call_ms;
                            batch.diagnostics.recovery_call_count = recovery_call_count;
                            self.client.record_endpoint_success().await;
                            self.client.record_fetch_tool_success().await;
                            return Ok(batch);
                        }
                        Err(error) => {
                            self.client
                                .record_fetch_error(
                                    FetchToolFailureKind::MalformedResponse,
                                    Instant::now(),
                                )
                                .await;
                            return Err(error);
                        }
                    }
                }
                Err(ManagedMcpCallError::Peer(McpPeerError::SessionExpired))
                    if !session_retried =>
                {
                    self.clear_compatibility().await;
                    session_retried = true;
                    recovery_call_count += 1;
                }
                Err(ManagedMcpCallError::Peer(error)) => {
                    if !tool_rediscovered && is_explicit_unknown_tool_rpc(&error) {
                        self.clear_compatibility().await;
                        tool_rediscovered = true;
                        recovery_call_count += 1;
                        continue;
                    }
                    let mapped = map_peer_error(error);
                    self.record_extract_error(&mapped).await;
                    return Err(mapped);
                }
                Err(ManagedMcpCallError::QuotaExhausted) => {
                    return Err(RemoteExtractError::Upstream);
                }
                Err(ManagedMcpCallError::UnsafeArguments) => {
                    return Err(RemoteExtractError::InvalidRequest);
                }
                Err(ManagedMcpCallError::RetryLimitExceeded) => {
                    return Err(RemoteExtractError::Upstream);
                }
                Err(ManagedMcpCallError::LedgerFailure) => {
                    return Err(RemoteExtractError::Upstream);
                }
            }
        }
    }
}

impl ParallelFetchAdapter {
    async fn record_extract_error(&self, error: &RemoteExtractError) {
        match error {
            RemoteExtractError::Unauthorized => {
                self.client
                    .record_fetch_error(EndpointFailureKind::Unauthorized, Instant::now())
                    .await;
            }
            RemoteExtractError::Forbidden => {
                self.client
                    .record_fetch_error(EndpointFailureKind::Forbidden, Instant::now())
                    .await;
            }
            RemoteExtractError::RateLimited(retry_after) => {
                self.client
                    .record_fetch_error(
                        EndpointFailureKind::RateLimited(*retry_after),
                        Instant::now(),
                    )
                    .await;
            }
            RemoteExtractError::Network => {
                self.client
                    .record_fetch_error(EndpointFailureKind::Network, Instant::now())
                    .await;
            }
            RemoteExtractError::MalformedResponse => {
                self.client
                    .record_fetch_error(
                        FetchToolFailureKind::MalformedResponse,
                        Instant::now(),
                    )
                    .await;
            }
            RemoteExtractError::SourceContractViolation { .. } => {
                self.client
                    .record_fetch_error(
                        FetchToolFailureKind::MalformedResponse,
                        Instant::now(),
                    )
                    .await;
            }
            RemoteExtractError::Timeout { .. } => {
                self.client
                    .record_fetch_error(FetchToolFailureKind::Timeout, Instant::now())
                    .await;
            }
            RemoteExtractError::Upstream => {
                self.client
                    .record_fetch_error(FetchToolFailureKind::Upstream, Instant::now())
                    .await;
            }
            RemoteExtractError::ToolMissing
            | RemoteExtractError::SchemaMismatch
            | RemoteExtractError::RpcMethodUnavailable
            | RemoteExtractError::SessionExpired
            | RemoteExtractError::InvalidRequest => {}
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
    malformed_error_count: usize,
}

struct DecodedResult {
    url: String,
    final_url: Option<String>,
    title: Option<String>,
    markdown: String,
    body_source: RemoteBodySource,
    source_truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteBodySource {
    Excerpts,
    FullContent,
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
    let mut decoded_errors = Vec::new();
    let mut malformed_error_count = 0usize;
    for item in errors {
        if let Some(decoded) = decode_error_item(item) {
            decoded_errors.push(decoded);
        } else {
            malformed_error_count += 1;
        }
    }
    Ok(DecodedPayload {
        results: decoded_results,
        errors: decoded_errors,
        malformed_result_count,
        malformed_error_count,
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
    let body_source = if excerpts.is_empty() {
        RemoteBodySource::FullContent
    } else {
        RemoteBodySource::Excerpts
    };
    let explicit_truncated = item
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if markdown.trim().is_empty() {
        return None;
    }
    Some(DecodedResult {
        url,
        final_url,
        title,
        markdown,
        body_source,
        source_truncated: body_source == RemoteBodySource::Excerpts || explicit_truncated,
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
    for (request_index, request_item) in request.items.iter().enumerate() {
        let request_canonical = request_item.canonical_url().clone();
        let matching = payload
            .results
            .iter()
            .enumerate()
            .filter(|(_, result)| {
                canonical_requested_url(&result.url) == request_canonical
            })
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
            requested_url: request_item.prepared.requested_url.clone(),
            final_url: result.final_url.clone(),
            title: result.title.clone(),
            markdown: result.markdown.clone(),
            source_truncated: result.source_truncated,
        });
    }

    let requested_urls = request
        .items
        .iter()
        .map(|item| item.canonical_url().clone())
        .collect::<HashSet<_>>();
    let result_urls = payload
        .results
        .iter()
        .map(|result| canonical_requested_url(&result.url))
        .collect::<HashSet<_>>();
    let dropped_error_count = payload
        .errors
        .iter()
        .filter(|error| {
            let error_url = canonical_requested_url(&error.url);
            !requested_urls.contains(&error_url) || result_urls.contains(&error_url)
        })
        .count();
    let dropped_item_count = payload
        .results
        .iter()
        .enumerate()
        .filter(|(result_index, _)| assignment_counts[*result_index] == 0)
        .count()
        + payload.malformed_result_count
        + payload.malformed_error_count;
    let dropped_item_count = dropped_item_count + dropped_error_count;
    let source_truncated_count = items
        .iter()
        .filter(|item| item.source_truncated)
        .count();
    if unmatched_item_count > 0 || dropped_item_count > 0 {
        return Err(RemoteExtractError::SourceContractViolation {
            unmatched_items: unmatched_item_count,
            dropped_items: dropped_item_count,
        });
    }
    Ok(RemoteExtractBatch {
        items,
        diagnostics: RemoteFetchDiagnostics {
            dropped_item_count,
            unmatched_item_count,
            source_truncated_count,
            used_text_fallback,
            queue_ms: None,
            call_ms: None,
            recovery_call_count: 0,
        },
    })
}

fn map_compatibility_error(error: FetchCompatibilityError) -> RemoteExtractError {
    match error {
        FetchCompatibilityError::ToolMissing => RemoteExtractError::ToolMissing,
        FetchCompatibilityError::SchemaMismatch => RemoteExtractError::SchemaMismatch,
    }
}

fn map_peer_error(error: McpPeerError) -> RemoteExtractError {
    match error {
        McpPeerError::Timeout => RemoteExtractError::Timeout {
            kind: RemoteTimeoutKind::CallDeadline,
        },
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
        Mock, MockServer, Request, ResponseTemplate,
        matchers::{body_partial_json, method},
    };

    fn request(urls: &[&str]) -> RemoteExtractRequest {
        RemoteExtractRequest {
            items: urls
                .iter()
                .enumerate()
                .map(|(index, url)| {
                    RemoteExtractRequestItem::new(index, (*url).to_owned(), false)
                        .expect("test URLs must be remote eligible")
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

    #[derive(Default)]
    struct RecordingGate {
        calls: Mutex<Vec<(ManagedMcpTool, Value, u8)>>,
    }

    #[async_trait]
    impl ManagedMcpCallGate for RecordingGate {
        async fn before_call(
            &self,
            tool: ManagedMcpTool,
            arguments: &Value,
            attempt: u8,
        ) -> Result<(), ManagedMcpCallError> {
            self.calls
                .lock()
                .await
                .push((tool, arguments.clone(), attempt));
            Ok(())
        }

        fn observe(
            &self,
            _tool: ManagedMcpTool,
            _attempt: u8,
            _result: &Result<McpToolResult, McpPeerError>,
        ) {
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
        assert!(matches!(
            decode_fetch(
                &remote,
                &request(&["https://example.com/a", "https://example.com/missing"])
            ),
            Err(RemoteExtractError::SourceContractViolation {
                unmatched_items: 1,
                dropped_items: 1,
            })
        ));
    }

    #[test]
    fn unmapped_extra_result_is_dropped_without_position_fallback() {
        let remote = result(
            json!({
                "results": [
                    {"url": "https://example.com/extra", "excerpts": ["Extra"]}
                ],
                "errors": []
            }),
            None,
        );
        assert!(matches!(
            decode_fetch(&remote, &request(&["https://example.com/a"])),
            Err(RemoteExtractError::SourceContractViolation {
                unmatched_items: 1,
                dropped_items: 1,
            })
        ));
    }

    #[test]
    fn malformed_error_item_is_a_source_contract_violation() {
        let remote = result(
            json!({
                "results": [{"url": "https://example.com/a", "excerpts": ["A"]}],
                "errors": [{"url": 42}]
            }),
            None,
        );
        assert!(matches!(
            decode_fetch(&remote, &request(&["https://example.com/a"])),
            Err(RemoteExtractError::SourceContractViolation {
                unmatched_items: 0,
                dropped_items: 1,
            })
        ));
    }

    #[test]
    fn extra_error_item_is_a_source_contract_violation() {
        let remote = result(
            json!({
                "results": [{"url": "https://example.com/a", "excerpts": ["A"]}],
                "errors": [{"url": "https://example.com/extra", "http_status_code": 404}]
            }),
            None,
        );
        assert!(matches!(
            decode_fetch(&remote, &request(&["https://example.com/a"])),
            Err(RemoteExtractError::SourceContractViolation {
                unmatched_items: 0,
                dropped_items: 1,
            })
        ));
    }

    #[test]
    fn plain_fragment_is_stripped_from_outbound_request_item() {
        let remote_request = request(&["https://example.com/a#section-2"]);
        assert_eq!(
            remote_request.items[0].prepared.outbound_url,
            "https://example.com/a"
        );
        assert_eq!(
            remote_request.items[0].prepared.requested_url,
            "https://example.com/a#section-2"
        );
    }

    #[test]
    fn excerpts_are_source_truncated_but_full_content_is_not() {
        let excerpted = result(
            json!({
                "results": [{"url": "https://example.com/a", "excerpts": ["A"]}],
                "errors": []
            }),
            None,
        );
        let batch = decode_fetch(&excerpted, &request(&["https://example.com/a"])).unwrap();
        assert!(batch.items[0].source_truncated);
        assert_eq!(batch.diagnostics.source_truncated_count, 1);

        let full = result(
            json!({
                "results": [{"url": "https://example.com/a", "full_content": "# Full"}],
                "errors": []
            }),
            None,
        );
        let batch = decode_fetch(&full, &request(&["https://example.com/a"])).unwrap();
        assert!(!batch.items[0].source_truncated);
        assert_eq!(batch.diagnostics.source_truncated_count, 0);
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
        assert!(matches!(
            map_peer_error(McpPeerError::Timeout),
            RemoteExtractError::Timeout {
                kind: RemoteTimeoutKind::CallDeadline
            }
        ));
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
            .up_to_n_times(1)
            .expect(1)
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(
                json!({"method": "notifications/initialized"}),
            ))
            .respond_with(ResponseTemplate::new(202))
            .up_to_n_times(1)
            .expect(1)
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "tools/list", "id": 2})))
            .respond_with(ResponseTemplate::new(200).set_body_json(tools_list_response()))
            .up_to_n_times(1)
            .expect(1)
            .mount(server)
            .await;
    }

    async fn mount_stateful_peer(server: &MockServer) {
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "initialize"})))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Mcp-Session-Id", "session-1")
                    .set_body_json(json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "result": {
                            "protocolVersion": "2025-11-25",
                            "capabilities": {},
                            "serverInfo": {"name": "mock", "version": "1"}
                        }
                    })),
            )
            .up_to_n_times(1)
            .expect(1)
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(
                json!({"method": "notifications/initialized"}),
            ))
            .respond_with(ResponseTemplate::new(202))
            .up_to_n_times(1)
            .expect(1)
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "tools/list", "id": 2})))
            .respond_with(ResponseTemplate::new(200).set_body_json(tools_list_response()))
            .up_to_n_times(1)
            .expect(1)
            .mount(server)
            .await;
    }

    #[cfg(feature = "fetch-eval")]
    async fn mount_repeating_peer(server: &MockServer, stateful: bool) {
        mount_repeating_peer_with_lists(server, stateful, 2).await;
    }

    #[cfg(feature = "fetch-eval")]
    async fn mount_repeating_peer_with_lists(
        server: &MockServer,
        stateful: bool,
        tool_list_count: usize,
    ) {
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "initialize"})))
            .respond_with(move |request: &Request| {
                let id = serde_json::from_slice::<Value>(&request.body)
                    .ok()
                    .and_then(|value| value.get("id").and_then(Value::as_u64))
                    .unwrap_or(0);
                let response = ResponseTemplate::new(200).set_body_json(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "protocolVersion": "2025-11-25",
                        "capabilities": {},
                        "serverInfo": {"name": "mock", "version": "1"}
                    }
                }));
                if stateful {
                    response.insert_header("Mcp-Session-Id", "session-1")
                } else {
                    response
                }
            })
            .expect(if stateful { 2 } else { 1 })
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(
                json!({"method": "notifications/initialized"}),
            ))
            .respond_with(ResponseTemplate::new(202))
            .expect(if stateful { 2 } else { 1 })
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "tools/list"})))
            .respond_with(|request: &Request| {
                let id = serde_json::from_slice::<Value>(&request.body)
                    .ok()
                    .and_then(|value| value.get("id").and_then(Value::as_u64))
                    .unwrap_or(0);
                let mut response = tools_list_response();
                response["id"] = json!(id);
                ResponseTemplate::new(200).set_body_json(response)
            })
            .expect(tool_list_count as u64)
            .mount(server)
            .await;
    }

    fn fetch_success_response(id: u64, url: &str) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "structuredContent": {
                    "results": [{
                        "url": url,
                        "excerpts": ["# Hello"]
                    }],
                    "errors": []
                }
            }
        })
    }

    fn fetch_success_responder(
        url: &'static str,
    ) -> impl Fn(&Request) -> ResponseTemplate {
        move |request: &Request| {
            let id = serde_json::from_slice::<Value>(&request.body)
                .ok()
                .and_then(|value| value.get("id").and_then(Value::as_u64))
                .unwrap_or(0);
            ResponseTemplate::new(200)
                .set_body_json(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "structuredContent": {
                            "results": [{
                                "url": url,
                                "excerpts": ["# Hello"]
                            }],
                            "errors": []
                        }
                    }
                }))
                .set_delay(Duration::from_millis(300))
        }
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
        let adapter = ParallelFetchAdapter::new(client.clone());
        assert_eq!(
            adapter.fetch_readiness().await,
            FetchReadiness::ColdTransport
        );
        let batch = adapter
            .extract_batch(
                request(&["https://example.com/a"]),
                Instant::now() + Duration::from_secs(5),
            )
            .await
            .expect("remote batch");
        assert_eq!(batch.items.len(), 1);
        assert_eq!(batch.items[0].markdown, "# Hello");
        assert!(batch.diagnostics.queue_ms.is_some());
        assert!(batch.diagnostics.call_ms.is_some());
        assert!(matches!(
            adapter.fetch_readiness().await,
            FetchReadiness::Ready { generation: 1 }
        ));
    }

    #[tokio::test]
    async fn real_parallel_fetch_call_crosses_gate_with_exact_arguments() {
        let server = MockServer::start().await;
        mount_peer(&server).await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "tools/call"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(fetch_success_response(
                3,
                "https://example.com/a",
            )))
            .expect(1)
            .mount(&server)
            .await;

        let gate = Arc::new(RecordingGate::default());
        let client = Arc::new(
            ParallelMcpClient::new_for_test_endpoint(
                server.uri(),
                Some(Arc::clone(&gate) as Arc<dyn ManagedMcpCallGate>),
            )
            .expect("offline construction"),
        );
        let adapter = ParallelFetchAdapter::new(client);
        adapter
            .extract_batch(
                request(&["https://example.com/a"]),
                Instant::now() + Duration::from_secs(5),
            )
            .await
            .expect("remote batch");
        let calls = gate.calls.lock().await;
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, ManagedMcpTool::Fetch);
        assert_eq!(calls[0].2, 1);
        assert_eq!(
            calls[0].1.as_object().unwrap().keys().collect::<Vec<_>>(),
            vec!["full_content", "urls"]
        );
        assert_eq!(calls[0].1["full_content"], false);
    }

    #[cfg(feature = "fetch-eval")]
    #[tokio::test]
    async fn real_parallel_fetch_session_recovery_crosses_gate_and_counts_recovery() {
        let server = MockServer::start().await;
        mount_repeating_peer(&server, true).await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "tools/call", "id": 3})))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "tools/call"})))
            .respond_with(fetch_success_responder("https://example.com/a"))
            .expect(1)
            .mount(&server)
            .await;

        let quota_path = std::env::temp_dir().join(format!(
            "allo-fetch-eval-session-gate-{}.json",
            uuid::Uuid::now_v7()
        ));
        let gate = Arc::new(crate::evaluation::runner::FileQuotaGate::new(
            quota_path.clone(),
            60,
            10,
        ));
        let client = Arc::new(
            ParallelMcpClient::new_for_test_endpoint(
                server.uri(),
                Some(Arc::clone(&gate) as Arc<dyn ManagedMcpCallGate>),
            )
            .expect("offline construction"),
        );
        let adapter = ParallelFetchAdapter::new(client);
        let result = adapter
            .extract_batch(
                request(&["https://example.com/a"]),
                Instant::now() + Duration::from_secs(5),
            )
            .await;
        let batch = result.expect("session recovery");

        assert_eq!(batch.items[0].markdown, "# Hello");
        assert_eq!(gate.actual_calls(), 2);
        assert_eq!(gate.fetch_calls(), 2);
        assert_eq!(gate.recovery_calls(), 1);
        let _ = std::fs::remove_file(quota_path);
    }

    #[cfg(feature = "fetch-eval")]
    #[tokio::test]
    async fn real_parallel_fetch_unknown_tool_recovery_crosses_gate() {
        let server = MockServer::start().await;
        mount_repeating_peer(&server, false).await;
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
            .respond_with(fetch_success_responder("https://example.com/a"))
            .expect(1)
            .mount(&server)
            .await;

        let quota_path = std::env::temp_dir().join(format!(
            "allo-fetch-eval-tool-gate-{}.json",
            uuid::Uuid::now_v7()
        ));
        let gate = Arc::new(crate::evaluation::runner::FileQuotaGate::new(
            quota_path.clone(),
            60,
            10,
        ));
        let client = Arc::new(
            ParallelMcpClient::new_for_test_endpoint(
                server.uri(),
                Some(Arc::clone(&gate) as Arc<dyn ManagedMcpCallGate>),
            )
            .expect("offline construction"),
        );
        let adapter = ParallelFetchAdapter::new(client);
        let batch = adapter
            .extract_batch(
                request(&["https://example.com/a"]),
                Instant::now() + Duration::from_secs(5),
            )
            .await
            .expect("unknown tool recovery");

        assert_eq!(batch.items[0].markdown, "# Hello");
        assert_eq!(gate.actual_calls(), 2);
        assert_eq!(gate.fetch_calls(), 2);
        assert_eq!(gate.recovery_calls(), 1);
        let _ = std::fs::remove_file(quota_path);
    }

    #[cfg(feature = "fetch-eval")]
    #[tokio::test]
    async fn real_parallel_fetch_session_and_tool_recovery_use_only_three_calls() {
        let server = MockServer::start().await;
        mount_repeating_peer_with_lists(&server, true, 3).await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "tools/call", "id": 3})))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "tools/call", "id": 6})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 6,
                "result": {
                    "content": [{"type": "text", "text": "unknown tool: web_fetch"}],
                    "isError": true
                }
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "tools/call", "id": 8})))
            .respond_with(fetch_success_responder("https://example.com/a"))
            .expect(1)
            .mount(&server)
            .await;

        let quota_path = std::env::temp_dir().join(format!(
            "allo-fetch-eval-combined-recovery-{}.json",
            uuid::Uuid::now_v7()
        ));
        let gate = Arc::new(crate::evaluation::runner::FileQuotaGate::new(
            quota_path.clone(),
            60,
            10,
        ));
        let client = Arc::new(
            ParallelMcpClient::new_for_test_endpoint(
                server.uri(),
                Some(Arc::clone(&gate) as Arc<dyn ManagedMcpCallGate>),
            )
            .expect("offline construction"),
        );
        let batch = ParallelFetchAdapter::new(client)
            .extract_batch(
                request(&["https://example.com/a"]),
                Instant::now() + Duration::from_secs(5),
            )
            .await
            .expect("combined recovery");

        assert_eq!(batch.items[0].markdown, "# Hello");
        assert_eq!(gate.actual_calls(), 3);
        assert_eq!(gate.fetch_calls(), 3);
        assert_eq!(gate.recovery_calls(), 2);
        assert_eq!(gate.retry_limit_violations(), 0);
        let _ = std::fs::remove_file(quota_path);
    }

    #[cfg(feature = "fetch-eval")]
    #[tokio::test]
    async fn real_parallel_fetch_fourth_call_is_blocked_before_wiremock() {
        let server = MockServer::start().await;
        mount_peer(&server).await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "tools/call"})))
            .respond_with(fetch_success_responder("https://example.com/a"))
            .expect(3)
            .mount(&server)
            .await;

        let quota_path = std::env::temp_dir().join(format!(
            "allo-fetch-eval-fourth-call-{}.json",
            uuid::Uuid::now_v7()
        ));
        let gate = Arc::new(crate::evaluation::runner::FileQuotaGate::new(
            quota_path.clone(),
            60,
            10,
        ));
        let client = ParallelMcpClient::new_for_test_endpoint(
            server.uri(),
            Some(Arc::clone(&gate) as Arc<dyn ManagedMcpCallGate>),
        )
        .expect("offline construction");
        client
            .peer()
            .discover_tools(Instant::now() + Duration::from_secs(5))
            .await
            .expect("tool discovery");
        let arguments = json!({
            "urls": ["https://example.com/a"],
            "full_content": false,
        });
        for attempt in 1..=3 {
            client
                .call_tool(
                    FETCH_TOOL,
                    arguments.clone(),
                    Instant::now() + Duration::from_secs(5),
                    attempt,
                )
                .await
                .expect("allowed call");
        }
        assert!(matches!(
            client
                .call_tool(
                    FETCH_TOOL,
                    arguments,
                    Instant::now() + Duration::from_secs(5),
                    4,
                )
                .await,
            Err(ManagedMcpCallError::RetryLimitExceeded)
        ));
        assert_eq!(gate.actual_calls(), 3);
        assert_eq!(gate.retry_limit_violations(), 1);
        let _ = std::fs::remove_file(quota_path);
    }

    #[cfg(feature = "fetch-eval")]
    #[tokio::test]
    async fn real_parallel_fetch_429_persists_cooldown_and_blocks_later_call() {
        let server = MockServer::start().await;
        mount_peer(&server).await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "tools/call"})))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("Retry-After", "2")
                    .set_body_string("rate limited"),
            )
            .expect(1)
            .mount(&server)
            .await;

        let quota_path = std::env::temp_dir().join(format!(
            "allo-fetch-eval-rate-gate-{}.json",
            uuid::Uuid::now_v7()
        ));
        let gate = Arc::new(crate::evaluation::runner::FileQuotaGate::new(
            quota_path.clone(),
            60,
            10,
        ));
        let client = Arc::new(
            ParallelMcpClient::new_for_test_endpoint(
                server.uri(),
                Some(Arc::clone(&gate) as Arc<dyn ManagedMcpCallGate>),
            )
            .expect("offline construction"),
        );
        let adapter = ParallelFetchAdapter::new(client);
        let first = adapter
            .extract_batch(
                request(&["https://example.com/a"]),
                Instant::now() + Duration::from_secs(5),
            )
            .await;
        assert!(matches!(
            first,
            Err(RemoteExtractError::RateLimited(Some(_)))
        ));
        let second = adapter
            .extract_batch(
                request(&["https://example.com/a"]),
                Instant::now() + Duration::from_secs(5),
            )
            .await;
        assert!(second.is_err());
        assert_eq!(gate.actual_calls(), 1);
        assert_eq!(gate.state().0.as_deref(), Some("rate_limited"));
        assert!(gate.state().1.is_some());
        assert!(gate.state().2.is_some());
        let ledger: Value = serde_json::from_slice(
            &std::fs::read(&quota_path).expect("quota ledger"),
        )
        .expect("valid quota ledger");
        assert!(ledger["cooldown_until_unix"].as_i64().is_some());
        let _ = std::fs::remove_file(quota_path);
    }

    #[tokio::test]
    async fn parallel_fetch_readiness_is_warm_after_peer_discovery() {
        let server = MockServer::start().await;
        mount_peer(&server).await;
        let client = Arc::new(
            ParallelMcpClient::new_at_endpoint(server.uri()).expect("offline construction"),
        );
        client
            .peer()
            .discover_tools(Instant::now() + Duration::from_secs(5))
            .await
            .expect("peer discovery");
        let adapter = ParallelFetchAdapter::new(client.clone());
        assert_eq!(
            adapter.fetch_readiness().await,
            FetchReadiness::WarmTransportToolUnknown
        );
    }

    #[tokio::test]
    async fn parallel_fetch_readiness_returns_cold_after_session_expired() {
        let server = MockServer::start().await;
        mount_stateful_peer(&server).await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "tools/call"})))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;
        let client = Arc::new(
            ParallelMcpClient::new_at_endpoint(server.uri()).expect("offline construction"),
        );
        client
            .peer()
            .discover_tools(Instant::now() + Duration::from_secs(5))
            .await
            .expect("initial peer discovery");
        let result = client
            .peer()
            .call_tool(
                FETCH_TOOL,
                json!({"urls": ["https://example.com/a"]}),
                Instant::now() + Duration::from_secs(5),
            )
            .await;
        assert!(matches!(result, Err(McpPeerError::SessionExpired)));
        let adapter = ParallelFetchAdapter::new(client);
        assert_eq!(
            adapter.fetch_readiness().await,
            FetchReadiness::ColdTransport
        );
    }

    #[tokio::test]
    async fn parallel_fetch_compatibility_revalidates_after_generation_change() {
        let server = MockServer::start().await;
        mount_stateful_peer(&server).await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "tools/call", "id": 3})))
            .respond_with(ResponseTemplate::new(200).set_body_json(fetch_success_response(
                3,
                "https://example.com/a",
            )))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        let client = Arc::new(
            ParallelMcpClient::new_at_endpoint(server.uri()).expect("offline construction"),
        );
        let adapter = ParallelFetchAdapter::new(client.clone());
        adapter
            .extract_batch(
                request(&["https://example.com/a"]),
                Instant::now() + Duration::from_secs(5),
            )
            .await
            .expect("first remote batch");
        assert!(matches!(
            adapter.fetch_readiness().await,
            FetchReadiness::Ready { generation: 1 }
        ));

        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "tools/call", "id": 4})))
            .respond_with(ResponseTemplate::new(404))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        let result = client
            .peer()
            .call_tool(
                FETCH_TOOL,
                json!({"urls": ["https://example.com/a"]}),
                Instant::now() + Duration::from_secs(5),
            )
            .await;
        assert!(
            matches!(result, Err(McpPeerError::SessionExpired)),
            "{result:?}"
        );
        assert_eq!(
            adapter.fetch_readiness().await,
            FetchReadiness::ColdTransport
        );

        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "initialize"})))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Mcp-Session-Id", "session-2")
                    .set_body_json(json!({
                        "jsonrpc": "2.0",
                        "id": 5,
                        "result": {
                            "protocolVersion": "2025-11-25",
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
            .and(body_partial_json(json!({"method": "tools/list", "id": 6})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 6,
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
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "tools/call", "id": 7})))
            .respond_with(ResponseTemplate::new(200).set_body_json(fetch_success_response(
                7,
                "https://example.com/a",
            )))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;

        let batch = adapter
            .extract_batch(
                request(&["https://example.com/a"]),
                Instant::now() + Duration::from_secs(5),
            )
            .await
            .expect("second remote batch");
        assert_eq!(batch.items.len(), 1);
        assert!(matches!(
            adapter.fetch_readiness().await,
            FetchReadiness::Ready { generation: 2 }
        ));
    }

    #[tokio::test]
    async fn cancelling_fetch_while_waiting_for_semaphore_does_not_call_tool() {
        let server = MockServer::start().await;
        mount_peer(&server).await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "tools/call"})))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        let client = Arc::new(
            ParallelMcpClient::new_at_endpoint(server.uri()).expect("offline construction"),
        );
        let _permit = client
            .fetch_semaphore()
            .try_acquire()
            .expect("hold fetch permit");
        let adapter = ParallelFetchAdapter::new(client.clone());
        let task = tokio::spawn(async move {
            adapter
                .extract_batch(
                    request(&["https://example.com/a"]),
                    Instant::now() + Duration::from_secs(5),
                )
                .await
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        task.abort();
        let _ = task.await;
    }

    #[tokio::test]
    async fn cancelling_inflight_fetch_does_not_leave_a_second_request() {
        let server = MockServer::start().await;
        mount_peer(&server).await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "tools/call"})))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(fetch_success_response(3, "https://example.com/a"))
                    .set_delay(Duration::from_secs(10)),
            )
            .expect(1)
            .mount(&server)
            .await;
        let client = Arc::new(
            ParallelMcpClient::new_at_endpoint(server.uri()).expect("offline construction"),
        );
        let adapter = ParallelFetchAdapter::new(client);
        let task = tokio::spawn(async move {
            adapter
                .extract_batch(
                    request(&["https://example.com/a"]),
                    Instant::now() + Duration::from_secs(5),
                )
                .await
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        task.abort();
        let _ = task.await;
    }

    #[tokio::test]
    async fn two_concurrent_fetch_calls_share_one_fetch_permit() {
        let server = MockServer::start().await;
        mount_peer(&server).await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "tools/call"})))
            .respond_with(fetch_success_responder("https://example.com/a"))
            .up_to_n_times(2)
            .expect(2)
            .mount(&server)
            .await;
        let client = Arc::new(
            ParallelMcpClient::new_at_endpoint(server.uri()).expect("offline construction"),
        );
        let left = ParallelFetchAdapter::new(client.clone());
        let right = ParallelFetchAdapter::new(client);
        let left_task = tokio::spawn(async move {
            left.extract_batch(
                request(&["https://example.com/a"]),
                Instant::now() + Duration::from_secs(5),
            )
            .await
        });
        let right_task = tokio::spawn(async move {
            right.extract_batch(
                request(&["https://example.com/a"]),
                Instant::now() + Duration::from_secs(5),
            )
            .await
        });
        let (left, right) = tokio::join!(left_task, right_task);
        let left = left.expect("left task").expect("left batch");
        let right = right.expect("right task").expect("right batch");
        assert_eq!(left.items.len(), 1);
        assert_eq!(right.items.len(), 1);
        assert!(
            left.diagnostics.queue_ms.is_some_and(|ms| ms > 0)
                || right.diagnostics.queue_ms.is_some_and(|ms| ms > 0),
            "one of two concurrent fetch calls must wait for the shared permit"
        );
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
