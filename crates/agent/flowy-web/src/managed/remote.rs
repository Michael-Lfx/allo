use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use nomi_mcp::{
    protocol::{McpToolDef, McpToolResult},
    remote_peer::{McpPeerError, RemoteMcpPeer},
};
use reqwest::StatusCode;
use serde_json::{Value, json};
use tokio::sync::{Mutex, Semaphore};
use tokio::time::Instant;

use crate::provider::extract_policy::prepare_remote_url;
use crate::types::{MAX_EXTRACT_URLS, SearchQuery, SearchResult, WebError};

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
struct CooldownTracker {
    consecutive_failures: u32,
    cooldown_until: Option<Instant>,
}

impl CooldownTracker {
    fn is_available(&self, now: Instant) -> bool {
        self.cooldown_until.is_none_or(|cooldown_until| cooldown_until <= now)
    }

    fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.cooldown_until = None;
    }

    fn record_network_error(&mut self, now: Instant) {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        let exponent = self.consecutive_failures.saturating_sub(1).min(5);
        let seconds = 15u64.saturating_mul(1u64 << exponent).min(5 * 60);
        self.cooldown_until = Some(now + Duration::from_secs(seconds));
    }

    fn schedule_until(&mut self, until: Instant) {
        self.cooldown_until = Some(until);
    }
}

#[derive(Default)]
struct EndpointHealth {
    cooldown: CooldownTracker,
    disable_reason: Option<EndpointDisableReason>,
    epoch: u64,
}

