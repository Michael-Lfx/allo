//! Opt-in, production-backed managed fetch evaluation.
//!
//! This module is compiled only with `fetch-eval`. It keeps the evaluation
//! interface small while reusing the production Local provider, Parallel MCP
//! adapter, and managed coordinator. URLs and page bodies never appear in the
//! serialized result types.

use std::sync::Arc;
use std::time::{Duration, Instant as StdInstant};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::time::Instant;
use uuid::Uuid;

use crate::coordinator::{
    ExtractBudget, ExtractCoordinator, LocalExtractAdapter, ManagedExtractCoordinator,
};
use crate::managed::{
    FetchReadiness, ManagedMcpCallControl, ManagedMcpCallGate, ManagedSearchService,
    ParallelFetchAdapter, ParallelMcpClient,
    RemoteExtractError, RemoteExtractFallback, RemoteExtractRequest,
    RemoteExtractRequestItem,
};
#[cfg(test)]
use crate::managed::{RemoteExtractBatch, RemoteExtractItem};
use crate::provider::{HttpExtractProvider, SearchProvider};
use crate::provider::extract_policy::{
    LocalExtractDiagnostics, LocalExtractFailure, LocalExtractFailureKind, LocalExtractOutcome,
    RemoteExtractCapabilities, RemoteFallbackDecision, RemoteFallbackPolicy,
    decide_remote_fallback,
};
use crate::types::{ExtractRequest, ExtractedPage, SearchQuery, WebError};

pub mod runner;

