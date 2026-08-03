//! Feature-gated evaluation runner and deterministic Demo.
//!
//! The example binary owns only command-line parsing. This module owns the
//! quota ledger, resumable status, sanitized JSONL writes and admission
//! statistics so tests and the unattended runner cross the same seam.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{
    CaseCategory, EvaluationMode, EvaluationProfile, FetchEvaluationCase, FetchEvaluationHarness,
    FetchEvaluationManifest, FetchEvaluationResult, PeerMode, QualityGrade,
    EVALUATION_SCORING_VERSION,
};

#[cfg(test)]
use std::fs;
#[cfg(test)]
use std::time::Duration;
#[cfg(test)]
use std::sync::Arc;
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(test)]
use serde_json::Value;
#[cfg(test)]
use uuid::Uuid;

use super::{EvaluationBackend, EvaluationBackendFactory};
#[cfg(test)]
use crate::managed::{ManagedMcpCallControl, ParallelCallRejection};
#[cfg(test)]
use crate::types::WebError;

#[cfg(test)]
use crate::managed::ParallelMcpCallPolicy;

pub const MAX_CALLS_PER_RUN: u32 = 25;
pub const MAX_CALLS_PER_DAY: u32 = 60;
pub const SCORING_VERSION: &str = EVALUATION_SCORING_VERSION;

