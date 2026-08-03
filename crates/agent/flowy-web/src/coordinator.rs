use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant as StdInstant};

use async_trait::async_trait;
use futures_util::stream::{FuturesUnordered, StreamExt};
use tokio::time::{Instant, timeout_at};

use crate::managed::fetch::{
    FetchReadiness, RemoteExtractError, RemoteExtractFallback, RemoteExtractItem,
    RemoteExtractRequest, RemoteExtractRequestItem, RemoteTimeoutKind,
};
use crate::provider::extract_policy::{
    LocalExtractDiagnostics, LocalExtractFailure, LocalExtractFailureKind, LocalExtractOutcome,
    RemoteFallbackDecision, RemoteFallbackReason, RemoteForbiddenReason,
    canonical_requested_url, classify_web_error, decide_remote_fallback_with_private,
};
use crate::provider::{ExtractProvider, HttpExtractProvider};
use crate::types::{
    EXTRACT_CHAR_LIMIT, ExtractRequest, ExtractedPage, MAX_EXTRACT_URLS, WebError,
};

const MAX_LOCAL_CONCURRENCY: usize = 2;

#[derive(Debug, Clone)]
pub struct ExtractBudget {
    pub absolute_deadline: Instant,
    pub local_per_url_timeout: Duration,
}

