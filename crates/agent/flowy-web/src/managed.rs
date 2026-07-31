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
use tokio::sync::{Mutex, Semaphore};
use tokio::time::Instant;
use uuid::Uuid;

use crate::{
    coordinator::{ExtractCoordinator, ManagedExtractCoordinator},
    provider::{DuckDuckGoSearchProvider, HttpExtractProvider, SearchProvider},
    types::{MAX_SEARCH_COUNT, SearchQuery, SearchResult, WebError},
};

mod decoders;
pub(crate) mod fetch;
mod remote;

pub use fetch::{
    ParallelFetchAdapter, RemoteExtractBatch, RemoteExtractError, RemoteExtractFallback,
    RemoteExtractItem, RemoteExtractRequest, RemoteExtractRequestItem,
};
use remote::{ParallelMcpClient, RemoteSearchAdapter};

const TOTAL_BUDGET: Duration = Duration::from_secs(10);
const MAX_TITLE_CHARS: usize = 300;
const MAX_URL_BYTES: usize = 2048;
const MAX_SNIPPET_CHARS: usize = 2000;
#[cfg(debug_assertions)]
const DEV_DISABLED_PROVIDERS_ENV: &str = "NOMI_MANAGED_SEARCH_DISABLE_PROVIDERS";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchProviderId {
    Parallel,
    You,
    DuckDuckGo,
}

impl SearchProviderId {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Parallel => "parallel",
            Self::You => "you",
            Self::DuckDuckGo => "duckduckgo",
        }
    }
}

#[derive(Debug)]
enum SearchAttemptError {
    Network,
    Timeout,
    QueueBusy,
    RateLimited(Option<Duration>),
    Unauthorized,
    Forbidden,
    ToolMissing,
    SchemaMismatch,
    RpcMethodUnavailable,
    SessionExpired,
    InvalidRequest,
    MalformedResponse,
    Upstream,
}

struct SearchDecodeDiagnostics {
    decode_source: &'static str,
    structured_fallback: bool,
    dropped_items: usize,
    contract_degraded: bool,
}

struct SearchAttemptOutput {
    result: SearchResult,
    diagnostics: Option<SearchDecodeDiagnostics>,
}

#[async_trait]
trait ManagedSearchProvider: Send + Sync {
    fn id(&self) -> SearchProviderId;

    async fn search_attempt(
        &self,
        query: &SearchQuery,
        deadline: Instant,
    ) -> Result<SearchResult, SearchAttemptError>;

    async fn search_attempt_with_diagnostics(
        &self,
        query: &SearchQuery,
        deadline: Instant,
    ) -> Result<SearchAttemptOutput, SearchAttemptError> {
        self.search_attempt(query, deadline)
            .await
            .map(|result| SearchAttemptOutput {
                result,
                diagnostics: None,
            })
    }

