use async_trait::async_trait;
use nomi_mcp::{
    protocol::McpToolDef,
    remote_peer::{McpPeerError, RemoteMcpPeer},
};
use reqwest::StatusCode;
use serde_json::{Map, Value, json};
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::types::{SearchHit, SearchQuery, SearchResult, WebError};

use super::{ManagedSearchProvider, SearchAttemptError, SearchProviderId};
use super::decoders::{DecodeError, decode_parallel, decode_you};

pub(super) struct RemoteSearchAdapter {
    id: SearchProviderId,
    peer: RemoteMcpPeer,
    tool_name: &'static str,
    required_properties: &'static [&'static str],
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
            |query| {
                json!({
                    "objective": query.query,
                    "search_queries": [query.query],
                })
            },
        )
    }

    pub(super) fn exa() -> Result<Self, WebError> {
        Self::new(
            SearchProviderId::Exa,
            "https://mcp.exa.ai/mcp?tools=web_search_exa",
            "web_search_exa",
            &["query"],
            |query| json!({ "query": query.query, "numResults": query.count }),
        )
    }

    #[allow(dead_code)] // wired into the production chain in the next commit
    pub(super) fn you() -> Result<Self, WebError> {
        Self::new(
            SearchProviderId::You,
            "https://api.you.com/mcp?profile=free",
            "you-search",
            &["query", "count"],
            |query| json!({ "query": query.query, "count": query.count }),
        )
    }

    fn new(
        id: SearchProviderId,
        endpoint: &'static str,
        tool_name: &'static str,
        required_properties: &'static [&'static str],
        argument_builder: fn(&SearchQuery) -> Value,
    ) -> Result<Self, WebError> {
        Ok(Self {
            id,
            peer: RemoteMcpPeer::new(endpoint)
                .map_err(|_| WebError::Provider("could not initialize managed search".to_owned()))?,
            tool_name,
            required_properties,
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
                validate_tool_schema(tool, self.required_properties)?;
                validate_output_schema(tool)?;
                Ok(())
            });
        *cache = Some(result);
        result.map_err(map_compatibility_error)
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
        self.ensure_compatible(deadline).await?;
        let result = self
            .peer
            .call_tool(self.tool_name, (self.argument_builder)(query), deadline)
            .await
            .map_err(map_peer_error)?;
        if result.is_error {
            return Err(SearchAttemptError::Upstream);
        }

        let hits = match self.id {
            SearchProviderId::Parallel => decode_parallel(&result, query.count as usize)
                .map_err(map_decode_error)?,
            SearchProviderId::You => decode_you(&result, query.count as usize)
                .map_err(map_decode_error)?,
            SearchProviderId::Exa => {
                let text = super::decoders::text_blocks(&result).join("\n");
                let hits = parse_legacy_provider_results(&text, query.count as usize);
                if !text.trim().is_empty() && hits.is_empty() {
                    return Err(SearchAttemptError::MalformedResponse);
                }
                hits
            }
            SearchProviderId::DuckDuckGo => unreachable!("DDG has a dedicated adapter"),
        };
        Ok(SearchResult {
            provider: self.id.as_str().to_owned(),
            hits,
        })
    }

    async fn shutdown(&self, deadline: Instant) {
        let _ = self.peer.shutdown(deadline).await;
    }
}

fn validate_tool_schema(
    tool: &McpToolDef,
    required: &[&str],
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
    for field in required {
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
        McpPeerError::JsonRpc { code: -32601, .. } => SearchAttemptError::ToolMissing,
        McpPeerError::Http { .. } | McpPeerError::JsonRpc { .. } => SearchAttemptError::Upstream,
        McpPeerError::BodyTooLarge
        | McpPeerError::Protocol(_)
        | McpPeerError::SessionExpired
        | McpPeerError::UnsupportedProtocolVersion { .. }
        | McpPeerError::UnsupportedServerRequest { .. }
        | McpPeerError::ResponseIdMismatch { .. }
        | McpPeerError::DuplicateResponseId => SearchAttemptError::MalformedResponse,
    }
}

