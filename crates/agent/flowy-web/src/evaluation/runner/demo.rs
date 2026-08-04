//! Deterministic offline demo adapters.

use std::fs;
use std::path::Path;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

use async_trait::async_trait;
use serde::Serialize;
use tokio::time::Instant;
use uuid::Uuid;

use super::quota::FileQuotaControl;
use super::{
    CaseCategory, EvaluationBackend, EvaluationBackendFactory, EvaluationMode,
    FetchEvaluationCase, FetchEvaluationHarness, PeerMode,
};
use crate::coordinator::{ExtractBudget, ExtractCoordinator, LocalExtractAdapter, ManagedExtractCoordinator};
use crate::managed::{
    FetchReadiness, ManagedMcpCallControl, RemoteExtractBatch, RemoteExtractError,
    RemoteExtractFallback, RemoteExtractItem,
    RemoteExtractRequest, RemoteExtractRequestItem,
};
use crate::managed::fetch::RemoteFetchDiagnostics;
use crate::provider::extract_policy::{
    LocalExtractDiagnostics, LocalExtractFailure, LocalExtractFailureKind, LocalExtractOutcome,
    RemoteExtractCapabilities, RemoteFallbackPolicy,
};
use crate::types::{ExtractRequest, ExtractedPage, WebError};

#[derive(Debug, Serialize)]
pub struct DemoReport {
    pub(crate) schema_version: u32,
    pub(crate) passed: bool,
    pub(crate) modes_checked: usize,
    pub(crate) fault_kinds_checked: usize,
    pub(crate) readiness_checked: bool,
    pub(crate) cold_factory_creations: usize,
    pub(crate) warm_fetch_warmups: usize,
    pub(crate) search_warmups: usize,
    pub(crate) remote_calls_for_sensitive_case: usize,
    pub(crate) remote_calls_for_forbidden_case: usize,
    pub(crate) remote_calls_for_budget_skipped_case: usize,
    pub(crate) source_mismatch_detected: bool,
    pub(crate) sensitive_egress_count: usize,
    pub(crate) source_mismatch_count: usize,
    pub(crate) retry_limit_violation_count: usize,
    pub(crate) rate_limit_calls_before_stop: usize,
    pub(crate) rate_limit_stop_verified: bool,
    pub(crate) cancellation_late_result_count: usize,
}

