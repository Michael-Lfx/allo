//! Desktop-host managed web search.
//!
//! The public surface is one [`SearchProvider`](crate::provider::SearchProvider).
//! Provider routing, MCP discovery, health and concurrency remain private and
//! never enter user MCP configuration or the model tool registry.

use std::{
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use nomi_mcp::{
    protocol::{McpContent, McpToolDef},
    remote_peer::{McpPeerError, RemoteMcpPeer},
};
use reqwest::StatusCode;
use serde_json::{Map, Value, json};
use tokio::sync::{Mutex, Semaphore};
use tokio::time::Instant;
use uuid::Uuid;

use crate::{
    provider::{DuckDuckGoSearchProvider, SearchProvider},
    types::{MAX_SEARCH_COUNT, SearchHit, SearchQuery, SearchResult, WebError},
};

const TOTAL_BUDGET: Duration = Duration::from_secs(10);
const MAX_TITLE_CHARS: usize = 300;
const MAX_URL_BYTES: usize = 2048;
const MAX_SNIPPET_CHARS: usize = 2000;
const MAX_MODEL_CHARS: usize = 12_000;
#[cfg(debug_assertions)]
const DEV_DISABLED_PROVIDERS_ENV: &str = "NOMI_MANAGED_SEARCH_DISABLE_PROVIDERS";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchProviderId {
    Parallel,
    Exa,
    DuckDuckGo,
}

impl SearchProviderId {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Parallel => "parallel",
            Self::Exa => "exa",
            Self::DuckDuckGo => "duckduckgo",
        }
    }
}

#[derive(Debug)]
enum SearchAttemptError {
    Network,
    Timeout,
    RateLimited(Option<Duration>),
    Unauthorized,
    Forbidden,
    ToolMissing,
    SchemaMismatch,
    InvalidRequest,
    MalformedResponse,
    Upstream,
}

#[async_trait]
trait ManagedSearchProvider: Send + Sync {
    fn id(&self) -> SearchProviderId;

    async fn search_attempt(
        &self,
        query: &SearchQuery,
        deadline: Instant,
    ) -> Result<SearchResult, SearchAttemptError>;

    async fn shutdown(&self, _deadline: Instant) {}
}

#[derive(Default)]
struct SearchProviderHealth {
    consecutive_failures: u32,
    cooldown_until: Option<Instant>,
    disabled: bool,
}

impl SearchProviderHealth {
    fn is_available(&self, now: Instant) -> bool {
        !self.disabled && self.cooldown_until.is_none_or(|until| until <= now)
    }

    fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.cooldown_until = None;
    }

    fn record_error(&mut self, error: &SearchAttemptError, now: Instant) {
        match error {
            SearchAttemptError::Unauthorized
            | SearchAttemptError::ToolMissing
            | SearchAttemptError::SchemaMismatch => self.disabled = true,
            SearchAttemptError::Forbidden => {
                self.cooldown_until = Some(now + Duration::from_secs(10 * 60));
            }
            SearchAttemptError::RateLimited(retry_after) => {
                let delay = retry_after
                    .unwrap_or(Duration::from_secs(30))
                    .clamp(Duration::from_secs(30), Duration::from_secs(15 * 60));
                self.cooldown_until = Some(now + delay);
            }
            SearchAttemptError::Network
            | SearchAttemptError::Timeout
            | SearchAttemptError::Upstream
            | SearchAttemptError::MalformedResponse => {
                self.consecutive_failures = self.consecutive_failures.saturating_add(1);
                let exponent = self.consecutive_failures.saturating_sub(1).min(5);
                let seconds = 15u64.saturating_mul(1u64 << exponent).min(5 * 60);
                self.cooldown_until = Some(now + Duration::from_secs(seconds));
            }
            SearchAttemptError::InvalidRequest => {}
        }
    }
}