fn map_decode_error(error: DecodeError) -> SearchAttemptError {
    match error {
        DecodeError::MalformedResponse => SearchAttemptError::MalformedResponse,
    }
}

fn parse_legacy_provider_results(text: &str, limit: usize) -> Vec<SearchHit> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        let mut objects = Vec::new();
        collect_result_objects(&value, &mut objects);
        let hits = objects_to_hits(objects, limit);
        if !hits.is_empty() {
            return hits;
        }
    }
    parse_markdown_results(trimmed, limit)
}

fn collect_result_objects<'a>(value: &'a Value, output: &mut Vec<&'a Map<String, Value>>) {
    match value {
        Value::Object(object) => {
            if object_has_url(object) {
                output.push(object);
            } else {
                for child in object.values() {
                    collect_result_objects(child, output);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_result_objects(item, output);
            }
        }
        _ => {}
    }
}

fn object_has_url(object: &Map<String, Value>) -> bool {
    ["url", "link", "href"]
        .iter()
        .any(|key| object.get(*key).and_then(Value::as_str).is_some())
}

fn objects_to_hits(objects: Vec<&Map<String, Value>>, limit: usize) -> Vec<SearchHit> {
    objects
        .into_iter()
        .filter_map(|object| {
            let url = string_field(object, &["url", "link", "href"])?;
            let title = string_field(object, &["title", "name"]).unwrap_or(url);
            let snippet = text_field(
                object,
                &[
                    "snippet",
                    "summary",
                    "excerpt",
                    "excerpts",
                    "text",
                    "content",
                    "description",
                ],
            )
            .unwrap_or_default();
            Some((title.to_owned(), url.to_owned(), snippet))
        })
        .take(limit)
        .enumerate()
        .map(|(index, (title, url, snippet))| SearchHit {
            title,
            url,
            snippet,
            published_at: None,
            rank: index as u32 + 1,
        })
        .collect()
}

fn string_field<'a>(object: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
}

fn text_field(object: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| match object.get(*key) {
        Some(Value::String(value)) if !value.trim().is_empty() => Some(value.clone()),
        Some(Value::Array(values)) => {
            let joined = values
                .iter()
                .filter_map(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            (!joined.is_empty()).then_some(joined)
        }
        _ => None,
    })
}

fn parse_markdown_results(text: &str, limit: usize) -> Vec<SearchHit> {
    let mut hits: Vec<SearchHit> = Vec::new();
    let mut pending_title: Option<String> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(title) = trimmed
            .strip_prefix("Title:")
            .or_else(|| trimmed.strip_prefix("title:"))
        {
            let title = title.trim();
            if !title.is_empty() {
                pending_title = Some(title.to_owned());
            }
            continue;
        }
        if let Some(snippet) = trimmed
            .strip_prefix("Text:")
            .or_else(|| trimmed.strip_prefix("Snippet:"))
            .or_else(|| trimmed.strip_prefix("Summary:"))
        {
            if let Some(hit) = hits.last_mut()
                && hit.snippet.is_empty()
            {
                hit.snippet = snippet.trim().to_owned();
            }
            continue;
        }
        let Some(url_start) = line.find("http://").or_else(|| line.find("https://")) else {
            continue;
        };
        let tail = &line[url_start..];
        let url_end = tail
            .find(|character: char| {
                character.is_whitespace() || matches!(character, ')' | ']' | '>' | '"' | '\'')
            })
            .unwrap_or(tail.len());
        let url = tail[..url_end].trim_end_matches(['.', ',']).to_owned();
        if url.is_empty() {
            continue;
        }
        let inline_title = line[..url_start]
            .trim()
            .trim_matches(['-', '*', '#', '[', ']', '(', ':'])
            .trim()
            .to_owned();
        hits.push(SearchHit {
            title: pending_title.take().unwrap_or_else(|| {
                if inline_title.is_empty() || inline_title.eq_ignore_ascii_case("url") {
                    url.clone()
                } else {
                    inline_title
                }
            }),
            url,
            snippet: String::new(),
            published_at: None,
            rank: hits.len() as u32 + 1,
        });
        if hits.len() >= limit {
            break;
        }
    }
    hits
}