const DEFAULT_EVALUATION_TIMEOUT: Duration = Duration::from_secs(30);
const E2E_TOTAL_TIMEOUT: Duration = Duration::from_secs(12);
const E2E_LOCAL_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationMode {
    Local,
    Mcp,
    Compare,
    E2e,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationProfile {
    Diagnostic,
    Preflight,
    Admission,
}

pub(crate) const EVALUATION_SCORING_VERSION: &str = "managed-fetch-2-of-3-e2e-v1";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PeerMode {
    Cold,
    Warm,
    SearchWarmed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum CaseCategory {
    PublicPdfText,
    PublicPdfScan,
    JavascriptShell,
    StaticHtmlControl,
    RealPdfPrivate,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QualityGrade {
    Q0,
    Q1,
    Q2,
    Q3,
    Q4,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FetchEvaluationCase {
    pub id: String,
    pub category: CaseCategory,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub url_env: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub expected_markers: Vec<String>,
    pub minimum_content_chars: usize,
    pub minimum_marker_hits: usize,
    pub verified_at: String,
    pub stale_after_days: u64,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub notes: Option<String>,
}

impl FetchEvaluationCase {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty() {
            return Err("case id must not be empty".to_owned());
        }
        if self.id.contains("://") || self.id.contains('?') || self.id.contains('#') {
            return Err(format!(
                "case {} id must be a stable identifier, not a URL or fragment",
                self.id
            ));
        }
        if self.url.is_some() == self.url_env.is_some() {
            return Err(format!(
                "case {} must define exactly one of url or url_env",
                self.id
            ));
        }
        if self.category == CaseCategory::RealPdfPrivate && self.url_env.is_none() {
            return Err(format!(
                "case {} real_pdf_private cases must use url_env",
                self.id
            ));
        }
        if let Some(url) = &self.url {
            let parsed = url::Url::parse(url)
                .map_err(|_| format!("case {} URL is not a valid HTTP(S) URL", self.id))?;
            if !matches!(parsed.scheme(), "http" | "https") {
                return Err(format!("case {} URL must use HTTP or HTTPS", self.id));
            }
            if self.enabled && (parsed.query().is_some() || parsed.fragment().is_some()) {
                return Err(format!(
                    "case {} URL must not contain a query or fragment",
                    self.id
                ));
            }
        }
        if let Some(name) = &self.url_env
            && name.trim().is_empty()
        {
            return Err(format!("case {} url_env must not be empty", self.id));
        }
        if self.minimum_content_chars == 0 {
            return Err(format!(
                "case {} minimum_content_chars must be positive",
                self.id
            ));
        }
        if self.expected_markers.is_empty() {
            return Err(format!(
                "case {} expected_markers must not be empty",
                self.id
            ));
        }
        let mut markers = std::collections::HashSet::new();
        for marker in &self.expected_markers {
            if marker.trim().is_empty() {
                return Err(format!("case {} markers must not be blank", self.id));
            }
            if !markers.insert(marker) {
                return Err(format!("case {} markers must be unique", self.id));
            }
        }
        if self.minimum_marker_hits == 0 {
            return Err(format!(
                "case {} minimum_marker_hits must be positive",
                self.id
            ));
        }
        if self.minimum_marker_hits > self.expected_markers.len() {
            return Err(format!(
                "case {} minimum_marker_hits exceeds expected_markers",
                self.id
            ));
        }
        if self.stale_after_days == 0 {
            return Err(format!(
                "case {} stale_after_days must be positive",
                self.id
            ));
        }
        let verified_at = chrono::NaiveDate::parse_from_str(&self.verified_at, "%Y-%m-%d")
            .map_err(|_| format!("case {} verified_at must be YYYY-MM-DD", self.id))?;
        if verified_at > chrono::Utc::now().date_naive() {
            return Err(format!("case {} verified_at must not be in the future", self.id));
        }
        Ok(())
    }

    pub fn is_stale_on(&self, today: chrono::NaiveDate) -> Result<bool, String> {
        self.validate()?;
        let verified_at = chrono::NaiveDate::parse_from_str(&self.verified_at, "%Y-%m-%d")
            .map_err(|_| format!("case {} verified_at must be YYYY-MM-DD", self.id))?;
        let age_days = today.signed_duration_since(verified_at).num_days();
        let stale_after_days = i64::try_from(self.stale_after_days).unwrap_or(i64::MAX);
        Ok(age_days > stale_after_days)
    }

    pub fn resolve_url(&self) -> Result<String, String> {
        self.validate()?;
        let url = match (&self.url, &self.url_env) {
            (Some(url), None) => url.clone(),
            (None, Some(name)) => std::env::var(name)
                .map_err(|_| format!("case {} URL environment variable is unavailable", self.id))?,
            _ => return Err(format!("case {} has an invalid URL source", self.id)),
        };
        if url.trim().is_empty() {
            return Err(format!("case {} resolved URL is empty", self.id));
        }
        let parsed = url::Url::parse(&url)
            .map_err(|_| format!("case {} URL is not a valid HTTP(S) URL", self.id))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(format!("case {} URL must use HTTP or HTTPS", self.id));
        }
        if parsed.query().is_some() || parsed.fragment().is_some() {
            return Err(format!(
                "case {} resolved URL must not contain a query or fragment",
                self.id
            ));
        }
        Ok(url)
    }
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FetchEvaluationManifest {
    pub schema_version: u32,
    pub corpus_version: String,
    pub cases: Vec<FetchEvaluationCase>,
}

impl FetchEvaluationManifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err(format!(
                "unsupported corpus schema version {}",
                self.schema_version
            ));
        }
        if self.corpus_version.trim().is_empty() {
            return Err("corpus_version must not be empty".to_owned());
        }
        let mut ids = std::collections::HashSet::new();
        for case in &self.cases {
            case.validate()?;
            if !ids.insert(case.id.as_str()) {
                return Err(format!("duplicate case id {}", case.id));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FetchEvaluationResult {
    pub schema_version: u32,
    pub scoring_version: String,
    pub evaluation_profile: EvaluationProfile,
    pub run_id: String,
    pub git_sha: String,
    pub corpus_version: String,
    pub case_id: String,
    pub category: CaseCategory,
    pub mode: EvaluationMode,
    pub peer_mode: PeerMode,
    pub attempt: u32,
    pub local_failure_kind: Option<String>,
    pub remote_attempted: bool,
    pub remote_budget_skipped: bool,
    pub remote_eligible: bool,
    pub local_success: bool,
    pub remote_success: bool,
    pub effective_success: bool,
    pub incremental_success: bool,
    pub quality_grade: QualityGrade,
    pub elapsed_ms: u128,
    pub queue_ms: Option<u128>,
    pub call_ms: Option<u128>,
    pub local_content_chars: usize,
    pub remote_content_chars: usize,
    pub content_chars: usize,
    pub local_marker_hits: usize,
    pub remote_marker_hits: usize,
    pub marker_hits: usize,
    pub marker_count: usize,
    pub marker_hit_rate: f64,
    pub source_truncated: bool,
    pub source_mismatch_count: usize,
    pub dropped_remote_item_count: usize,
    pub retry_after_ms: Option<u64>,
    pub challenge_detected: bool,
    pub error_class: Option<String>,
    pub outcome_class: String,
}

impl FetchEvaluationResult {
    fn base(
        case: &FetchEvaluationCase,
        mode: EvaluationMode,
        peer_mode: PeerMode,
        run_id: &str,
        git_sha: &str,
        attempt: u32,
    ) -> Self {
        Self {
            schema_version: 3,
            scoring_version: EVALUATION_SCORING_VERSION.to_owned(),
            evaluation_profile: EvaluationProfile::Diagnostic,
            run_id: run_id.to_owned(),
            git_sha: git_sha.to_owned(),
            corpus_version: String::new(),
            case_id: case.id.clone(),
            category: case.category,
            mode,
            peer_mode,
            attempt,
            local_failure_kind: None,
            remote_attempted: false,
            remote_budget_skipped: false,
            remote_eligible: false,
            local_success: false,
            remote_success: false,
            effective_success: false,
            incremental_success: false,
            quality_grade: QualityGrade::Q0,
            elapsed_ms: 0,
            queue_ms: None,
            call_ms: None,
            local_content_chars: 0,
            remote_content_chars: 0,
            content_chars: 0,
            local_marker_hits: 0,
            remote_marker_hits: 0,
            marker_hits: 0,
            marker_count: case.expected_markers.len(),
            marker_hit_rate: 0.0,
            source_truncated: false,
            source_mismatch_count: 0,
            dropped_remote_item_count: 0,
            retry_after_ms: None,
            challenge_detected: false,
            error_class: None,
            outcome_class: "not_started".to_owned(),
        }
    }
}

#[async_trait]
pub(crate) trait EvaluationBackend: Send + Sync {
    fn local(&self) -> Arc<dyn LocalExtractAdapter>;
    fn remote(&self) -> Arc<dyn RemoteExtractFallback>;
    async fn fetch_readiness(&self) -> FetchReadiness;
    async fn warm_fetch(&self) -> Result<(), RemoteExtractError>;
    async fn warm_search(&self) -> Result<(), WebError>;
    async fn shutdown(&self);
}

pub(crate) trait EvaluationBackendFactory: Send + Sync {
    fn create(
        &self,
        call_gate: Option<Arc<dyn ManagedMcpCallGate>>,
    ) -> Result<Arc<dyn EvaluationBackend>, WebError>;
}

struct ProductionBackendFactory {
    control: Option<Arc<dyn ManagedMcpCallControl>>,
}

struct ProductionBackend {
    local: Arc<HttpExtractProvider>,
    fetch: Arc<ParallelFetchAdapter>,
    remote: Arc<dyn RemoteExtractFallback>,
    search: Arc<ManagedSearchService>,
}

#[async_trait]
impl EvaluationBackend for ProductionBackend {
    fn local(&self) -> Arc<dyn LocalExtractAdapter> {
        Arc::clone(&self.local) as Arc<dyn LocalExtractAdapter>
    }

    fn remote(&self) -> Arc<dyn RemoteExtractFallback> {
        Arc::clone(&self.remote)
    }

    async fn fetch_readiness(&self) -> FetchReadiness {
        self.remote.readiness().await
    }

    async fn warm_fetch(&self) -> Result<(), RemoteExtractError> {
        self.fetch
            .warm_compatibility(Instant::now() + DEFAULT_EVALUATION_TIMEOUT)
            .await
    }

    async fn warm_search(&self) -> Result<(), WebError> {
        self.search
            .search(SearchQuery {
                query: "managed fetch evaluation warmup".to_owned(),
                count: 1,
            })
            .await
            .map(|_| ())
    }

    async fn shutdown(&self) {
        self.search.shutdown().await;
    }
}

impl EvaluationBackendFactory for ProductionBackendFactory {
    fn create(
        &self,
        call_gate: Option<Arc<dyn ManagedMcpCallGate>>,
    ) -> Result<Arc<dyn EvaluationBackend>, WebError> {
        let client = match (&self.control, call_gate) {
            (Some(control), _) => Arc::new(ParallelMcpClient::new_with_call_control(Arc::clone(
                control,
            ))?),
            (None, Some(gate)) => {
                Arc::new(ParallelMcpClient::new_with_call_gate(Arc::clone(&gate))?)
            }
            (None, None) => Arc::new(ParallelMcpClient::new()?),
        };
        let search = Arc::new(ManagedSearchService::keyless_with_shared_client(
            Arc::clone(&client),
        )?);
        let fetch = Arc::new(ParallelFetchAdapter::new(Arc::clone(&client)));
        let remote: Arc<dyn RemoteExtractFallback> = Arc::clone(&fetch) as Arc<dyn RemoteExtractFallback>;
        Ok(Arc::new(ProductionBackend {
            local: Arc::new(HttpExtractProvider::new()),
            fetch,
            remote,
            search,
        }))
    }
}

/// Deep evaluation module: callers choose a mode and receive sanitized,
/// comparable evidence; peer construction, policy and result decoding stay
/// behind this interface. Test/Demo adapters cross the same seam.
pub struct FetchEvaluationHarness {
    factory: Arc<dyn EvaluationBackendFactory>,
    call_gate: Option<Arc<dyn ManagedMcpCallGate>>,
    backend: Arc<dyn EvaluationBackend>,
    fetch_warmed: tokio::sync::Mutex<bool>,
    search_warmed: tokio::sync::Mutex<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InjectedFailure {
    Dns,
    Tls,
    Network,
    Timeout,
}

/// Deterministic local-failure Adapter for coordination tests. Results from
/// this Adapter are never valid Provider-gain evidence.
#[allow(dead_code)]
pub(crate) struct FaultInjectingLocalAdapter {
    #[allow(dead_code)]
    delegated: Arc<dyn LocalExtractAdapter>,
    failures: std::collections::HashMap<String, InjectedFailure>,
}

#[allow(dead_code)]
impl FaultInjectingLocalAdapter {
    pub(crate) fn new(
        delegated: Arc<dyn LocalExtractAdapter>,
        failures: std::collections::HashMap<String, InjectedFailure>,
    ) -> Self {
        Self { delegated, failures }
    }
}

#[async_trait]
impl LocalExtractAdapter for FaultInjectingLocalAdapter {
    async fn extract_with_metadata(&self, req: ExtractRequest) -> LocalExtractOutcome {
        let requested_url = req.url.clone();
        let Some(failure) = self.failures.get(&requested_url).copied() else {
            return self.delegated.extract_with_metadata(req).await;
        };
        let (kind, error) = match failure {
            InjectedFailure::Dns => (
                LocalExtractFailureKind::Dns,
                WebError::Network("injected DNS failure".to_owned()),
            ),
            InjectedFailure::Tls => (
                LocalExtractFailureKind::Tls,
                WebError::Network("injected TLS failure".to_owned()),
            ),
            InjectedFailure::Network => (
                LocalExtractFailureKind::Network,
                WebError::Network("injected network failure".to_owned()),
            ),
            InjectedFailure::Timeout => (
                LocalExtractFailureKind::Timeout,
                WebError::Timeout("injected timeout".to_owned()),
            ),
        };
        LocalExtractOutcome {
            requested_url,
            result: Err(LocalExtractFailure { kind, error }),
            diagnostics: LocalExtractDiagnostics::default(),
        }
    }
}

impl FetchEvaluationHarness {
    pub fn keyless_production() -> Result<Self, WebError> {
        Self::from_factory_with_gate(
            Arc::new(ProductionBackendFactory { control: None }),
            None,
        )
    }

    pub(crate) fn keyless_production_with_call_control(
        control: Arc<dyn ManagedMcpCallControl>,
    ) -> Result<Self, WebError> {
        Self::from_factory_with_gate(
            Arc::new(ProductionBackendFactory {
                control: Some(control),
            }),
            None,
        )
    }

    pub(crate) fn from_factory(
        factory: Arc<dyn EvaluationBackendFactory>,
    ) -> Result<Self, WebError> {
        Self::from_factory_with_gate(factory, None)
    }

    pub(crate) fn from_factory_with_gate(
        factory: Arc<dyn EvaluationBackendFactory>,
        call_gate: Option<Arc<dyn ManagedMcpCallGate>>,
    ) -> Result<Self, WebError> {
        let backend = factory.create(call_gate.clone())?;
        Ok(Self {
            factory,
            call_gate,
            backend,
            fetch_warmed: tokio::sync::Mutex::new(false),
            search_warmed: tokio::sync::Mutex::new(false),
        })
    }

    pub async fn shutdown(&self) {
        self.backend.shutdown().await;
    }

    /// Return the production fetch peer readiness without sending a business
    /// Fetch request. Evaluation callers can record this as lifecycle
    /// diagnostic evidence while keeping the request path behind the harness.
    pub async fn fetch_readiness(&self) -> FetchReadiness {
        self.backend.fetch_readiness().await
    }

    pub async fn run_case(
        &self,
        case: &FetchEvaluationCase,
        mode: EvaluationMode,
        peer_mode: PeerMode,
    ) -> FetchEvaluationResult {
        let run_id = Uuid::now_v7().to_string();
        self.run_case_with_metadata(case, mode, peer_mode, &run_id, "unknown", 1)
            .await
    }

    pub async fn run_case_with_metadata(
        &self,
        case: &FetchEvaluationCase,
        mode: EvaluationMode,
        peer_mode: PeerMode,
        run_id: &str,
        git_sha: &str,
        attempt: u32,
    ) -> FetchEvaluationResult {
        if peer_mode == PeerMode::Cold {
            let mut result = FetchEvaluationResult::base(
                case, mode, peer_mode, run_id, git_sha, attempt,
            );
            let fresh_backend = match self.factory.create(self.call_gate.clone()) {
                Ok(backend) => backend,
                Err(error) => {
                    result.error_class = Some(error_class(&error));
                    result.outcome_class = "harness_initialization_failed".to_owned();
                    return result;
                }
            };
            let result = self
                .run_case_with_backend(
                    fresh_backend.as_ref(),
                    case,
                    mode,
                    peer_mode,
                    run_id,
                    git_sha,
                    attempt,
                )
                .await;
            fresh_backend.shutdown().await;
            return result;
        }

        if peer_mode == PeerMode::Warm && mode != EvaluationMode::Local {
            let should_warm = {
                let mut warmed = self.fetch_warmed.lock().await;
                if *warmed {
                    false
                } else {
                    *warmed = true;
                    true
                }
            };
            if should_warm && let Err(error) = self.backend.warm_fetch().await {
                let mut result = FetchEvaluationResult::base(
                    case, mode, peer_mode, run_id, git_sha, attempt,
                );
                result.error_class = Some(format_remote_error(&error));
                result.outcome_class = "fetch_warmup_failed".to_owned();
                return result;
            }
        }

        if peer_mode == PeerMode::SearchWarmed && mode != EvaluationMode::Local {
            let should_warm = {
                let mut warmed = self.search_warmed.lock().await;
                if *warmed {
                    false
                } else {
                    *warmed = true;
                    true
                }
            };
            if should_warm
                && let Err(error) = self
                    .backend
                    .warm_search()
                    .await
            {
                *self.search_warmed.lock().await = false;
                let mut result = FetchEvaluationResult::base(
                    case, mode, peer_mode, run_id, git_sha, attempt,
                );
                result.error_class = Some(error_class(&error));
                result.outcome_class = "search_warmup_failed".to_owned();
                return result;
            }
        }

        self.run_case_with_backend(
            self.backend.as_ref(),
            case,
            mode,
            peer_mode,
            run_id,
            git_sha,
            attempt,
        )
            .await
    }

    async fn run_case_with_backend(
        &self,
        backend: &dyn EvaluationBackend,
        case: &FetchEvaluationCase,
        mode: EvaluationMode,
        peer_mode: PeerMode,
        run_id: &str,
        git_sha: &str,
        attempt: u32,
    ) -> FetchEvaluationResult {
        let started = StdInstant::now();
        let mut result = FetchEvaluationResult::base(
            case, mode, peer_mode, run_id, git_sha, attempt,
        );
        let url = match case.resolve_url() {
            Ok(url) => url,
            Err(error) => {
                result.error_class = Some("invalid_case".to_owned());
                result.outcome_class = "invalid_case".to_owned();
                result.elapsed_ms = started.elapsed().as_millis();
                let _ = error;
                return result;
            }
        };

        match mode {
            EvaluationMode::Local => {
                let local = backend
                    .local()
                    .extract_with_metadata(ExtractRequest { url })
                    .await;
                apply_local(&mut result, case, &local);
                result.remote_eligible = matches!(
                    decide_remote_fallback(&local),
                    RemoteFallbackDecision::Eligible { .. }
                );
                result.effective_success = result.local_success;
                result.content_chars = result.local_content_chars;
                result.marker_hits = result.local_marker_hits;
                result.marker_hit_rate = marker_rate(result.marker_hits, result.marker_count);
                result.outcome_class = if result.local_success {
                    "local_success"
                } else {
                    "local_failure"
                }
                .to_owned();
            }
            EvaluationMode::Mcp => {
                apply_remote(
                    &mut result,
                    case,
                    self.run_remote(backend.remote(), &url).await,
                );
                result.effective_success = result.remote_success;
                result.content_chars = result.remote_content_chars;
                result.marker_hits = result.remote_marker_hits;
                result.marker_hit_rate = marker_rate(result.marker_hits, result.marker_count);
                result.outcome_class = if result.remote_success {
                    "remote_success"
                } else {
                    "remote_failure"
                }
                .to_owned();
            }
            EvaluationMode::Compare => {
                let local = backend
                    .local()
                    .extract_with_metadata(ExtractRequest { url: url.clone() })
                    .await;
                apply_local(&mut result, case, &local);
                if matches!(decide_remote_fallback(&local), RemoteFallbackDecision::Eligible { .. })
                {
                    apply_remote(
                        &mut result,
                        case,
                        self.run_remote(backend.remote(), &url).await,
                    );
                } else if result.local_success {
                    result.error_class = None;
                } else {
                    result.error_class = Some("remote_forbidden_by_policy".to_owned());
                }
                result.incremental_success =
                    result.local_failure_kind.is_some() && result.remote_success;
                result.effective_success = result.remote_success;
                result.content_chars = result.remote_content_chars;
                result.marker_hits = result.remote_marker_hits;
                result.marker_hit_rate = marker_rate(result.marker_hits, result.marker_count);
                result.outcome_class = if result.incremental_success {
                    "incremental_success"
                } else if result.local_success && result.remote_success {
                    "both_success"
                } else if result.local_success {
                    "local_only_success"
                } else if result.remote_success {
                    "remote_only_success"
                } else {
                    "both_failed"
                }
                .to_owned();
            }
            EvaluationMode::E2e => {
                let coordinator = ManagedExtractCoordinator::with_profile_and_capabilities(
                    backend.local(),
                    backend.remote(),
                    RemoteFallbackPolicy::all_eligible(),
                    RemoteExtractCapabilities::all_eligible(),
                );
                let batch = coordinator
                    .extract_many(
                        vec![ExtractRequest { url }],
                        ExtractBudget {
                            absolute_deadline: Instant::now() + E2E_TOTAL_TIMEOUT,
                            local_per_url_timeout: E2E_LOCAL_TIMEOUT,
                        },
                    )
                    .await;
                apply_e2e(&mut result, case, &batch);
            }
        }
        result.elapsed_ms = started.elapsed().as_millis();
        result
    }

    async fn run_remote(&self, remote: Arc<dyn RemoteExtractFallback>, url: &str) -> RemoteRun {
        let item = match RemoteExtractRequestItem::new(0, url.to_owned(), false) {
            Ok(item) => item,
            Err(reason) => {
                return RemoteRun::Rejected {
                    reason: format!("{reason:?}"),
                };
            }
        };
        let request = RemoteExtractRequest { items: vec![item] };
        match remote
            .extract_batch(request, Instant::now() + DEFAULT_EVALUATION_TIMEOUT)
            .await
        {
            Ok(batch) => RemoteRun::Batch(batch),
            Err(error) => RemoteRun::Error {
                error: format_remote_error(&error),
                retry_after_ms: match error {
                    RemoteExtractError::RateLimited(value) =>
                        value.map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64),
                    _ => None,
                },
            },
        }
    }
}

enum RemoteRun {
    Batch(crate::managed::RemoteExtractBatch),
    Error {
        error: String,
        retry_after_ms: Option<u64>,
    },
    Rejected {
        reason: String,
    },
}

fn apply_local(
    result: &mut FetchEvaluationResult,
    case: &FetchEvaluationCase,
    local: &crate::provider::extract_policy::LocalExtractOutcome,
) {
    match &local.result {
        Ok(page) => {
            result.local_success = page_quality(case, page).0;
            result.local_content_chars = page.markdown.chars().count();
            result.local_marker_hits = marker_hits(case, &page.markdown);
            result.local_failure_kind = None;
            result.source_truncated = page.truncated;
            result.challenge_detected = result.challenge_detected || challenge_detected(&page.markdown);
            result.quality_grade = grade(
                case,
                result.local_content_chars,
                result.local_marker_hits,
                result.marker_count,
                page.truncated,
                result.challenge_detected,
            );
        }
        Err(failure) => {
            result.local_failure_kind = Some(format!("{:?}", failure.kind));
            result.error_class = Some(error_class(&failure.error));
            result.local_success = false;
        }
    }
}

fn apply_remote(result: &mut FetchEvaluationResult, case: &FetchEvaluationCase, remote: RemoteRun) {
    match remote {
        RemoteRun::Batch(batch) => {
            result.remote_attempted = true;
            result.remote_eligible = true;
            result.queue_ms = batch.diagnostics.queue_ms;
            result.call_ms = batch.diagnostics.call_ms;
            result.source_mismatch_count = batch.diagnostics.unmatched_item_count;
            result.dropped_remote_item_count = batch.diagnostics.dropped_item_count;
            if let Some(item) = batch.items.first() {
                result.remote_content_chars = item.markdown.chars().count();
                result.remote_marker_hits = marker_hits(case, &item.markdown);
            result.source_truncated = item.source_truncated;
                result.challenge_detected = challenge_detected(&item.markdown);
                result.remote_success = effective_content(
                    case,
                    result.remote_content_chars,
                    result.remote_marker_hits,
                    result.challenge_detected,
                );
                result.quality_grade = grade(
                    case,
                    result.remote_content_chars,
                    result.remote_marker_hits,
                    result.marker_count,
                    item.source_truncated,
                    result.challenge_detected,
                );
                result.error_class = if result.source_mismatch_count > 0 {
                result.remote_success = false;
                    Some("source_mismatch".to_owned())
                } else if result.remote_success {
                    None
                } else if result.challenge_detected {
                    Some("challenge_content".to_owned())
                } else if result.remote_content_chars == 0 {
                    Some("empty_content".to_owned())
                } else {
                    Some("quality_below_minimum".to_owned())
                };
            } else {
                result.error_class = Some("unmatched_result".to_owned());
                result.quality_grade = QualityGrade::Q0;
            }
            if result.source_mismatch_count > 0 || result.dropped_remote_item_count > 0 {
                result.remote_success = false;
                result.effective_success = false;
                result.incremental_success = false;
                result.quality_grade = QualityGrade::Q0;
            }
        }
        RemoteRun::Error {
            error,
            retry_after_ms,
        } => {
            result.remote_attempted = true;
            result.remote_eligible = true;
            result.error_class = Some(error);
            result.retry_after_ms = retry_after_ms;
            result.quality_grade = QualityGrade::Q0;
            result.outcome_class = "remote_error".to_owned();
        }
        RemoteRun::Rejected { reason } => {
            result.error_class = Some(format!("remote_forbidden_{reason}"));
            result.quality_grade = QualityGrade::Q0;
            result.outcome_class = "remote_forbidden".to_owned();
        }
    }
}

fn apply_e2e(
    result: &mut FetchEvaluationResult,
    case: &FetchEvaluationCase,
    batch: &crate::coordinator::ExtractBatchOutcome,
) {
    let diagnostics = &batch.diagnostics;
    result.remote_attempted = diagnostics.remote_attempted;
    result.remote_budget_skipped = diagnostics.remote_budget_skipped_count > 0;
    result.remote_eligible = diagnostics.remote_eligible_count > 0;
    result.queue_ms = diagnostics.remote_queue_ms;
    result.call_ms = diagnostics.remote_call_ms;
    result.source_mismatch_count = diagnostics.remote_unmatched_count;
    result.dropped_remote_item_count = diagnostics.remote_dropped_count;
    if let Some(item) = batch.items.first() {
        if let Some(failure) = &item.local_failure {
            result.local_failure_kind = Some(format!("{:?}", failure.kind));
        }
        if let Some(page) = &item.page {
            result.content_chars = page.markdown.chars().count();
            result.marker_hits = marker_hits(case, &page.markdown);
            result.marker_hit_rate = marker_rate(result.marker_hits, result.marker_count);
            result.source_truncated = page.truncated;
            result.challenge_detected = challenge_detected(&page.markdown);
            result.effective_success = effective_content(
                case,
                result.content_chars,
                result.marker_hits,
                result.challenge_detected,
            );
            result.remote_success = item.page.as_ref().is_some_and(|page| {
                page.provider == "managed" && result.effective_success
            });
            result.incremental_success = result.remote_success && result.local_failure_kind.is_some();
            result.quality_grade = grade(
                case,
                result.content_chars,
                result.marker_hits,
                result.marker_count,
                result.source_truncated,
                result.challenge_detected,
            );
            if page.provider == "http" {
                result.local_success = true;
                result.local_content_chars = result.content_chars;
                result.local_marker_hits = result.marker_hits;
            } else {
                result.remote_content_chars = result.content_chars;
                result.remote_marker_hits = result.marker_hits;
            }
        }
    }
    result.error_class = if result.source_mismatch_count > 0 {
        Some("source_mismatch".to_owned())
    } else if diagnostics.final_failure_count > 0 {
        Some("final_failure".to_owned())
    } else {
        None
    };
    if result.source_mismatch_count > 0 || result.dropped_remote_item_count > 0 {
        result.remote_success = false;
        result.effective_success = false;
        result.incremental_success = false;
        result.quality_grade = QualityGrade::Q0;
    }
    result.outcome_class = if result.remote_attempted && result.remote_success {
        "e2e_remote_success"
    } else if result.remote_budget_skipped {
        "e2e_remote_budget_skipped"
    } else if result.local_success {
        "e2e_local_success"
    } else {
        "e2e_failure"
    }
    .to_owned();
}

fn page_quality(case: &FetchEvaluationCase, page: &ExtractedPage) -> (bool, usize) {
    let chars = page.markdown.chars().count();
    let hits = marker_hits(case, &page.markdown);
    (
        effective_content(case, chars, hits, challenge_detected(&page.markdown)),
        hits,
    )
}

fn effective_content(case: &FetchEvaluationCase, chars: usize, hits: usize, challenge: bool) -> bool {
    !challenge && chars >= case.minimum_content_chars && hits >= case.minimum_marker_hits
}

fn grade(
    case: &FetchEvaluationCase,
    chars: usize,
    hits: usize,
    marker_count: usize,
    source_truncated: bool,
    challenge: bool,
) -> QualityGrade {
    if challenge || chars == 0 {
        QualityGrade::Q0
    } else if chars < case.minimum_content_chars || hits < case.minimum_marker_hits {
        QualityGrade::Q1
    } else if marker_count == 0 || hits < marker_count {
        QualityGrade::Q2
    } else if source_truncated {
        QualityGrade::Q3
    } else {
        QualityGrade::Q4
    }
}

fn marker_hits(case: &FetchEvaluationCase, content: &str) -> usize {
    let lower = content.to_ascii_lowercase();
    case.expected_markers
        .iter()
        .filter(|marker| lower.contains(&marker.to_ascii_lowercase()))
        .count()
}

fn marker_rate(hits: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        hits as f64 / total as f64
    }
}

fn challenge_detected(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    [
        "checking your browser",
        "verify you are human",
        "captcha challenge",
        "sign in to continue",
        "subscribe to continue",
        "access denied",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn format_remote_error(error: &RemoteExtractError) -> String {
    match error {
        RemoteExtractError::Timeout { kind } => format!("timeout_{kind:?}"),
        RemoteExtractError::RateLimited(_) => "rate_limited".to_owned(),
        other => format!("{other:?}").to_ascii_lowercase(),
    }
}

fn error_class(error: &WebError) -> String {
    match error {
        WebError::Timeout(_) => "timeout".to_owned(),
        WebError::Network(_) => "network".to_owned(),
        WebError::Provider(_) => "provider".to_owned(),
        WebError::BlockedUrl(_) => "blocked_url".to_owned(),
        WebError::InvalidArgument(_) => "invalid_argument".to_owned(),
        WebError::Parse(_) => "parse".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case() -> FetchEvaluationCase {
        FetchEvaluationCase {
            id: "case-1".to_owned(),
            category: CaseCategory::JavascriptShell,
            url: Some("https://example.com".to_owned()),
            url_env: None,
            tags: Vec::new(),
            expected_markers: vec!["Title".to_owned(), "middle".to_owned()],
            minimum_content_chars: 10,
            minimum_marker_hits: 1,
            verified_at: "2026-08-01".to_owned(),
            stale_after_days: 30,
            enabled: true,
            notes: None,
        }
    }

    #[test]
    fn manifest_requires_exactly_one_url_source() {
        let mut value = case();
        value.url_env = Some("CASE_URL".to_owned());
        assert!(value.validate().is_err());
        value.url = None;
        assert!(value.validate().is_ok());
    }

    #[test]
    fn manifest_rejects_empty_markers_and_unknown_fields() {
        let mut value = case();
        value.expected_markers.clear();
        assert!(value.validate().is_err());

        value.expected_markers = vec!["Marker".to_owned()];
        value.url = Some("file:///private.txt".to_owned());
        assert!(value.validate().is_err());

        let mut json = serde_json::to_value(case()).unwrap();
        json.as_object_mut()
            .unwrap()
            .insert("unexpected".to_owned(), serde_json::json!(true));
        assert!(serde_json::from_value::<FetchEvaluationCase>(json).is_err());
    }

    #[test]
    fn manifest_rejects_blank_duplicate_future_and_private_inline_cases() {
        let mut value = case();
        value.expected_markers = vec![" ".to_owned()];
        assert!(value.validate().is_err());
        value.expected_markers = vec!["Marker".to_owned(), "Marker".to_owned()];
        assert!(value.validate().is_err());
        value.expected_markers = vec!["Marker".to_owned()];
        value.verified_at = (chrono::Utc::now().date_naive() + chrono::Duration::days(1))
            .to_string();
        assert!(value.validate().is_err());

        value.category = CaseCategory::RealPdfPrivate;
        value.verified_at = "2026-08-01".to_owned();
        assert!(value.validate().is_err());
        value.url = None;
        value.url_env = Some("ALLO_FETCH_CASE_REAL_PDF".to_owned());
        assert!(value.validate().is_ok());
    }

    #[test]
    fn stale_cases_and_url_like_ids_are_rejected_or_excluded() {
        let mut value = case();
        assert!(!value
            .is_stale_on(chrono::NaiveDate::from_ymd_opt(2026, 8, 1).unwrap())
            .unwrap());
        value.verified_at = "2020-01-01".to_owned();
        assert!(value
            .is_stale_on(chrono::NaiveDate::from_ymd_opt(2026, 8, 1).unwrap())
            .unwrap());
        value.id = "https://example.com".to_owned();
        assert!(value.validate().is_err());
    }

    #[test]
    fn quality_requires_content_and_markers() {
        let value = case();
        assert!(!effective_content(&value, 9, 1, false));
        assert!(!effective_content(&value, 10, 0, false));
        assert!(effective_content(&value, 10, 1, false));
        assert_eq!(grade(&value, 0, 0, 2, false, false), QualityGrade::Q0);
        assert_eq!(grade(&value, 300, 1, 2, false, false), QualityGrade::Q2);
        assert_eq!(grade(&value, 300, 2, 2, true, false), QualityGrade::Q3);
        assert_eq!(grade(&value, 300, 2, 2, false, false), QualityGrade::Q4);
    }

    #[test]
    fn challenge_markers_are_not_effective_content() {
        let value = case();
        assert!(challenge_detected("Checking your browser before accessing"));
        assert!(!effective_content(&value, 500, 2, true));
    }

    #[test]
    fn result_never_contains_url_fields() {
        let result = FetchEvaluationResult::base(
            &case(),
            EvaluationMode::Compare,
            PeerMode::Warm,
            &uuid::Uuid::now_v7().to_string(),
            "sha",
            1,
        );
        let text = serde_json::to_string(&result).unwrap();
        assert!(!text.contains("example.com"));
        assert!(!text.contains("Title"));
    }

    #[tokio::test]
    async fn fault_injector_returns_each_failure_kind_without_network_io() {
        let delegated = Arc::new(HttpExtractProvider::new()) as Arc<dyn LocalExtractAdapter>;
        for (url, expected) in [
            ("https://dns.invalid/", LocalExtractFailureKind::Dns),
            ("https://tls.invalid/", LocalExtractFailureKind::Tls),
            ("https://network.invalid/", LocalExtractFailureKind::Network),
            ("https://timeout.invalid/", LocalExtractFailureKind::Timeout),
        ] {
            let mut failures = std::collections::HashMap::new();
            failures.insert(url.to_owned(), match expected {
                LocalExtractFailureKind::Dns => InjectedFailure::Dns,
                LocalExtractFailureKind::Tls => InjectedFailure::Tls,
                LocalExtractFailureKind::Network => InjectedFailure::Network,
                LocalExtractFailureKind::Timeout => InjectedFailure::Timeout,
                _ => unreachable!(),
            });
            let adapter = FaultInjectingLocalAdapter::new(Arc::clone(&delegated), failures);
            let outcome = adapter
                .extract_with_metadata(ExtractRequest { url: url.to_owned() })
                .await;
            assert_eq!(outcome.result.unwrap_err().kind, expected);
        }
    }

    #[tokio::test]
    async fn fault_injector_exercises_managed_coordinator_without_local_network() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        #[derive(Default)]
        struct RecordingRemote {
            calls: AtomicUsize,
        }

        #[async_trait]
        impl RemoteExtractFallback for RecordingRemote {
            async fn extract_batch(
                &self,
                request: RemoteExtractRequest,
                _deadline: Instant,
            ) -> Result<RemoteExtractBatch, RemoteExtractError> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok(RemoteExtractBatch {
                    items: request
                        .items
                        .iter()
                        .map(|item| RemoteExtractItem {
                            index: item.index,
                            requested_url: item.requested_url().to_owned(),
                            final_url: None,
                            title: Some("Injected remote page".to_owned()),
                            markdown: "Injected remote page body".to_owned(),
                            source_truncated: false,
                        })
                        .collect(),
                    diagnostics: Default::default(),
                })
            }
        }

        let url = "https://dns.invalid/".to_owned();
        let delegated = Arc::new(HttpExtractProvider::new()) as Arc<dyn LocalExtractAdapter>;
        let mut failures = std::collections::HashMap::new();
        failures.insert(url.clone(), InjectedFailure::Dns);
        let local = Arc::new(FaultInjectingLocalAdapter::new(delegated, failures))
            as Arc<dyn LocalExtractAdapter>;
        let remote = Arc::new(RecordingRemote::default());
        let coordinator = ManagedExtractCoordinator::with_profile_and_capabilities(
            local,
            Arc::clone(&remote) as Arc<dyn RemoteExtractFallback>,
            RemoteFallbackPolicy::all_eligible(),
            RemoteExtractCapabilities::all_eligible(),
        );
        let outcome = coordinator
            .extract_many(
                vec![ExtractRequest { url }],
                ExtractBudget {
                    absolute_deadline: Instant::now() + Duration::from_secs(20),
                    local_per_url_timeout: Duration::from_secs(1),
                },
            )
            .await;

        assert_eq!(remote.calls.load(Ordering::SeqCst), 1);
        assert_eq!(outcome.items[0].local_failure.as_ref().unwrap().kind, LocalExtractFailureKind::Dns);
        assert_eq!(outcome.items[0].page.as_ref().unwrap().provider, "managed");
    }
}