struct SearchProviderSlot {
    adapter: Arc<dyn ManagedSearchProvider>,
    timeout: Duration,
    health: Mutex<SearchProviderHealth>,
    semaphore: Semaphore,
}

impl SearchProviderSlot {
    fn new(adapter: Arc<dyn ManagedSearchProvider>, timeout: Duration) -> Self {
        Self {
            adapter,
            timeout,
            health: Mutex::new(SearchProviderHealth::default()),
            semaphore: Semaphore::new(2),
        }
    }
}

/// Process-level shared provider injected by the desktop host.
///
/// The service owns only provider clients, discovery state, health and permits.
/// Query and result values remain local to each [`search`](SearchProvider::search)
/// future and are never cached.
pub struct ManagedSearchService {
    slots: Vec<SearchProviderSlot>,
}

impl ManagedSearchService {
    /// Build the keyless chain verified by the explicit compatibility probe.
    /// Construction is offline; provider connections are lazy.
    pub fn keyless_default() -> Result<Self, WebError> {
        let disabled = development_disabled_providers();
        if !disabled.is_empty() {
            let providers = disabled
                .iter()
                .map(|provider| provider.as_str())
                .collect::<Vec<_>>()
                .join(",");
            tracing::warn!(
                target: "managed_search",
                disabled_providers = providers,
                "managed web search development override applied"
            );
        }

        let mut adapters = Vec::new();
        if !disabled.contains(&SearchProviderId::Parallel) {
            adapters.push((
                Arc::new(RemoteSearchAdapter::parallel()?) as Arc<dyn ManagedSearchProvider>,
                Duration::from_secs(3),
            ));
        }
        if !disabled.contains(&SearchProviderId::Exa) {
            adapters.push((
                Arc::new(RemoteSearchAdapter::exa()?) as Arc<dyn ManagedSearchProvider>,
                Duration::from_secs(3),
            ));
        }
        adapters.push((
            Arc::new(DuckDuckGoAdapter::new()) as Arc<dyn ManagedSearchProvider>,
            Duration::from_secs(4),
        ));
        Ok(Self::from_adapters(adapters))
    }

    pub fn ddg_only() -> Self {
        Self::from_adapters(vec![(
            Arc::new(DuckDuckGoAdapter::new()) as Arc<dyn ManagedSearchProvider>,
            Duration::from_secs(4),
        )])
    }

    fn from_adapters(
        adapters: Vec<(Arc<dyn ManagedSearchProvider>, Duration)>,
    ) -> Self {
        Self {
            slots: adapters
                .into_iter()
                .map(|(adapter, timeout)| SearchProviderSlot::new(adapter, timeout))
                .collect(),
        }
    }

    pub async fn shutdown(&self) {
        let deadline = Instant::now() + Duration::from_secs(2);
        for slot in &self.slots {
            slot.adapter.shutdown(deadline).await;
        }
    }

