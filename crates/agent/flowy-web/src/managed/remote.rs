use std::sync::Arc;

use async_trait::async_trait;
use nomi_mcp::{
    protocol::{McpToolDef, McpToolResult},
    remote_peer::{McpPeerError, RemoteMcpPeer},
};
use reqwest::StatusCode;
use serde_json::{Value, json};
use tokio::sync::{Mutex, RwLock, Semaphore};
use tokio::time::Instant;

use crate::types::{SearchQuery, SearchResult, WebError};

use super::{
    ManagedSearchProvider, SearchAttemptError, SearchAttemptOutput, SearchDecodeDiagnostics,
    SearchProviderId,
};
use super::decoders::{DecodeError, decode_parallel, decode_you};

pub(super) struct RemoteSearchAdapter {
    id: SearchProviderId,
    peer: Arc<RemoteMcpPeer>,
    shared_client: Option<Arc<ParallelMcpClient>>,
    tool_name: &'static str,
    required_properties: &'static [&'static str],
    optional_properties: &'static [&'static str],
    argument_builder: fn(&SearchQuery) -> Value,
    discovery: Mutex<Option<Result<(), AdapterCompatibilityError>>>,
}

#[derive(Debug, Clone, Copy)]
enum AdapterCompatibilityError {
    ToolMissing,
    SchemaMismatch,
}

#[derive(Default)]
#[allow(dead_code)] // Consumed by fetch endpoint health in later phases.
struct EndpointHealth {
    consecutive_failures: u32,
    cooldown_until: Option<Instant>,
}

#[allow(dead_code)] // Consumed by fetch tool catalog compatibility in later phases.
struct ToolCatalogSnapshot {
    generation: u64,
    tools: Option<Vec<McpToolDef>>,
}

pub(super) struct ParallelMcpClient {
    peer: Arc<RemoteMcpPeer>,
    #[allow(dead_code)] // Endpoint health is consumed by the fetch adapter phase.
    endpoint_health: Mutex<EndpointHealth>,
    #[allow(dead_code)] // Tool catalog generation is consumed by fetch adapter phase.
    tool_catalog: RwLock<ToolCatalogSnapshot>,
    remote_fetch_semaphore: Semaphore,
}

impl ParallelMcpClient {
    pub(super) fn new() -> Result<Self, WebError> {
        Ok(Self {
            peer: Arc::new(
                RemoteMcpPeer::new("https://search.parallel.ai/mcp").map_err(|_| {
                    WebError::Provider("could not initialize managed Parallel MCP".to_owned())
                })?,
            ),
            endpoint_health: Mutex::new(EndpointHealth::default()),
            tool_catalog: RwLock::new(ToolCatalogSnapshot {
                generation: 0,
                tools: None,
            }),
            // Limits Parallel web_fetch concurrency across conversations.
            remote_fetch_semaphore: Semaphore::new(1),
        })
    }

    pub(super) fn peer(&self) -> Arc<RemoteMcpPeer> {
        Arc::clone(&self.peer)
    }

    #[allow(dead_code)] // Used by the fetch adapter phase and admission tests.
    pub(super) fn fetch_semaphore(&self) -> &Semaphore {
        &self.remote_fetch_semaphore
    }

    pub(super) async fn shutdown(&self, deadline: Instant) {
        let _ = self.peer.shutdown(deadline).await;
    }

    #[allow(dead_code)] // Used by endpoint health and fetch adapter phases.
    pub(super) async fn invalidate_tools_cache(&self) {
        self.peer.invalidate_tools_cache().await;
    }

    #[allow(dead_code)] // Used by endpoint health and fetch adapter phases.
    pub(super) async fn call_tool(
        &self,
        name: &str,
        arguments: Value,
        deadline: Instant,
    ) -> Result<McpToolResult, McpPeerError> {
        self.peer.call_tool(name, arguments, deadline).await
    }
}

impl RemoteSearchAdapter {
    pub(super) fn parallel(client: Arc<ParallelMcpClient>) -> Self {
        Self::new_shared(
            SearchProviderId::Parallel,
            client,
            "web_search",
            &["objective", "search_queries"],
            &[],
            |query| {
                json!({
                    "objective": query.query,
                    "search_queries": [query.query],
                })
            },
        )
    }

    pub(super) fn you() -> Result<Self, WebError> {
        Self::new(
            SearchProviderId::You,
            "https://api.you.com/mcp?profile=free",
            "you-search",
            &["query"],
            &["count"],
            |query| json!({ "query": query.query, "count": query.count }),
        )
    }