#[derive(Debug)]
pub struct ExtractBatchOutcome {
    pub items: Vec<ExtractItemOutcome>,
    pub diagnostics: ExtractBatchDiagnostics,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExtractTimeoutCounts {
    pub per_url_deadline: usize,
    pub tool_deadline_before_start: usize,
    pub tool_deadline_while_running: usize,
    pub remote_queue_deadline: usize,
    pub remote_call_deadline: usize,
}

impl ExtractTimeoutCounts {
    fn record(&mut self, kind: ExtractTimeoutKind) {
        match kind {
            ExtractTimeoutKind::PerUrlDeadline => self.per_url_deadline += 1,
            ExtractTimeoutKind::ToolDeadlineBeforeStart => {
                self.tool_deadline_before_start += 1;
            }
            ExtractTimeoutKind::ToolDeadlineWhileRunning => {
                self.tool_deadline_while_running += 1;
            }
            ExtractTimeoutKind::RemoteQueueDeadline => self.remote_queue_deadline += 1,
            ExtractTimeoutKind::RemoteCallDeadline => self.remote_call_deadline += 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExtractTimeoutKind {
    PerUrlDeadline,
    ToolDeadlineBeforeStart,
    ToolDeadlineWhileRunning,
    RemoteQueueDeadline,
    RemoteCallDeadline,
}

#[derive(Debug, Clone)]
pub struct ExtractItemOutcome {
    pub index: usize,
    pub requested_url: String,
    pub page: Option<ExtractedPage>,
    pub local_failure: Option<LocalExtractFailure>,
    pub final_error: Option<WebError>,
}

#[derive(Debug, Clone, Default)]
pub struct ExtractBatchDiagnostics {
    pub requested_count: usize,
    pub success_count: usize,
    pub failure_count: usize,
    pub local_success_count: usize,
    pub local_failure_count: usize,
    pub final_success_count: usize,
    pub final_failure_count: usize,
    pub remote_eligible_count: usize,
    pub remote_stage_count: usize,
    pub remote_attempted: bool,
    pub remote_success_count: usize,
    pub remote_failure_count: usize,
    pub remote_forbidden_count: usize,
    pub remote_budget_skipped_count: usize,
    pub remote_queue_ms: Option<u128>,
    pub remote_call_ms: Option<u128>,
    pub remote_unmatched_count: usize,
    pub remote_dropped_count: usize,
    pub total_elapsed_ms: u128,
    pub timeout_counts: ExtractTimeoutCounts,
    pub fallback_reason_counts: BTreeMap<RemoteFallbackReason, usize>,
    pub remote_forbidden_reason_counts: BTreeMap<RemoteForbiddenReason, usize>,
    pub source_truncated_count: usize,
    pub context_truncated_count: usize,
}

#[async_trait]
pub trait ExtractCoordinator: Send + Sync {
    async fn extract_many(
        &self,
        requests: Vec<ExtractRequest>,
        budget: ExtractBudget,
    ) -> ExtractBatchOutcome;
}

/// Local extraction seam used by the managed coordinator.
///
/// The production adapter is [`HttpExtractProvider`]. Evaluation and tests can
/// inject a deterministic adapter without changing the remote fallback
/// contract or pretending that an injected failure is provider evidence.
#[async_trait]
pub(crate) trait LocalExtractAdapter: Send + Sync {
    async fn extract_with_metadata(&self, req: ExtractRequest) -> LocalExtractOutcome;
}

#[async_trait]
impl LocalExtractAdapter for HttpExtractProvider {
    async fn extract_with_metadata(&self, req: ExtractRequest) -> LocalExtractOutcome {
        HttpExtractProvider::extract_with_metadata(self, req).await
    }
}

pub struct LocalExtractCoordinator {
    provider: Arc<dyn ExtractProvider>,
    max_concurrency: usize,
}

impl LocalExtractCoordinator {
    pub fn new(provider: Arc<dyn ExtractProvider>) -> Self {
        Self {
            provider,
            max_concurrency: MAX_LOCAL_CONCURRENCY,
        }
    }

    pub fn with_max_concurrency(mut self, max_concurrency: usize) -> Self {
        self.max_concurrency = max_concurrency.max(1);
        self
    }
}

#[async_trait]
impl ExtractCoordinator for LocalExtractCoordinator {
    async fn extract_many(
        &self,
        requests: Vec<ExtractRequest>,
        budget: ExtractBudget,
    ) -> ExtractBatchOutcome {
        let started_at = StdInstant::now();
        let requested_count = requests.len();
        let labels = requests
            .iter()
            .map(|request| request.url.clone())
            .collect::<Vec<_>>();
        let mut outcomes: Vec<Option<ExtractItemOutcome>> =
            (0..requested_count).map(|_| None).collect();
        let mut started = vec![false; requested_count];
        let mut timeout_counts = ExtractTimeoutCounts::default();
        let mut in_flight = FuturesUnordered::new();
        let mut next_index = 0usize;

        while next_index < requested_count && in_flight.len() < self.max_concurrency {
            started[next_index] = true;
            in_flight.push(run_local_extract(
                Arc::clone(&self.provider),
                next_index,
                requests[next_index].url.clone(),
                budget.clone(),
            ));
            next_index += 1;
        }

        while !in_flight.is_empty() {
            let completed = timeout_at(budget.absolute_deadline, in_flight.next()).await;
            let Some(outcome) = (match completed {
                Ok(outcome) => outcome,
                Err(_) => break,
            }) else {
                break;
            };
            let index = outcome.outcome.index;
            if let Some(kind) = outcome.timeout_kind {
                timeout_counts.record(kind);
            }
            outcomes[index] = Some(outcome.outcome);

            if next_index < requested_count {
                started[next_index] = true;
                in_flight.push(run_local_extract(
                    Arc::clone(&self.provider),
                    next_index,
                    requests[next_index].url.clone(),
                    budget.clone(),
                ));
                next_index += 1;
            }
        }

        for (index, outcome) in outcomes.iter_mut().enumerate() {
            if outcome.is_some() {
                continue;
            }
            let timeout_kind = if started[index] {
                ExtractTimeoutKind::ToolDeadlineWhileRunning
            } else {
                ExtractTimeoutKind::ToolDeadlineBeforeStart
            };
            timeout_counts.record(timeout_kind);
            let error = WebError::Timeout(
                "Page extraction did not complete before the tool deadline.".to_owned(),
            );
            *outcome = Some(ExtractItemOutcome {
                index,
                requested_url: labels[index].clone(),
                page: None,
                local_failure: Some(LocalExtractFailure {
                    kind: LocalExtractFailureKind::Timeout,
                    error: error.clone(),
                }),
                final_error: Some(error),
            });
        }

        let items = outcomes
            .into_iter()
            .map(|outcome| outcome.expect("every extract request has an outcome"))
            .collect::<Vec<_>>();
        let mut success_count = 0usize;
        let mut failure_count = 0usize;
        for item in &items {
            if item.page.is_some() {
                success_count += 1;
            } else {
                failure_count += 1;
            }
        }
        let source_truncated_count = 0usize;
        let mut context_truncated_count = 0usize;
        for item in &items {
            if item.page.as_ref().is_some_and(|page| page.truncated) {
                context_truncated_count += 1;
            }
        }
        ExtractBatchOutcome {
            diagnostics: ExtractBatchDiagnostics {
                requested_count,
                success_count,
                failure_count,
                local_success_count: success_count,
                local_failure_count: failure_count,
                final_success_count: success_count,
                final_failure_count: failure_count,
                remote_eligible_count: 0,
                remote_stage_count: 0,
                remote_attempted: false,
                remote_success_count: 0,
                remote_failure_count: 0,
                remote_forbidden_count: 0,
                remote_budget_skipped_count: 0,
                remote_queue_ms: None,
                remote_call_ms: None,
                remote_unmatched_count: 0,
                remote_dropped_count: 0,
                total_elapsed_ms: started_at.elapsed().as_millis(),
                timeout_counts,
                fallback_reason_counts: BTreeMap::new(),
                remote_forbidden_reason_counts: BTreeMap::new(),
                source_truncated_count,
                context_truncated_count,
            },
            items,
        }
    }
}

struct LocalExtractItemOutcome {
    outcome: ExtractItemOutcome,
    timeout_kind: Option<ExtractTimeoutKind>,
}

#[derive(Debug, Clone, Copy)]
pub struct RemoteBudgetPolicy {
    pub warm_fetch_budget: Duration,
    pub cold_fetch_budget: Duration,
    pub safety_margin: Duration,
}

impl Default for RemoteBudgetPolicy {
    fn default() -> Self {
        Self {
            warm_fetch_budget: Duration::from_secs(6),
            cold_fetch_budget: Duration::from_secs(10),
            safety_margin: Duration::from_secs(1),
        }
    }
}

impl RemoteBudgetPolicy {
    fn allows(&self, readiness: FetchReadiness, remaining: Duration) -> bool {
        let required = match readiness {
            FetchReadiness::ColdTransport | FetchReadiness::WarmTransportToolUnknown => {
                self.cold_fetch_budget
            }
            FetchReadiness::Ready { .. } => self.warm_fetch_budget,
        };
        remaining >= required + self.safety_margin
    }
}

struct EligibleItem {
    index: usize,
    requested_url: String,
    local_failure: LocalExtractFailure,
}

pub struct ManagedExtractCoordinator {
    local: Arc<dyn LocalExtractAdapter>,
    remote: Arc<dyn RemoteExtractFallback>,
    budget_policy: RemoteBudgetPolicy,
    allow_private_for_tests: bool,
}

impl ManagedExtractCoordinator {
    pub fn new(
        local: Arc<HttpExtractProvider>,
        remote: Arc<dyn RemoteExtractFallback>,
    ) -> Self {
        Self::from_local_adapter(local, remote)
    }

    pub(crate) fn from_local_adapter(
        local: Arc<dyn LocalExtractAdapter>,
        remote: Arc<dyn RemoteExtractFallback>,
    ) -> Self {
        Self {
            local,
            remote,
            budget_policy: RemoteBudgetPolicy::default(),
            allow_private_for_tests: false,
        }
    }

    pub fn with_remote_budget(mut self, budget_policy: RemoteBudgetPolicy) -> Self {
        self.budget_policy = budget_policy;
        self
    }

    #[cfg(test)]
    pub fn allow_private_for_tests(mut self) -> Self {
        self.allow_private_for_tests = true;
        self
    }
}

#[async_trait]
impl ExtractCoordinator for ManagedExtractCoordinator {
    async fn extract_many(
        &self,
        requests: Vec<ExtractRequest>,
        budget: ExtractBudget,
    ) -> ExtractBatchOutcome {
        let started_at = StdInstant::now();
        let requested_count = requests.len();
        let local_count = requested_count.min(MAX_EXTRACT_URLS);
        let labels = requests
            .iter()
            .map(|request| request.url.clone())
            .collect::<Vec<_>>();
        let mut local_outcomes: Vec<Option<LocalExtractOutcome>> =
            (0..local_count).map(|_| None).collect();
        let mut item_outcomes: Vec<Option<ExtractItemOutcome>> =
            (0..requested_count).map(|_| None).collect();
        for index in MAX_EXTRACT_URLS..requested_count {
            let error = WebError::InvalidArgument(format!(
                "urls length {requested_count} exceeds max {MAX_EXTRACT_URLS}; choose the 1-3 most relevant URLs"
            ));
            item_outcomes[index] = Some(ExtractItemOutcome {
                index,
                requested_url: labels[index].clone(),
                page: None,
                local_failure: Some(LocalExtractFailure {
                    kind: LocalExtractFailureKind::InvalidUrl,
                    error: error.clone(),
                }),
                final_error: Some(error),
            });
        }
        let mut eligible = Vec::new();
        let mut remote_forbidden_count = 0usize;
        let mut remote_budget_skipped_count = 0usize;
        let mut local_success_count = 0usize;
        let mut local_failure_count = 0usize;
        let mut timeout_counts = ExtractTimeoutCounts::default();
        let mut fallback_reason_counts = BTreeMap::new();
        let mut remote_forbidden_reason_counts = BTreeMap::new();
        let mut source_truncated_count = 0usize;
        let mut context_truncated_count = 0usize;
        let mut started = vec![false; local_count];
        let mut in_flight = FuturesUnordered::new();
        let mut next_index = 0usize;

        while next_index < local_count && in_flight.len() < MAX_LOCAL_CONCURRENCY {
            started[next_index] = true;
            in_flight.push(run_managed_local(
                Arc::clone(&self.local),
                next_index,
                requests[next_index].clone(),
                budget.clone(),
            ));
            next_index += 1;
        }

        while !in_flight.is_empty() {
            let completed = timeout_at(budget.absolute_deadline, in_flight.next()).await;
            let Some(outcome) = (match completed {
                Ok(outcome) => outcome,
                Err(_) => break,
            }) else {
                break;
            };
            let index = outcome.index;
            if let Some(kind) = outcome.timeout_kind {
                timeout_counts.record(kind);
            }
            local_outcomes[index] = Some(outcome.outcome);

            if next_index < local_count {
                started[next_index] = true;
                in_flight.push(run_managed_local(
                    Arc::clone(&self.local),
                    next_index,
                    requests[next_index].clone(),
                    budget.clone(),
                ));
                next_index += 1;
            }
        }

        for (index, slot) in local_outcomes.iter_mut().enumerate() {
            let timeout_kind = if slot.is_none() {
                Some(if started[index] {
                    ExtractTimeoutKind::ToolDeadlineWhileRunning
                } else {
                    ExtractTimeoutKind::ToolDeadlineBeforeStart
                })
            } else {
                None
            };
            let outcome = slot.take().unwrap_or_else(|| {
                let error = WebError::Timeout(
                    "Page extraction did not complete before the tool deadline.".to_owned(),
                );
                LocalExtractOutcome {
                    requested_url: labels[index].clone(),
                    result: Err(LocalExtractFailure {
                        kind: LocalExtractFailureKind::Timeout,
                        error: error.clone(),
                    }),
                    diagnostics: LocalExtractDiagnostics::default(),
                }
            });
            if let Some(kind) = timeout_kind {
                timeout_counts.record(kind);
            }
            match &outcome.result {
                Ok(page) => {
                    local_success_count += 1;
                    if page.truncated {
                        context_truncated_count += 1;
                    }
                    item_outcomes[index] = Some(ExtractItemOutcome {
                        index,
                        requested_url: outcome.requested_url.clone(),
                        page: Some(page.clone()),
                        local_failure: None,
                        final_error: None,
                    });
                }
                Err(local_failure) => {
                    local_failure_count += 1;
                    let decision =
                        decide_remote_fallback_with_private(&outcome, self.allow_private_for_tests);
                    let local_failure = local_failure.clone();
                    match decision {
                        RemoteFallbackDecision::Eligible { reason } => {
                            record_fallback_reason(&mut fallback_reason_counts, reason);
                            eligible.push(EligibleItem {
                                index,
                                requested_url: outcome.requested_url.clone(),
                                local_failure,
                            });
                        }
                        RemoteFallbackDecision::Forbidden { reason } => {
                            record_forbidden_reason(&mut remote_forbidden_reason_counts, reason);
                            remote_forbidden_count += 1;
                            item_outcomes[index] = Some(ExtractItemOutcome {
                                index,
                                requested_url: outcome.requested_url.clone(),
                                page: None,
                                local_failure: Some(local_failure.clone()),
                                final_error: Some(local_failure.error),
                            });
                        }
                        RemoteFallbackDecision::NotNeeded => {}
                        RemoteFallbackDecision::BudgetInsufficient { reason } => {
                            record_fallback_reason(&mut fallback_reason_counts, reason);
                            item_outcomes[index] = Some(ExtractItemOutcome {
                                index,
                                requested_url: outcome.requested_url.clone(),
                                page: None,
                                local_failure: Some(local_failure.clone()),
                                final_error: Some(local_failure.error),
                            });
                        }
                    }
                }
            }
        }

        let remote_eligible_count = eligible.len();
        let mut remote_stage_count = 0usize;
        let mut remote_attempted = false;
        let mut remote_success_count = 0usize;
        let mut remote_failure_count = 0usize;
        let mut remote_queue_ms = None;
        let mut remote_call_ms = None;
        let mut remote_unmatched_count = 0usize;
        let mut remote_dropped_count = 0usize;
        if !eligible.is_empty() {
            let readiness = self.remote.fetch_readiness().await;
            let remaining = budget
                .absolute_deadline
                .checked_duration_since(Instant::now())
                .unwrap_or_default();
            if self.budget_policy.allows(readiness, remaining) {
                remote_stage_count = 1;
                remote_attempted = true;
                let request = RemoteExtractRequest {
                    items: dedupe_eligible(&eligible, self.allow_private_for_tests),
                };
                match self.remote.extract_batch(request, budget.absolute_deadline).await {
                    Ok(remote_batch) => {
                        remote_queue_ms = remote_batch.diagnostics.queue_ms;
                        remote_call_ms = remote_batch.diagnostics.call_ms;
                        remote_unmatched_count = remote_batch.diagnostics.unmatched_item_count;
                        remote_dropped_count = remote_batch.diagnostics.dropped_item_count;
                        let remote_by_url = remote_batch
                            .items
                            .iter()
                            .map(|item| (canonical_requested_url(&item.requested_url), item))
                            .collect::<HashMap<_, _>>();
                        for item in eligible {
                            let remote_item = remote_by_url
                                .get(&canonical_requested_url(&item.requested_url))
                                .copied();
                            let Some(remote_item) = remote_item else {
                                item_outcomes[item.index] = Some(ExtractItemOutcome {
                                    index: item.index,
                                    requested_url: item.requested_url.clone(),
                                    page: None,
                                    local_failure: Some(item.local_failure.clone()),
                                    final_error: Some(item.local_failure.error),
                                });
                                remote_failure_count += 1;
                                continue;
                            };
                            let page = remote_page(&item, remote_item);
                            if remote_item.source_truncated {
                                source_truncated_count += 1;
                            }
                            if remote_item.markdown.chars().count() > EXTRACT_CHAR_LIMIT {
                                context_truncated_count += 1;
                            }
                            item_outcomes[item.index] = Some(ExtractItemOutcome {
                                index: item.index,
                                requested_url: item.requested_url.clone(),
                                page: Some(page),
                                local_failure: Some(item.local_failure),
                                final_error: None,
                            });
                            remote_success_count += 1;
                        }
                    }
                    Err(error) => {
                        if let RemoteExtractError::Timeout { kind } = error {
                            timeout_counts.record(match kind {
                                RemoteTimeoutKind::QueueDeadline => {
                                    ExtractTimeoutKind::RemoteQueueDeadline
                                }
                                RemoteTimeoutKind::CallDeadline => {
                                    ExtractTimeoutKind::RemoteCallDeadline
                                }
                            });
                        }
                        remote_failure_count = eligible.len();
                        for item in eligible {
                            item_outcomes[item.index] = Some(ExtractItemOutcome {
                                index: item.index,
                                requested_url: item.requested_url.clone(),
                                page: None,
                                local_failure: Some(item.local_failure.clone()),
                                final_error: Some(item.local_failure.error),
                            });
                        }
                    }
                }
            } else {
                remote_budget_skipped_count = eligible.len();
                for item in eligible {
                    item_outcomes[item.index] = Some(ExtractItemOutcome {
                        index: item.index,
                        requested_url: item.requested_url.clone(),
                        page: None,
                        local_failure: Some(item.local_failure.clone()),
                        final_error: Some(item.local_failure.error),
                    });
                }
            }
        }

        let items = item_outcomes
            .into_iter()
            .map(|outcome| outcome.expect("every extract request has an outcome"))
            .collect::<Vec<_>>();
        let mut success_count = 0usize;
        let mut failure_count = 0usize;
        for item in &items {
            if item.page.is_some() {
                success_count += 1;
            } else {
                failure_count += 1;
            }
        }
        let final_success_count = success_count;
        let final_failure_count = failure_count;
        tracing::info!(
            target: "flowy_web::managed_extract",
            requested_count,
            local_success_count,
            local_failure_count,
            final_success_count,
            final_failure_count,
            remote_eligible_count,
            remote_forbidden_count,
            remote_budget_skipped_count,
            remote_attempted,
            remote_success_count,
            remote_failure_count,
            remote_queue_ms = remote_queue_ms.unwrap_or(0),
            remote_call_ms = remote_call_ms.unwrap_or(0),
            total_elapsed_ms = started_at.elapsed().as_millis(),
            per_url_deadline = timeout_counts.per_url_deadline,
            tool_deadline_before_start = timeout_counts.tool_deadline_before_start,
            tool_deadline_while_running = timeout_counts.tool_deadline_while_running,
            remote_queue_deadline = timeout_counts.remote_queue_deadline,
            remote_call_deadline = timeout_counts.remote_call_deadline,
            fallback_reason_counts = ?fallback_reason_counts,
            remote_forbidden_reason_counts = ?remote_forbidden_reason_counts,
            source_truncated_count,
            context_truncated_count,
            "managed extract completed"
        );
        ExtractBatchOutcome {
            diagnostics: ExtractBatchDiagnostics {
                requested_count,
                success_count,
                failure_count,
                local_success_count,
                local_failure_count,
                final_success_count,
                final_failure_count,
                remote_eligible_count,
                remote_stage_count,
                remote_attempted,
                remote_success_count,
                remote_failure_count,
                remote_forbidden_count,
                remote_budget_skipped_count,
                remote_queue_ms,
                remote_call_ms,
                remote_unmatched_count,
                remote_dropped_count,
                total_elapsed_ms: started_at.elapsed().as_millis(),
                timeout_counts,
                fallback_reason_counts,
                remote_forbidden_reason_counts,
                source_truncated_count,
                context_truncated_count,
            },
            items,
        }
    }
}

async fn run_managed_local(
    provider: Arc<dyn LocalExtractAdapter>,
    index: usize,
    request: ExtractRequest,
    budget: ExtractBudget,
) -> ManagedLocalOutcome {
    if let Some(invalid_index) = invalid_url_index(&request.url) {
        let error = WebError::InvalidArgument(format!(
            "urls[{invalid_index}] must be a string"
        ));
        return ManagedLocalOutcome {
            index,
            outcome: LocalExtractOutcome {
                requested_url: request.url,
                result: Err(LocalExtractFailure {
                    kind: LocalExtractFailureKind::InvalidUrl,
                    error: error.clone(),
                }),
                diagnostics: LocalExtractDiagnostics::default(),
            },
            timeout_kind: None,
        };
    }
    let requested_url = request.url.clone();
    let per_url_deadline = Instant::now() + budget.local_per_url_timeout;
    let deadline = std::cmp::min(per_url_deadline, budget.absolute_deadline);
    let result = timeout_at(deadline, provider.extract_with_metadata(request)).await;
    match result {
        Ok(outcome) => ManagedLocalOutcome {
            index,
            outcome,
            timeout_kind: None,
        },
        Err(_) => {
            let (timeout_kind, message) = if deadline < per_url_deadline {
                (
                    ExtractTimeoutKind::ToolDeadlineWhileRunning,
                    "Page extraction did not complete before the tool deadline.",
                )
            } else {
                (
                    ExtractTimeoutKind::PerUrlDeadline,
                    "Page extraction timed out.",
                )
            };
            let error = WebError::Timeout(message.to_owned());
            ManagedLocalOutcome {
                index,
                outcome: LocalExtractOutcome {
                    requested_url,
                    result: Err(LocalExtractFailure {
                        kind: LocalExtractFailureKind::Timeout,
                        error: error.clone(),
                    }),
                    diagnostics: LocalExtractDiagnostics::default(),
                },
                timeout_kind: Some(timeout_kind),
            }
        }
    }
}

struct ManagedLocalOutcome {
    index: usize,
    outcome: LocalExtractOutcome,
    timeout_kind: Option<ExtractTimeoutKind>,
}

fn record_fallback_reason(
    counts: &mut BTreeMap<RemoteFallbackReason, usize>,
    reason: RemoteFallbackReason,
) {
    *counts.entry(reason).or_default() += 1;
}

fn record_forbidden_reason(
    counts: &mut BTreeMap<RemoteForbiddenReason, usize>,
    reason: RemoteForbiddenReason,
) {
    *counts.entry(reason).or_default() += 1;
}

fn dedupe_eligible(
    items: &[EligibleItem],
    allow_private: bool,
) -> Vec<RemoteExtractRequestItem> {
    let mut seen = HashSet::new();
    items
        .iter()
        .filter(|item| seen.insert(canonical_requested_url(&item.requested_url)))
        .filter_map(|item| {
            RemoteExtractRequestItem::new(item.index, item.requested_url.clone(), allow_private)
                .ok()
        })
        .collect()
}

fn remote_page(item: &EligibleItem, remote_item: &RemoteExtractItem) -> ExtractedPage {
    ExtractedPage {
        url: item.requested_url.clone(),
        title: remote_item.title.clone(),
        markdown: remote_item.markdown.clone(),
        truncated: remote_item.source_truncated
            || remote_item.markdown.chars().count() > EXTRACT_CHAR_LIMIT,
        provider: "managed".to_owned(),
        extractor: "remote".to_owned(),
    }
}

async fn run_local_extract(
    provider: Arc<dyn ExtractProvider>,
    index: usize,
    url: String,
    budget: ExtractBudget,
) -> LocalExtractItemOutcome {
    if let Some(invalid_index) = invalid_url_index(&url) {
        let error = WebError::InvalidArgument(format!(
            "urls[{invalid_index}] must be a string"
        ));
        return LocalExtractItemOutcome {
            outcome: ExtractItemOutcome {
                index,
                requested_url: url,
                page: None,
                local_failure: Some(LocalExtractFailure {
                    kind: LocalExtractFailureKind::InvalidUrl,
                    error: error.clone(),
                }),
                final_error: Some(error),
            },
            timeout_kind: None,
        };
    }

    let per_url_deadline = Instant::now() + budget.local_per_url_timeout;
    let url_deadline = std::cmp::min(per_url_deadline, budget.absolute_deadline);
    match timeout_at(
        url_deadline,
        provider.extract(ExtractRequest {
            url: url.clone(),
        }),
    )
    .await
    {
        Ok(Ok(page)) => LocalExtractItemOutcome {
            outcome: ExtractItemOutcome {
                index,
                requested_url: url,
                page: Some(page),
                local_failure: None,
                final_error: None,
            },
            timeout_kind: None,
        },
        Ok(Err(error)) => {
            let kind = classify_web_error(&error);
            LocalExtractItemOutcome {
                outcome: ExtractItemOutcome {
                    index,
                    requested_url: url,
                    page: None,
                    local_failure: Some(LocalExtractFailure {
                        kind,
                        error: error.clone(),
                    }),
                    final_error: Some(error),
                },
                timeout_kind: None,
            }
        }
        Err(_) => {
            let (timeout_kind, message) = if url_deadline < per_url_deadline {
                (
                    ExtractTimeoutKind::ToolDeadlineWhileRunning,
                    "Page extraction did not complete before the tool deadline.",
                )
            } else {
                (
                    ExtractTimeoutKind::PerUrlDeadline,
                    "Page extraction timed out.",
                )
            };
            let error = WebError::Timeout(message.to_owned());
            LocalExtractItemOutcome {
                outcome: ExtractItemOutcome {
                    index,
                    requested_url: url,
                    page: None,
                    local_failure: Some(LocalExtractFailure {
                        kind: LocalExtractFailureKind::Timeout,
                        error: error.clone(),
                    }),
                    final_error: Some(error),
                },
                timeout_kind: Some(timeout_kind),
            }
        }
    }
}

fn invalid_url_index(url: &str) -> Option<usize> {
    const PREFIX: &str = "(invalid urls[";
    let start = url.find(PREFIX)?;
    let rest = &url[start + PREFIX.len()..];
    let end = rest.find(']')?;
    rest[..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::time::Duration;

    use async_trait::async_trait;
    use tokio::sync::Mutex;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    use crate::managed::fetch::{
        RemoteExtractBatch, RemoteExtractError, RemoteExtractFallback, RemoteExtractItem,
        RemoteExtractRequest, RemoteTimeoutKind,
    };
    use crate::provider::ExtractProvider;
    use crate::provider::HttpExtractProvider;
    use crate::types::{
        EXTRACT_CHAR_LIMIT, EXTRACTOR_READABILITY, ExtractRequest, ExtractedPage, WebError,
    };

    use super::{
        ExtractBudget, ExtractCoordinator, LocalExtractCoordinator, ManagedExtractCoordinator,
        RemoteBudgetPolicy, invalid_url_index,
    };
    use crate::provider::extract_policy::{RemoteFallbackReason, RemoteForbiddenReason};

    struct MockProvider {
        fail_urls: Vec<String>,
    }

    #[async_trait]
    impl ExtractProvider for MockProvider {
        fn name(&self) -> &str {
            "mock"
        }

        async fn extract(&self, req: ExtractRequest) -> Result<ExtractedPage, WebError> {
            if self.fail_urls.iter().any(|url| url == &req.url) {
                return Err(WebError::Provider(format!("failed: {}", req.url)));
            }
            Ok(ExtractedPage {
                url: req.url.clone(),
                title: Some("Title".to_owned()),
                markdown: format!("Body for {}", req.url),
                truncated: false,
                provider: "mock".to_owned(),
                extractor: EXTRACTOR_READABILITY.to_owned(),
            })
        }
    }

    struct TimedProvider {
        delay: Duration,
        active: AtomicUsize,
        max_active: AtomicUsize,
        calls: AtomicUsize,
    }

    struct FakeRemote {
        calls: Arc<AtomicUsize>,
        request_items: Mutex<Vec<crate::managed::fetch::RemoteExtractRequestItem>>,
        fail: bool,
        unmapped: bool,
        timeout: Option<RemoteTimeoutKind>,
        source_truncated: bool,
        long_markdown: bool,
    }

    #[async_trait]
    impl RemoteExtractFallback for FakeRemote {
        async fn extract_batch(
            &self,
            request: RemoteExtractRequest,
            _deadline: tokio::time::Instant,
        ) -> Result<RemoteExtractBatch, RemoteExtractError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.request_items.lock().await = request.items.clone();
            if self.fail {
                return Err(RemoteExtractError::Upstream);
            }
            if let Some(kind) = self.timeout {
                return Err(RemoteExtractError::Timeout { kind });
            }
            let items = request
                .items
                .iter()
                .map(|item| {
                    let requested_url = if self.unmapped {
                        "https://example.com/unmapped".to_owned()
                    } else {
                        item.prepared.requested_url.clone()
                    };
                    RemoteExtractItem {
                        index: item.index,
                        requested_url,
                        final_url: None,
                        title: Some("Remote".to_owned()),
                        markdown: if self.long_markdown {
                            "x".repeat(EXTRACT_CHAR_LIMIT + 1)
                        } else {
                            "remote content".to_owned()
                        },
                        source_truncated: self.source_truncated,
                    }
                })
                .collect();
            Ok(RemoteExtractBatch {
                items,
                diagnostics: Default::default(),
            })
        }
    }

    impl FakeRemote {
        fn new(fail: bool) -> Self {
            Self {
                calls: Arc::new(AtomicUsize::new(0)),
                request_items: Mutex::new(Vec::new()),
                fail,
                unmapped: false,
                timeout: None,
                source_truncated: false,
                long_markdown: false,
            }
        }

        fn unmapped(mut self) -> Self {
            self.unmapped = true;
            self
        }

        fn timeout(mut self, kind: RemoteTimeoutKind) -> Self {
            self.timeout = Some(kind);
            self
        }

        fn source_truncated(mut self) -> Self {
            self.source_truncated = true;
            self
        }

        fn long_markdown(mut self) -> Self {
            self.long_markdown = true;
            self
        }
    }

    #[async_trait]
    impl ExtractProvider for TimedProvider {
        fn name(&self) -> &str {
            "timed"
        }

        async fn extract(&self, _req: ExtractRequest) -> Result<ExtractedPage, WebError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active.fetch_max(active, Ordering::SeqCst);
            tokio::time::sleep(self.delay).await;
            self.active.fetch_sub(1, Ordering::SeqCst);
            Ok(ExtractedPage {
                url: "https://example.com/".to_owned(),
                title: Some("Timed".to_owned()),
                markdown: "completed".to_owned(),
                truncated: false,
                provider: "timed".to_owned(),
                extractor: EXTRACTOR_READABILITY.to_owned(),
            })
        }
    }

    fn request(url: &str) -> ExtractRequest {
        ExtractRequest { url: url.to_owned() }
    }

    fn budget() -> ExtractBudget {
        ExtractBudget {
            absolute_deadline: tokio::time::Instant::now() + Duration::from_secs(10),
            local_per_url_timeout: Duration::from_secs(8),
        }
    }

    fn managed_budget_policy() -> RemoteBudgetPolicy {
        RemoteBudgetPolicy {
            warm_fetch_budget: Duration::from_millis(1),
            cold_fetch_budget: Duration::from_millis(1),
            safety_margin: Duration::from_millis(1),
        }
    }

    async fn mount_pdf(server: &MockServer, url_path: &str) {
        Mock::given(method("GET"))
            .and(path(url_path))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/pdf")
                    .set_body_bytes("%PDF-1.4\n%%EOF"),
            )
            .mount(server)
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn coordinator_runs_at_most_two_concurrently_and_preserves_order() {
        let provider = Arc::new(TimedProvider {
            delay: Duration::from_secs(2),
            active: AtomicUsize::new(0),
            max_active: AtomicUsize::new(0),
            calls: AtomicUsize::new(0),
        });
        let coordinator = LocalExtractCoordinator::new(provider.clone());
        let task = tokio::spawn(async move {
            coordinator
                .extract_many(
                    vec![
                        request("https://example.com/one"),
                        request("https://example.com/two"),
                        request("https://example.com/three"),
                    ],
                    budget(),
                )
                .await
        });
        tokio::task::yield_now().await;
        assert_eq!(provider.max_active.load(Ordering::SeqCst), 2);
        tokio::time::advance(Duration::from_secs(2)).await;
        let outcome = task.await.unwrap();
        assert_eq!(outcome.diagnostics.success_count, 3);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 3);
        assert_eq!(provider.max_active.load(Ordering::SeqCst), 2);
        let urls = outcome
            .items
            .iter()
            .map(|item| item.requested_url.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            urls,
            vec![
                "https://example.com/one",
                "https://example.com/two",
                "https://example.com/three"
            ]
        );
    }

    #[tokio::test]
    async fn coordinator_preserves_partial_failure() {
        let provider = Arc::new(MockProvider {
            fail_urls: vec!["https://example.com/bad".to_owned()],
        });
        let coordinator = LocalExtractCoordinator::new(provider);
        let outcome = coordinator
            .extract_many(
                vec![
                    request("https://example.com/ok"),
                    request("https://example.com/bad"),
                ],
                budget(),
            )
            .await;
        assert_eq!(outcome.diagnostics.success_count, 1);
        assert_eq!(outcome.diagnostics.failure_count, 1);
        assert!(outcome.items[0].page.is_some());
        assert!(outcome.items[1].final_error.is_some());
    }

    #[tokio::test]
    async fn coordinator_preserves_invalid_argument_message() {
        assert_eq!(invalid_url_index("(invalid urls[0])"), Some(0));
        let provider = Arc::new(MockProvider {
            fail_urls: Vec::new(),
        });
        let coordinator = LocalExtractCoordinator::new(provider);
        let outcome = coordinator
            .extract_many(
                vec![ExtractRequest {
                    url: "(invalid urls[0])".to_owned(),
                }],
                budget(),
            )
            .await;
        assert_eq!(
            outcome.items[0].final_error.as_ref().unwrap().to_string(),
            "invalid argument: urls[0] must be a string"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn coordinator_bounds_total_deadline() {
        let provider = Arc::new(TimedProvider {
            delay: Duration::from_secs(20),
            active: AtomicUsize::new(0),
            max_active: AtomicUsize::new(0),
            calls: AtomicUsize::new(0),
        });
        let coordinator = LocalExtractCoordinator::new(provider);
        let task = tokio::spawn(async move {
            coordinator
                .extract_many(
                    vec![
                        request("https://example.com/one"),
                        request("https://example.com/two"),
                    ],
                    budget(),
                )
                .await
        });
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(10)).await;
        let outcome = task.await.unwrap();
        assert_eq!(outcome.diagnostics.success_count, 0);
        assert_eq!(outcome.diagnostics.failure_count, 2);
        assert_eq!(
            outcome.diagnostics.timeout_counts.per_url_deadline,
            2
        );
        assert!(
            outcome
                .items
                .iter()
                .all(|item| item.final_error.is_some())
        );
    }

    #[tokio::test]
    async fn managed_coordinator_keeps_local_success_without_remote() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/html")
                    .set_body_bytes("<html><body><p>Hello</p></body></html>"),
            )
            .mount(&server)
            .await;
        let remote = Arc::new(FakeRemote::new(false));
        let coordinator = ManagedExtractCoordinator::new(
            Arc::new(HttpExtractProvider::new().allow_private_for_tests()),
            remote.clone(),
        )
        .allow_private_for_tests()
        .with_remote_budget(managed_budget_policy());

        let outcome = coordinator
            .extract_many(vec![request(&server.uri())], budget())
            .await;
        assert_eq!(outcome.diagnostics.success_count, 1);
        assert_eq!(outcome.diagnostics.remote_stage_count, 0);
        assert!(!outcome.diagnostics.remote_attempted);
        assert_eq!(remote.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn managed_coordinator_uses_remote_once_for_pdf() {
        let server = MockServer::start().await;
        mount_pdf(&server, "/pdf").await;
        let remote = Arc::new(FakeRemote::new(false));
        let coordinator = ManagedExtractCoordinator::new(
            Arc::new(HttpExtractProvider::new().allow_private_for_tests()),
            remote.clone(),
        )
        .allow_private_for_tests()
        .with_remote_budget(managed_budget_policy());

        let url = format!("{}/pdf", server.uri());
        let outcome = coordinator
            .extract_many(vec![request(&url)], budget())
            .await;
        assert_eq!(outcome.diagnostics.success_count, 1);
        assert_eq!(outcome.diagnostics.remote_eligible_count, 1);
        assert_eq!(outcome.diagnostics.remote_stage_count, 1);
        assert!(outcome.diagnostics.remote_attempted);
        assert_eq!(outcome.diagnostics.remote_success_count, 1);
        assert_eq!(outcome.diagnostics.remote_failure_count, 0);
        assert_eq!(remote.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            outcome.items[0].page.as_ref().unwrap().markdown,
            "remote content"
        );
    }

    #[tokio::test]
    async fn managed_coordinator_forbids_sensitive_urls_from_remote() {
        let server = MockServer::start().await;
        mount_pdf(&server, "/pdf").await;
        let remote = Arc::new(FakeRemote::new(false));
        let coordinator = ManagedExtractCoordinator::new(
            Arc::new(HttpExtractProvider::new().allow_private_for_tests()),
            remote.clone(),
        )
        .allow_private_for_tests()
        .with_remote_budget(managed_budget_policy());

        let url = format!("{}/pdf?token=secret", server.uri());
        let outcome = coordinator
            .extract_many(vec![request(&url)], budget())
            .await;
        assert!(outcome.items[0].page.is_none());
        assert!(outcome.items[0].final_error.is_some());
        assert_eq!(outcome.diagnostics.remote_stage_count, 0);
        assert_eq!(outcome.diagnostics.remote_forbidden_count, 1);
        assert_eq!(
            outcome
                .diagnostics
                .remote_forbidden_reason_counts
                .get(&RemoteForbiddenReason::SensitiveQuery),
            Some(&1)
        );
        assert_eq!(remote.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn managed_coordinator_forbids_sensitive_fragment_from_remote() {
        let server = MockServer::start().await;
        mount_pdf(&server, "/pdf").await;
        let remote = Arc::new(FakeRemote::new(false));
        let coordinator = ManagedExtractCoordinator::new(
            Arc::new(HttpExtractProvider::new().allow_private_for_tests()),
            remote.clone(),
        )
        .allow_private_for_tests()
        .with_remote_budget(managed_budget_policy());

        let url = format!("{}/pdf#access_token=secret", server.uri());
        let outcome = coordinator
            .extract_many(vec![request(&url)], budget())
            .await;
        assert!(outcome.items[0].page.is_none());
        assert!(outcome.items[0].final_error.is_some());
        assert_eq!(outcome.diagnostics.remote_stage_count, 0);
        assert_eq!(outcome.diagnostics.remote_forbidden_count, 1);
        assert_eq!(remote.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn managed_coordinator_remote_failure_keeps_local_error() {
        let server = MockServer::start().await;
        mount_pdf(&server, "/pdf").await;
        let remote = Arc::new(FakeRemote::new(true));
        let coordinator = ManagedExtractCoordinator::new(
            Arc::new(HttpExtractProvider::new().allow_private_for_tests()),
            remote.clone(),
        )
        .allow_private_for_tests()
        .with_remote_budget(managed_budget_policy());

        let url = format!("{}/pdf", server.uri());
        let outcome = coordinator
            .extract_many(vec![request(&url)], budget())
            .await;
        assert!(outcome.items[0].page.is_none());
        assert!(outcome.items[0].final_error.is_some());
        assert_eq!(outcome.diagnostics.remote_stage_count, 1);
        assert_eq!(outcome.diagnostics.remote_failure_count, 1);
    }

    #[tokio::test]
    async fn managed_coordinator_dedupes_duplicate_urls_and_fans_out() {
        let server = MockServer::start().await;
        mount_pdf(&server, "/pdf").await;
        let remote = Arc::new(FakeRemote::new(false));
        let coordinator = ManagedExtractCoordinator::new(
            Arc::new(HttpExtractProvider::new().allow_private_for_tests()),
            remote.clone(),
        )
        .allow_private_for_tests()
        .with_remote_budget(managed_budget_policy());

        let url = format!("{}/pdf", server.uri());
        let outcome = coordinator
            .extract_many(vec![request(&url), request(&url)], budget())
            .await;
        assert_eq!(outcome.items.len(), 2);
        assert_eq!(remote.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            remote.request_items.lock().await.len(),
            1,
            "duplicate eligible URLs must be deduplicated before remote"
        );
        assert!(
            outcome.items.iter().all(|item| item.page.is_some()),
            "one remote result must fan out to every original index"
        );
        assert_eq!(outcome.diagnostics.remote_success_count, 2);
    }

    #[tokio::test]
    async fn managed_coordinator_skips_remote_when_budget_is_insufficient() {
        let server = MockServer::start().await;
        mount_pdf(&server, "/pdf").await;
        let remote = Arc::new(FakeRemote::new(false));
        let coordinator = ManagedExtractCoordinator::new(
            Arc::new(HttpExtractProvider::new().allow_private_for_tests()),
            remote.clone(),
        )
        .allow_private_for_tests()
        .with_remote_budget(RemoteBudgetPolicy {
            warm_fetch_budget: Duration::from_secs(60),
            cold_fetch_budget: Duration::from_secs(60),
            safety_margin: Duration::from_secs(60),
        });

        let url = format!("{}/pdf", server.uri());
        let outcome = coordinator
            .extract_many(vec![request(&url)], budget())
            .await;
        assert!(outcome.items[0].page.is_none());
        assert_eq!(outcome.diagnostics.remote_stage_count, 0);
        assert_eq!(outcome.diagnostics.remote_budget_skipped_count, 1);
        assert_eq!(remote.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn managed_coordinator_rejects_more_than_three_urls() {
        let server = MockServer::start().await;
        mount_pdf(&server, "/pdf").await;
        let remote = Arc::new(FakeRemote::new(false));
        let coordinator = ManagedExtractCoordinator::new(
            Arc::new(HttpExtractProvider::new().allow_private_for_tests()),
            remote.clone(),
        )
        .with_remote_budget(managed_budget_policy());

        let url = format!("{}/pdf", server.uri());
        let outcome = coordinator
            .extract_many(
                vec![
                    request(&url),
                    request(&url),
                    request(&url),
                    request(&url),
                ],
                budget(),
            )
            .await;
        assert_eq!(outcome.items.len(), 4);
        assert!(outcome.items[3].page.is_none());
        assert!(outcome.items[3].final_error.is_some());
        assert!(
            outcome
                .items[3]
                .final_error
                .as_ref()
                .unwrap()
                .to_string()
                .contains("exceeds max")
        );
    }

    #[tokio::test]
    async fn managed_coordinator_rejects_unmapped_remote_result() {
        let server = MockServer::start().await;
        mount_pdf(&server, "/pdf").await;
        let remote = Arc::new(FakeRemote::new(false).unmapped());
        let coordinator = ManagedExtractCoordinator::new(
            Arc::new(HttpExtractProvider::new().allow_private_for_tests()),
            remote.clone(),
        )
        .allow_private_for_tests()
        .with_remote_budget(managed_budget_policy());

        let url = format!("{}/pdf", server.uri());
        let outcome = coordinator
            .extract_many(vec![request(&url)], budget())
            .await;
        assert!(outcome.items[0].page.is_none());
        assert!(outcome.items[0].final_error.is_some());
        assert_eq!(outcome.diagnostics.remote_success_count, 0);
        assert_eq!(outcome.diagnostics.remote_failure_count, 1);
    }

    #[tokio::test]
    async fn managed_coordinator_strips_plain_fragment_from_outbound() {
        let server = MockServer::start().await;
        mount_pdf(&server, "/pdf").await;
        let remote = Arc::new(FakeRemote::new(false));
        let coordinator = ManagedExtractCoordinator::new(
            Arc::new(HttpExtractProvider::new().allow_private_for_tests()),
            remote.clone(),
        )
        .allow_private_for_tests()
        .with_remote_budget(managed_budget_policy());

        let url = format!("{}/pdf#section-2", server.uri());
        let outcome = coordinator
            .extract_many(vec![request(&url)], budget())
            .await;
        assert!(outcome.items[0].page.is_some());
        let sent = remote.request_items.lock().await;
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].prepared.requested_url, url);
        assert_eq!(
            sent[0].prepared.outbound_url,
            format!("{}/pdf", server.uri())
        );
    }

    #[tokio::test]
    async fn managed_coordinator_separates_local_and_final_metrics() {
        let server = MockServer::start().await;
        mount_pdf(&server, "/pdf").await;
        let remote = Arc::new(FakeRemote::new(true));
        let coordinator = ManagedExtractCoordinator::new(
            Arc::new(HttpExtractProvider::new().allow_private_for_tests()),
            remote.clone(),
        )
        .allow_private_for_tests()
        .with_remote_budget(managed_budget_policy());

        let url = format!("{}/pdf", server.uri());
        let outcome = coordinator
            .extract_many(vec![request(&url)], budget())
            .await;
        assert_eq!(outcome.diagnostics.local_success_count, 0);
        assert_eq!(outcome.diagnostics.local_failure_count, 1);
        assert_eq!(outcome.diagnostics.final_success_count, 0);
        assert_eq!(outcome.diagnostics.final_failure_count, 1);
        assert!(outcome
            .diagnostics
            .fallback_reason_counts
            .get(&RemoteFallbackReason::Pdf)
            .is_some_and(|count| *count == 1));
    }

    #[tokio::test]
    async fn managed_coordinator_counts_source_and_context_truncation() {
        let server = MockServer::start().await;
        mount_pdf(&server, "/pdf").await;
        let remote = Arc::new(FakeRemote::new(false).source_truncated().long_markdown());
        let coordinator = ManagedExtractCoordinator::new(
            Arc::new(HttpExtractProvider::new().allow_private_for_tests()),
            remote.clone(),
        )
        .allow_private_for_tests()
        .with_remote_budget(managed_budget_policy());

        let url = format!("{}/pdf", server.uri());
        let outcome = coordinator
            .extract_many(vec![request(&url)], budget())
            .await;
        assert_eq!(outcome.diagnostics.source_truncated_count, 1);
        assert_eq!(outcome.diagnostics.context_truncated_count, 1);
    }

    #[tokio::test]
    async fn managed_coordinator_counts_remote_timeout_categories() {
        for (kind, expected_field) in [
            (RemoteTimeoutKind::QueueDeadline, "queue"),
            (RemoteTimeoutKind::CallDeadline, "call"),
        ] {
            let server = MockServer::start().await;
            mount_pdf(&server, "/pdf").await;
            let remote = Arc::new(FakeRemote::new(false).timeout(kind));
            let coordinator = ManagedExtractCoordinator::new(
                Arc::new(HttpExtractProvider::new().allow_private_for_tests()),
                remote.clone(),
            )
            .allow_private_for_tests()
            .with_remote_budget(managed_budget_policy());

            let url = format!("{}/pdf", server.uri());
            let outcome = coordinator
                .extract_many(vec![request(&url)], budget())
                .await;
            assert_eq!(outcome.diagnostics.remote_failure_count, 1);
            if expected_field == "queue" {
                assert_eq!(outcome.diagnostics.timeout_counts.remote_queue_deadline, 1);
                assert_eq!(outcome.diagnostics.timeout_counts.remote_call_deadline, 0);
            } else {
                assert_eq!(outcome.diagnostics.timeout_counts.remote_call_deadline, 1);
                assert_eq!(outcome.diagnostics.timeout_counts.remote_queue_deadline, 0);
            }
        }
    }
}