    async fn route(&self, query: &SearchQuery) -> Result<SearchResult, WebError> {
        let request_id = Uuid::now_v7();
        let overall_deadline = Instant::now() + TOTAL_BUDGET;
        let mut fallback_count = 0usize;
        let mut last_class = None;

        for (attempt, slot) in self.slots.iter().enumerate() {
            let now = Instant::now();
            if now >= overall_deadline {
                last_class = Some("timeout");
                break;
            }
            if !slot.health.lock().await.is_available(now) {
                fallback_count += 1;
                continue;
            }
            let provider_deadline = std::cmp::min(overall_deadline, now + slot.timeout);
            let started = Instant::now();
            let permit = match tokio::time::timeout_at(
                provider_deadline,
                slot.semaphore.acquire(),
            )
            .await
            {
                Ok(Ok(permit)) => permit,
                Ok(Err(_)) => {
                    last_class = Some("semaphore_closed");
                    fallback_count += 1;
                    continue;
                }
                Err(_) => {
                    slot.health
                        .lock()
                        .await
                        .record_error(&SearchAttemptError::Timeout, Instant::now());
                    last_class = Some("timeout");
                    fallback_count += 1;
                    continue;
                }
            };
            let result = match tokio::time::timeout_at(
                provider_deadline,
                slot.adapter.search_attempt(query, provider_deadline),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => Err(SearchAttemptError::Timeout),
            };
            drop(permit);

            match result {
                Ok(mut result) if !result.hits.is_empty() => {
                    let truncated = normalize_result(&mut result, query.count);
                    slot.health.lock().await.record_success();
                    tracing::info!(
                        target: "managed_search",
                        request_id = %request_id,
                        provider = slot.adapter.id().as_str(),
                        attempt,
                        elapsed_ms = started.elapsed().as_millis(),
                        result_count = result.hits.len(),
                        fallback_count,
                        truncated,
                        "managed web search succeeded"
                    );
                    return Ok(result);
                }
                Ok(_) => {
                    fallback_count += 1;
                    tracing::debug!(
                        target: "managed_search",
                        request_id = %request_id,
                        provider = slot.adapter.id().as_str(),
                        attempt,
                        elapsed_ms = started.elapsed().as_millis(),
                        "managed web search returned no results"
                    );
                }
                Err(error) => {
                    let class = error_class(&error);
                    last_class = Some(class);
                    slot.health.lock().await.record_error(&error, Instant::now());
                    fallback_count += 1;
                    tracing::warn!(
                        target: "managed_search",
                        request_id = %request_id,
                        provider = slot.adapter.id().as_str(),
                        attempt,
                        elapsed_ms = started.elapsed().as_millis(),
                        error_class = class,
                        fallback_count,
                        "managed web search provider failed"
                    );
                }
            }
        }

        tracing::warn!(
            target: "managed_search",
            request_id = %request_id,
            fallback_count,
            error_class = last_class.unwrap_or("unavailable"),
            "all managed web search providers failed"
        );
        Err(WebError::Provider(
            "web search is temporarily unavailable".to_owned(),
        ))
    }
}

#[cfg(debug_assertions)]
fn development_disabled_providers() -> Vec<SearchProviderId> {
    std::env::var(DEV_DISABLED_PROVIDERS_ENV)
        .ok()
        .map(|value| parse_disabled_providers(&value))
        .unwrap_or_default()
}

#[cfg(not(debug_assertions))]
fn development_disabled_providers() -> Vec<SearchProviderId> {
    Vec::new()
}

#[cfg(debug_assertions)]
fn parse_disabled_providers(value: &str) -> Vec<SearchProviderId> {
    value
        .split(',')
        .filter_map(|name| match name.trim().to_ascii_lowercase().as_str() {
            "parallel" => Some(SearchProviderId::Parallel),
            "exa" => Some(SearchProviderId::Exa),
            _ => None,
        })
        .fold(Vec::new(), |mut providers, provider| {
            if !providers.contains(&provider) {
                providers.push(provider);
            }
            providers
        })
}

#[async_trait]
impl SearchProvider for ManagedSearchService {
    fn name(&self) -> &str {
        "managed"
    }

    async fn search(&self, query: SearchQuery) -> Result<SearchResult, WebError> {
        if query.query.trim().is_empty() {
            return Err(WebError::InvalidArgument(
                "query must not be empty".to_owned(),
            ));
        }
        self.route(&query).await
    }
}

struct DuckDuckGoAdapter {
    provider: DuckDuckGoSearchProvider,
}

impl DuckDuckGoAdapter {
    fn new() -> Self {
        Self {
            provider: DuckDuckGoSearchProvider::new(),
        }
    }
}

#[async_trait]
impl ManagedSearchProvider for DuckDuckGoAdapter {
    fn id(&self) -> SearchProviderId {
        SearchProviderId::DuckDuckGo
    }