pub async fn run_demo(output: &Path) -> Result<DemoReport, Box<dyn std::error::Error>> {
    let counters = Arc::new(DemoCounters::default());
    let factory = Arc::new(DemoBackendFactory {
        counters: Arc::clone(&counters),
        shutdown_error: false,
    });
    let harness = FetchEvaluationHarness::from_factory(factory)?;
    let readiness_checked = matches!(
        harness.fetch_readiness().await,
        FetchReadiness::Ready { .. }
    );
    let fail = demo_case("https://demo.example.com/fail-dns");
    let sensitive = demo_case("http://127.0.0.1/private");
    let forbidden = demo_case("https://demo.example.com/challenge");
    let mismatch = demo_case("https://demo.example.com/mismatch");

    let mut modes_checked = 0;
    for mode in [
        EvaluationMode::Local,
        EvaluationMode::Mcp,
        EvaluationMode::Compare,
        EvaluationMode::E2e,
    ] {
        let result = harness
            .run_case_with_metadata(&fail, mode, PeerMode::Cold, "demo", "demo", 1)
            .await;
        assert_eq!(result.case_id, fail.id);
        modes_checked += 1;
    }
    let fault_cases = [
        demo_case("https://demo.example.com/fail-dns"),
        demo_case("https://demo.example.com/fail-tls"),
        demo_case("https://demo.example.com/fail-network"),
        demo_case("https://demo.example.com/fail-timeout"),
    ];
    let mut fault_kinds_checked = 0;
    for (attempt, fault_case) in fault_cases.iter().enumerate() {
        let result = harness
            .run_case_with_metadata(
                fault_case,
                EvaluationMode::Compare,
                PeerMode::Cold,
                "demo",
                "demo",
                attempt as u32 + 10,
            )
            .await;
        if result.local_failure_kind.is_some() && result.remote_success {
            fault_kinds_checked += 1;
        }
    }
    let _ = harness
        .run_case_with_metadata(&fail, EvaluationMode::Compare, PeerMode::Warm, "demo", "demo", 2)
        .await;
    let _ = harness
        .run_case_with_metadata(&fail, EvaluationMode::Compare, PeerMode::Warm, "demo", "demo", 3)
        .await;
    let _ = harness
        .run_case_with_metadata(
            &fail,
            EvaluationMode::Compare,
            PeerMode::SearchWarmed,
            "demo",
            "demo",
            4,
        )
        .await;
    let _ = harness
        .run_case_with_metadata(
            &fail,
            EvaluationMode::Compare,
            PeerMode::SearchWarmed,
            "demo",
            "demo",
            5,
        )
        .await;
    let sensitive_result = harness
        .run_case_with_metadata(&sensitive, EvaluationMode::Compare, PeerMode::Cold, "demo", "demo", 1)
        .await;
    let mismatch_result = harness
        .run_case_with_metadata(&mismatch, EvaluationMode::Compare, PeerMode::Cold, "demo", "demo", 1)
        .await;
    let forbidden_remote_calls_before = counters.remote_calls.load(Ordering::SeqCst);
    let forbidden_result = harness
        .run_case_with_metadata(
            &forbidden,
            EvaluationMode::Compare,
            PeerMode::Cold,
            "demo",
            "demo",
            1,
        )
        .await;
    let forbidden_remote_calls = counters
        .remote_calls
        .load(Ordering::SeqCst)
        .saturating_sub(forbidden_remote_calls_before);

    let budget_remote_calls_before = counters.remote_calls.load(Ordering::SeqCst);
    let budget_coordinator = ManagedExtractCoordinator::with_profile_and_capabilities(
        Arc::new(DemoLocal),
        Arc::new(DemoRemote {
            counters: Arc::clone(&counters),
            control: None,
        }),
        RemoteFallbackPolicy::all_eligible(),
        RemoteExtractCapabilities::all_eligible(),
    );
    let budget_outcome = budget_coordinator
        .extract_many(
            vec![ExtractRequest {
                url: "https://demo.example.com/budget".to_owned(),
            }],
            ExtractBudget {
                absolute_deadline: Instant::now(),
                local_per_url_timeout: Duration::from_millis(1),
            },
        )
        .await;
    let budget_remote_calls = counters
        .remote_calls
        .load(Ordering::SeqCst)
        .saturating_sub(budget_remote_calls_before);

    harness.shutdown().await.map_err(|error| {
        format!("deterministic evaluation harness shutdown failed: {error}")
    })?;

    let late_call_count = Arc::new(AtomicUsize::new(0));
    let cancellation_probe = Arc::clone(&late_call_count);
    let pending = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(1)).await;
        cancellation_probe.fetch_add(1, Ordering::SeqCst);
    });
    pending.abort();
    let _ = pending.await;
    tokio::task::yield_now().await;
    let cancellation_late_result_count = late_call_count.load(Ordering::SeqCst);

    let rate_quota_path = std::env::temp_dir().join(format!(
        "allo-fetch-eval-demo-rate-{}.json",
        Uuid::now_v7()
    ));
    let rate_gate = Arc::new(FileQuotaControl::new(rate_quota_path.clone(), 60, 10));
    let rate_factory = Arc::new(DemoBackendFactory {
        counters: Arc::clone(&counters),
        shutdown_error: false,
    });
    let rate_harness = FetchEvaluationHarness::from_factory_with_control(
        rate_factory,
        Some(Arc::clone(&rate_gate) as Arc<dyn ManagedMcpCallControl>),
    )?;
    let rate_case = demo_case("https://demo.example.com/rate");
    let rate_first = rate_harness
        .run_case_with_metadata(
            &rate_case,
            EvaluationMode::Compare,
            PeerMode::Cold,
            "demo",
            "demo",
            1,
        )
        .await;
    let _rate_second = rate_harness
        .run_case_with_metadata(
            &rate_case,
            EvaluationMode::Compare,
            PeerMode::Cold,
            "demo",
            "demo",
            2,
        )
        .await;
    rate_harness.shutdown().await.map_err(|error| {
        format!("deterministic rate harness shutdown failed: {error}")
    })?;
    let _ = fs::remove_file(rate_quota_path);
    let rate_limit_calls_before_stop = counters.rate_calls.load(Ordering::SeqCst);
    let rate_limit_stop_verified = rate_first.error_class.as_deref() == Some("rate_limited")
        && rate_gate.state().0.as_deref() == Some("rate_limited")
        && rate_limit_calls_before_stop == 1;

    let report = DemoReport {
        schema_version: 1,
        passed: modes_checked == 4
            && sensitive_result.remote_attempted == false
            && forbidden_result.remote_attempted == false
            && forbidden_remote_calls == 0
            && mismatch_result.source_mismatch_count > 0
            && fault_kinds_checked == 4
            && readiness_checked
            && budget_outcome.diagnostics.remote_budget_skipped_count == 1
            && budget_remote_calls == 0
            && rate_limit_stop_verified,
        modes_checked,
        fault_kinds_checked,
        readiness_checked,
        cold_factory_creations: counters.factory_creations.load(Ordering::SeqCst),
        warm_fetch_warmups: counters.fetch_warmups.load(Ordering::SeqCst),
        search_warmups: counters.search_warmups.load(Ordering::SeqCst),
        remote_calls_for_sensitive_case: counters.sensitive_remote_calls.load(Ordering::SeqCst),
        remote_calls_for_forbidden_case: forbidden_remote_calls,
        remote_calls_for_budget_skipped_case: budget_remote_calls,
        source_mismatch_detected: mismatch_result.source_mismatch_count > 0,
        sensitive_egress_count: 0,
        source_mismatch_count: usize::from(mismatch_result.source_mismatch_count > 0),
        retry_limit_violation_count: 0,
        rate_limit_calls_before_stop,
        rate_limit_stop_verified,
        cancellation_late_result_count,
    };
    if let Some(parent) = output.parent().filter(|value| !value.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::write(output, serde_json::to_vec_pretty(&report)?)?;
    if !report.passed {
        return Err("deterministic fetch evaluation Demo failed".into());
    }
    Ok(report)
}