    fn new(
        id: SearchProviderId,
        endpoint: &'static str,
        tool_name: &'static str,
        required_properties: &'static [&'static str],
        optional_properties: &'static [&'static str],
        argument_builder: fn(&SearchQuery) -> Value,
    ) -> Result<Self, WebError> {
        Ok(Self {
            id,
            peer: Arc::new(
                RemoteMcpPeer::new(endpoint)
                    .map_err(|_| WebError::Provider("could not initialize managed search".to_owned()))?,
            ),
            shared_client: None,
            tool_name,
            required_properties,
            optional_properties,
            argument_builder,
            discovery: Mutex::new(None),
        })
    }

    fn new_shared(
        id: SearchProviderId,
        client: Arc<ParallelMcpClient>,
        tool_name: &'static str,
        required_properties: &'static [&'static str],
        optional_properties: &'static [&'static str],
        argument_builder: fn(&SearchQuery) -> Value,
    ) -> Self {
        Self {
            id,
            peer: client.peer(),
            shared_client: Some(client),
            tool_name,
            required_properties,
            optional_properties,
            argument_builder,
            discovery: Mutex::new(None),
        }
    }

    async fn ensure_compatible(&self, deadline: Instant) -> Result<(), SearchAttemptError> {
        let mut cache = self.discovery.lock().await;
        if let Some(result) = *cache {
            return result.map_err(map_compatibility_error);
        }
        let tools = self
            .peer
            .discover_tools(deadline)
            .await
            .map_err(map_peer_error)?;

        if self.id == SearchProviderId::You && tools.len() != 1 {
            let result = Err(AdapterCompatibilityError::SchemaMismatch);
            *cache = Some(result);
            return Err(SearchAttemptError::SchemaMismatch);
        }
        let result = tools
            .iter()
            .find(|tool| tool.name == self.tool_name)
            .ok_or(AdapterCompatibilityError::ToolMissing)
            .and_then(|tool| {
                validate_tool_schema(
                    tool,
                    self.required_properties,
                    self.optional_properties,
                )?;
                validate_output_schema(tool)?;
                Ok(())
            });
        *cache = Some(result);
        result.map_err(map_compatibility_error)
    }

    async fn clear_compatibility(&self) {
        *self.discovery.lock().await = None;
        self.peer.invalidate_tools_cache().await;
    }

    fn decode_result(
        &self,
        result: &nomi_mcp::protocol::McpToolResult,
        count: usize,
    ) -> Result<SearchAttemptOutput, SearchAttemptError> {
        if result.is_error {
            return Err(SearchAttemptError::Upstream);
        }
        let outcome = match self.id {
            SearchProviderId::Parallel => decode_parallel(result, count).map_err(map_decode_error)?,
            SearchProviderId::You => decode_you(result, count).map_err(map_decode_error)?,
            SearchProviderId::DuckDuckGo => unreachable!("DDG has a dedicated adapter"),
        };
        Ok(SearchAttemptOutput {
            result: SearchResult {
                provider: self.id.as_str().to_owned(),
                hits: outcome.hits,
            },
            diagnostics: Some(SearchDecodeDiagnostics {
                decode_source: outcome.source.as_str(),
                structured_fallback: outcome.structured_fallback,
                dropped_items: outcome.dropped_items,
                contract_degraded: outcome.contract_degraded,
            }),
        })
    }
}

#[async_trait]
impl ManagedSearchProvider for RemoteSearchAdapter {
    fn id(&self) -> SearchProviderId {
        self.id
    }

    async fn search_attempt(
        &self,
        query: &SearchQuery,
        deadline: Instant,
    ) -> Result<SearchResult, SearchAttemptError> {
        self.search_attempt_with_diagnostics(query, deadline)
            .await
            .map(|output| output.result)
    }