    async fn search_attempt(
        &self,
        query: &SearchQuery,
        deadline: Instant,
    ) -> Result<SearchResult, SearchAttemptError> {
        tokio::time::timeout_at(deadline, self.provider.search(query.clone()))
            .await
            .map_err(|_| SearchAttemptError::Timeout)?
            .map_err(map_web_error)
    }
}

struct RemoteSearchAdapter {
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
    fn parallel() -> Result<Self, WebError> {
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

    fn exa() -> Result<Self, WebError> {
        Self::new(
            SearchProviderId::Exa,
            "https://mcp.exa.ai/mcp?tools=web_search_exa",
            "web_search_exa",
            &["query"],
            |query| json!({ "query": query.query, "numResults": query.count }),
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
        let result = tools
            .iter()
            .find(|tool| tool.name == self.tool_name)
            .ok_or(AdapterCompatibilityError::ToolMissing)
            .and_then(|tool| validate_tool_schema(tool, self.required_properties));
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
        let text = result
            .content
            .into_iter()
            .filter_map(|content| match content {
                McpContent::Text { text } => Some(text),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        let hits = parse_provider_results(&text, query.count as usize);
        if !text.trim().is_empty() && hits.is_empty() {
            return Err(SearchAttemptError::MalformedResponse);
        }
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
        let expected_type = if *field == "search_queries" {
            "array"
        } else {
            "string"
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
        McpPeerError::BodyTooLarge | McpPeerError::Protocol(_) => {
            SearchAttemptError::MalformedResponse
        }
    }
}

fn map_web_error(error: WebError) -> SearchAttemptError {
    match error {
        WebError::InvalidArgument(_) => SearchAttemptError::InvalidRequest,
        WebError::Timeout(_) => SearchAttemptError::Timeout,
        WebError::Network(_) => SearchAttemptError::Network,
        WebError::Parse(_) => SearchAttemptError::MalformedResponse,
        WebError::Provider(_) | WebError::BlockedUrl(_) => SearchAttemptError::Upstream,
    }
}

fn error_class(error: &SearchAttemptError) -> &'static str {
    match error {
        SearchAttemptError::Network => "network",
        SearchAttemptError::Timeout => "timeout",
        SearchAttemptError::RateLimited(_) => "rate_limited",
        SearchAttemptError::Unauthorized => "unauthorized",
        SearchAttemptError::Forbidden => "forbidden",
        SearchAttemptError::ToolMissing => "tool_missing",
        SearchAttemptError::SchemaMismatch => "schema_mismatch",
        SearchAttemptError::InvalidRequest => "invalid_request",
        SearchAttemptError::MalformedResponse => "malformed_response",
        SearchAttemptError::Upstream => "upstream",
    }
}

fn parse_provider_results(text: &str, limit: usize) -> Vec<SearchHit> {
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
        Value::String(nested) if nested.trim_start().starts_with(['{', '[']) => {
            // Nested provider JSON cannot borrow into this traversal. It is
            // handled by the caller's markdown fallback if it contains URLs.
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
            let title = string_field(object, &["title", "name"])
                .unwrap_or(url);
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
            rank: hits.len() as u32 + 1,
        });
        if hits.len() >= limit {
            break;
        }
    }
    hits
}

fn normalize_result(result: &mut SearchResult, requested: u32) -> bool {
    let mut truncated = false;
    result.hits.truncate(requested.clamp(1, MAX_SEARCH_COUNT) as usize);
    let mut remaining = MAX_MODEL_CHARS;
    let mut kept = Vec::with_capacity(result.hits.len());
    for mut hit in result.hits.drain(..) {
        truncated |= truncate_chars(&mut hit.title, MAX_TITLE_CHARS);
        truncated |= truncate_bytes(&mut hit.url, MAX_URL_BYTES);
        truncated |= truncate_chars(&mut hit.snippet, MAX_SNIPPET_CHARS);
        if hit.url.is_empty() {
            truncated = true;
            continue;
        }
        let fixed = hit.title.chars().count() + hit.url.chars().count() + 16;
        if fixed >= remaining {
            truncated = true;
            break;
        }
        remaining -= fixed;
        truncated |= truncate_chars(&mut hit.snippet, remaining);
        remaining = remaining.saturating_sub(hit.snippet.chars().count());
        hit.rank = kept.len() as u32 + 1;
        kept.push(hit);
    }
    result.hits = kept;
    truncated
}

fn truncate_chars(value: &mut String, limit: usize) -> bool {
    if value.chars().count() <= limit {
        return false;
    }
    *value = value.chars().take(limit).collect();
    true
}

fn truncate_bytes(value: &mut String, limit: usize) -> bool {
    if value.len() <= limit {
        return false;
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::VecDeque,
        sync::atomic::{AtomicUsize, Ordering},
    };

    struct FakeAdapter {
        id: SearchProviderId,
        result: Mutex<Option<Result<SearchResult, SearchAttemptError>>>,
    }

    #[async_trait]
    impl ManagedSearchProvider for FakeAdapter {
        fn id(&self) -> SearchProviderId {
            self.id
        }

        async fn search_attempt(
            &self,
            _query: &SearchQuery,
            _deadline: Instant,
        ) -> Result<SearchResult, SearchAttemptError> {
            self.result
                .lock()
                .await
                .take()
                .expect("fake called exactly once")
        }
    }

    fn fake(
        id: SearchProviderId,
        result: Result<SearchResult, SearchAttemptError>,
    ) -> Arc<dyn ManagedSearchProvider> {
        Arc::new(FakeAdapter {
            id,
            result: Mutex::new(Some(result)),
        })
    }

    fn successful(provider: &str) -> SearchResult {
        SearchResult {
            provider: provider.to_owned(),
            hits: vec![SearchHit {
                title: "title".to_owned(),
                url: "https://example.com".to_owned(),
                snippet: "snippet".to_owned(),
                rank: 1,
            }],
        }
    }

    #[cfg(debug_assertions)]
    #[test]
    fn development_provider_override_is_explicit_and_deduplicated() {
        assert_eq!(
            parse_disabled_providers(" Parallel,exa,parallel,unknown "),
            vec![SearchProviderId::Parallel, SearchProviderId::Exa]
        );
    }

    #[tokio::test]
    async fn first_non_empty_result_wins() {
        let service = ManagedSearchService::from_adapters(vec![
            (
                fake(SearchProviderId::Parallel, Ok(successful("parallel"))),
                Duration::from_secs(1),
            ),
            (
                fake(SearchProviderId::DuckDuckGo, Ok(successful("duckduckgo"))),
                Duration::from_secs(1),
            ),
        ]);
        let result = service
            .search(SearchQuery {
                query: "test".to_owned(),
                count: 5,
            })
            .await
            .expect("search succeeds");
        assert_eq!(result.provider, "parallel");
    }

    #[tokio::test]
    async fn empty_and_failure_fall_back() {
        let service = ManagedSearchService::from_adapters(vec![
            (
                fake(
                    SearchProviderId::Parallel,
                    Ok(SearchResult {
                        provider: "parallel".to_owned(),
                        hits: Vec::new(),
                    }),
                ),
                Duration::from_secs(1),
            ),
            (
                fake(SearchProviderId::Exa, Err(SearchAttemptError::Timeout)),
                Duration::from_secs(1),
            ),
            (
                fake(SearchProviderId::DuckDuckGo, Ok(successful("duckduckgo"))),
                Duration::from_secs(1),
            ),
        ]);
        let result = service
            .search(SearchQuery {
                query: "test".to_owned(),
                count: 5,
            })
            .await
            .expect("fallback succeeds");
        assert_eq!(result.provider, "duckduckgo");
    }

    #[test]
    fn parses_nested_json_results() {
        let hits = parse_provider_results(
            r#"{"results":[{"title":"One","url":"https://example.com","text":"Body"}]}"#,
            5,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].snippet, "Body");
    }

    #[test]
    fn parses_exa_label_blocks_without_exposing_provider_format() {
        let hits = parse_provider_results(
            "Title: One result\nURL: https://example.com/one\nText: Useful summary\n",
            5,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "One result");
        assert_eq!(hits[0].snippet, "Useful summary");
    }

    #[test]
    fn health_policy_distinguishes_permanent_and_temporary_failures() {
        let now = Instant::now();
        let mut health = SearchProviderHealth::default();
        health.record_error(&SearchAttemptError::Unauthorized, now);
        assert!(health.disabled);

        let mut health = SearchProviderHealth::default();
        health.record_error(&SearchAttemptError::Forbidden, now);
        assert!(!health.disabled);
        assert!(health.cooldown_until > Some(now));
    }

    #[test]
    fn rate_limit_honors_bounded_retry_after_and_network_backoff_grows() {
        let now = Instant::now();
        let mut health = SearchProviderHealth::default();
        health.record_error(
            &SearchAttemptError::RateLimited(Some(Duration::from_secs(1))),
            now,
        );
        assert_eq!(
            health.cooldown_until,
            Some(now + Duration::from_secs(30))
        );

        let mut health = SearchProviderHealth::default();
        health.record_error(&SearchAttemptError::Network, now);
        assert_eq!(
            health.cooldown_until,
            Some(now + Duration::from_secs(15))
        );
        health.record_error(&SearchAttemptError::Network, now);
        assert_eq!(
            health.cooldown_until,
            Some(now + Duration::from_secs(30))
        );
    }

    struct SequentialAdapter {
        id: SearchProviderId,
        calls: AtomicUsize,
        results: Mutex<VecDeque<Result<SearchResult, SearchAttemptError>>>,
    }

    #[async_trait]
    impl ManagedSearchProvider for SequentialAdapter {
        fn id(&self) -> SearchProviderId {
            self.id
        }

        async fn search_attempt(
            &self,
            _query: &SearchQuery,
            _deadline: Instant,
        ) -> Result<SearchResult, SearchAttemptError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.results
                .lock()
                .await
                .pop_front()
                .expect("unexpected provider call")
        }
    }