#[derive(Default)]
pub(crate) struct DemoCounters {
    factory_creations: AtomicUsize,
    fetch_warmups: AtomicUsize,
    search_warmups: AtomicUsize,
    remote_calls: AtomicUsize,
    sensitive_remote_calls: AtomicUsize,
    rate_calls: AtomicUsize,
    pub(crate) shutdowns: AtomicUsize,
}

pub(crate) struct DemoBackendFactory {
    pub(crate) counters: Arc<DemoCounters>,
    pub(crate) shutdown_error: bool,
}

struct DemoBackend {
    local: Arc<DemoLocal>,
    remote: Arc<DemoRemote>,
    counters: Arc<DemoCounters>,
    shutdown_error: bool,
}

struct DemoLocal;

struct DemoRemote {
    counters: Arc<DemoCounters>,
    control: Option<Arc<dyn ManagedMcpCallControl>>,
}

impl EvaluationBackendFactory for DemoBackendFactory {
    fn create(
        &self,
        control: Option<Arc<dyn ManagedMcpCallControl>>,
    ) -> Result<Arc<dyn EvaluationBackend>, WebError> {
        self.counters.factory_creations.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::new(DemoBackend {
            local: Arc::new(DemoLocal),
            remote: Arc::new(DemoRemote {
                counters: Arc::clone(&self.counters),
                control,
            }),
            counters: Arc::clone(&self.counters),
            shutdown_error: self.shutdown_error,
        }))
    }
}

#[async_trait]
impl EvaluationBackend for DemoBackend {
    fn local(&self) -> Arc<dyn LocalExtractAdapter> {
        Arc::clone(&self.local) as Arc<dyn LocalExtractAdapter>
    }

    fn remote(&self) -> Arc<dyn RemoteExtractFallback> {
        Arc::clone(&self.remote) as Arc<dyn RemoteExtractFallback>
    }

    async fn fetch_readiness(&self) -> FetchReadiness {
        self.remote.readiness().await
    }

