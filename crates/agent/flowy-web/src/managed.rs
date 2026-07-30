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
    provider::{DuckDuckGoSearchProvider, SearchProvider},
    types::{MAX_SEARCH_COUNT, SearchQuery, SearchResult, WebError},
};

mod decoders;
mod remote;

use remote::RemoteSearchAdapter;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisableReason {
    Unauthorized,
    RpcMethodUnavailable,
    ToolMissing,
    SchemaMismatch,
    RateLimitUntilRestart,
}

#[derive(Default)]
struct SearchProviderHealth {
    consecutive_failures: u32,
    cooldown_until: Option<Instant>,
    disable_reason: Option<DisableReason>,
    you_unhinted_rate_limits: u8,
}

impl SearchProviderHealth {
    fn is_available(&self, now: Instant) -> bool {
        self.disable_reason.is_none() && self.cooldown_until.is_none_or(|until| until <= now)
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
            | SearchAttemptError::QueueBusy
            | SearchAttemptError::SessionExpired
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
        Ok(Self::from_adapters(adapters))
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
            let queue_wait_ms = attempt_started.elapsed().as_millis();
            let request_started = Instant::now();
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
                        "managed web search succeeded"
                    );
                    return Ok(result);
                }
                Ok(_) => {
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
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].snippet, "Body");
        assert_eq!(hits[0].published_at.as_deref(), Some("2026-07-30"));
    }

    #[test]
    fn parses_you_label_blocks_without_exposing_provider_format() {
        let result: McpToolResult = serde_json::from_value(json!({
            "content": [{"type": "text", "text": "WEB RESULTS\nTitle: One result\nURL: https://example.com/one\nDescription: Useful summary\nPublished: 2026-07-30"}]
        }))
        .unwrap();
        let hits = decoders::decode_you(&result, 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "One result");
        assert_eq!(hits[0].snippet, "Useful summary");
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
}