    #[tokio::test]
    async fn unauthorized_disables_provider_for_process_lifetime() {
        let adapter = Arc::new(SequentialAdapter {
            id: SearchProviderId::Parallel,
            calls: AtomicUsize::new(0),
            results: Mutex::new(VecDeque::from([Err(
                SearchAttemptError::Unauthorized,
            )])),
        });
        let service = ManagedSearchService::from_adapters(vec![(
            adapter.clone(),
            Duration::from_secs(1),
        )]);
        let first_error = service
            .search(SearchQuery {
                query: "test".to_owned(),
                count: 5,
            })
            .await
            .expect_err("all-provider failure is safe");
        assert_eq!(
            first_error.to_string(),
            "provider error: web search is temporarily unavailable"
        );
        assert!(
            service
                .search(SearchQuery {
                    query: "test".to_owned(),
                    count: 5,
                })
                .await
                .is_err()
        );
        assert_eq!(adapter.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn forbidden_provider_recovers_through_the_next_normal_request() {
        let adapter = Arc::new(SequentialAdapter {
            id: SearchProviderId::Parallel,
            calls: AtomicUsize::new(0),
            results: Mutex::new(VecDeque::from([
                Err(SearchAttemptError::Forbidden),
                Ok(successful("parallel")),
            ])),
        });
        let service = ManagedSearchService::from_adapters(vec![(
            adapter.clone(),
            Duration::from_secs(1),
        )]);
        let query = || SearchQuery {
            query: "test".to_owned(),
            count: 5,
        };
        assert!(service.search(query()).await.is_err());
        assert!(service.search(query()).await.is_err());
        assert_eq!(adapter.calls.load(Ordering::SeqCst), 1);

        tokio::time::advance(Duration::from_secs(10 * 60)).await;
        assert!(service.search(query()).await.is_ok());
        assert_eq!(adapter.calls.load(Ordering::SeqCst), 2);
    }

    struct ConcurrencyAdapter {
        active: AtomicUsize,
        max_active: AtomicUsize,
    }

    #[async_trait]
    impl ManagedSearchProvider for ConcurrencyAdapter {
        fn id(&self) -> SearchProviderId {
            SearchProviderId::Parallel
        }

        async fn search_attempt(
            &self,
            _query: &SearchQuery,
            _deadline: Instant,
        ) -> Result<SearchResult, SearchAttemptError> {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active.fetch_max(active, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(40)).await;
            self.active.fetch_sub(1, Ordering::SeqCst);
            Ok(successful("parallel"))
        }
    }

    #[tokio::test]
    async fn provider_concurrency_is_limited_to_two() {
        let adapter = Arc::new(ConcurrencyAdapter {
            active: AtomicUsize::new(0),
            max_active: AtomicUsize::new(0),
        });
        let service = Arc::new(ManagedSearchService::from_adapters(vec![(
            adapter.clone(),
            Duration::from_secs(1),
        )]));
        let searches = (0..5)
            .map(|index| {
                let service = service.clone();
                tokio::spawn(async move {
                    service
                        .search(SearchQuery {
                            query: format!("query-{index}"),
                            count: 5,
                        })
                        .await
                })
            })
            .collect::<Vec<_>>();
        for search in searches {
            search.await.expect("join").expect("search");
        }
        assert_eq!(adapter.max_active.load(Ordering::SeqCst), 2);
    }

    struct CancellationAdapter {
        calls: AtomicUsize,
        first_started: tokio::sync::Notify,
    }

    #[async_trait]
    impl ManagedSearchProvider for CancellationAdapter {
        fn id(&self) -> SearchProviderId {
            SearchProviderId::Parallel
        }

        async fn search_attempt(
            &self,
            _query: &SearchQuery,
            _deadline: Instant,
        ) -> Result<SearchResult, SearchAttemptError> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                self.first_started.notify_one();
                std::future::pending().await
            } else {
                Ok(successful("parallel"))
            }
        }
    }