#[derive(Debug, Clone, Copy)]
struct EndpointAttemptToken {
    epoch: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EndpointFailureKind {
    Unauthorized,
    Forbidden,
    RateLimited(Option<Duration>),
    Network,
    MalformedResponse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FetchToolFailureKind {
    Upstream,
    Timeout,
    MalformedResponse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ManagedFetchErrorKind {
    Endpoint(EndpointFailureKind),
    Tool(FetchToolFailureKind),
}

impl From<EndpointFailureKind> for ManagedFetchErrorKind {
    fn from(kind: EndpointFailureKind) -> Self {
        Self::Endpoint(kind)
    }
}

impl From<FetchToolFailureKind> for ManagedFetchErrorKind {
    fn from(kind: FetchToolFailureKind) -> Self {
        Self::Tool(kind)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EndpointDisableReason {
    Unauthorized,
}

#[derive(Default)]
struct FetchToolHealth {
    cooldown: CooldownTracker,
}

impl FetchToolHealth {
    fn is_available(&self, now: Instant) -> bool {
        self.cooldown.is_available(now)
    }

    fn record_success(&mut self) {
        self.cooldown.record_success();
    }

    fn record_error(&mut self, _kind: FetchToolFailureKind, now: Instant) {
        self.cooldown.record_network_error(now);
    }
}

impl EndpointHealth {
    fn is_available(&self, now: Instant) -> bool {
        self.disable_reason.is_none()
            && self.cooldown.is_available(now)
    }

    fn begin_attempt(&self, now: Instant) -> Option<EndpointAttemptToken> {
        self.is_available(now).then_some(EndpointAttemptToken { epoch: self.epoch })
    }

    fn record_success(&mut self, token: EndpointAttemptToken) {
        // A response from an attempt that started before a newer failure must
        // not erase that newer cooldown/disable state.
        if token.epoch == self.epoch {
            self.cooldown.record_success();
        }
    }

    fn record_reinitialized(&mut self) {
        self.epoch = self.epoch.saturating_add(1);
        // Session recovery may clear an Unauthorized disable, but it must not
        // bypass an active 403/429/network cooldown.
        self.disable_reason = None;
    }

    fn record_error(&mut self, kind: EndpointFailureKind, now: Instant) {
        self.epoch = self.epoch.saturating_add(1);
        match kind {
            EndpointFailureKind::Unauthorized => {
                self.disable_reason = Some(EndpointDisableReason::Unauthorized);
            }
            EndpointFailureKind::Forbidden => {
                self.cooldown
                    .schedule_until(now + Duration::from_secs(10 * 60));
            }
            EndpointFailureKind::RateLimited(retry_after) => {
                let delay = retry_after
                    .unwrap_or(Duration::from_secs(30))
                    .clamp(Duration::from_secs(30), Duration::from_secs(24 * 60 * 60));
                self.cooldown.schedule_until(now + delay);
            }
            EndpointFailureKind::Network | EndpointFailureKind::MalformedResponse => {
                self.cooldown.record_network_error(now);
            }
        }
    }
}

pub(crate) struct ParallelMcpClient {
    peer: Arc<RemoteMcpPeer>,
    endpoint_health: Mutex<EndpointHealth>,
    fetch_tool_health: Mutex<FetchToolHealth>,
    remote_fetch_semaphore: Semaphore,
    call_control: Arc<dyn ManagedMcpCallControl>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ManagedMcpTool {
    Fetch,
    Search,
}

impl ManagedMcpTool {
    fn as_str(self) -> &'static str {
        match self {
            Self::Fetch => "web_fetch",
            Self::Search => "web_search",
        }
    }
}

#[derive(Debug)]
pub(crate) enum ManagedMcpCallError {
    #[cfg_attr(not(any(test, feature = "fetch-eval")), allow(dead_code))]
    QuotaExhausted,
    UnsafeArguments,
    RetryLimitExceeded,
    #[cfg_attr(not(any(test, feature = "fetch-eval")), allow(dead_code))]
    LedgerFailure,
    EndpointUnavailable,
    Peer(McpPeerError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParallelCallRejection {
    UnsafeArguments,
    RetryLimitExceeded,
}

#[allow(dead_code)] // Evaluation-only controls construct these variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ManagedMcpControlError {
    #[cfg(any(test, feature = "fetch-eval"))]
    QuotaExhausted,
    #[cfg(any(test, feature = "fetch-eval"))]
    LedgerFailure,
}

#[derive(Debug)]
pub(crate) struct AuthorizedParallelCall {
    tool: ManagedMcpTool,
    arguments: Value,
    #[cfg(any(test, feature = "fetch-eval"))]
    attempt: u8,
}

impl AuthorizedParallelCall {
    #[cfg(any(test, feature = "fetch-eval"))]
    pub(crate) fn tool(&self) -> ManagedMcpTool {
        self.tool
    }

    #[cfg(test)]
    pub(crate) fn arguments(&self) -> &Value {
        &self.arguments
    }

    #[cfg(any(test, feature = "fetch-eval"))]
    pub(crate) fn attempt(&self) -> u8 {
        self.attempt
    }
}

#[derive(Debug, Default)]
pub(crate) struct ParallelMcpCallPolicy;

impl ParallelMcpCallPolicy {
    pub(crate) fn authorize(
        &self,
        tool_name: &str,
        arguments: Value,
        attempt: u8,
    ) -> Result<AuthorizedParallelCall, ParallelCallRejection> {
        if attempt == 0 || attempt > 3 {
            return Err(ParallelCallRejection::RetryLimitExceeded);
        }
        let tool = match tool_name {
            "web_fetch" => ManagedMcpTool::Fetch,
            "web_search" => ManagedMcpTool::Search,
            _ => return Err(ParallelCallRejection::UnsafeArguments),
        };
        let safe = match tool {
            ManagedMcpTool::Fetch => safe_fetch_arguments(&arguments),
            ManagedMcpTool::Search => safe_search_arguments(&arguments),
        };
        if !safe {
            return Err(ParallelCallRejection::UnsafeArguments);
        }
        Ok(AuthorizedParallelCall {
            tool,
            arguments,
            #[cfg(any(test, feature = "fetch-eval"))]
            attempt,
        })
    }
}

#[async_trait]
pub(crate) trait ManagedMcpCallControl: Send + Sync {
    async fn reserve(
        &self,
        call: &AuthorizedParallelCall,
    ) -> Result<(), ManagedMcpControlError>;

    fn observe_rejection(&self, _rejection: ParallelCallRejection) {}

    fn observe_result(
        &self,
        _call: &AuthorizedParallelCall,
        _result: &Result<McpToolResult, McpPeerError>,
    ) {
    }
}

#[derive(Debug, Default)]
struct NoopManagedMcpCallControl;

#[async_trait]
impl ManagedMcpCallControl for NoopManagedMcpCallControl {
    async fn reserve(
        &self,
        _call: &AuthorizedParallelCall,
    ) -> Result<(), ManagedMcpControlError> {
        Ok(())
    }
}

#[derive(Debug)]
enum ManagedSearchCallError {
    Peer(McpPeerError),
    Upstream,
    QuotaExhausted,
    UnsafeArguments,
    RetryLimitExceeded,
    LedgerFailure,
}

pub(crate) fn safe_fetch_arguments(arguments: &Value) -> bool {
    let Some(object) = arguments.as_object() else {
        return false;
    };
    let keys = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
    if keys != BTreeSet::from(["full_content", "urls"]) {
        return false;
    }
    if arguments.get("full_content").and_then(Value::as_bool) != Some(false) {
        return false;
    }
    let Some(urls) = arguments.get("urls").and_then(Value::as_array) else {
        return false;
    };
    !urls.is_empty()
        && urls.len() <= MAX_EXTRACT_URLS
        && urls.iter().all(|value| {
            let Some(raw) = value.as_str() else {
                return false;
            };
            let Ok(prepared) = prepare_remote_url(raw, false) else {
                return false;
            };
            prepared.outbound_url == raw
        })
}

pub(crate) fn safe_search_arguments(arguments: &Value) -> bool {
    let Some(object) = arguments.as_object() else {
        return false;
    };
    let keys = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
    if keys != BTreeSet::from(["objective", "search_queries"]) {
        return false;
    }
    let objective_ok = arguments
        .get("objective")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty());
    let queries_ok = arguments
        .get("search_queries")
        .and_then(Value::as_array)
        .is_some_and(|queries| {
            !queries.is_empty()
                && queries.iter().all(|query| {
                    query
                        .as_str()
                        .is_some_and(|value| !value.trim().is_empty())
                })
        });
    objective_ok && queries_ok
}

impl ParallelMcpClient {
    pub(crate) fn new() -> Result<Self, WebError> {
        Self::new_at_endpoint("https://search.parallel.ai/mcp")
    }

    #[allow(dead_code)]
    pub(super) fn new_at_endpoint(endpoint: impl Into<String>) -> Result<Self, WebError> {
        Self::new_at_endpoint_with_control(endpoint, Arc::new(NoopManagedMcpCallControl))
    }

    #[cfg(feature = "fetch-eval")]
    pub(crate) fn new_with_call_control(
        control: Arc<dyn ManagedMcpCallControl>,
    ) -> Result<Self, WebError> {
        Self::new_at_endpoint_with_control("https://search.parallel.ai/mcp", control)
    }

    #[cfg(test)]
    pub(crate) fn new_for_test_endpoint(
        endpoint: impl Into<String>,
        control: Option<Arc<dyn ManagedMcpCallControl>>,
    ) -> Result<Self, WebError> {
        let control = control.unwrap_or_else(|| Arc::new(NoopManagedMcpCallControl));
        Self::new_at_endpoint_with_control(endpoint, control)
    }

    #[cfg(test)]
    pub(crate) fn new_for_test_endpoint_with_control(
        endpoint: impl Into<String>,
        control: Arc<dyn ManagedMcpCallControl>,
    ) -> Result<Self, WebError> {
        Self::new_at_endpoint_with_control(endpoint, control)
    }

    fn new_at_endpoint_with_control(
        endpoint: impl Into<String>,
        call_control: Arc<dyn ManagedMcpCallControl>,
    ) -> Result<Self, WebError> {
        Ok(Self {
            peer: Arc::new(RemoteMcpPeer::new(endpoint).map_err(|_| {
                WebError::Provider("could not initialize managed Parallel MCP".to_owned())
            })?),
            endpoint_health: Mutex::new(EndpointHealth::default()),
            fetch_tool_health: Mutex::new(FetchToolHealth::default()),
            // Limits Parallel web_fetch concurrency across conversations.
            remote_fetch_semaphore: Semaphore::new(1),
            call_control,
        })
    }

    pub(super) fn peer(&self) -> Arc<RemoteMcpPeer> {
        Arc::clone(&self.peer)
    }

    #[allow(dead_code)] // Used by the fetch adapter phase and admission tests.
    pub(super) fn fetch_semaphore(&self) -> &Semaphore {
        &self.remote_fetch_semaphore
    }

    pub(crate) async fn endpoint_available(&self, now: Instant) -> bool {
        self.endpoint_health.lock().await.is_available(now)
    }

    async fn begin_endpoint_attempt(&self, now: Instant) -> Result<EndpointAttemptToken, ManagedMcpCallError> {
        self.endpoint_health
            .lock()
            .await
            .begin_attempt(now)
            .ok_or(ManagedMcpCallError::EndpointUnavailable)
    }

    async fn record_endpoint_success_for(&self, token: EndpointAttemptToken) {
        self.endpoint_health.lock().await.record_success(token);
    }

    pub(crate) async fn record_endpoint_reinitialized(&self) {
        self.endpoint_health.lock().await.record_reinitialized();
    }

    pub(crate) async fn record_fetch_error(
        &self,
        kind: impl Into<ManagedFetchErrorKind>,
        now: Instant,
    ) {
        match kind.into() {
            ManagedFetchErrorKind::Endpoint(kind) => {
                self.endpoint_health.lock().await.record_error(kind, now);
            }
            ManagedFetchErrorKind::Tool(kind) => {
                self.fetch_tool_health.lock().await.record_error(kind, now);
            }
        }
    }

    pub(crate) async fn fetch_tool_available(&self, now: Instant) -> bool {
        self.fetch_tool_health.lock().await.is_available(now)
    }

    pub(crate) async fn record_fetch_tool_success(&self) {
        self.fetch_tool_health.lock().await.record_success();
    }

    pub(super) async fn shutdown(&self, deadline: Instant) -> Result<(), WebError> {
        self.peer
            .shutdown(deadline)
            .await
            .map_err(|error| WebError::Provider(format!("managed Parallel shutdown failed: {error}")))
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
        attempt: u8,
    ) -> Result<McpToolResult, ManagedMcpCallError> {
        let authorized = match ParallelMcpCallPolicy.authorize(name, arguments, attempt) {
            Ok(call) => call,
            Err(rejection) => {
                self.call_control.observe_rejection(rejection);
                return Err(match rejection {
                    ParallelCallRejection::UnsafeArguments => {
                        ManagedMcpCallError::UnsafeArguments
                    }
                    ParallelCallRejection::RetryLimitExceeded => {
                        ManagedMcpCallError::RetryLimitExceeded
                    }
                });
            }
        };
        self.call_authorized(authorized, deadline).await
    }

    /// The transport boundary accepts only a call that has already crossed
    /// the production argument/retry policy. Keeping the peer invocation here
    /// prevents future adapters from accidentally sending an un-authorized
    /// tool payload directly to the MCP transport.
    async fn call_authorized(
        &self,
        authorized: AuthorizedParallelCall,
        deadline: Instant,
    ) -> Result<McpToolResult, ManagedMcpCallError> {
        let endpoint_attempt = self.begin_endpoint_attempt(Instant::now()).await?;
        self.call_control
            .reserve(&authorized)
            .await
            .map_err(|error| match error {
                #[cfg(any(test, feature = "fetch-eval"))]
                ManagedMcpControlError::QuotaExhausted => ManagedMcpCallError::QuotaExhausted,
                #[cfg(any(test, feature = "fetch-eval"))]
                ManagedMcpControlError::LedgerFailure => ManagedMcpCallError::LedgerFailure,
            })?;
        let result = self
            .peer
            .call_tool(authorized.tool.as_str(), authorized.arguments.clone(), deadline)
            .await;
        if result.is_ok() {
            self.record_endpoint_success_for(endpoint_attempt).await;
        }
        self.call_control.observe_result(&authorized, &result);
        result.map_err(ManagedMcpCallError::Peer)
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
        let mut session_recovered = false;
        let mut tool_rediscovered = false;
        let mut tool_attempt = 0_u8;
        loop {
            match self.ensure_compatible(deadline).await {
                Ok(()) => {
                    if session_recovered {
                        if let Some(client) = self.shared_client.as_ref() {
                            client.record_endpoint_reinitialized().await;
                        }
                        session_recovered = false;
                    }
                }
                Err(SearchAttemptError::SessionExpired) if !session_retried => {
                    self.clear_compatibility().await;
                    session_retried = true;
                    session_recovered = true;
                    continue;
                }
                Err(error) => {
                    if let Some(client) = self.shared_client.as_ref()
                        && matches!(error, SearchAttemptError::MalformedResponse)
                    {
                        client
                            .record_fetch_error(
                                EndpointFailureKind::MalformedResponse,
                                Instant::now(),
                            )
                            .await;
                    }
                    return Err(error);
                }
            }

            if let Some(client) = self.shared_client.as_ref()
                && !client.endpoint_available(Instant::now()).await
            {
                return Err(SearchAttemptError::Upstream);
            }
            tool_attempt = tool_attempt.saturating_add(1);
            let call = if let Some(client) = self.shared_client.as_ref() {
                client
                    .call_tool(
                        self.tool_name,
                        (self.argument_builder)(query),
                        deadline,
                        tool_attempt,
                    )
                    .await
                    .map_err(map_managed_call_error_for_search)
            } else {
                self.peer
                    .call_tool(self.tool_name, (self.argument_builder)(query), deadline)
                    .await
                    .map_err(ManagedSearchCallError::Peer)
            };
            match call {
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
                Err(ManagedSearchCallError::Peer(McpPeerError::SessionExpired))
                    if !session_retried =>
                {
                    self.clear_compatibility().await;
                    session_retried = true;
                }
                Err(ManagedSearchCallError::Peer(error)) => {
                    if !tool_rediscovered && is_explicit_unknown_tool_rpc(&error) {
                        self.clear_compatibility().await;
                        tool_rediscovered = true;
                        continue;
                    }
                    let mapped = map_peer_error(error);
                    if let Some(client) = self.shared_client.as_ref()
                        && let Some(kind) = search_endpoint_failure_kind(&mapped)
                    {
                        client
                            .record_fetch_error(kind, Instant::now())
                            .await;
                    }
                    return Err(mapped);
                }
                Err(ManagedSearchCallError::QuotaExhausted) => {
                    return Err(SearchAttemptError::QuotaExhausted);
                }
                Err(ManagedSearchCallError::Upstream) => {
                    return Err(SearchAttemptError::Upstream);
                }
                Err(ManagedSearchCallError::UnsafeArguments) => {
                    return Err(SearchAttemptError::InvalidRequest);
                }
                Err(ManagedSearchCallError::RetryLimitExceeded) => {
                    return Err(SearchAttemptError::Upstream);
                }
                Err(ManagedSearchCallError::LedgerFailure) => {
                    return Err(SearchAttemptError::Upstream);
                }
            }
        }
    }

    async fn shutdown(&self, deadline: Instant) -> Result<(), WebError> {
        if self.shared_client.is_none() {
            self.peer.shutdown(deadline).await.map_err(|error| {
                WebError::Provider(format!("managed Parallel search shutdown failed: {error}"))
            })?;
        }
        Ok(())
    }
}

fn map_managed_call_error_for_search(error: ManagedMcpCallError) -> ManagedSearchCallError {
    match error {
        ManagedMcpCallError::Peer(error) => ManagedSearchCallError::Peer(error),
        ManagedMcpCallError::EndpointUnavailable => ManagedSearchCallError::Upstream,
        ManagedMcpCallError::QuotaExhausted => ManagedSearchCallError::QuotaExhausted,
        ManagedMcpCallError::UnsafeArguments => ManagedSearchCallError::UnsafeArguments,
        ManagedMcpCallError::RetryLimitExceeded => ManagedSearchCallError::RetryLimitExceeded,
        ManagedMcpCallError::LedgerFailure => ManagedSearchCallError::LedgerFailure,
    }
}

fn search_endpoint_failure_kind(error: &SearchAttemptError) -> Option<EndpointFailureKind> {
    match error {
        SearchAttemptError::Unauthorized => Some(EndpointFailureKind::Unauthorized),
        SearchAttemptError::Forbidden => Some(EndpointFailureKind::Forbidden),
        SearchAttemptError::RateLimited(retry_after) => {
            Some(EndpointFailureKind::RateLimited(*retry_after))
        }
        SearchAttemptError::Network => Some(EndpointFailureKind::Network),
        SearchAttemptError::MalformedResponse => Some(EndpointFailureKind::MalformedResponse),
        SearchAttemptError::Timeout
        | SearchAttemptError::QueueBusy
        | SearchAttemptError::ToolMissing
        | SearchAttemptError::SchemaMismatch
        | SearchAttemptError::RpcMethodUnavailable
        | SearchAttemptError::SessionExpired
        | SearchAttemptError::InvalidRequest
        | SearchAttemptError::QuotaExhausted
        | SearchAttemptError::Upstream => None,
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

pub(super) fn is_explicit_unknown_tool(result: &nomi_mcp::protocol::McpToolResult) -> bool {
    result.content.iter().any(|content| {
        let nomi_mcp::protocol::McpContent::Text { text } = content else {
            return false;
        };
        is_unknown_tool_message(text, None)
    })
}

pub(super) fn is_explicit_unknown_tool_rpc(error: &McpPeerError) -> bool {
    match error {
        McpPeerError::JsonRpc {
            code: -32602,
            message,
            data,
        } => is_unknown_tool_message(message, data.as_ref()),
        _ => false,
    }
}

pub(super) fn is_unknown_tool_message(message: &str, data: Option<&Value>) -> bool {
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
    use crate::managed::fetch::{FetchReadiness, ParallelFetchAdapter};
    #[cfg(feature = "fetch-eval")]
    use crate::managed::fetch::RemoteExtractProvider;
    use nomi_mcp::protocol::McpToolDef;
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_partial_json, method},
    };

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
        assert!(Arc::strong_count(&client.call_control) >= 1);
    }

    #[test]
    fn test_endpoint_requires_an_explicit_call_control() {
        let client = ParallelMcpClient::new_for_test_endpoint_with_control(
            "http://127.0.0.1:9",
            Arc::new(NoopManagedMcpCallControl),
        )
        .expect("test endpoint construction is offline");
        assert!(Arc::strong_count(&client.call_control) >= 1);
    }

    #[test]
    fn production_fetch_gate_accepts_only_safe_arguments() {
        assert!(safe_fetch_arguments(&json!({
            "urls": ["https://example.com/"],
            "full_content": false
        })));
        assert!(!safe_fetch_arguments(&json!({
            "urls": ["https://example.com/"],
            "full_content": true
        })));
        assert!(!safe_fetch_arguments(&json!({
            "urls": ["https://example.com/"],
            "full_content": false,
            "objective": "leak"
        })));
        assert!(!safe_fetch_arguments(&json!({
            "urls": ["https://example.com/?token=secret"],
            "full_content": false
        })));
        assert!(!safe_fetch_arguments(&json!({
            "urls": ["http://127.0.0.1/"],
            "full_content": false
        })));
    }

    #[test]
    fn parallel_call_policy_is_the_non_bypassable_authorization_seam() {
        let policy = ParallelMcpCallPolicy;
        let authorized = policy
            .authorize(
                "web_fetch",
                json!({
                    "urls": ["https://example.com/"],
                    "full_content": false
                }),
                1,
            )
            .expect("safe fetch is authorized");
        assert_eq!(authorized.tool(), ManagedMcpTool::Fetch);
        assert_eq!(authorized.attempt(), 1);
        assert!(matches!(
            policy.authorize(
                "web_fetch",
                json!({
                    "urls": ["https://example.com/?token=secret"],
                    "full_content": false
                }),
                1,
            ),
            Err(ParallelCallRejection::UnsafeArguments)
        ));
        assert!(matches!(
            policy.authorize(
                "web_fetch",
                json!({
                    "urls": ["https://example.com/"],
                    "full_content": false
                }),
                4,
            ),
            Err(ParallelCallRejection::RetryLimitExceeded)
        ));
    }

    #[test]
    fn production_search_gate_accepts_only_parallel_shape() {
        assert!(safe_search_arguments(&json!({
            "objective": "find docs",
            "search_queries": ["docs"]
        })));
        assert!(!safe_search_arguments(&json!({
            "objective": "find docs",
            "search_queries": ["docs"],
            "session_id": "secret"
        })));
        assert!(!safe_search_arguments(&json!({
            "objective": "",
            "search_queries": ["docs"]
        })));
    }

    #[tokio::test]
    async fn production_gate_blocks_fourth_attempt_before_network() {
        let policy = ParallelMcpCallPolicy;
        let arguments = json!({
            "urls": ["https://example.com/"],
            "full_content": false
        });
        for attempt in 1..=3 {
            policy
                .authorize("web_fetch", arguments.clone(), attempt)
                .expect("first three attempts are allowed");
        }
        assert!(matches!(
            policy.authorize("web_fetch", arguments, 4),
            Err(ParallelCallRejection::RetryLimitExceeded)
        ));
    }

    #[tokio::test]
    async fn production_gate_blocks_sensitive_fetch_before_wiremock_call() {
        let server = MockServer::start().await;
        let client = ParallelMcpClient::new_for_test_endpoint(server.uri(), None)
        .expect("offline construction");
        for url in [
            "https://example.com/?token=secret",
            "https://example.com/?code=oauth-code",
            "https://example.com/?clientSecret=secret",
            "https://localhost./file.pdf",
            "https://foo.localhost/file.pdf",
        ] {
            let result = client
                .call_tool(
                    "web_fetch",
                    json!({
                        "urls": [url],
                        "full_content": false
                    }),
                    Instant::now() + Duration::from_secs(1),
                    1,
                )
                .await;
            assert!(matches!(result, Err(ManagedMcpCallError::UnsafeArguments)), "{url}");
        }
        assert_eq!(server.received_requests().await.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn search_initialization_makes_fetch_readiness_warm_transport() {
        let server = MockServer::start().await;
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
            .and(body_partial_json(json!({"method": "tools/list", "id": 2})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 2,
                "result": {
                    "tools": [{
                        "name": "web_search",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "objective": {"type": "string"},
                                "search_queries": {"type": "array"}
                            },
                            "required": ["objective", "search_queries"]
                        },
                        "outputSchema": {
                            "type": "object",
                            "properties": {"results": {"type": "array"}}
                        }
                    }]
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = Arc::new(
            ParallelMcpClient::new_at_endpoint(server.uri()).expect("offline construction"),
        );
        let search = RemoteSearchAdapter::parallel(client.clone());
        search
            .ensure_compatible(Instant::now() + Duration::from_secs(5))
            .await
            .expect("search tool discovery");
        let fetch = ParallelFetchAdapter::new(client);
        assert_eq!(
            fetch.fetch_readiness().await,
            FetchReadiness::WarmTransportToolUnknown
        );
    }

    #[cfg(feature = "fetch-eval")]
    #[tokio::test]
    async fn real_parallel_search_call_crosses_gate_and_counts_once() {
        let server = MockServer::start().await;
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
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "notifications/initialized"})))
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
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "objective": {"type": "string"},
                                "search_queries": {"type": "array"}
                            },
                            "required": ["objective", "search_queries"]
                        },
                        "outputSchema": {
                            "type": "object",
                            "properties": {"results": {"type": "array"}}
                        }
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
                "id": 3,
                "result": {
                    "structuredContent": {
                        "results": [{
                            "title": "Example",
                            "url": "https://example.com/",
                            "excerpts": ["example result"]
                        }]
                    }
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let quota_path = std::env::temp_dir().join(format!(
            "allo-fetch-eval-search-gate-{}.json",
            uuid::Uuid::now_v7()
        ));
        let gate = Arc::new(crate::evaluation::runner::FileQuotaControl::new(
            quota_path.clone(),
            60,
            10,
        ));
        let client = Arc::new(
            ParallelMcpClient::new_for_test_endpoint(
                server.uri(),
                Some(Arc::clone(&gate) as Arc<dyn ManagedMcpCallControl>),
            )
            .expect("offline construction"),
        );
        let search = RemoteSearchAdapter::parallel(client);
        let result = search
            .search_attempt_with_diagnostics(
                &SearchQuery {
                    query: "managed fetch gate".to_owned(),
                    count: 1,
                },
                Instant::now() + Duration::from_secs(5),
            )
            .await
            .expect("remote search");

        assert_eq!(result.result.hits.len(), 1);
        assert_eq!(gate.actual_calls(), 1);
        assert_eq!(gate.search_calls(), 1);
        assert_eq!(gate.fetch_calls(), 0);
        assert_eq!(gate.recovery_calls(), 0);
        let _ = std::fs::remove_file(quota_path);
    }

    #[cfg(feature = "fetch-eval")]
    #[tokio::test]
    async fn real_search_warmup_and_fetch_share_one_gate_and_peer() {
        let server = MockServer::start().await;
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
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"method": "notifications/initialized"})))
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
                    "tools": [
                        {
                            "name": "web_search",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "objective": {"type": "string"},
                                    "search_queries": {"type": "array"}
                                },
                                "required": ["objective", "search_queries"]
                            },
                            "outputSchema": {
                                "type": "object",
                                "properties": {"results": {"type": "array"}}
                            }
                        },
                        {
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
                        }
                    ]
                }
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({
                "method": "tools/call",
                "params": {"name": "web_search"}
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 3,
                "result": {
                    "structuredContent": {
                        "results": [{
                            "title": "Example",
                            "url": "https://example.com/",
                            "excerpts": ["example result"]
                        }]
                    }
                }
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(body_partial_json(json!({
                "method": "tools/call",
                "params": {"name": "web_fetch"}
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 4,
                "result": {
                    "structuredContent": {
                        "results": [{
                            "url": "https://example.com/",
                            "title": "Example",
                            "full_content": "# Example\n\nManaged fetch gate"
                        }],
                        "errors": []
                    }
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let quota_path = std::env::temp_dir().join(format!(
            "allo-fetch-eval-search-warm-gate-{}.json",
            uuid::Uuid::now_v7()
        ));
        let gate = Arc::new(crate::evaluation::runner::FileQuotaControl::new(
            quota_path.clone(),
            60,
            10,
        ));
        let client = Arc::new(
            ParallelMcpClient::new_for_test_endpoint(
                server.uri(),
                Some(Arc::clone(&gate) as Arc<dyn ManagedMcpCallControl>),
            )
            .expect("offline construction"),
        );
        let search = RemoteSearchAdapter::parallel(Arc::clone(&client));
        search
            .search_attempt_with_diagnostics(
                &SearchQuery {
                    query: "managed fetch gate".to_owned(),
                    count: 1,
                },
                Instant::now() + Duration::from_secs(5),
            )
            .await
            .expect("search warmup");
        let fetch = ParallelFetchAdapter::new(client);
        let request = crate::managed::fetch::RemoteExtractRequest {
            items: vec![
                crate::managed::fetch::RemoteExtractRequestItem::new(
                    "https://example.com/".to_owned(),
                    false,
                )
                .expect("public fetch request"),
            ],
        };
        let batch = fetch
            .extract_batch(request, Instant::now() + Duration::from_secs(5))
            .await
            .expect("fetch after search warmup");
        assert_eq!(batch.items.len(), 1);
        assert_eq!(gate.actual_calls(), 2);
        assert_eq!(gate.search_calls(), 1);
        assert_eq!(gate.fetch_calls(), 1);
        assert_eq!(gate.recovery_calls(), 0);
        let _ = std::fs::remove_file(quota_path);
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

    #[tokio::test]
    async fn fetch_endpoint_health_does_not_let_stale_success_clear_unauthorized() {
        let client = ParallelMcpClient::new().expect("offline construction");
        let now = Instant::now();
        assert!(client.endpoint_available(now).await);
        let token = client
            .begin_endpoint_attempt(now)
            .await
            .expect("attempt token");
        client
            .record_fetch_error(EndpointFailureKind::Unauthorized, now)
            .await;
        assert!(!client.endpoint_available(now).await);
        client.record_endpoint_success_for(token).await;
        assert!(!client.endpoint_available(now).await);
        client.record_endpoint_reinitialized().await;
        assert!(client.endpoint_available(now).await);
    }

    #[tokio::test]
    async fn fetch_endpoint_health_bounds_rate_limit_cooldown() {
        let client = ParallelMcpClient::new().expect("offline construction");
        let now = Instant::now();
        let stale_token = client
            .begin_endpoint_attempt(now)
            .await
            .expect("stale attempt token");
        client
            .record_fetch_error(
                EndpointFailureKind::RateLimited(Some(Duration::from_secs(5))),
                now,
            )
            .await;
        assert!(!client.endpoint_available(now).await);
        client.record_endpoint_success_for(stale_token).await;
        assert!(!client.endpoint_available(now).await);
        let later = now + Duration::from_secs(30);
        assert!(client.endpoint_available(later).await);
        let fresh_token = client
            .begin_endpoint_attempt(later)
            .await
            .expect("fresh attempt token");
        client.record_endpoint_success_for(fresh_token).await;
        assert!(client.endpoint_available(later).await);
    }

    #[tokio::test]
    async fn fetch_tool_health_is_independent_of_endpoint_health() {
        let client = ParallelMcpClient::new().expect("offline construction");
        let now = Instant::now();
        client
            .record_fetch_error(FetchToolFailureKind::Upstream, now)
            .await;
        assert!(client.endpoint_available(now).await);
        assert!(!client.fetch_tool_available(now).await);
        client.record_fetch_tool_success().await;
        assert!(client.fetch_tool_available(now).await);
    }

    #[test]
    fn search_endpoint_health_excludes_tool_local_failures() {
        assert!(matches!(
            search_endpoint_failure_kind(&SearchAttemptError::MalformedResponse),
            Some(EndpointFailureKind::MalformedResponse)
        ));
        assert_eq!(
            search_endpoint_failure_kind(&SearchAttemptError::ToolMissing),
            None
        );
        assert_eq!(
            search_endpoint_failure_kind(&SearchAttemptError::Timeout),
            None
        );
    }
}