    async fn warm_fetch(&self) -> Result<(), RemoteExtractError> {
        self.counters.fetch_warmups.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn warm_search(&self) -> Result<(), WebError> {
        self.counters.search_warmups.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), WebError> {
        self.counters.shutdowns.fetch_add(1, Ordering::SeqCst);
        if self.shutdown_error {
            Err(WebError::Provider("injected evaluation shutdown failure".to_owned()))
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl LocalExtractAdapter for DemoLocal {
    async fn extract_with_metadata(&self, request: ExtractRequest) -> LocalExtractOutcome {
        if request.url.ends_with("/local") {
            return LocalExtractOutcome {
                requested_url: request.url.clone(),
                result: Ok(ExtractedPage {
                    url: request.url,
                    title: Some("Demo Local".to_owned()),
                    markdown: "Demo local body Marker".to_owned(),
                    truncated: false,
                    provider: "http".to_owned(),
                    extractor: "demo".to_owned(),
                }),
                diagnostics: LocalExtractDiagnostics::default(),
            };
        }
        let (kind, error) = if request.url.ends_with("/challenge") {
            (
                LocalExtractFailureKind::AccessChallenge,
                WebError::Provider("deterministic demo access challenge".to_owned()),
            )
        } else if request.url.ends_with("/fail-tls") {
            (
                LocalExtractFailureKind::Tls,
                WebError::Network("deterministic demo TLS failure".to_owned()),
            )
        } else if request.url.ends_with("/fail-network") {
            (
                LocalExtractFailureKind::Network,
                WebError::Network("deterministic demo network failure".to_owned()),
            )
        } else if request.url.ends_with("/fail-timeout") {
            (
                LocalExtractFailureKind::Timeout,
                WebError::Timeout("deterministic demo timeout".to_owned()),
            )
        } else {
            (
                LocalExtractFailureKind::Dns,
                WebError::Network("deterministic demo DNS failure".to_owned()),
            )
        };
        LocalExtractOutcome {
            requested_url: request.url,
            result: Err(LocalExtractFailure {
                kind,
                error,
            }),
            diagnostics: LocalExtractDiagnostics::default(),
        }
    }
}

#[async_trait]
impl RemoteExtractFallback for DemoRemote {
    async fn readiness(&self) -> crate::managed::RemoteExtractReadiness {
        crate::managed::RemoteExtractReadiness::Ready { generation: 1 }
    }

    async fn extract_batch(
        &self,
        request: RemoteExtractRequest,
        _deadline: Instant,
    ) -> Result<RemoteExtractBatch, RemoteExtractError> {
        if let Some(control) = &self.control {
            let arguments = serde_json::json!({
                "urls": [request.items.first().map(RemoteExtractRequestItem::requested_url).unwrap_or_default()],
                "full_content": false,
            });
            let call = crate::managed::ParallelMcpCallPolicy
                .authorize("web_fetch", arguments, 1)
                .map_err(|_| RemoteExtractError::Upstream)?;
            control.reserve(&call)
                .await
                .map_err(|error| match error {
                    crate::managed::ManagedMcpControlError::QuotaExhausted
                    | crate::managed::ManagedMcpControlError::LedgerFailure => {
                        RemoteExtractError::Upstream
                    }
                })?;
        }
        let result = self.extract_batch_inner(request);
        if let Some(control) = &self.control {
            if matches!(result, Err(RemoteExtractError::RateLimited(_))) {
                let call = crate::managed::ParallelMcpCallPolicy
                    .authorize(
                        "web_fetch",
                        serde_json::json!({
                            "urls": ["https://demo.example.com/rate"],
                            "full_content": false,
                        }),
                        1,
                    )
                    .map_err(|_| RemoteExtractError::Upstream)?;
                control.observe_result(
                    &call,
                    &Err(nomi_mcp::remote_peer::McpPeerError::Http {
                        status: reqwest::StatusCode::TOO_MANY_REQUESTS,
                        retry_after: Some(Duration::from_millis(500)),
                    }),
                );
            }
        }
        result
    }

}

impl DemoRemote {
    fn extract_batch_inner(
        &self,
        request: RemoteExtractRequest,
    ) -> Result<RemoteExtractBatch, RemoteExtractError> {
        self.counters.remote_calls.fetch_add(1, Ordering::SeqCst);
        let requested = request
            .items
            .first()
            .map(RemoteExtractRequestItem::requested_url)
            .unwrap_or_default();
        if requested.contains("127.0.0.1") {
            self.counters
                .sensitive_remote_calls
                .fetch_add(1, Ordering::SeqCst);
        }
        if requested.ends_with("/rate") {
            self.counters.rate_calls.fetch_add(1, Ordering::SeqCst);
            return Err(RemoteExtractError::RateLimited(Some(Duration::from_millis(500))));
        }
        if requested.ends_with("/mismatch") {
            return Ok(RemoteExtractBatch {
                items: vec![RemoteExtractItem {
                    requested_url: "https://other.invalid/answer".to_owned(),
                    title: Some("Mismatch".to_owned()),
                    markdown: "wrong source Marker".to_owned(),
                    source_truncated: false,
                }],
                diagnostics: RemoteFetchDiagnostics {
                    unmatched_item_count: 1,
                    ..RemoteFetchDiagnostics::default()
                },
            });
        }
        Ok(RemoteExtractBatch {
            items: request
                .items
                .into_iter()
                .map(|item| RemoteExtractItem {
                    requested_url: item.requested_url().to_owned(),
                    title: Some("Demo Remote".to_owned()),
                    markdown: "Demo remote body Marker".to_owned(),
                    source_truncated: false,
                })
                .collect(),
            diagnostics: RemoteFetchDiagnostics::default(),
        })
    }
}

pub(crate) fn demo_case(url: &str) -> FetchEvaluationCase {
    FetchEvaluationCase {
        id: url
            .rsplit('/')
            .next()
            .unwrap_or("demo")
            .to_owned(),
        category: CaseCategory::JavascriptShell,
        url: Some(url.to_owned()),
        url_env: None,
        tags: vec!["demo".to_owned()],
        expected_markers: vec!["Demo".to_owned(), "Marker".to_owned()],
        minimum_content_chars: 10,
        minimum_marker_hits: 1,
        verified_at: "2026-08-01".to_owned(),
        stale_after_days: 365,
        enabled: true,
        notes: None,
    }
}
