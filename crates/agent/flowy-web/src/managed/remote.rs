use async_trait::async_trait;
use nomi_mcp::{
    protocol::McpToolDef,
    remote_peer::{McpPeerError, RemoteMcpPeer},
};
use reqwest::StatusCode;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::types::{SearchQuery, SearchResult, WebError};

use super::{ManagedSearchProvider, SearchAttemptError, SearchProviderId};
use super::decoders::{DecodeError, decode_parallel, decode_you};

pub(super) struct RemoteSearchAdapter {
    id: SearchProviderId,
    peer: RemoteMcpPeer,
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

impl RemoteSearchAdapter {
    pub(super) fn parallel() -> Result<Self, WebError> {
        Self::new(
            SearchProviderId::Parallel,
            "https://search.parallel.ai/mcp",
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
            peer: RemoteMcpPeer::new(endpoint)
                .map_err(|_| WebError::Provider("could not initialize managed search".to_owned()))?,
            tool_name,
            required_properties,
            optional_properties,
            argument_builder,
            discovery: Mutex::new(None),
        })
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
    ) -> Result<SearchResult, SearchAttemptError> {
        if result.is_error {
            return Err(SearchAttemptError::Upstream);
        }
        let hits = match self.id {
            SearchProviderId::Parallel => decode_parallel(result, count).map_err(map_decode_error)?,
            SearchProviderId::You => decode_you(result, count).map_err(map_decode_error)?,
            SearchProviderId::DuckDuckGo => unreachable!("DDG has a dedicated adapter"),
        };
        Ok(SearchResult {
            provider: self.id.as_str().to_owned(),
            hits,
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
                Err(error) => return Err(map_peer_error(error)),
            }
        }
    }

    async fn shutdown(&self, deadline: Instant) {
        let _ = self.peer.shutdown(deadline).await;
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
    let Some(schema) = tool.output_schema.as_ref() else {
        return Ok(());
    };
    if schema.get("type").and_then(Value::as_str) != Some("object") {
        return Ok(());
    }
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return Ok(());
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
        McpPeerError::JsonRpc { code: -32602, .. } => SearchAttemptError::InvalidRequest,
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
        let text = text.to_ascii_lowercase();
        text.contains("unknown tool")
            || text.contains("tool not found")
            || text.contains("tool_missing")
    })
}