    async fn shutdown(&self, _deadline: Instant) {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisableReason {
    Unauthorized,
    RpcMethodUnavailable,
    ToolMissing,
    SchemaMismatch,
    RateLimitUntilRestart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderAvailability {
    Ready,
    Cooldown,
    Disabled(DisableReason),
}

#[derive(Default)]
struct SearchProviderHealth {
    consecutive_failures: u32,
    cooldown_until: Option<Instant>,
    disable_reason: Option<DisableReason>,
    you_unhinted_rate_limits: u8,
}

impl SearchProviderHealth {
    fn availability(&self, now: Instant) -> ProviderAvailability {
        if let Some(reason) = self.disable_reason {
            ProviderAvailability::Disabled(reason)
        } else if self.cooldown_until.is_some_and(|until| until > now) {
            ProviderAvailability::Cooldown
        } else {
            ProviderAvailability::Ready
        }
    }

    fn is_available(&self, now: Instant) -> bool {
        self.availability(now) == ProviderAvailability::Ready
    }

    fn record_success(&mut self, provider: SearchProviderId) {
        self.consecutive_failures = 0;
        self.cooldown_until = None;
        if provider == SearchProviderId::You {
            self.you_unhinted_rate_limits = 0;
        }
    }

    fn record_error(
        &mut self,
        provider: SearchProviderId,
        error: &SearchAttemptError,
        now: Instant,
    ) {
        match error {
            SearchAttemptError::Unauthorized => {
                self.disable_reason = Some(DisableReason::Unauthorized);
            }
            SearchAttemptError::RpcMethodUnavailable => {
                self.disable_reason = Some(DisableReason::RpcMethodUnavailable);
            }
            SearchAttemptError::ToolMissing => {
                self.disable_reason = Some(DisableReason::ToolMissing);
            }
            SearchAttemptError::SchemaMismatch => {
                self.disable_reason = Some(DisableReason::SchemaMismatch);
            }
            SearchAttemptError::Forbidden => {
                self.cooldown_until = Some(now + Duration::from_secs(10 * 60));
            }
            SearchAttemptError::RateLimited(retry_after) => {
                let delay = if provider == SearchProviderId::You {
                    match retry_after {
                        Some(retry_after) => (*retry_after)
                            .clamp(Duration::from_secs(30), Duration::from_secs(24 * 60 * 60)),
                        None if self.you_unhinted_rate_limits == 0 => {
                            self.you_unhinted_rate_limits = 1;
                            Duration::from_secs(30 * 60)
                        }
                        None => {
                            self.disable_reason = Some(DisableReason::RateLimitUntilRestart);
                            return;
                        }
                    }
                } else {
                    retry_after
                        .as_ref()
                        .copied()
                        .unwrap_or(Duration::from_secs(30))
                        .clamp(Duration::from_secs(30), Duration::from_secs(15 * 60))
                };
                self.cooldown_until = Some(now + delay);
            }
            SearchAttemptError::Network
            | SearchAttemptError::Timeout
            | SearchAttemptError::SessionExpired
            | SearchAttemptError::Upstream
            | SearchAttemptError::MalformedResponse => {
                self.consecutive_failures = self.consecutive_failures.saturating_add(1);
                let exponent = self.consecutive_failures.saturating_sub(1).min(5);
                let seconds = 15u64.saturating_mul(1u64 << exponent).min(5 * 60);
                self.cooldown_until = Some(now + Duration::from_secs(seconds));
            }
            SearchAttemptError::InvalidRequest => {}
            SearchAttemptError::QueueBusy => {}
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
    parallel_client: Option<Arc<ParallelMcpClient>>,
}

impl ManagedSearchService {
    /// Build the keyless chain verified by the explicit compatibility probe.
    /// Construction is offline; provider connections are lazy.
    pub fn keyless_default() -> Result<Self, WebError> {
        let disabled = development_disabled_providers();
        let parallel_client = if !disabled.contains(&SearchProviderId::Parallel) {
            Some(Arc::new(ParallelMcpClient::new()?))
        } else {
            None
        };
        Self::keyless_with_optional_client(parallel_client)
    }

    pub(crate) fn keyless_with_shared_client(
        client: Arc<ParallelMcpClient>,
    ) -> Result<Self, WebError> {
        Self::keyless_with_optional_client(Some(client))
    }

    fn keyless_with_optional_client(
        parallel_client: Option<Arc<ParallelMcpClient>>,
    ) -> Result<Self, WebError> {
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
        let use_parallel =
            !disabled.contains(&SearchProviderId::Parallel) && parallel_client.is_some();
        if use_parallel {
            adapters.push((
                Arc::new(RemoteSearchAdapter::parallel(Arc::clone(
                    parallel_client
                        .as_ref()
                        .expect("parallel client exists when Parallel is enabled"),
                ))) as Arc<dyn ManagedSearchProvider>,
                Duration::from_secs(3),
            ));
        }
        if !disabled.contains(&SearchProviderId::You) {
            adapters.push((
                Arc::new(RemoteSearchAdapter::you()?) as Arc<dyn ManagedSearchProvider>,
                Duration::from_secs(3),
            ));
        }
        adapters.push((
            Arc::new(DuckDuckGoAdapter::try_new()?) as Arc<dyn ManagedSearchProvider>,
            Duration::from_secs(4),
        ));
        let mut service = Self::from_adapters(adapters);
        service.parallel_client = parallel_client;
        Ok(service)
    }

    pub fn ddg_only() -> Result<Self, WebError> {
        Ok(Self::from_adapters(vec![(
            Arc::new(DuckDuckGoAdapter::try_new()?) as Arc<dyn ManagedSearchProvider>,
            Duration::from_secs(4),
        )]))
    }

    fn from_adapters(
        adapters: Vec<(Arc<dyn ManagedSearchProvider>, Duration)>,
    ) -> Self {
        Self {
            slots: adapters
                .into_iter()
                .map(|(adapter, timeout)| SearchProviderSlot::new(adapter, timeout))
                .collect(),
            parallel_client: None,
        }
    }

    pub async fn shutdown(&self) {
        let deadline = Instant::now() + Duration::from_secs(2);
        if let Some(client) = self.parallel_client.as_ref() {
            client.shutdown(deadline).await;
        }
        for slot in &self.slots {
            slot.adapter.shutdown(deadline).await;
        }
    }
}

pub struct ManagedWebService {
    search: Arc<ManagedSearchService>,
    extract: Option<Arc<dyn ExtractCoordinator>>,
}

impl ManagedWebService {
    pub fn keyless_default(managed_extract: bool) -> Result<Self, WebError> {
        let parallel_client = Arc::new(ParallelMcpClient::new()?);
        let search = Arc::new(ManagedSearchService::keyless_with_shared_client(Arc::clone(
            &parallel_client,
        ))?);
        let extract = if managed_extract {
            let fetch = Arc::new(ParallelFetchAdapter::new(Arc::clone(&parallel_client)));
            Some(Arc::new(ManagedExtractCoordinator::new(
                Arc::new(HttpExtractProvider::new()),
                fetch,
            )) as Arc<dyn ExtractCoordinator>)
        } else {
            None
        };
        Ok(Self { search, extract })
    }

    pub fn ddg_only() -> Result<Self, WebError> {
        Ok(Self {
            search: Arc::new(ManagedSearchService::ddg_only()?),
            extract: None,
        })
    }

    pub fn search_provider(&self) -> Arc<dyn SearchProvider> {
        self.search.clone()
    }

    pub fn extract_coordinator(&self) -> Option<Arc<dyn ExtractCoordinator>> {
        self.extract.clone()
    }

    pub async fn shutdown(&self) {
        self.search.shutdown().await;
    }
}

impl ManagedSearchService {
    async fn route(&self, query: &SearchQuery) -> Result<SearchResult, WebError> {
        let request_id = Uuid::now_v7();
        let overall_deadline = Instant::now() + TOTAL_BUDGET;
        let mut fallback_count = 0usize;
        let mut last_class = None;
        let mut saw_successful_empty = false;
        let mut attempted_provider = false;

        for (attempt, slot) in self.slots.iter().enumerate() {
            let now = Instant::now();
            if now >= overall_deadline {
                last_class = Some("timeout");
                break;
            }
            let availability = slot.health.lock().await.availability(now);
            if availability != ProviderAvailability::Ready {
                fallback_count += 1;
                tracing::debug!(
                    target: "managed_search",
                    request_id = %request_id,
                    provider = slot.adapter.id().as_str(),
                    attempt,
                    availability = ?availability,
                    fallback_count,
                    "managed web search provider skipped"
                );
                continue;
            }
            let provider_deadline = std::cmp::min(overall_deadline, now + slot.timeout);
            let attempt_started = Instant::now();
            let permit = match tokio::time::timeout_at(
                provider_deadline,
                slot.semaphore.acquire(),
            )
            .await
            {
                Ok(Ok(permit)) => permit,
                Ok(Err(_)) => {
                    let error = SearchAttemptError::QueueBusy;
                    last_class = Some(error_class(&error));
                    fallback_count += 1;
                    continue;
                }
                Err(_) => {
                    let error = SearchAttemptError::QueueBusy;
                    last_class = Some(error_class(&error));
                    fallback_count += 1;
                    tracing::warn!(
                        target: "managed_search",
                        request_id = %request_id,
                        provider = slot.adapter.id().as_str(),
                        attempt,
                        queue_wait_ms = attempt_started.elapsed().as_millis(),
                        request_elapsed_ms = 0u128,
                        error_class = "queue_busy",
                        fallback_count,
                        "managed web search provider queue is busy"
                    );
                    continue;
                }
            };
            // Only count a provider as attempted after a network-capable permit
            // is held. QueueBusy must not claim an attempt occurred.
            attempted_provider = true;
            let queue_wait_ms = attempt_started.elapsed().as_millis();
            let request_started = Instant::now();
            let result = match tokio::time::timeout_at(
                provider_deadline,
                slot.adapter
                    .search_attempt_with_diagnostics(query, provider_deadline),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => Err(SearchAttemptError::Timeout),
            };
            drop(permit);

            match result {
                Ok(output) if !output.result.hits.is_empty() => {
                    let mut result = output.result;
                    let diagnostics = output.diagnostics;
                    let truncated = normalize_result(&mut result, query.count);
                    let request_elapsed_ms = request_started.elapsed().as_millis();
                    slot.health
                        .lock()
                        .await
                        .record_success(slot.adapter.id());
                    tracing::info!(
                        target: "managed_search",
                        request_id = %request_id,
                        provider = slot.adapter.id().as_str(),
                        attempt,
                        elapsed_ms = attempt_started.elapsed().as_millis(),
                        queue_wait_ms,
                        request_elapsed_ms,
                        result_count = result.hits.len(),
                        fallback_count,
                        truncated,
                        decode_source = diagnostics
                            .as_ref()
                            .map_or("native", |diagnostics| diagnostics.decode_source),
                        structured_fallback = diagnostics
                            .as_ref()
                            .is_some_and(|diagnostics| diagnostics.structured_fallback),
                        dropped_items = diagnostics
                            .as_ref()
                            .map_or(0, |diagnostics| diagnostics.dropped_items),
                        contract_degraded = diagnostics
                            .as_ref()
                            .is_some_and(|diagnostics| diagnostics.contract_degraded),
                        "managed web search succeeded"
                    );
                    return Ok(result);
                }
                Ok(output) => {
                    saw_successful_empty = true;
                    let diagnostics = output.diagnostics;
                    let request_elapsed_ms = request_started.elapsed().as_millis();
                    slot.health
                        .lock()
                        .await
                        .record_success(slot.adapter.id());
                    fallback_count += 1;
                    tracing::debug!(
                        target: "managed_search",
                        request_id = %request_id,
                        provider = slot.adapter.id().as_str(),
                        attempt,
                        elapsed_ms = attempt_started.elapsed().as_millis(),
                        queue_wait_ms,
                        request_elapsed_ms,
                        decode_source = diagnostics
                            .as_ref()
                            .map_or("native", |diagnostics| diagnostics.decode_source),
                        structured_fallback = diagnostics
                            .as_ref()
                            .is_some_and(|diagnostics| diagnostics.structured_fallback),
                        dropped_items = diagnostics
                            .as_ref()
                            .map_or(0, |diagnostics| diagnostics.dropped_items),
                        contract_degraded = diagnostics
                            .as_ref()
                            .is_some_and(|diagnostics| diagnostics.contract_degraded),
                        "managed web search returned no results"
                    );
                }
                Err(error) => {
                    let request_elapsed_ms = request_started.elapsed().as_millis();
                    let class = error_class(&error);
                    last_class = Some(class);
                    slot.health.lock().await.record_error(
                        slot.adapter.id(),
                        &error,
                        Instant::now(),
                    );
                    fallback_count += 1;
                    tracing::warn!(
                        target: "managed_search",
                        request_id = %request_id,
                        provider = slot.adapter.id().as_str(),
                        attempt,
                        elapsed_ms = attempt_started.elapsed().as_millis(),
                        queue_wait_ms,
                        request_elapsed_ms,
                        error_class = class,
                        fallback_count,
                        "managed web search provider failed"
                    );
                }
            }
        }

        if saw_successful_empty {
            tracing::info!(
                target: "managed_search",
                request_id = %request_id,
                fallback_count,
                attempted_provider,
                had_provider_errors = last_class.is_some(),
                "managed web search completed with no results"
            );
            return Ok(SearchResult {
                provider: "managed".to_owned(),
                hits: Vec::new(),
            });
        }
        tracing::warn!(
            target: "managed_search",
            request_id = %request_id,
            fallback_count,
            attempted_provider,
            error_class = last_class.unwrap_or("unavailable"),
            "all managed web search providers were unavailable"
        );
        Err(WebError::Provider(
            "web search is temporarily unavailable; do not repeat the same search this turn"
                .to_owned(),
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
            "you" => Some(SearchProviderId::You),
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
    fn try_new() -> Result<Self, WebError> {
        Ok(Self {
            provider: DuckDuckGoSearchProvider::try_new()?,
        })
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
        SearchAttemptError::QueueBusy => "queue_busy",
        SearchAttemptError::RateLimited(_) => "rate_limited",
        SearchAttemptError::Unauthorized => "unauthorized",
        SearchAttemptError::Forbidden => "forbidden",
        SearchAttemptError::ToolMissing => "tool_missing",
        SearchAttemptError::SchemaMismatch => "schema_mismatch",
        SearchAttemptError::RpcMethodUnavailable => "rpc_method_unavailable",
        SearchAttemptError::SessionExpired => "session_expired",
        SearchAttemptError::InvalidRequest => "invalid_request",
        SearchAttemptError::MalformedResponse => "malformed_response",
        SearchAttemptError::Upstream => "upstream",
    }
}


fn normalize_result(result: &mut SearchResult, requested: u32) -> bool {
    let mut truncated = false;
    result.hits.truncate(requested.clamp(1, MAX_SEARCH_COUNT) as usize);
    let mut kept = Vec::with_capacity(result.hits.len());
    for mut hit in result.hits.drain(..) {
        truncated |= truncate_chars(&mut hit.title, MAX_TITLE_CHARS);
        truncated |= truncate_bytes(&mut hit.url, MAX_URL_BYTES);
        truncated |= truncate_chars(&mut hit.snippet, MAX_SNIPPET_CHARS);
        if hit.url.is_empty() {
            truncated = true;
            continue;
        }
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
    use crate::types::SearchHit;
    use nomi_mcp::protocol::McpToolResult;
    use serde_json::json;
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
                published_at: None,
                rank: 1,
            }],
        }
    }

    #[cfg(debug_assertions)]
    #[test]
    fn development_provider_override_is_explicit_and_deduplicated() {
        assert_eq!(
            parse_disabled_providers(" Parallel,you,parallel,legacy,unknown "),
            vec![SearchProviderId::Parallel, SearchProviderId::You]
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
    async fn parallel_empty_result_falls_through_to_you_before_ddg() {
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
                fake(SearchProviderId::You, Ok(successful("you"))),
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
            .expect("You fallback succeeds");
        assert_eq!(result.provider, "you");
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
                fake(SearchProviderId::You, Err(SearchAttemptError::Timeout)),
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

    #[tokio::test]
    async fn all_successful_empty_results_are_a_valid_empty_search() {
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
                fake(
                    SearchProviderId::You,
                    Ok(SearchResult {
                        provider: "you".to_owned(),
                        hits: Vec::new(),
                    }),
                ),
                Duration::from_secs(1),
            ),
            (
                fake(
                    SearchProviderId::DuckDuckGo,
                    Ok(SearchResult {
                        provider: "duckduckgo".to_owned(),
                        hits: Vec::new(),
                    }),
                ),
                Duration::from_secs(1),
            ),
        ]);
        let result = service
            .search(SearchQuery {
                query: "no-match".to_owned(),
                count: 5,
            })
            .await
            .expect("legitimate empty search must not be unavailable");
        assert!(result.hits.is_empty());
    }

    #[tokio::test]
    async fn early_timeout_then_empty_providers_still_return_empty_search() {
        // Parallel timeout must not turn a later legitimate empty chain into
        // "temporarily unavailable".
        let service = ManagedSearchService::from_adapters(vec![
            (
                fake(SearchProviderId::Parallel, Err(SearchAttemptError::Timeout)),
                Duration::from_secs(1),
            ),
            (
                fake(
                    SearchProviderId::You,
                    Ok(SearchResult {
                        provider: "you".to_owned(),
                        hits: Vec::new(),
                    }),
                ),
                Duration::from_secs(1),
            ),
            (
                fake(
                    SearchProviderId::DuckDuckGo,
                    Ok(SearchResult {
                        provider: "duckduckgo".to_owned(),
                        hits: Vec::new(),
                    }),
                ),
                Duration::from_secs(1),
            ),
        ]);
        let result = service
            .search(SearchQuery {
                query: "no-match".to_owned(),
                count: 5,
            })
            .await
            .expect("empty after timeout must remain a valid empty search");
        assert!(result.hits.is_empty());
        assert_eq!(result.provider, "managed");
    }

    #[test]
    fn parses_nested_json_results() {
        let result: McpToolResult = serde_json::from_value(json!({
            "structuredContent": {"results": [{
                "title": "One",
                "url": "https://example.com",
                "publish_date": "2026-07-30",
                "excerpts": ["Body"]
            }]}
        }))
        .unwrap();
        let hits = decoders::decode_parallel(&result, 5).unwrap();
        assert_eq!(hits.hits.len(), 1);
        assert_eq!(hits.hits[0].snippet, "Body");
        assert_eq!(hits.hits[0].published_at.as_deref(), Some("2026-07-30"));
    }

    #[test]
    fn parses_you_label_blocks_without_exposing_provider_format() {
        let result: McpToolResult = serde_json::from_value(json!({
            "content": [{"type": "text", "text": "WEB RESULTS\nTitle: One result\nURL: https://example.com/one\nDescription: Useful summary\nPublished: 2026-07-30"}]
        }))
        .unwrap();
        let hits = decoders::decode_you(&result, 5).unwrap();
        assert_eq!(hits.hits.len(), 1);
        assert_eq!(hits.hits[0].title, "One result");
        assert_eq!(hits.hits[0].snippet, "Useful summary");
    }

    #[test]
    fn health_policy_distinguishes_permanent_and_temporary_failures() {
        let now = Instant::now();
        let mut health = SearchProviderHealth::default();
        health.record_error(SearchProviderId::Parallel, &SearchAttemptError::Unauthorized, now);
        assert_eq!(health.disable_reason, Some(DisableReason::Unauthorized));

        let mut health = SearchProviderHealth::default();
        health.record_error(SearchProviderId::Parallel, &SearchAttemptError::Forbidden, now);
        assert!(health.disable_reason.is_none());
        assert!(health.cooldown_until > Some(now));
    }

    #[test]
    fn queue_busy_does_not_pollute_provider_health() {
        let now = Instant::now();
        let mut health = SearchProviderHealth::default();
        health.record_error(SearchProviderId::Parallel, &SearchAttemptError::QueueBusy, now);
        assert_eq!(health.consecutive_failures, 0);
        assert_eq!(health.cooldown_until, None);
        assert_eq!(health.availability(now), ProviderAvailability::Ready);
    }

    #[test]
    fn rate_limit_honors_bounded_retry_after_and_network_backoff_grows() {
        let now = Instant::now();
        let mut health = SearchProviderHealth::default();
        health.record_error(
            SearchProviderId::Parallel,
            &SearchAttemptError::RateLimited(Some(Duration::from_secs(1))),
            now,
        );
        assert_eq!(
            health.cooldown_until,
            Some(now + Duration::from_secs(30))
        );

        let mut health = SearchProviderHealth::default();
        health.record_error(SearchProviderId::Parallel, &SearchAttemptError::Network, now);
        assert_eq!(
            health.cooldown_until,
            Some(now + Duration::from_secs(15))
        );
        health.record_error(SearchProviderId::Parallel, &SearchAttemptError::Network, now);
        assert_eq!(
            health.cooldown_until,
            Some(now + Duration::from_secs(30))
        );
    }

    #[test]
    fn you_unhinted_rate_limit_disables_only_on_the_second_consecutive_event() {
        let now = Instant::now();
        let mut health = SearchProviderHealth::default();
        health.record_error(SearchProviderId::You, &SearchAttemptError::RateLimited(None), now);
        assert_eq!(health.you_unhinted_rate_limits, 1);
        assert_eq!(health.disable_reason, None);
        assert_eq!(health.cooldown_until, Some(now + Duration::from_secs(30 * 60)));

        health.record_error(SearchProviderId::You, &SearchAttemptError::RateLimited(None), now);
        assert_eq!(
            health.disable_reason,
            Some(DisableReason::RateLimitUntilRestart)
        );
        assert!(!health.is_available(now + Duration::from_secs(24 * 60 * 60)));
    }

    #[test]
    fn you_hinted_rate_limit_is_bounded_without_incrementing_unhinted_counter() {
        let now = Instant::now();
        let mut health = SearchProviderHealth::default();
        health.record_error(
            SearchProviderId::You,
            &SearchAttemptError::RateLimited(Some(Duration::from_secs(1))),
            now,
        );
        assert_eq!(health.you_unhinted_rate_limits, 0);
        assert_eq!(health.cooldown_until, Some(now + Duration::from_secs(30)));
        health.record_success(SearchProviderId::You);
        assert!(health.cooldown_until.is_none());
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
            "provider error: web search is temporarily unavailable; do not repeat the same search this turn"
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

    #[tokio::test(start_paused = true)]
    async fn queue_pressure_does_not_cool_down_provider() {
        let service = ManagedSearchService::from_adapters(vec![(
            fake(SearchProviderId::Parallel, Ok(successful("parallel"))),
            Duration::from_millis(5),
        )]);
        let permit_one = service.slots[0]
            .semaphore
            .try_acquire()
            .expect("first permit");
        let permit_two = service.slots[0]
            .semaphore
            .try_acquire()
            .expect("second permit");

        let result = service
            .search(SearchQuery {
                query: "queue-pressure".to_owned(),
                count: 5,
            })
            .await;
        assert!(result.is_err());
        assert!(service.slots[0].health.lock().await.cooldown_until.is_none());

        drop(permit_one);
        drop(permit_two);
        assert!(service
            .search(SearchQuery {
                query: "after-queue-pressure".to_owned(),
                count: 5,
            })
            .await
            .is_ok());
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
                published_at: None,
                rank: 99,
            }],
        };
        assert!(normalize_result(&mut result, 1));
        assert_eq!(result.hits[0].rank, 1);
        assert!(result.hits[0].title.chars().count() <= MAX_TITLE_CHARS);
        assert!(result.hits[0].url.len() <= MAX_URL_BYTES);
        assert!(result.hits[0].snippet.chars().count() <= MAX_SNIPPET_CHARS);
    }

    #[test]
    fn managed_web_service_search_only_constructs_offline() {
        let service = ManagedWebService::keyless_default(false).expect("offline construction");
        assert!(service.extract_coordinator().is_none());
        let _provider = service.search_provider();
    }

    #[test]
    fn managed_web_service_composes_extract_capability() {
        let service = ManagedWebService::keyless_default(true).expect("offline construction");
        assert!(service.extract_coordinator().is_some());
    }
}