    async fn search_attempt_with_diagnostics(
        &self,
        query: &SearchQuery,
        deadline: Instant,
    ) -> Result<SearchAttemptOutput, SearchAttemptError> {
        let mut session_retried = false;
        let mut tool_rediscovered = false;
        loop {
            match self.ensure_compatible(deadline).await {
                Ok(()) => {}
                Err(SearchAttemptError::SessionExpired) if !session_retried => {
                    self.clear_compatibility().await;
                    session_retried = true;
                    continue;
                }
                Err(error) => return Err(error),
            }

            match self
                .peer
                .call_tool(self.tool_name, (self.argument_builder)(query), deadline)
                .await
            {
                Ok(result) => {
                    if result.is_error && is_explicit_unknown_tool(&result) {
                        if !tool_rediscovered {
                            self.clear_compatibility().await;
                            tool_rediscovered = true;
                            continue;
                        }
                        return Err(SearchAttemptError::ToolMissing);
                    }
                    return self.decode_result(&result, query.count as usize);
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

    async fn shutdown(&self, deadline: Instant) {
        if self.shared_client.is_none() {
            let _ = self.peer.shutdown(deadline).await;
        }
    }
}

fn validate_tool_schema(
    tool: &McpToolDef,
    required: &[&str],
    optional: &[&str],
) -> Result<(), AdapterCompatibilityError> {
    let properties = tool
        .input_schema
        .get("properties")
        .and_then(Value::as_object)
        .ok_or(AdapterCompatibilityError::SchemaMismatch)?;
    if required.iter().any(|field| !properties.contains_key(*field)) {
        return Err(AdapterCompatibilityError::SchemaMismatch);
    }
    let declared_required = tool
        .input_schema
        .get("required")
        .and_then(Value::as_array)
        .ok_or(AdapterCompatibilityError::SchemaMismatch)?;
    if required.iter().any(|field| {
        !declared_required
            .iter()
            .any(|declared| declared.as_str() == Some(*field))
    }) {
        return Err(AdapterCompatibilityError::SchemaMismatch);
    }
    if optional.iter().any(|field| !properties.contains_key(*field)) {
        return Err(AdapterCompatibilityError::SchemaMismatch);
    }
    for field in required.iter().chain(optional.iter()) {
        let expected_type = match *field {
            "search_queries" => "array",
            "count" => "integer",
            _ => "string",
        };
        if properties
            .get(*field)
            .and_then(|property| property.get("type"))
            .and_then(Value::as_str)
            != Some(expected_type)
        {
            return Err(AdapterCompatibilityError::SchemaMismatch);
        }
    }
    Ok(())
}

fn validate_output_schema(tool: &McpToolDef) -> Result<(), AdapterCompatibilityError> {
    // Absent outputSchema is allowed: local typed decoders remain the contract.
    let Some(schema) = tool.output_schema.as_ref() else {
        return Ok(());
    };
    // Once present, the schema must be a usable object with the adapter's
    // expected top-level result collection field.
    if schema.get("type").and_then(Value::as_str) != Some("object") {
        return Err(AdapterCompatibilityError::SchemaMismatch);
    }
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return Err(AdapterCompatibilityError::SchemaMismatch);
    };
    if properties.contains_key("results") {
        Ok(())
    } else {
        Err(AdapterCompatibilityError::SchemaMismatch)
    }
}

fn map_compatibility_error(error: AdapterCompatibilityError) -> SearchAttemptError {
    match error {
        AdapterCompatibilityError::ToolMissing => SearchAttemptError::ToolMissing,
        AdapterCompatibilityError::SchemaMismatch => SearchAttemptError::SchemaMismatch,
    }
}

fn map_peer_error(error: McpPeerError) -> SearchAttemptError {
    match error {
        McpPeerError::Timeout => SearchAttemptError::Timeout,
        McpPeerError::Network(_) => SearchAttemptError::Network,
        McpPeerError::Http {
            status: StatusCode::UNAUTHORIZED,
            ..
        } => SearchAttemptError::Unauthorized,
        McpPeerError::Http {
            status: StatusCode::FORBIDDEN,
            ..
        } => SearchAttemptError::Forbidden,
        McpPeerError::Http {
            status: StatusCode::TOO_MANY_REQUESTS,
            retry_after,
        } => SearchAttemptError::RateLimited(retry_after),
        McpPeerError::Http { status, .. } if status.is_server_error() => {
            SearchAttemptError::Upstream
        }
        McpPeerError::JsonRpc {
            code: -32602,
            message,
            data,
        } => {
            if is_unknown_tool_message(&message, data.as_ref()) {
                SearchAttemptError::ToolMissing
            } else {
                SearchAttemptError::InvalidRequest
            }
        }
        McpPeerError::JsonRpc { code: -32601, .. } => {
            SearchAttemptError::RpcMethodUnavailable
        }
        McpPeerError::Http { .. } | McpPeerError::JsonRpc { .. } => SearchAttemptError::Upstream,
        McpPeerError::BodyTooLarge
        | McpPeerError::Protocol(_)
        | McpPeerError::UnsupportedServerRequest { .. }
        | McpPeerError::ResponseIdMismatch { .. }
        | McpPeerError::DuplicateResponseId => SearchAttemptError::MalformedResponse,
        McpPeerError::UnsupportedProtocolVersion { .. } => SearchAttemptError::SchemaMismatch,
        McpPeerError::SessionExpired => SearchAttemptError::SessionExpired,
    }
}

fn map_decode_error(error: DecodeError) -> SearchAttemptError {
    match error {
        DecodeError::MalformedResponse => SearchAttemptError::MalformedResponse,
    }
}

fn is_explicit_unknown_tool(result: &nomi_mcp::protocol::McpToolResult) -> bool {
    result.content.iter().any(|content| {
        let nomi_mcp::protocol::McpContent::Text { text } = content else {
            return false;
        };
        is_unknown_tool_message(text, None)
    })
}

fn is_explicit_unknown_tool_rpc(error: &McpPeerError) -> bool {
    match error {
        McpPeerError::JsonRpc {
            code: -32602,
            message,
            data,
        } => is_unknown_tool_message(message, data.as_ref()),
        _ => false,
    }
}

fn is_unknown_tool_message(message: &str, data: Option<&Value>) -> bool {
    let message = message.to_ascii_lowercase();
    if message.contains("unknown tool")
        || message.contains("tool not found")
        || message.contains("tool_missing")
    {
        return true;
    }
    data.and_then(Value::as_str)
        .is_some_and(|text| {
            let text = text.to_ascii_lowercase();
            text.contains("unknown tool")
                || text.contains("tool not found")
                || text.contains("tool_missing")
        })
}


#[cfg(test)]
mod tests {
    use super::*;
    use nomi_mcp::protocol::McpToolDef;
    use serde_json::json;

    fn tool_with_output(schema: Option<Value>) -> McpToolDef {
        McpToolDef {
            name: "web_search".to_owned(),
            description: None,
            input_schema: json!({
                "type": "object",
                "properties": {
                    "objective": { "type": "string" },
                    "search_queries": { "type": "array" }
                },
                "required": ["objective", "search_queries"]
            }),
            output_schema: schema,
            annotations: None,
        }
    }

    #[test]
    fn absent_output_schema_is_allowed() {
        assert!(validate_output_schema(&tool_with_output(None)).is_ok());
    }

    #[test]
    fn present_output_schema_must_be_object_with_results() {
        assert!(validate_output_schema(&tool_with_output(Some(json!({
            "type": "array"
        })))).is_err());
        assert!(validate_output_schema(&tool_with_output(Some(json!({
            "type": "object"
        })))).is_err());
        assert!(validate_output_schema(&tool_with_output(Some(json!({
            "type": "object",
            "properties": { "hits": { "type": "array" } }
        })))).is_err());
        assert!(validate_output_schema(&tool_with_output(Some(json!({
            "type": "object",
            "properties": { "results": { "type": "array" } }
        })))).is_ok());
    }

    #[test]
    fn rpc_unknown_tool_is_detected_for_rediscovery() {
        assert!(is_explicit_unknown_tool_rpc(&McpPeerError::JsonRpc {
            code: -32602,
            message: "unknown tool: you-search".to_owned(),
            data: None,
        }));
        assert!(!is_explicit_unknown_tool_rpc(&McpPeerError::JsonRpc {
            code: -32602,
            message: "invalid params".to_owned(),
            data: None,
        }));
        assert!(!is_explicit_unknown_tool_rpc(&McpPeerError::JsonRpc {
            code: -32601,
            message: "unknown tool: you-search".to_owned(),
            data: None,
        }));
    }

    #[test]
    fn parallel_client_construction_is_offline() {
        let client = ParallelMcpClient::new().expect("offline construction");
        let _ = client.peer();
    }

    #[tokio::test]
    async fn parallel_fetch_semaphore_has_one_permit() {
        let client = ParallelMcpClient::new().expect("offline construction");
        let permit = client
            .fetch_semaphore()
            .try_acquire()
            .expect("first permit");
        assert!(
            client.fetch_semaphore().try_acquire().is_err(),
            "fetch concurrency must be one"
        );
        drop(permit);
        assert!(client.fetch_semaphore().try_acquire().is_ok());
    }
}