#[derive(Debug, Clone)]
pub struct RunConfig {
    pub mode: EvaluationMode,
    pub peer_mode: PeerMode,
    pub profile: EvaluationProfile,
    pub manifest: PathBuf,
    pub case_ids: Option<Vec<String>>,
    pub category: Option<CaseCategory>,
    pub tag: Option<String>,
    pub repeat: u32,
    pub pacing_ms: u64,
    pub max_calls: u32,
    pub daily_cap: u32,
    pub quota_path: PathBuf,
    pub output: PathBuf,
    pub status: Option<PathBuf>,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct TestRunMetadata {
    pub(crate) git_sha: Option<String>,
    pub(crate) allow_dirty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunStatus {
    pub schema_version: u32,
    pub scoring_version: String,
    #[serde(default = "default_diagnostic_profile")]
    pub evaluation_profile: EvaluationProfile,
    pub run_id: String,
    pub git_sha: String,
    pub corpus_version: String,
    pub dirty_worktree: bool,
    pub planned_attempts: usize,
    pub completed_attempts: usize,
    pub actual_remote_calls: u32,
    #[serde(default)]
    pub actual_fetch_calls: u32,
    #[serde(default)]
    pub actual_search_calls: u32,
    #[serde(default)]
    pub recovery_retry_calls: u32,
    #[serde(default)]
    pub retry_limit_violation_count: usize,
    #[serde(default)]
    pub sensitive_egress_count: usize,
    pub stop_reason: String,
    pub retry_after_ms: Option<u64>,
    pub cooldown_until_unix: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub status: RunStatus,
    pub output_path: PathBuf,
    pub status_path: PathBuf,
    pub safety_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SafetyReport {
    pub schema_version: u32,
    pub scoring_version: String,
    pub run_id: String,
    pub git_sha: String,
    pub corpus_version: String,
    #[serde(default = "default_diagnostic_profile")]
    pub evaluation_profile: EvaluationProfile,
    #[serde(default)]
    pub dirty_worktree: bool,
    pub actual_remote_calls: u32,
    pub actual_fetch_calls: u32,
    pub actual_search_calls: u32,
    pub recovery_retry_calls: u32,
    pub source_mismatch_count: usize,
    pub dropped_remote_item_count: usize,
    pub sensitive_egress_count: usize,
    pub retry_limit_violation_count: usize,
    pub cancellation_late_result_count: usize,
    #[serde(default)]
    pub cancellation_events_observed: usize,
    pub stop_reason: String,
    pub complete: bool,
    pub all_zero: bool,
}

fn default_diagnostic_profile() -> EvaluationProfile {
    EvaluationProfile::Diagnostic
}

mod quota;
mod execution;
mod evidence;
mod scoring;
mod demo;
mod campaign;

#[cfg(test)]
pub(crate) use quota::{FileQuotaControl, QuotaLedger};
#[cfg(test)]
pub(crate) use evidence::{default_safety_path, default_status_path, read_json};
pub use execution::run;
#[cfg(test)]
pub(crate) use execution::run_with_factory;
#[cfg(test)]
pub(crate) use evidence::atomic_json_write;
pub use scoring::{
    summarize, summarize_with_evidence, CategorySummary, EvaluationSummary,
    SafetySummary, SensitivityPoint,
};
#[cfg(test)]
pub(crate) use scoring::{
    admission_decision, percentile, sensitivity,
    summarize_category, validate_admission_composition, wilson_interval,
};
pub use demo::{run_demo, DemoReport};
#[cfg(test)]
pub(crate) use demo::{demo_case, DemoBackendFactory, DemoCounters};
pub use campaign::{AdmissionCampaign, CampaignConfig, CampaignOutcome};
#[cfg(test)]
pub(crate) use campaign::{
    admission_pool_shortage, build_campaign_batches, campaign_next_action, CampaignNextAction,
    validate_campaign_config,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn completed_status(
        run_id: &str,
        git_sha: &str,
        corpus_version: &str,
        planned_attempts: usize,
    ) -> RunStatus {
        RunStatus {
            schema_version: 3,
            scoring_version: SCORING_VERSION.to_owned(),
            evaluation_profile: EvaluationProfile::Diagnostic,
            run_id: run_id.to_owned(),
            git_sha: git_sha.to_owned(),
            corpus_version: corpus_version.to_owned(),
            dirty_worktree: false,
            planned_attempts,
            completed_attempts: planned_attempts,
            actual_remote_calls: 0,
            actual_fetch_calls: 0,
            actual_search_calls: 0,
            recovery_retry_calls: 0,
            retry_limit_violation_count: 0,
            sensitive_egress_count: 0,
            stop_reason: "completed".to_owned(),
            retry_after_ms: None,
            cooldown_until_unix: None,
        }
    }

    fn completed_safety(run_id: &str, git_sha: &str, corpus_version: &str) -> SafetyReport {
        SafetyReport {
            schema_version: 2,
            scoring_version: SCORING_VERSION.to_owned(),
            run_id: run_id.to_owned(),
            git_sha: git_sha.to_owned(),
            corpus_version: corpus_version.to_owned(),
            evaluation_profile: EvaluationProfile::Diagnostic,
            dirty_worktree: false,
            actual_remote_calls: 0,
            actual_fetch_calls: 0,
            actual_search_calls: 0,
            recovery_retry_calls: 0,
            source_mismatch_count: 0,
            dropped_remote_item_count: 0,
            sensitive_egress_count: 0,
            retry_limit_violation_count: 0,
            cancellation_late_result_count: 0,
            cancellation_events_observed: 0,
            stop_reason: "completed".to_owned(),
            complete: true,
            all_zero: true,
        }
    }

    #[test]
    fn campaign_batches_are_stable_and_keep_categories_separate() {
        let mut cases = Vec::new();
        for index in (0..7).rev() {
            let mut case = demo_case(&format!("https://demo.example.com/js-{index}"));
            case.id = format!("js-{index}");
            cases.push(case);
        }
        for index in (0..3).rev() {
            let mut case = demo_case(&format!("https://demo.example.com/pdf-{index}"));
            case.id = format!("pdf-{index}");
            case.category = CaseCategory::PublicPdfText;
            cases.push(case);
        }
        let batches = build_campaign_batches(&cases, "demo", 5).expect("campaign plan");
        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0].category, "public_pdf_text");
        assert_eq!(batches[0].case_ids, ["pdf-0", "pdf-1", "pdf-2"]);
        assert_eq!(batches[1].category, "javascript_shell");
        assert_eq!(batches[1].case_ids, ["js-0", "js-1", "js-2", "js-3", "js-4"]);
        assert_eq!(batches[2].case_ids, ["js-5", "js-6"]);
        assert!(batches.iter().all(|batch| batch.case_ids.len() <= 5));
    }

    #[test]
    fn campaign_cap_only_stops_when_work_remains() {
        assert_eq!(campaign_next_action(false, 0), CampaignNextAction::Completed);
        assert_eq!(
            campaign_next_action(true, 0),
            CampaignNextAction::InconclusiveDueToQuota
        );
        assert_eq!(campaign_next_action(true, 1), CampaignNextAction::RunBatch);
    }

    #[test]
    fn campaign_config_rejects_caps_outside_public_limits() {
        let config = CampaignConfig {
            manifest: PathBuf::from("manifest.json"),
            tag: "admission".to_owned(),
            batch_size: 6,
            pacing_ms: 0,
            max_calls_per_batch: 25,
            daily_cap: 60,
            campaign_cap: 200,
            quota_path: PathBuf::from("quota.json"),
            output_dir: PathBuf::from("out"),
        };
        assert!(validate_campaign_config(&config).is_err());
    }

    #[test]
    fn formal_campaign_rejects_category_pool_shortage() {
        let cases = (0..20)
            .map(|index| {
                let mut case = demo_case(&format!("https://demo.example.com/case-{index}"));
                case.id = format!("case-{index}");
                case
            })
            .collect::<Vec<_>>();
        let batches = build_campaign_batches(&cases, "demo", 5).expect("campaign plan");
        let reason = admission_pool_shortage(&batches).expect("shortage must be detected");
        assert!(reason.contains("pdf=0"));
        assert!(reason.contains("js=20"));
    }

    #[test]
    fn percentile_uses_nearest_rank() {
        assert_eq!(percentile(&[1, 2, 3, 4], 0.50), Some(2));
        assert_eq!(percentile(&[], 0.95), None);
    }

    #[test]
    fn wilson_interval_is_bounded() {
        let (low, high) = wilson_interval(2, 5);
        assert!(0.0 <= low && low <= high && high <= 1.0);
    }

    #[test]
    fn admission_decision_respects_safety_and_sample_gates() {
        assert_eq!(
            admission_decision(
                "public_pdf_text",
                9,
                1.0,
                1.0,
                Some(1),
                Some(1),
                true,
                true,
                false,
            ),
            "insufficient_evidence"
        );
        assert_eq!(
            admission_decision(
                "public_pdf_text",
                15,
                1.0,
                1.0,
                Some(1),
                Some(1),
                true,
                false,
                false,
            ),
            "reject"
        );
        assert_eq!(
            admission_decision(
                "public_pdf_text",
                15,
                1.0,
                1.0,
                Some(1),
                Some(1),
                true,
                true,
                true,
            ),
            "inconclusive_due_to_quota"
        );
    }

    #[test]
    fn sensitivity_covers_all_required_combinations() {
        let points = sensitivity(
            "public_pdf_text",
            15,
            0.8,
            0.9,
            Some(4_000),
            Some(7_000),
            true,
            true,
            false,
        );
        assert_eq!(points.len(), 27);
    }

    #[tokio::test]
    async fn quota_ledger_is_process_safe_and_enforces_caps() {
        let path = std::env::temp_dir().join(format!("allo-fetch-eval-quota-{}.json", Uuid::now_v7()));
        let gate = FileQuotaControl::new(path.clone(), 2, 2);
        let fetch = serde_json::json!({"urls": ["https://example.com/"], "full_content": false});
        let call = |attempt| ParallelMcpCallPolicy.authorize("web_fetch", fetch.clone(), attempt).unwrap();
        ManagedMcpCallControl::reserve(&gate, &call(1)).await.unwrap();
        ManagedMcpCallControl::reserve(&gate, &call(1)).await.unwrap();
        assert!(matches!(
            ManagedMcpCallControl::reserve(&gate, &call(1)).await,
            Err(crate::managed::ManagedMcpControlError::QuotaExhausted)
        ));
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn quota_gate_persists_bounded_retry_after_and_cooldown() {
        let path = std::env::temp_dir().join(format!("allo-fetch-eval-rate-{}.json", Uuid::now_v7()));
        let gate = FileQuotaControl::new(path.clone(), 60, 25);
        let call = ParallelMcpCallPolicy
            .authorize(
                "web_fetch",
                serde_json::json!({"urls": ["https://example.com/"], "full_content": false}),
                1,
            )
            .unwrap();
        ManagedMcpCallControl::observe_result(
            &gate,
            &call,
            &Err(nomi_mcp::remote_peer::McpPeerError::Http {
                status: reqwest::StatusCode::TOO_MANY_REQUESTS,
                retry_after: Some(Duration::from_millis(1_500)),
            }),
        );
        let (reason, retry_after_ms, cooldown) = gate.state();
        assert_eq!(reason.as_deref(), Some("rate_limited"));
        assert_eq!(retry_after_ms, Some(1_500));
        assert!(cooldown.is_some());
        let fetch = serde_json::json!({"urls": ["https://example.com/"], "full_content": false});
        assert!(matches!(
            ManagedMcpCallControl::reserve(
                &gate,
                &ParallelMcpCallPolicy.authorize("web_fetch", fetch, 1).unwrap(),
            ).await,
            Err(crate::managed::ManagedMcpControlError::QuotaExhausted)
        ));
        let ledger: QuotaLedger = read_json(&path).unwrap();
        assert_eq!(ledger.used_calls, 0);
        assert!(ledger.cooldown_until_unix.is_some());
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn managed_call_gate_counts_fetch_search_and_recovery_attempts() {
        let path = std::env::temp_dir().join(format!(
            "allo-fetch-eval-call-gate-{}.json",
            Uuid::now_v7()
        ));
        let gate = FileQuotaControl::new(path.clone(), 60, 10);
        let fetch = serde_json::json!({
            "urls": ["https://example.com/"],
            "full_content": false,
        });
        let fetch_call = |attempt| ParallelMcpCallPolicy.authorize("web_fetch", fetch.clone(), attempt).unwrap();
        let search_call = ParallelMcpCallPolicy.authorize(
            "web_search",
            serde_json::json!({"objective": "warm", "search_queries": ["warm"]}),
            1,
        ).unwrap();
        ManagedMcpCallControl::reserve(&gate, &fetch_call(1)).await.unwrap();
        ManagedMcpCallControl::reserve(&gate, &search_call).await.unwrap();
        ManagedMcpCallControl::reserve(&gate, &fetch_call(2)).await.unwrap();
        ManagedMcpCallControl::reserve(&gate, &fetch_call(3)).await.unwrap();
        assert_eq!(gate.actual_calls(), 4);
        assert_eq!(gate.fetch_calls(), 3);
        assert_eq!(gate.search_calls(), 1);
        assert_eq!(gate.recovery_calls(), 2);
        let rejection = ParallelMcpCallPolicy
            .authorize("web_fetch", fetch.clone(), 4)
            .expect_err("the production policy owns retry limits");
        assert!(matches!(
            rejection,
            ParallelCallRejection::RetryLimitExceeded
        ));
        ManagedMcpCallControl::observe_rejection(&gate, rejection);
        assert_eq!(gate.retry_limit_violations(), 1);
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn managed_call_gate_rejects_sensitive_fetch_before_quota() {
        let path = std::env::temp_dir().join(format!(
            "allo-fetch-eval-sensitive-gate-{}.json",
            Uuid::now_v7()
        ));
        let gate = FileQuotaControl::new(path.clone(), 60, 10);
        let sensitive = serde_json::json!({
            "urls": ["https://example.com/?token=secret"],
            "full_content": false,
        });
        let rejection = ParallelMcpCallPolicy
            .authorize("web_fetch", sensitive, 1)
            .expect_err("the production policy owns URL safety");
        assert!(matches!(rejection, ParallelCallRejection::UnsafeArguments));
        ManagedMcpCallControl::observe_rejection(&gate, rejection);
        assert_eq!(gate.actual_calls(), 0);
        assert_eq!(gate.sensitive_egress_violations(), 1);
        assert!(!path.exists(), "blocked calls must not initialize the quota ledger");
    }

    #[tokio::test]
    async fn managed_call_gate_rejects_unsafe_search_before_quota() {
        let path = std::env::temp_dir().join(format!(
            "allo-fetch-eval-unsafe-search-{}.json",
            Uuid::now_v7()
        ));
        let gate = FileQuotaControl::new(path.clone(), 60, 10);
        let unsafe_search = serde_json::json!({
            "objective": "warm",
            "search_queries": ["warm"],
            "session_id": "must be blocked",
        });
        let rejection = ParallelMcpCallPolicy
            .authorize("web_search", unsafe_search, 1)
            .expect_err("the production policy owns Search argument safety");
        assert!(matches!(rejection, ParallelCallRejection::UnsafeArguments));
        ManagedMcpCallControl::observe_rejection(&gate, rejection);
        assert_eq!(gate.actual_calls(), 0);
        assert_eq!(gate.sensitive_egress_violations(), 1);
        assert!(!path.exists(), "blocked calls must not initialize the quota ledger");
    }

    #[tokio::test]
    async fn managed_call_gate_rejects_extra_fetch_arguments_before_network() {
        let path = std::env::temp_dir().join(format!(
            "allo-fetch-eval-extra-args-{}.json",
            Uuid::now_v7()
        ));
        let gate = FileQuotaControl::new(path.clone(), 60, 10);
        let extra = serde_json::json!({
            "urls": ["https://example.com/"],
            "full_content": false,
            "objective": "must be blocked",
        });
        let rejection = ParallelMcpCallPolicy
            .authorize("web_fetch", extra, 1)
            .expect_err("the production policy owns Fetch argument safety");
        assert!(matches!(rejection, ParallelCallRejection::UnsafeArguments));
        ManagedMcpCallControl::observe_rejection(&gate, rejection);
        assert_eq!(gate.actual_calls(), 0);
        assert_eq!(gate.sensitive_egress_violations(), 1);
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn injected_runner_flushes_jsonl_and_writes_safety_report() {
        let suffix = Uuid::now_v7().to_string();
        let manifest_path = std::env::temp_dir().join(format!("allo-fetch-eval-{suffix}.json"));
        let output_path = std::env::temp_dir().join(format!("allo-fetch-eval-{suffix}.jsonl"));
        let manifest = FetchEvaluationManifest {
            schema_version: 1,
            corpus_version: "test-corpus".to_owned(),
            cases: vec![demo_case("https://demo.example.com/fail-network")],
        };
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let counters = Arc::new(DemoCounters::default());
        let factory = Arc::new(DemoBackendFactory {
            counters: Arc::clone(&counters),
            shutdown_error: false,
        });
        let outcome = run_with_factory(
            RunConfig {
                mode: EvaluationMode::Compare,
                peer_mode: PeerMode::Cold,
                profile: EvaluationProfile::Preflight,
                manifest: manifest_path.clone(),
                case_ids: None,
                category: None,
                tag: None,
                repeat: 1,
                pacing_ms: 0,
                max_calls: 1,
                daily_cap: 1,
                quota_path: std::env::temp_dir().join(format!("allo-fetch-quota-{suffix}.json")),
                output: output_path.clone(),
                status: None,
            },
            factory,
        )
        .await
        .unwrap();
        assert_eq!(outcome.status.completed_attempts, 1);
        assert_eq!(fs::read_to_string(&output_path).unwrap().lines().count(), 1);
        let safety: SafetyReport = read_json(&outcome.safety_path).unwrap();
        assert!(safety.complete);
        assert_eq!(safety.evaluation_profile, EvaluationProfile::Preflight);
        // Compare/Cold closes the per-attempt backend and the shared harness;
        // both shutdowns are expected and neither may be skipped on success.
        assert_eq!(counters.shutdowns.load(Ordering::SeqCst), 2);
        let _ = fs::remove_file(manifest_path);
        let _ = fs::remove_file(output_path);
        let _ = fs::remove_file(outcome.status_path);
        let _ = fs::remove_file(outcome.safety_path);
    }

    #[tokio::test]
    async fn runner_marks_shutdown_failure_as_incomplete_evidence() {
        let suffix = Uuid::now_v7().to_string();
        let manifest_path = std::env::temp_dir().join(format!("allo-fetch-eval-shutdown-{suffix}.json"));
        let output_path = std::env::temp_dir().join(format!("allo-fetch-eval-shutdown-{suffix}.jsonl"));
        let status_path = std::env::temp_dir().join(format!("allo-fetch-eval-shutdown-{suffix}.status.json"));
        let safety_path = std::env::temp_dir().join(format!("allo-fetch-eval-shutdown-{suffix}.safety.json"));
        let manifest = FetchEvaluationManifest {
            schema_version: 1,
            corpus_version: "test-corpus".to_owned(),
            cases: vec![demo_case("https://demo.example.com/shutdown")],
        };
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let counters = Arc::new(DemoCounters::default());
        let result = run_with_factory(
            RunConfig {
                mode: EvaluationMode::Compare,
                peer_mode: PeerMode::Cold,
                profile: EvaluationProfile::Preflight,
                manifest: manifest_path.clone(),
                case_ids: None,
                category: None,
                tag: None,
                repeat: 1,
                pacing_ms: 0,
                max_calls: 1,
                daily_cap: 1,
                quota_path: std::env::temp_dir().join(format!("allo-fetch-quota-{suffix}.json")),
                output: output_path.clone(),
                status: Some(status_path.clone()),
            },
            Arc::new(DemoBackendFactory {
                counters: Arc::clone(&counters),
                shutdown_error: true,
            }),
        )
        .await;
        assert!(result.is_err());
        let status: RunStatus = read_json(&status_path).unwrap();
        assert_eq!(status.stop_reason, "shutdown_failed");
        let safety: SafetyReport = read_json(&safety_path).unwrap();
        assert!(!safety.complete);
        assert!(!safety.all_zero);
        assert_eq!(counters.shutdowns.load(Ordering::SeqCst), 2);
        for path in [manifest_path, output_path, status_path, safety_path] {
            let _ = fs::remove_file(path);
        }
    }

    struct SetupFailureFactory;

    impl EvaluationBackendFactory for SetupFailureFactory {
        fn create(
            &self,
            _control: Option<Arc<dyn ManagedMcpCallControl>>,
        ) -> Result<Arc<dyn EvaluationBackend>, WebError> {
            Err(WebError::Provider("deterministic setup failure".to_owned()))
        }
    }

    struct PostSetupStatusFailureFactory {
        status_path: PathBuf,
        counters: Arc<DemoCounters>,
        mutated: Arc<AtomicUsize>,
    }

    impl EvaluationBackendFactory for PostSetupStatusFailureFactory {
        fn create(
            &self,
            control: Option<Arc<dyn ManagedMcpCallControl>>,
        ) -> Result<Arc<dyn EvaluationBackend>, WebError> {
            if self.mutated.fetch_add(1, Ordering::SeqCst) == 0 {
                fs::remove_file(&self.status_path)
                    .map_err(|error| WebError::Provider(error.to_string()))?;
                fs::create_dir(&self.status_path)
                    .map_err(|error| WebError::Provider(error.to_string()))?;
            }
            EvaluationBackendFactory::create(
                &DemoBackendFactory {
                    counters: Arc::clone(&self.counters),
                    shutdown_error: false,
                },
                control,
            )
        }
    }

    #[tokio::test]
    async fn runner_shutdowns_after_post_setup_evidence_failure() {
        let suffix = Uuid::now_v7().to_string();
        let manifest_path = std::env::temp_dir().join(format!("allo-fetch-eval-write-{suffix}.json"));
        let output_path = std::env::temp_dir().join(format!("allo-fetch-eval-write-{suffix}.jsonl"));
        let status_path = std::env::temp_dir().join(format!("allo-fetch-eval-write-{suffix}.status.json"));
        let manifest = FetchEvaluationManifest {
            schema_version: 1,
            corpus_version: "test-corpus".to_owned(),
            cases: vec![demo_case("https://demo.example.com/fail-network")],
        };
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let counters = Arc::new(DemoCounters::default());
        let result = run_with_factory(
            RunConfig {
                mode: EvaluationMode::Compare,
                peer_mode: PeerMode::Cold,
                profile: EvaluationProfile::Preflight,
                manifest: manifest_path.clone(),
                case_ids: None,
                category: None,
                tag: None,
                repeat: 1,
                pacing_ms: 0,
                max_calls: 1,
                daily_cap: 1,
                quota_path: std::env::temp_dir().join(format!("allo-fetch-quota-{suffix}.json")),
                output: output_path.clone(),
                status: Some(status_path.clone()),
            },
            Arc::new(PostSetupStatusFailureFactory {
                status_path: status_path.clone(),
                counters: Arc::clone(&counters),
                mutated: Arc::new(AtomicUsize::new(0)),
            }),
        )
        .await;
        assert!(result.is_err());
        // Compare/Cold closes the per-attempt backend and then the shared
        // harness even when final evidence persistence fails.
        assert_eq!(counters.shutdowns.load(Ordering::SeqCst), 2);
        let _ = fs::remove_file(manifest_path);
        let _ = fs::remove_file(output_path);
        let _ = fs::remove_dir(status_path);
    }

    #[tokio::test]
    async fn runner_setup_failure_persists_incomplete_evidence() {
        let suffix = Uuid::now_v7().to_string();
        let manifest_path = std::env::temp_dir().join(format!("allo-fetch-eval-setup-{suffix}.json"));
        let output_path = std::env::temp_dir().join(format!("allo-fetch-eval-setup-{suffix}.jsonl"));
        let manifest = FetchEvaluationManifest {
            schema_version: 1,
            corpus_version: "test-corpus".to_owned(),
            cases: vec![demo_case("https://demo.example.com/setup")],
        };
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();

        let result = run_with_factory(
            RunConfig {
                mode: EvaluationMode::Compare,
                peer_mode: PeerMode::Cold,
                profile: EvaluationProfile::Preflight,
                manifest: manifest_path.clone(),
                case_ids: None,
                category: None,
                tag: None,
                repeat: 1,
                pacing_ms: 0,
                max_calls: 1,
                daily_cap: 1,
                quota_path: std::env::temp_dir().join(format!("allo-fetch-quota-{suffix}.json")),
                output: output_path.clone(),
                status: None,
            },
            Arc::new(SetupFailureFactory),
        )
        .await;

        assert!(result.is_err());
        let status_path = default_status_path(&output_path);
        let safety_path = default_safety_path(&output_path);
        let status: RunStatus = read_json(&status_path).unwrap();
        let safety: SafetyReport = read_json(&safety_path).unwrap();
        assert_eq!(status.stop_reason, "setup_failed");
        assert_eq!(status.completed_attempts, 0);
        assert!(safety.complete);
        assert!(!safety.all_zero);
        for path in [manifest_path, output_path, status_path, safety_path] {
            let _ = fs::remove_file(path);
        }
    }

    #[tokio::test]
    async fn runner_ledger_failure_stops_before_later_case() {
        let suffix = Uuid::now_v7().to_string();
        let manifest_path = std::env::temp_dir().join(format!("allo-fetch-eval-ledger-{suffix}.json"));
        let output_path = std::env::temp_dir().join(format!("allo-fetch-eval-ledger-{suffix}.jsonl"));
        let quota_path = std::env::temp_dir().join(format!("allo-fetch-eval-ledger-{suffix}.lock"));
        fs::create_dir(&quota_path).unwrap();
        let manifest = FetchEvaluationManifest {
            schema_version: 1,
            corpus_version: "test-corpus".to_owned(),
            cases: vec![
                demo_case("https://demo.example.com/ledger-one"),
                demo_case("https://demo.example.com/ledger-two"),
            ],
        };
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let outcome = run_with_factory(
            RunConfig {
                mode: EvaluationMode::Compare,
                peer_mode: PeerMode::Cold,
                profile: EvaluationProfile::Preflight,
                manifest: manifest_path.clone(),
                case_ids: None,
                category: None,
                tag: None,
                repeat: 1,
                pacing_ms: 0,
                max_calls: 2,
                daily_cap: 2,
                quota_path: quota_path.clone(),
                output: output_path.clone(),
                status: None,
            },
            Arc::new(DemoBackendFactory {
                counters: Arc::new(DemoCounters::default()),
                shutdown_error: false,
            }),
        )
        .await
        .unwrap();

        assert_eq!(outcome.status.stop_reason, "quota_ledger_failed");
        assert_eq!(outcome.status.completed_attempts, 1);
        assert_eq!(outcome.status.actual_remote_calls, 0);
        assert_eq!(fs::read_to_string(&output_path).unwrap().lines().count(), 1);
        for path in [manifest_path, output_path, outcome.status_path, outcome.safety_path] {
            let _ = fs::remove_file(path);
        }
        let _ = fs::remove_dir(quota_path);
    }

    #[tokio::test]
    async fn runner_source_mismatch_stops_before_later_case() {
        let suffix = Uuid::now_v7().to_string();
        let manifest_path = std::env::temp_dir().join(format!("allo-fetch-eval-source-{suffix}.json"));
        let output_path = std::env::temp_dir().join(format!("allo-fetch-eval-source-{suffix}.jsonl"));
        let manifest = FetchEvaluationManifest {
            schema_version: 1,
            corpus_version: "test-corpus".to_owned(),
            cases: vec![
                demo_case("https://demo.example.com/mismatch"),
                demo_case("https://demo.example.com/source-later"),
            ],
        };
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let outcome = run_with_factory(
            RunConfig {
                mode: EvaluationMode::Compare,
                peer_mode: PeerMode::Cold,
                profile: EvaluationProfile::Preflight,
                manifest: manifest_path.clone(),
                case_ids: None,
                category: None,
                tag: None,
                repeat: 1,
                pacing_ms: 0,
                max_calls: 2,
                daily_cap: 2,
                quota_path: std::env::temp_dir().join(format!("allo-fetch-quota-{suffix}.json")),
                output: output_path.clone(),
                status: None,
            },
            Arc::new(DemoBackendFactory {
                counters: Arc::new(DemoCounters::default()),
                shutdown_error: false,
            }),
        )
        .await
        .unwrap();

        assert_eq!(outcome.status.stop_reason, "safety_violation");
        assert_eq!(outcome.status.completed_attempts, 1);
        assert_eq!(fs::read_to_string(&output_path).unwrap().lines().count(), 1);
        for path in [manifest_path, output_path, outcome.status_path, outcome.safety_path] {
            let _ = fs::remove_file(path);
        }
    }

    #[test]
    fn incomplete_running_safety_cannot_be_zero_gate_evidence() {
        let suffix = Uuid::now_v7().to_string();
        let result_path = std::env::temp_dir().join(format!("allo-incomplete-{suffix}.jsonl"));
        let status_path = std::env::temp_dir().join(format!("allo-incomplete-{suffix}.status.json"));
        let safety_path = std::env::temp_dir().join(format!("allo-incomplete-{suffix}.safety.json"));
        let output_path = std::env::temp_dir().join(format!("allo-incomplete-{suffix}.summary.json"));
        let case = demo_case("https://demo.example.com/incomplete");
        let mut result = FetchEvaluationResult::base(
            &case,
            EvaluationMode::Local,
            PeerMode::Cold,
            "run-incomplete",
            "sha",
            1,
        );
        result.corpus_version = "test-corpus".to_owned();
        let status = RunStatus {
            schema_version: 3,
            scoring_version: SCORING_VERSION.to_owned(),
            evaluation_profile: EvaluationProfile::Diagnostic,
            run_id: "run-incomplete".to_owned(),
            git_sha: "sha".to_owned(),
            corpus_version: "test-corpus".to_owned(),
            dirty_worktree: false,
            planned_attempts: 1,
            completed_attempts: 0,
            actual_remote_calls: 0,
            actual_fetch_calls: 0,
            actual_search_calls: 0,
            recovery_retry_calls: 0,
            retry_limit_violation_count: 0,
            sensitive_egress_count: 0,
            stop_reason: "running".to_owned(),
            retry_after_ms: None,
            cooldown_until_unix: None,
        };
        let safety = SafetyReport {
            schema_version: 2,
            scoring_version: SCORING_VERSION.to_owned(),
            run_id: "run-incomplete".to_owned(),
            git_sha: "sha".to_owned(),
            corpus_version: "test-corpus".to_owned(),
            evaluation_profile: EvaluationProfile::Diagnostic,
            dirty_worktree: false,
            actual_remote_calls: 0,
            actual_fetch_calls: 0,
            actual_search_calls: 0,
            recovery_retry_calls: 0,
            source_mismatch_count: 0,
            dropped_remote_item_count: 0,
            sensitive_egress_count: 0,
            retry_limit_violation_count: 0,
            cancellation_late_result_count: 0,
            cancellation_events_observed: 0,
            stop_reason: "running".to_owned(),
            complete: false,
            all_zero: false,
        };
        fs::write(&result_path, format!("{}\n", serde_json::to_string(&result).unwrap())).unwrap();
        fs::write(&status_path, serde_json::to_vec(&status).unwrap()).unwrap();
        fs::write(&safety_path, serde_json::to_vec(&safety).unwrap()).unwrap();
        let summary = summarize_with_evidence(
            &[result_path.clone()],
            &output_path,
            &[status_path.clone()],
            &[safety_path.clone()],
        )
        .unwrap();
        assert!(!summary.evidence_complete);
        assert!(!summary.safety.all_zero);
        assert_eq!(summary.decision_reason, "incomplete_run");
        for path in [result_path, status_path, safety_path, output_path] {
            let _ = fs::remove_file(path);
        }
    }

    #[test]
    fn summary_rejects_status_safety_counter_mismatch() {
        let suffix = Uuid::now_v7().to_string();
        let result_path = std::env::temp_dir().join(format!("allo-counter-mismatch-{suffix}.jsonl"));
        let output_path = std::env::temp_dir().join(format!("allo-counter-mismatch-{suffix}.summary.json"));
        let case = demo_case("https://demo.example.com/counter-mismatch");
        let mut result = FetchEvaluationResult::base(
            &case,
            EvaluationMode::Local,
            PeerMode::Cold,
            "run-counter-mismatch",
            "sha",
            1,
        );
        result.corpus_version = "test-corpus".to_owned();
        let mut status = completed_status("run-counter-mismatch", "sha", "test-corpus", 1);
        status.sensitive_egress_count = 1;
        let safety = completed_safety("run-counter-mismatch", "sha", "test-corpus");
        let status_path = default_status_path(&result_path);
        let safety_path = default_safety_path(&result_path);
        fs::write(&result_path, format!("{}\n", serde_json::to_string(&result).unwrap())).unwrap();
        fs::write(&status_path, serde_json::to_vec(&status).unwrap()).unwrap();
        fs::write(&safety_path, serde_json::to_vec(&safety).unwrap()).unwrap();

        let summary = summarize_with_evidence(
            &[result_path.clone()],
            &output_path,
            &[status_path.clone()],
            &[safety_path.clone()],
        )
        .unwrap();
        assert!(!summary.evidence_complete);
        assert!(!summary.safety.all_zero);
        assert_eq!(summary.decision_reason, "safety_violation");
        for path in [result_path, status_path, safety_path, output_path] {
            let _ = fs::remove_file(path);
        }
    }

    #[test]
    fn summary_binds_each_run_to_its_own_sidecars_and_record_count() {
        let suffix = Uuid::now_v7().to_string();
        let first_path = std::env::temp_dir().join(format!("allo-run-binding-a-{suffix}.jsonl"));
        let second_path = std::env::temp_dir().join(format!("allo-run-binding-b-{suffix}.jsonl"));
        let output_path = std::env::temp_dir().join(format!("allo-run-binding-{suffix}.summary.json"));
        let case = demo_case("https://demo.example.com/run-binding");
        let mut first = FetchEvaluationResult::base(
            &case,
            EvaluationMode::Local,
            PeerMode::Cold,
            "run-a",
            "sha-a",
            1,
        );
        first.corpus_version = "test-corpus".to_owned();
        let mut second = FetchEvaluationResult::base(
            &case,
            EvaluationMode::Local,
            PeerMode::Cold,
            "run-b",
            "sha-b",
            1,
        );
        second.corpus_version = "test-corpus".to_owned();
        let first_status = completed_status("run-a", "sha-a", "test-corpus", 1);
        let second_status = completed_status("run-b", "sha-b", "test-corpus", 1);
        let first_safety = completed_safety("run-a", "sha-a", "test-corpus");
        let second_safety = completed_safety("run-b", "sha-b", "test-corpus");
        let first_status_path = default_status_path(&first_path);
        let first_safety_path = default_safety_path(&first_path);
        let second_status_path = default_status_path(&second_path);
        let second_safety_path = default_safety_path(&second_path);
        fs::write(&first_path, format!("{}\n", serde_json::to_string(&first).unwrap())).unwrap();
        fs::write(&second_path, format!("{}\n", serde_json::to_string(&second).unwrap())).unwrap();
        fs::write(&first_status_path, serde_json::to_vec(&first_status).unwrap()).unwrap();
        fs::write(&first_safety_path, serde_json::to_vec(&first_safety).unwrap()).unwrap();
        fs::write(&second_status_path, serde_json::to_vec(&second_status).unwrap()).unwrap();
        fs::write(&second_safety_path, serde_json::to_vec(&second_safety).unwrap()).unwrap();

        let summary = summarize_with_evidence(
            &[first_path.clone(), second_path.clone()],
            &output_path,
            &[],
            &[],
        )
        .unwrap();
        assert!(summary.evidence_complete);
        assert_eq!(summary.git_shas, vec!["sha-a".to_owned(), "sha-b".to_owned()]);

        second.git_sha = "wrong-sha".to_owned();
        fs::write(&second_path, format!("{}\n", serde_json::to_string(&second).unwrap())).unwrap();
        let summary = summarize_with_evidence(
            &[first_path.clone(), second_path.clone()],
            &output_path,
            &[],
            &[],
        )
        .unwrap();
        assert!(!summary.evidence_complete);
        second.git_sha = "sha-b".to_owned();
        fs::write(&second_path, format!("{}\n", serde_json::to_string(&second).unwrap())).unwrap();

        let mut incomplete_status = second_status;
        incomplete_status.planned_attempts = 2;
        incomplete_status.completed_attempts = 2;
        fs::write(&second_status_path, serde_json::to_vec(&incomplete_status).unwrap()).unwrap();
        let summary = summarize_with_evidence(
            &[first_path.clone(), second_path.clone()],
            &output_path,
            &[],
            &[],
        )
        .unwrap();
        assert!(!summary.evidence_complete);
        assert_eq!(summary.decision_reason, "incomplete_run");
        for path in [
            first_path,
            second_path,
            first_status_path,
            first_safety_path,
            second_status_path,
            second_safety_path,
            output_path,
        ] {
            let _ = fs::remove_file(path);
        }
    }

    #[test]
    fn atomic_evidence_write_keeps_previous_json_on_serialization_failure() {
        struct FailingSerialize;
        impl Serialize for FailingSerialize {
            fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                Err(serde::ser::Error::custom("intentional failure"))
            }
        }

        let path = std::env::temp_dir().join(format!(
            "allo-atomic-evidence-{}.json",
            Uuid::now_v7()
        ));
        fs::write(&path, br#"{"valid":true}"#).unwrap();
        assert!(atomic_json_write(&path, &FailingSerialize).is_err());
        let value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["valid"], true);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn summary_ignores_diagnostic_modes_and_requires_real_local_failure() {
        let case = FetchEvaluationCase {
            id: "case-summary".to_owned(),
            category: CaseCategory::JavascriptShell,
            url: Some("https://example.invalid/summary".to_owned()),
            url_env: None,
            tags: vec![],
            expected_markers: vec!["Marker".to_owned()],
            minimum_content_chars: 10,
            minimum_marker_hits: 1,
            verified_at: "2026-08-01".to_owned(),
            stale_after_days: 30,
            enabled: true,
            notes: None,
        };
        let mut local = FetchEvaluationResult::base(
            &case,
            EvaluationMode::Local,
            PeerMode::Cold,
            "run",
            "sha",
            1,
        );
        local.remote_eligible = true;
        local.remote_success = true;
        local.effective_success = true;
        let mut mcp = FetchEvaluationResult::base(
            &case,
            EvaluationMode::Mcp,
            PeerMode::Cold,
            "run",
            "sha",
            1,
        );
        mcp.remote_eligible = true;
        mcp.remote_success = true;
        mcp.effective_success = true;
        let summary = summarize_category(
            "javascript_shell".to_owned(),
            vec![&local, &mcp],
            true,
            true,
            false,
        );
        assert_eq!(summary.eligible_case_count, 0);
        assert_eq!(summary.incremental_success_count, 0);
    }

    #[test]
    fn summary_applies_two_of_three_and_warm_e2e_requirement() {
        let case = FetchEvaluationCase {
            id: "case-two-of-three".to_owned(),
            category: CaseCategory::PublicPdfText,
            url: Some("https://example.invalid/two-of-three".to_owned()),
            url_env: None,
            tags: vec![],
            expected_markers: vec!["Marker".to_owned()],
            minimum_content_chars: 10,
            minimum_marker_hits: 1,
            verified_at: "2026-08-01".to_owned(),
            stale_after_days: 30,
            enabled: true,
            notes: None,
        };
        let mut cold = FetchEvaluationResult::base(
            &case,
            EvaluationMode::Compare,
            PeerMode::Cold,
            "run",
            "sha",
            1,
        );
        cold.local_failure_kind = Some("pdf".to_owned());
        cold.remote_eligible = true;
        cold.remote_attempted = true;
        cold.remote_success = true;
        cold.effective_success = true;
        cold.quality_grade = QualityGrade::Q2;

        let mut warm_success = cold.clone();
        warm_success.mode = EvaluationMode::E2e;
        warm_success.peer_mode = PeerMode::Warm;
        warm_success.attempt = 2;
        warm_success.elapsed_ms = 100;

        let mut warm_failure = cold.clone();
        warm_failure.mode = EvaluationMode::E2e;
        warm_failure.peer_mode = PeerMode::Warm;
        warm_failure.attempt = 3;
        warm_failure.remote_success = false;
        warm_failure.effective_success = false;
        warm_failure.quality_grade = QualityGrade::Q0;
        warm_failure.elapsed_ms = 200;

        let summary = summarize_category(
            "public_pdf_text".to_owned(),
            vec![&cold, &warm_success, &warm_failure],
            true,
            true,
            false,
        );
        assert_eq!(summary.eligible_case_count, 1);
        assert_eq!(summary.incremental_success_count, 1);
        assert_eq!(summary.quality_q2_plus_rate, 1.0);
        assert_eq!(summary.warm_p50_ms, Some(100));
    }

    #[test]
    fn summary_allows_two_of_three_real_local_failures() {
        let case = FetchEvaluationCase {
            id: "case-two-local-failures".to_owned(),
            category: CaseCategory::JavascriptShell,
            url: Some("https://example.invalid/two-local-failures".to_owned()),
            url_env: None,
            tags: vec![],
            expected_markers: vec!["Marker".to_owned()],
            minimum_content_chars: 10,
            minimum_marker_hits: 1,
            verified_at: "2026-08-01".to_owned(),
            stale_after_days: 30,
            enabled: true,
            notes: None,
        };
        let mut cold = FetchEvaluationResult::base(
            &case,
            EvaluationMode::Compare,
            PeerMode::Cold,
            "run",
            "sha",
            1,
        );
        cold.local_failure_kind = Some("network".to_owned());
        cold.remote_eligible = true;
        cold.remote_attempted = true;
        cold.remote_success = true;
        cold.effective_success = true;
        cold.quality_grade = QualityGrade::Q2;

        let mut warm_success = cold.clone();
        warm_success.mode = EvaluationMode::E2e;
        warm_success.peer_mode = PeerMode::Warm;
        warm_success.attempt = 2;
        warm_success.elapsed_ms = 100;

        let mut warm_local_success = cold.clone();
        warm_local_success.mode = EvaluationMode::E2e;
        warm_local_success.peer_mode = PeerMode::Warm;
        warm_local_success.attempt = 3;
        warm_local_success.local_failure_kind = None;
        warm_local_success.remote_eligible = false;
        warm_local_success.remote_attempted = false;
        warm_local_success.remote_success = false;
        warm_local_success.effective_success = true;
        warm_local_success.local_success = true;
        warm_local_success.elapsed_ms = 200;

        let summary = summarize_category(
            "javascript_shell".to_owned(),
            vec![&cold, &warm_success, &warm_local_success],
            true,
            true,
            false,
        );
        assert_eq!(summary.eligible_case_count, 1);
        assert_eq!(summary.incremental_success_count, 1);
        assert_eq!(summary.warm_p50_ms, Some(100));
    }

    #[test]
    fn admission_rejects_duplicate_phases() {
        let case = demo_case("https://demo.example.com/duplicate-phase");
        let mut first = FetchEvaluationResult::base(
            &case,
            EvaluationMode::Compare,
            PeerMode::Cold,
            "run",
            "sha",
            1,
        );
        first.evaluation_profile = EvaluationProfile::Admission;
        let mut duplicate = first.clone();
        duplicate.attempt = 2;
        assert!(validate_admission_composition(&[first, duplicate]).is_err());
    }

    #[test]
    fn admission_requires_one_exact_three_phase_triple() {
        let case = demo_case("https://demo.example.com/exact-triple");
        let mut cold = FetchEvaluationResult::base(
            &case,
            EvaluationMode::Compare,
            PeerMode::Cold,
            "run",
            "sha",
            1,
        );
        cold.evaluation_profile = EvaluationProfile::Admission;
        let mut warm_one = FetchEvaluationResult::base(
            &case,
            EvaluationMode::E2e,
            PeerMode::Warm,
            "run",
            "sha",
            2,
        );
        warm_one.evaluation_profile = EvaluationProfile::Admission;
        let mut warm_two = warm_one.clone();
        warm_two.attempt = 3;
        assert!(validate_admission_composition(&[cold.clone(), warm_one.clone(), warm_two.clone()]).is_ok());

        let mut cross_run = warm_two.clone();
        cross_run.run_id = "other-run".to_owned();
        assert!(validate_admission_composition(&[cold.clone(), warm_one.clone(), cross_run]).is_err());

        let mut wrong_phase = warm_two.clone();
        wrong_phase.mode = EvaluationMode::Compare;
        assert!(validate_admission_composition(&[cold.clone(), warm_one, wrong_phase]).is_err());

        assert!(validate_admission_composition(&[cold, warm_two.clone()]).is_err());
        let mut extra = warm_two;
        extra.attempt = 4;
        assert!(validate_admission_composition(&[
            FetchEvaluationResult::base(
                &case,
                EvaluationMode::Compare,
                PeerMode::Cold,
                "run",
                "sha",
                1,
            ),
            extra,
        ]).is_err());
    }

    #[tokio::test]
    async fn demo_covers_four_modes_and_safety_cases() {
        let path = std::env::temp_dir().join(format!("allo-fetch-eval-demo-{}.json", Uuid::now_v7()));
        let report = run_demo(&path).await.unwrap();
        assert!(report.passed);
        assert_eq!(report.modes_checked, 4);
        assert_eq!(report.search_warmups, 1);
        assert!(report.warm_fetch_warmups >= 1);
        assert_eq!(report.cancellation_late_result_count, 0);
        let _ = fs::remove_file(path);
    }
}