    #[tokio::test]
    async fn cancelling_one_search_does_not_cancel_another() {
        let adapter = Arc::new(CancellationAdapter {
            calls: AtomicUsize::new(0),
            first_started: tokio::sync::Notify::new(),
        });
        let service = Arc::new(ManagedSearchService::from_adapters(vec![(
            adapter.clone(),
            Duration::from_secs(1),
        )]));
        let first_service = service.clone();
        let first = tokio::spawn(async move {
            first_service
                .search(SearchQuery {
                    query: "first".to_owned(),
                    count: 5,
                })
                .await
        });
        adapter.first_started.notified().await;

        let second = service
            .search(SearchQuery {
                query: "second".to_owned(),
                count: 5,
            })
            .await
            .expect("second search remains independent");
        first.abort();
        assert_eq!(second.provider, "parallel");
    }

    #[test]
    fn result_limits_are_applied_without_provider_text() {
        let mut result = SearchResult {
            provider: "parallel".to_owned(),
            hits: vec![SearchHit {
                title: "t".repeat(MAX_TITLE_CHARS + 1),
                url: format!("https://example.com/{}", "u".repeat(MAX_URL_BYTES)),
                snippet: "s".repeat(MAX_SNIPPET_CHARS + 1),
                rank: 99,
            }],
        };
        assert!(normalize_result(&mut result, 1));
        assert_eq!(result.hits[0].rank, 1);
        assert!(result.hits[0].title.chars().count() <= MAX_TITLE_CHARS);
        assert!(result.hits[0].url.len() <= MAX_URL_BYTES);
        assert!(result.hits[0].snippet.chars().count() <= MAX_SNIPPET_CHARS);
    }
}
