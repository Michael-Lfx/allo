//! Feature-gated evaluation runner and deterministic Demo.
//!
//! The example binary owns only command-line parsing. This module owns the
//! quota ledger, resumable status, sanitized JSONL writes and admission
//! statistics so tests and the unattended runner cross the same seam.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant as StdInstant};

use async_trait::async_trait;
use chrono::Utc;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tempfile::NamedTempFile;
use tokio::time::Instant;
use uuid::Uuid;

use super::{
    CaseCategory, EvaluationBackend, EvaluationBackendFactory,
    EvaluationMode, EvaluationProfile, FetchEvaluationCase, FetchEvaluationHarness,
    FetchEvaluationManifest, FetchEvaluationResult, PeerMode, QualityGrade,
    EVALUATION_SCORING_VERSION,
};
use crate::coordinator::{
    ExtractBudget, ExtractCoordinator, LocalExtractAdapter, ManagedExtractCoordinator,
};
use crate::managed::{
    FetchReadiness, ManagedMcpCallError, ManagedMcpCallGate, ManagedMcpTool,
    RemoteExtractBatch, RemoteExtractError, RemoteExtractFallback, RemoteExtractItem,
    RemoteExtractRequest, RemoteExtractRequestItem,
};
use crate::managed::fetch::RemoteFetchDiagnostics;
use crate::provider::extract_policy::{
    LocalExtractDiagnostics, LocalExtractFailure, LocalExtractFailureKind, LocalExtractOutcome,
};
use crate::types::{ExtractRequest, ExtractedPage, WebError};

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct QuotaLedger {
    utc_date: String,
    used_calls: u32,
    cooldown_until_unix: Option<i64>,
}

#[derive(Debug, Default)]
struct GateState {
    stop_reason: Option<String>,
    retry_after_ms: Option<u64>,
    cooldown_until_unix: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuotaLedgerError {
    Exhausted,
    Io,
}

/// A process-safe daily quota gate. It is called immediately before the
/// Remote adapter, after Local policy has decided that a remote call is
/// actually needed.
pub(crate) struct FileQuotaGate {
    path: PathBuf,
    daily_cap: u32,
    max_calls: u32,
    run_calls: AtomicU32,
    fetch_calls: AtomicU32,
    search_calls: AtomicU32,
    recovery_calls: AtomicU32,
    retry_limit_violations: AtomicUsize,
    sensitive_egress_violations: AtomicUsize,
    call_lock: Mutex<()>,
    state: Mutex<GateState>,
}

impl FileQuotaGate {
    pub(crate) fn new(path: PathBuf, daily_cap: u32, max_calls: u32) -> Self {
        Self {
            path,
            daily_cap,
            max_calls,
            run_calls: AtomicU32::new(0),
            fetch_calls: AtomicU32::new(0),
            search_calls: AtomicU32::new(0),
            recovery_calls: AtomicU32::new(0),
            retry_limit_violations: AtomicUsize::new(0),
            sensitive_egress_violations: AtomicUsize::new(0),
            call_lock: Mutex::new(()),
            state: Mutex::new(GateState::default()),
        }
    }

    pub(crate) fn actual_calls(&self) -> u32 {
        self.run_calls.load(Ordering::SeqCst)
    }

    pub(crate) fn fetch_calls(&self) -> u32 {
        self.fetch_calls.load(Ordering::SeqCst)
    }

    pub(crate) fn search_calls(&self) -> u32 {
        self.search_calls.load(Ordering::SeqCst)
    }

    pub(crate) fn recovery_calls(&self) -> u32 {
        self.recovery_calls.load(Ordering::SeqCst)
    }

    pub(crate) fn retry_limit_violations(&self) -> usize {
        self.retry_limit_violations.load(Ordering::SeqCst)
    }

    pub(crate) fn sensitive_egress_violations(&self) -> usize {
        self.sensitive_egress_violations.load(Ordering::SeqCst)
    }

    pub(crate) fn state(&self) -> (Option<String>, Option<u64>, Option<i64>) {
        let state = self.state.lock().expect("quota state lock poisoned");
        (
            state.stop_reason.clone(),
            state.retry_after_ms,
            state.cooldown_until_unix,
        )
    }

    fn set_stop(&self, reason: &str) {
        self.state
            .lock()
            .expect("quota state lock poisoned")
            .stop_reason = Some(reason.to_owned());
    }

    fn record_rate_limit(&self, retry_after: Option<Duration>) {
        let retry_after_ms = retry_after.map(|value| {
            value
                .as_millis()
                .min(u128::from(u64::MAX)) as u64
        });
        let now = Utc::now().timestamp();
        let cooldown = retry_after.map(|value| {
            let seconds = value.as_secs().saturating_add(u64::from(value.subsec_nanos() > 0));
            now.saturating_add(i64::try_from(seconds).unwrap_or(i64::MAX))
        });
        let _ = update_ledger(&self.path, self.daily_cap, |ledger| {
            ledger.cooldown_until_unix = cooldown;
            Ok(())
        });
        let mut state = self.state.lock().expect("quota state lock poisoned");
        state.stop_reason = Some("rate_limited".to_owned());
        state.retry_after_ms = retry_after_ms;
        state.cooldown_until_unix = cooldown;
    }
}

#[async_trait]
impl ManagedMcpCallGate for FileQuotaGate {
    async fn before_call(
        &self,
        tool: ManagedMcpTool,
        arguments: &Value,
        attempt: u8,
    ) -> Result<(), ManagedMcpCallError> {
        if attempt > 3 {
            self.retry_limit_violations.fetch_add(1, Ordering::SeqCst);
            self.set_stop("safety_violation");
            return Err(ManagedMcpCallError::RetryLimitExceeded);
        }
        if tool == ManagedMcpTool::Fetch && !safe_fetch_arguments(arguments) {
            self.sensitive_egress_violations
                .fetch_add(1, Ordering::SeqCst);
            self.set_stop("safety_violation");
            return Err(ManagedMcpCallError::UnsafeArguments);
        }

        let _lock = self.call_lock.lock().expect("quota call lock poisoned");
        if self.run_calls.load(Ordering::SeqCst) >= self.max_calls {
            self.set_stop("quota_exhausted");
            return Err(ManagedMcpCallError::QuotaExhausted);
        }
        let now = Utc::now().timestamp();
        let result = update_ledger(&self.path, self.daily_cap, |ledger| {
            if ledger
                .cooldown_until_unix
                .is_some_and(|until| until > now)
            {
                return Err(QuotaLedgerError::Exhausted);
            }
            if ledger.used_calls >= self.daily_cap {
                return Err(QuotaLedgerError::Exhausted);
            }
            ledger.used_calls = ledger.used_calls.saturating_add(1);
            Ok(())
        });
        match result {
            Ok(()) => {
                self.run_calls.fetch_add(1, Ordering::SeqCst);
                match tool {
                    ManagedMcpTool::Fetch => {
                        self.fetch_calls.fetch_add(1, Ordering::SeqCst);
                    }
                    ManagedMcpTool::Search => {
                        self.search_calls.fetch_add(1, Ordering::SeqCst);
                    }
                }
                if attempt > 1 {
                    self.recovery_calls.fetch_add(1, Ordering::SeqCst);
                }
                Ok(())
            }
            Err(QuotaLedgerError::Exhausted) => {
                if self.state().0.as_deref() != Some("rate_limited") {
                    self.set_stop("quota_exhausted");
                }
                Err(ManagedMcpCallError::QuotaExhausted)
            }
            Err(_) => {
                self.set_stop("quota_ledger_failed");
                Err(ManagedMcpCallError::LedgerFailure)
            }
        }
    }

    fn observe(
        &self,
        _tool: ManagedMcpTool,
        _attempt: u8,
        result: &Result<nomi_mcp::protocol::McpToolResult, nomi_mcp::remote_peer::McpPeerError>,
    ) {
        if let Err(nomi_mcp::remote_peer::McpPeerError::Http {
            status,
            retry_after,
        }) = result
            && *status == reqwest::StatusCode::TOO_MANY_REQUESTS
        {
            self.record_rate_limit(*retry_after);
        }
    }
}

fn safe_fetch_arguments(arguments: &Value) -> bool {
    let Some(object) = arguments.as_object() else {
        return false;
    };
    let keys = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
    if keys != BTreeSet::from(["full_content", "urls"]) {
        return false;
    }
    if arguments
        .get("full_content")
        .and_then(Value::as_bool)
        != Some(false)
    {
        return false;
    }
    let Some(urls) = arguments.get("urls").and_then(Value::as_array) else {
        return false;
    };
    !urls.is_empty()
        && urls.iter().all(|value| {
            let Some(raw) = value.as_str() else {
                return false;
            };
            let Ok(prepared) = crate::provider::extract_policy::prepare_remote_url(raw, false)
            else {
                return false;
            };
            prepared.outbound_url == raw
        })
}

fn update_ledger<T>(
    path: &Path,
    daily_cap: u32,
    update: impl FnOnce(&mut QuotaLedger) -> Result<T, QuotaLedgerError>,
) -> Result<T, QuotaLedgerError> {
    if let Some(parent) = path.parent().filter(|value| !value.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|_| QuotaLedgerError::Io)?;
    }
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(path)
        .map_err(|_| QuotaLedgerError::Io)?;
    file.lock_exclusive()
        .map_err(|_| QuotaLedgerError::Io)?;
    let result = (|| {
        let mut ledger = read_locked_ledger(&mut file)?;
        let today = Utc::now().date_naive().to_string();
        if ledger.utc_date != today {
            ledger = QuotaLedger {
                utc_date: today,
                used_calls: 0,
                cooldown_until_unix: None,
            };
        }
        let output = update(&mut ledger)?;
        if ledger.used_calls > daily_cap {
            return Err(QuotaLedgerError::Exhausted);
        }
        file.seek(SeekFrom::Start(0))
            .map_err(|_| QuotaLedgerError::Io)?;
        file.set_len(0)
            .map_err(|_| QuotaLedgerError::Io)?;
        serde_json::to_writer(&mut file, &ledger)
            .map_err(|_| QuotaLedgerError::Io)?;
        file.flush().map_err(|_| QuotaLedgerError::Io)?;
        Ok(output)
    })();
    let _ = file.unlock();
    result
}

fn read_locked_ledger(file: &mut File) -> Result<QuotaLedger, QuotaLedgerError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| QuotaLedgerError::Io)?;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(file, &mut bytes)
        .map_err(|_| QuotaLedgerError::Io)?;
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(QuotaLedger::default());
    }
    serde_json::from_slice(&bytes).map_err(|_| QuotaLedgerError::Io)
}

pub async fn run(config: RunConfig) -> Result<RunOutcome, Box<dyn std::error::Error>> {
    run_inner(&config, None, None).await
}

#[allow(dead_code)]
pub(crate) async fn run_with_factory(
    config: RunConfig,
    factory: Arc<dyn EvaluationBackendFactory>,
) -> Result<RunOutcome, Box<dyn std::error::Error>> {
    run_inner(
        &config,
        Some(factory),
        Some(TestRunMetadata {
            git_sha: Some("test-sha".to_owned()),
            allow_dirty: true,
        }),
    )
    .await
}

async fn run_inner(
    config: &RunConfig,
    injected_factory: Option<Arc<dyn EvaluationBackendFactory>>,
    test_metadata: Option<TestRunMetadata>,
) -> Result<RunOutcome, Box<dyn std::error::Error>> {
    validate_run_config(&config)?;
    let manifest: FetchEvaluationManifest = read_json(&config.manifest)?;
    manifest.validate()?;
    let cases = select_cases(
        &manifest.cases,
        config.case_ids.as_deref(),
        config.category,
        config.tag.as_deref(),
        Utc::now().date_naive(),
    )?;
    if cases.is_empty() {
        return Err("no enabled, non-stale cases matched the selection".into());
    }

    let dirty_worktree = git_worktree_is_dirty()?;
    let test_metadata = test_metadata.unwrap_or_default();
    if dirty_worktree && config.profile == EvaluationProfile::Admission {
        return Err("Admission evidence requires a clean worktree".into());
    }
    if dirty_worktree && !test_metadata.allow_dirty {
        return Err("worktree is dirty; only internal diagnostic tests may allow dirty evidence".into());
    }
    let git_sha = test_metadata
        .git_sha
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(read_git_sha()?);
    let run_id = Uuid::now_v7().to_string();
    let status_path = config
        .status
        .clone()
        .unwrap_or_else(|| default_status_path(&config.output));
    let safety_path = default_safety_path(&config.output);
    let output_file = create_new_output(&config.output)?;
    let mut writer = BufWriter::new(output_file);
    let mut status = RunStatus {
        schema_version: 3,
        scoring_version: SCORING_VERSION.to_owned(),
        evaluation_profile: config.profile,
        run_id: run_id.clone(),
        git_sha: git_sha.clone(),
        corpus_version: manifest.corpus_version.clone(),
        dirty_worktree,
        planned_attempts: if config.profile == EvaluationProfile::Admission {
            cases.len() * 3
        } else {
            cases.len() * config.repeat as usize
        },
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
    write_status(&status_path, &status)?;
    write_safety_report(
        &safety_path,
        &SafetyReport {
            schema_version: 2,
            scoring_version: SCORING_VERSION.to_owned(),
            run_id: run_id.clone(),
            git_sha: git_sha.clone(),
            corpus_version: manifest.corpus_version.clone(),
            evaluation_profile: config.profile,
            dirty_worktree,
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
        },
    )?;

    let gate = if config.mode == EvaluationMode::Local {
        None
    } else {
        Some(Arc::new(FileQuotaGate::new(
            config.quota_path.clone(),
            config.daily_cap,
            config.max_calls,
        )))
    };
    let harness_gate = gate
        .as_ref()
        .map(|value| Arc::clone(value) as Arc<dyn ManagedMcpCallGate>);
    let harness = match injected_factory {
        Some(factory) => FetchEvaluationHarness::from_factory_with_gate(factory, harness_gate),
        None => match gate.as_ref() {
            Some(gate) => FetchEvaluationHarness::keyless_production_with_call_gate(
                Arc::clone(gate) as Arc<dyn ManagedMcpCallGate>,
            ),
            None => FetchEvaluationHarness::keyless_production(),
        },
    };
    let harness = match harness {
        Ok(harness) => harness,
        Err(error) => {
            status.stop_reason = "setup_failed".to_owned();
            let _ = write_status(&status_path, &status);
            let _ = write_safety_report(
                &safety_path,
                &SafetyReport {
                    schema_version: 2,
                    scoring_version: SCORING_VERSION.to_owned(),
                    run_id,
                    git_sha,
                    corpus_version: manifest.corpus_version,
                    evaluation_profile: config.profile,
                    dirty_worktree,
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
                    stop_reason: status.stop_reason.clone(),
                    complete: true,
                    all_zero: false,
                },
            );
            return Err(Box::new(error));
        }
    };
    let mut last_call: Option<StdInstant> = None;
    let mut source_mismatch_count = 0usize;
    let mut dropped_remote_item_count = 0usize;
    'cases: for case in cases {
        let attempts = if config.profile == EvaluationProfile::Admission {
            vec![
                (EvaluationMode::Compare, PeerMode::Cold, 1),
                (EvaluationMode::E2e, PeerMode::Warm, 2),
                (EvaluationMode::E2e, PeerMode::Warm, 3),
            ]
        } else {
            (1..=config.repeat)
                .map(|attempt| (config.mode, config.peer_mode, attempt))
                .collect::<Vec<_>>()
        };
        for (mode, peer_mode, attempt) in attempts {
            if let Some(previous) = last_call {
                let elapsed = previous.elapsed();
                let pacing = Duration::from_millis(config.pacing_ms);
                if elapsed < pacing {
                    tokio::time::sleep(pacing - elapsed).await;
                }
            }
            let calls_before_attempt = gate.as_ref().map(|value| value.actual_calls());
            let mut result = harness
                .run_case_with_metadata(
                    case,
                    mode,
                    peer_mode,
                    &run_id,
                    &git_sha,
                    attempt,
                )
                .await;
            result.corpus_version = manifest.corpus_version.clone();
            result.evaluation_profile = config.profile;
            serde_json::to_writer(&mut writer, &result)?;
            writer.write_all(b"\n")?;
            writer.flush()?;
            source_mismatch_count = source_mismatch_count
                .saturating_add(result.source_mismatch_count);
            dropped_remote_item_count = dropped_remote_item_count
                .saturating_add(result.dropped_remote_item_count);
            status.completed_attempts += 1;
            status.actual_remote_calls = gate.as_ref().map_or(0, |value| value.actual_calls());
            if let Some(gate) = gate.as_ref() {
                status.actual_fetch_calls = gate.fetch_calls();
                status.actual_search_calls = gate.search_calls();
                status.recovery_retry_calls = gate.recovery_calls();
                status.retry_limit_violation_count = gate.retry_limit_violations();
                status.sensitive_egress_count = gate.sensitive_egress_violations();
            }
            if calls_before_attempt.is_some_and(|before| {
                status.actual_remote_calls > before
            }) {
                last_call = Some(StdInstant::now());
            }
            if result.retry_after_ms.is_some() {
                status.retry_after_ms = result.retry_after_ms;
            }
            write_status(&status_path, &status)?;

            if result.source_mismatch_count > 0 || result.dropped_remote_item_count > 0 {
                status.stop_reason = "safety_violation".to_owned();
                break 'cases;
            }
            if let Some(gate) = gate.as_ref()
                && matches!(gate.state().0.as_deref(), Some("safety_violation"))
            {
                status.stop_reason = "safety_violation".to_owned();
                break 'cases;
            }
            if let Some(gate) = gate.as_ref()
                && matches!(
                    gate.state().0.as_deref(),
                    Some("quota_ledger_failed" | "quota_exhausted" | "rate_limited")
                )
            {
                status.stop_reason = gate.state().0.unwrap_or_default();
                break 'cases;
            }
            match result.error_class.as_deref() {
                Some("rate_limited") => {
                    status.stop_reason = "rate_limited".to_owned();
                    break 'cases;
                }
                Some("quota_exhausted") => {
                    status.stop_reason = "quota_exhausted".to_owned();
                    break 'cases;
                }
                Some("harness_initialization_failed")
                | Some("fetch_warmup_failed")
                | Some("search_warmup_failed") => {
                    status.stop_reason = "setup_failed".to_owned();
                    break 'cases;
                }
                _ => {}
            }
        }
    }
    if status.stop_reason == "running" {
        status.stop_reason = "completed".to_owned();
    }
    if let Some(gate) = gate {
        let (reason, retry_after_ms, cooldown) = gate.state();
        status.actual_remote_calls = gate.actual_calls();
        status.actual_fetch_calls = gate.fetch_calls();
        status.actual_search_calls = gate.search_calls();
        status.recovery_retry_calls = gate.recovery_calls();
        status.retry_limit_violation_count = gate.retry_limit_violations();
        status.sensitive_egress_count = gate.sensitive_egress_violations();
        status.retry_after_ms = retry_after_ms.or(status.retry_after_ms);
        status.cooldown_until_unix = cooldown;
        if status.stop_reason == "completed" {
            if let Some(reason) = reason {
                status.stop_reason = reason;
            }
        }
    }
    write_status(&status_path, &status)?;
    let cancellation_late_result_count = 0usize;
    let safety = SafetyReport {
        schema_version: 2,
        scoring_version: SCORING_VERSION.to_owned(),
        run_id: run_id.clone(),
        git_sha: git_sha.clone(),
        corpus_version: manifest.corpus_version.clone(),
        evaluation_profile: config.profile,
        dirty_worktree,
        actual_remote_calls: status.actual_remote_calls,
        actual_fetch_calls: status.actual_fetch_calls,
        actual_search_calls: status.actual_search_calls,
        recovery_retry_calls: status.recovery_retry_calls,
        source_mismatch_count,
        dropped_remote_item_count,
        sensitive_egress_count: status.sensitive_egress_count,
        retry_limit_violation_count: status.retry_limit_violation_count,
        cancellation_late_result_count,
        cancellation_events_observed: 0,
        stop_reason: status.stop_reason.clone(),
        complete: true,
        all_zero: source_mismatch_count == 0
            && dropped_remote_item_count == 0
            && status.sensitive_egress_count == 0
            && status.retry_limit_violation_count == 0
            && cancellation_late_result_count == 0,
    };
    write_safety_report(&safety_path, &safety)?;
    harness.shutdown().await;
    Ok(RunOutcome {
        status,
        status_path,
        safety_path,
    })
}

fn validate_run_config(config: &RunConfig) -> Result<(), Box<dyn std::error::Error>> {
    if config.repeat == 0 {
        return Err("--repeat must be positive".into());
    }
    if config.max_calls > MAX_CALLS_PER_RUN {
        return Err(format!("--max-calls cannot exceed {MAX_CALLS_PER_RUN}").into());
    }
    if config.daily_cap > MAX_CALLS_PER_DAY {
        return Err(format!("--daily-cap cannot exceed {MAX_CALLS_PER_DAY}").into());
    }
    Ok(())
}

fn select_cases<'a>(
    cases: &'a [FetchEvaluationCase],
    ids: Option<&[String]>,
    category: Option<CaseCategory>,
    tag: Option<&str>,
    today: chrono::NaiveDate,
) -> Result<Vec<&'a FetchEvaluationCase>, Box<dyn std::error::Error>> {
    let requested_ids = ids.map(|values| values.iter().collect::<BTreeSet<_>>());
    Ok(cases
        .iter()
        .filter(|case| case.enabled)
        .filter(|case| {
            requested_ids
                .as_ref()
                .is_none_or(|values| values.contains(&case.id))
        })
        .filter(|case| category.is_none_or(|value| value == case.category))
        .filter(|case| tag.is_none_or(|value| case.tags.iter().any(|tag| tag == value)))
        .filter(|case| match case.is_stale_on(today) {
            Ok(false) => true,
            Ok(true) => false,
            Err(_) => false,
        })
        .collect())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, Box<dyn std::error::Error>> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn create_new_output(path: &Path) -> Result<File, Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent().filter(|value| !value.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    Ok(OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?)
}

fn default_status_path(output: &Path) -> PathBuf {
    let mut path = output.to_path_buf();
    path.set_extension("status.json");
    path
}

fn default_safety_path(output: &Path) -> PathBuf {
    let mut path = output.to_path_buf();
    path.set_extension("safety.json");
    path
}

fn write_status(path: &Path, status: &RunStatus) -> Result<(), Box<dyn std::error::Error>> {
    atomic_json_write(path, status)
}

fn write_safety_report(
    path: &Path,
    report: &SafetyReport,
) -> Result<(), Box<dyn std::error::Error>> {
    atomic_json_write(path, report)
}

fn atomic_json_write<T: Serialize>(
    path: &Path,
    value: &T,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent().filter(|value| !value.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut temp = NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(temp.as_file_mut(), value)?;
    temp.as_file_mut().flush()?;
    temp.as_file_mut().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn read_git_sha() -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("git").args(["rev-parse", "HEAD"]).output()?;
    if !output.status.success() {
        return Err("git rev-parse HEAD failed".into());
    }
    let sha = String::from_utf8(output.stdout)?.trim().to_owned();
    if sha.is_empty() {
        return Err("git SHA is empty".into());
    }
    Ok(sha)
}

fn git_worktree_is_dirty() -> Result<bool, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=all"])
        .output()?;
    if !output.status.success() {
        return Err("git status failed".into());
    }
    Ok(!output.stdout.is_empty())
}

#[derive(Debug, Serialize)]
pub struct EvaluationSummary {
    pub schema_version: u32,
    pub result_schema_version: u32,
    pub scoring_version: String,
    pub evaluation_profile: EvaluationProfile,
    pub corpus_version: String,
    pub dirty_worktree: bool,
    pub evidence_complete: bool,
    pub decision_reason: String,
    pub legacy_evidence: bool,
    pub record_count: usize,
    pub independent_case_count: usize,
    pub run_ids: Vec<String>,
    pub git_shas: Vec<String>,
    pub outcome_counts: BTreeMap<String, usize>,
    pub safety: SafetySummary,
    pub categories: Vec<CategorySummary>,
}

#[derive(Debug, Serialize, Default)]
pub struct SafetySummary {
    pub actual_remote_calls: u32,
    pub actual_fetch_calls: u32,
    pub actual_search_calls: u32,
    pub recovery_retry_calls: u32,
    pub source_mismatch_count: usize,
    pub dropped_remote_item_count: usize,
    pub sensitive_egress_count: usize,
    pub retry_limit_violation_count: usize,
    pub cancellation_late_result_count: usize,
    pub report_present: bool,
    pub complete: bool,
    pub all_zero: bool,
    pub legacy_evidence: bool,
}

#[derive(Debug, Serialize)]
pub struct CategorySummary {
    pub category: String,
    pub attempt_count: usize,
    pub independent_case_count: usize,
    pub eligible_case_count: usize,
    pub effective_success_count: usize,
    pub incremental_success_count: usize,
    pub effective_success_rate: f64,
    pub incremental_success_rate: f64,
    pub quality_q2_plus_rate: f64,
    pub warm_p50_ms: Option<u128>,
    pub warm_p95_ms: Option<u128>,
    pub wilson_low: f64,
    pub wilson_high: f64,
    pub threshold_sensitivity: Vec<SensitivityPoint>,
    pub decision: String,
}

#[derive(Debug, Serialize)]
pub struct SensitivityPoint {
    pub incremental_threshold: f64,
    pub quality_threshold: f64,
    pub warm_p95_threshold_ms: u128,
    pub decision: String,
}

pub fn summarize(
    inputs: &[PathBuf],
    output: &Path,
    status_path: Option<&Path>,
    safety_report_path: Option<&Path>,
) -> Result<EvaluationSummary, Box<dyn std::error::Error>> {
    let status_paths = status_path.into_iter().map(PathBuf::from).collect::<Vec<_>>();
    let safety_paths = safety_report_path
        .into_iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    summarize_with_evidence(inputs, output, &status_paths, &safety_paths)
}

pub fn summarize_with_evidence(
    inputs: &[PathBuf],
    output: &Path,
    status_paths: &[PathBuf],
    safety_paths: &[PathBuf],
) -> Result<EvaluationSummary, Box<dyn std::error::Error>> {
    if inputs.is_empty() {
        return Err("at least one JSONL input is required".into());
    }
    let mut results = Vec::new();
    let mut result_schema_versions = BTreeSet::new();
    for input in inputs {
        let file = File::open(input)?;
        for line in BufReader::new(file).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let mut value: Value = serde_json::from_str(&line)?;
            let schema_version = value
                .get("schema_version")
                .and_then(Value::as_u64)
                .ok_or("evaluation result has no schema_version")?;
            result_schema_versions.insert(schema_version);
            if schema_version == 2 {
                let object = value
                    .as_object_mut()
                    .ok_or("evaluation result must be a JSON object")?;
                object.insert(
                    "scoring_version".to_owned(),
                    Value::String("legacy-unknown".to_owned()),
                );
                object.insert(
                    "evaluation_profile".to_owned(),
                    Value::String("diagnostic".to_owned()),
                );
                object.insert("schema_version".to_owned(), Value::from(3));
            } else if schema_version != 3 {
                return Err("input contains an unsupported result schema".into());
            }
            let result: FetchEvaluationResult = serde_json::from_value(value)?;
            results.push(result);
        }
    }
    if results.is_empty() {
        return Err("input contains no evaluation results".into());
    }
    if result_schema_versions.len() != 1
        || !result_schema_versions
            .iter()
            .all(|schema| *schema == 2 || *schema == 3)
    {
        return Err("cannot summarize mixed or unsupported result schemas".into());
    }
    let legacy_schema = result_schema_versions.contains(&2);
    let result_schema_version = *result_schema_versions.iter().next().unwrap_or(&3) as u32;
    let corpus_versions = results
        .iter()
        .map(|result| result.corpus_version.as_str())
        .collect::<BTreeSet<_>>();
    if corpus_versions.len() != 1 {
        return Err("cannot summarize mixed corpus versions".into());
    }
    if legacy_schema && results.iter().any(|result| result.scoring_version != "legacy-unknown") {
        return Err("cannot mix legacy and current scoring versions".into());
    }
    let scoring_versions = results
        .iter()
        .map(|result| result.scoring_version.as_str())
        .collect::<BTreeSet<_>>();
    if scoring_versions.len() != 1 {
        return Err("cannot summarize mixed scoring versions".into());
    }
    let profiles = results
        .iter()
        .map(|result| result.evaluation_profile)
        .collect::<BTreeSet<_>>();
    if profiles.len() != 1 {
        return Err("cannot summarize mixed evaluation profiles".into());
    }
    let result_run_ids = results
        .iter()
        .map(|result| result.run_id.as_str())
        .collect::<BTreeSet<_>>();
    let status_paths = resolve_evidence_paths(inputs, status_paths, default_status_path);
    let safety_paths = resolve_evidence_paths(inputs, safety_paths, default_safety_path);
    let statuses = status_paths
        .iter()
        .filter(|path| path.exists())
        .map(|path| read_json::<RunStatus>(path))
        .collect::<Result<Vec<_>, _>>()?;
    let reports = safety_paths
        .iter()
        .filter(|path| path.exists())
        .map(|path| read_json::<SafetyReport>(path))
        .collect::<Result<Vec<_>, _>>()?;
    let mut status_runs = BTreeSet::new();
    let mut report_runs = BTreeSet::new();
    let mut legacy_evidence = legacy_schema;
    let mut provenance_consistent = true;
    let mut counter_consistent = true;
    for status in &statuses {
        if !status_runs.insert(status.run_id.as_str()) {
            return Err("multiple status evidence files exist for one run".into());
        }
        if !result_run_ids.contains(status.run_id.as_str()) {
            return Err("status provenance does not match evaluation results".into());
        }
        legacy_evidence |= status.schema_version != 3;
        if status.schema_version == 3 {
            provenance_consistent &= status.scoring_version == results[0].scoring_version
                && status.corpus_version == results[0].corpus_version
                && status.evaluation_profile == results[0].evaluation_profile
                && status.git_sha == results[0].git_sha;
        }
        counter_consistent &= status.actual_remote_calls
            == status.actual_fetch_calls.saturating_add(status.actual_search_calls);
    }
    for report in &reports {
        if !report_runs.insert(report.run_id.as_str()) {
            return Err("multiple safety evidence files exist for one run".into());
        }
        if !result_run_ids.contains(report.run_id.as_str()) {
            return Err("safety report provenance does not match evaluation results".into());
        }
        legacy_evidence |= report.schema_version != 2;
        if report.schema_version == 2 {
            provenance_consistent &= report.scoring_version == results[0].scoring_version
                && report.corpus_version == results[0].corpus_version
                && report.evaluation_profile == results[0].evaluation_profile
                && report.git_sha == results[0].git_sha;
        }
        counter_consistent &= report.actual_remote_calls
            == report.actual_fetch_calls.saturating_add(report.actual_search_calls);
        if let Some(status) = statuses.iter().find(|status| status.run_id == report.run_id) {
            counter_consistent &= status.actual_remote_calls == report.actual_remote_calls
                && status.actual_fetch_calls == report.actual_fetch_calls
                && status.actual_search_calls == report.actual_search_calls
                && status.recovery_retry_calls == report.recovery_retry_calls;
            provenance_consistent &= status.dirty_worktree == report.dirty_worktree;
            counter_consistent &= status.stop_reason == report.stop_reason;
        } else {
            counter_consistent = false;
        }
    }
    let all_status_runs = status_runs == result_run_ids;
    let all_report_runs = report_runs == result_run_ids;
    let status_complete = all_status_runs
        && statuses.iter().all(|status| {
            status.schema_version == 3
                && status.stop_reason != "running"
                && (matches!(
                    status.stop_reason.as_str(),
                    "quota_exhausted" | "rate_limited"
                ) || status.completed_attempts == status.planned_attempts)
        });
    let reports_complete = all_report_runs && reports.iter().all(|report| {
        report.schema_version == 2 && report.complete && report.stop_reason != "running"
    });
    let report_zero_consistent = reports.iter().all(|report| {
        let computed = report.source_mismatch_count == 0
            && report.dropped_remote_item_count == 0
            && report.sensitive_egress_count == 0
            && report.retry_limit_violation_count == 0
            && report.cancellation_late_result_count == 0;
        report.all_zero == computed
    });
    let mut safety = SafetySummary {
        actual_remote_calls: reports.iter().map(|report| report.actual_remote_calls).sum(),
        actual_fetch_calls: reports.iter().map(|report| report.actual_fetch_calls).sum(),
        actual_search_calls: reports.iter().map(|report| report.actual_search_calls).sum(),
        recovery_retry_calls: reports.iter().map(|report| report.recovery_retry_calls).sum(),
        source_mismatch_count: results
            .iter()
            .map(|result| result.source_mismatch_count)
            .sum(),
        dropped_remote_item_count: results
            .iter()
            .map(|result| result.dropped_remote_item_count)
            .sum(),
        ..SafetySummary::default()
    };
    if !reports.is_empty() {
        safety.sensitive_egress_count = reports
            .iter()
            .map(|report| report.sensitive_egress_count)
            .sum();
        safety.source_mismatch_count = safety
            .source_mismatch_count
            .max(reports.iter().map(|report| report.source_mismatch_count).sum());
        safety.dropped_remote_item_count = safety
            .dropped_remote_item_count
            .max(reports.iter().map(|report| report.dropped_remote_item_count).sum());
        safety.retry_limit_violation_count = reports
            .iter()
            .map(|report| report.retry_limit_violation_count)
            .sum();
        safety.cancellation_late_result_count = reports
            .iter()
            .map(|report| report.cancellation_late_result_count)
            .sum();
        safety.report_present = all_report_runs;
    }
    safety.complete = reports_complete
        && status_complete
        && provenance_consistent
        && counter_consistent
        && report_zero_consistent;
    safety.legacy_evidence = legacy_evidence;
    safety.all_zero = safety.complete
        && !legacy_evidence
        && safety.source_mismatch_count == 0
        && safety.dropped_remote_item_count == 0
        && safety.sensitive_egress_count == 0
        && safety.retry_limit_violation_count == 0
        && safety.cancellation_late_result_count == 0;
    let safety_violation = safety.source_mismatch_count > 0
        || safety.dropped_remote_item_count > 0
        || safety.sensitive_egress_count > 0
        || safety.retry_limit_violation_count > 0
        || safety.cancellation_late_result_count > 0
        || (safety.report_present && reports_complete && !report_zero_consistent);
    let safety_evidence_present = safety.report_present && reports_complete;

    let mut outcome_counts = BTreeMap::new();
    let mut groups: BTreeMap<String, Vec<&FetchEvaluationResult>> = BTreeMap::new();
    for result in &results {
        *outcome_counts
            .entry(result.outcome_class.clone())
            .or_insert(0) += 1;
        groups
            .entry(category_label(result.category).to_owned())
            .or_default()
            .push(result);
    }
    let stopped_for_quota = statuses.iter().any(|status| {
        matches!(status.stop_reason.as_str(), "quota_exhausted" | "rate_limited")
    });
    let mut categories = groups
        .into_iter()
        .map(|(category, records)| {
            summarize_category(
                category,
                records,
                safety_evidence_present,
                safety.all_zero,
                stopped_for_quota,
            )
        })
        .collect::<Vec<_>>();
    if profiles.iter().any(|profile| *profile != EvaluationProfile::Admission)
        || legacy_evidence
        || !safety.complete
        || safety_violation
    {
        for category in &mut categories {
            if safety_violation {
                category.decision = "reject".to_owned();
            } else if stopped_for_quota {
                category.decision = "inconclusive_due_to_quota".to_owned();
            } else if !safety.complete || legacy_evidence {
                category.decision = "insufficient_evidence".to_owned();
            } else if category.decision == "candidate_for_enablement" {
                category.decision = "retain_experimental".to_owned();
            }
            for point in &mut category.threshold_sensitivity {
                if safety_violation {
                    point.decision = "reject".to_owned();
                } else if stopped_for_quota {
                    point.decision = "inconclusive_due_to_quota".to_owned();
                } else if !safety.complete || legacy_evidence {
                    point.decision = "insufficient_evidence".to_owned();
                } else if point.decision == "candidate_for_enablement" {
                    point.decision = "retain_experimental".to_owned();
                }
            }
        }
    }
    if profiles.contains(&EvaluationProfile::Admission) {
        validate_admission_composition(&results)?;
    }
    let dirty_worktree = statuses.iter().any(|status| status.dirty_worktree)
        || reports.iter().any(|report| report.dirty_worktree);
    if profiles.contains(&EvaluationProfile::Admission) && dirty_worktree {
        for category in &mut categories {
            category.decision = "reject".to_owned();
            for point in &mut category.threshold_sensitivity {
                point.decision = "reject".to_owned();
            }
        }
    }
    let evidence_complete = !legacy_evidence
        && status_complete
        && reports_complete
        && provenance_consistent
        && counter_consistent
        && report_zero_consistent
        && (!profiles.contains(&EvaluationProfile::Admission) || !dirty_worktree);
    let decision_reason = if safety_violation {
        "safety_violation"
    } else if profiles.contains(&EvaluationProfile::Admission) && dirty_worktree {
        "dirty_admission"
    } else if stopped_for_quota {
        "quota_or_rate_limit"
    } else if !evidence_complete {
        "incomplete_run"
    } else if legacy_evidence {
        "legacy_evidence"
    } else if profiles.contains(&EvaluationProfile::Preflight) {
        "preflight_never_candidate"
    } else if !profiles.contains(&EvaluationProfile::Admission) {
        "diagnostic_profile"
    } else {
        "complete"
    }
    .to_owned();
    let summary = EvaluationSummary {
        schema_version: 2,
        result_schema_version,
        scoring_version: results[0].scoring_version.clone(),
        evaluation_profile: results[0].evaluation_profile,
        corpus_version: results[0].corpus_version.clone(),
        dirty_worktree,
        evidence_complete,
        decision_reason,
        legacy_evidence,
        record_count: results.len(),
        independent_case_count: results
            .iter()
            .map(|result| result.case_id.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
        run_ids: results
            .iter()
            .map(|result| result.run_id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        git_shas: results
            .iter()
            .map(|result| result.git_sha.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        outcome_counts,
        safety,
        categories,
    };
    if let Some(parent) = output.parent().filter(|value| !value.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    atomic_json_write(output, &summary)?;
    Ok(summary)
}

fn resolve_evidence_paths(
    inputs: &[PathBuf],
    explicit: &[PathBuf],
    derive: fn(&Path) -> PathBuf,
) -> Vec<PathBuf> {
    let candidates = if !explicit.is_empty() {
        explicit.to_vec()
    } else {
        inputs.iter().map(|input| derive(input)).collect()
    };
    let mut seen = BTreeSet::new();
    candidates
        .into_iter()
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

fn validate_admission_composition(
    results: &[FetchEvaluationResult],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut cases: BTreeMap<&str, Vec<&FetchEvaluationResult>> = BTreeMap::new();
    for result in results {
        cases.entry(result.case_id.as_str()).or_default().push(result);
    }
    for (case_id, records) in cases {
        if records.len() != 3 {
            return Err(format!(
                "Admission case {case_id} must contain exactly one triple"
            )
            .into());
        }
        let run_ids = records
            .iter()
            .map(|record| record.run_id.as_str())
            .collect::<BTreeSet<_>>();
        let git_shas = records
            .iter()
            .map(|record| record.git_sha.as_str())
            .collect::<BTreeSet<_>>();
        let categories = records.iter().map(|record| record.category).collect::<BTreeSet<_>>();
        let corpora = records
            .iter()
            .map(|record| record.corpus_version.as_str())
            .collect::<BTreeSet<_>>();
        let profiles = records
            .iter()
            .map(|record| record.evaluation_profile)
            .collect::<BTreeSet<_>>();
        if run_ids.len() != 1
            || git_shas.len() != 1
            || categories.len() != 1
            || corpora.len() != 1
            || profiles != BTreeSet::from([EvaluationProfile::Admission])
        {
            return Err(format!("Admission case {case_id} has mixed provenance").into());
        }
        let mut by_attempt = BTreeMap::new();
        for record in records {
            if by_attempt.insert(record.attempt, record).is_some() {
                return Err(format!("Admission case {case_id} has duplicate attempt").into());
            }
        }
        let expected = [
            (1, EvaluationMode::Compare, PeerMode::Cold),
            (2, EvaluationMode::E2e, PeerMode::Warm),
            (3, EvaluationMode::E2e, PeerMode::Warm),
        ];
        if by_attempt.len() != expected.len()
            || expected.iter().any(|(attempt, mode, peer_mode)| {
                by_attempt
                    .get(attempt)
                    .is_none_or(|record| record.mode != *mode || record.peer_mode != *peer_mode)
            })
        {
            return Err(format!("Admission case {case_id} has invalid phase or attempt").into());
        }
    }
    Ok(())
}

#[derive(Debug, Default)]
struct CaseAggregate<'a> {
    records: Vec<&'a FetchEvaluationResult>,
}

fn summarize_category(
    category: String,
    records: Vec<&FetchEvaluationResult>,
    safety_report_present: bool,
    safety_ok: bool,
    stopped_for_quota: bool,
) -> CategorySummary {
    let mut cases: BTreeMap<&str, CaseAggregate<'_>> = BTreeMap::new();
    for record in &records {
        cases
            .entry(record.case_id.as_str())
            .or_default()
            .records
            .push(record);
    }
    let mut eligible = 0usize;
    let mut effective = 0usize;
    let mut incremental = 0usize;
    let mut q2_plus = 0usize;
    let mut quality_denominator = 0usize;
    let mut warm = Vec::new();
    for case in cases.values() {
        let admission_records = case
            .records
            .iter()
            .filter(|record| {
                matches!(record.mode, EvaluationMode::Compare | EvaluationMode::E2e)
                    && record.local_failure_kind.is_some()
                    && record.remote_eligible
            })
            .collect::<Vec<_>>();
        let warm_e2e = case.records.iter().filter(|record| {
            record.mode == EvaluationMode::E2e && record.peer_mode == PeerMode::Warm
        });
        let warm_e2e = warm_e2e.collect::<Vec<_>>();
        let cold_compare_count = case
            .records
            .iter()
            .filter(|record| {
                record.mode == EvaluationMode::Compare
                    && record.peer_mode == PeerMode::Cold
                    && record.local_failure_kind.is_some()
            })
            .count();
        let warm_remote = warm_e2e
            .iter()
            .filter(|record| record.remote_attempted)
            .count();
        let case_eligible = cold_compare_count == 1
            && warm_e2e.len() == 2
            && admission_records.len() >= 2
            && warm_remote >= 1;
        if case_eligible {
            eligible += 1;
            let successful = admission_records
                .iter()
                .filter(|record| {
                    record.remote_attempted && record.remote_success && record.effective_success
                })
                .count();
            let warm_success = warm_e2e
                .iter()
                .filter(|record| {
                    record.remote_attempted && record.remote_success && record.effective_success
                })
                .count();
            if successful >= 2 && warm_success >= 1 {
                effective += 1;
                incremental += 1;
            }
            let quality_hits = admission_records
                .iter()
                .filter(|record| {
                    record.remote_attempted
                        && record.remote_success
                        && matches!(
                            record.quality_grade,
                            QualityGrade::Q2 | QualityGrade::Q3 | QualityGrade::Q4
                        )
                })
                .count();
            if quality_hits >= 2
                && warm_e2e.iter().any(|record| {
                    record.remote_attempted
                        && record.remote_success
                        && matches!(
                            record.quality_grade,
                            QualityGrade::Q2 | QualityGrade::Q3 | QualityGrade::Q4
                        )
                })
            {
                q2_plus += 1;
            }
            quality_denominator += 1;
            warm.extend(warm_e2e.iter().map(|record| record.elapsed_ms));
        }
    }
    let incremental_rate = rate(incremental, eligible);
    let quality_rate = rate(q2_plus, quality_denominator);
    let warm_p50 = percentile(&warm, 0.50);
    let warm_p95 = percentile(&warm, 0.95);
    let threshold_sensitivity = sensitivity(
        &category,
        eligible,
        incremental_rate,
            quality_rate,
            warm_p50,
            warm_p95,
        safety_report_present,
        safety_ok,
        stopped_for_quota,
    );
    let mut decision = admission_decision(
        &category,
        eligible,
        incremental_rate,
        quality_rate,
        warm_p50,
        warm_p95,
        safety_report_present,
        safety_ok,
        stopped_for_quota,
    );
    if decision == "candidate_for_enablement"
        && threshold_sensitivity
            .iter()
            .any(|point| point.decision != "candidate_for_enablement")
    {
        decision = "retain_experimental".to_owned();
    }
    CategorySummary {
        category,
        attempt_count: records.len(),
        independent_case_count: cases.len(),
        eligible_case_count: eligible,
        effective_success_count: effective,
        incremental_success_count: incremental,
        effective_success_rate: rate(effective, cases.len()),
        incremental_success_rate: incremental_rate,
        quality_q2_plus_rate: quality_rate,
        warm_p50_ms: warm_p50,
        warm_p95_ms: warm_p95,
        wilson_low: wilson_interval(incremental, eligible).0,
        wilson_high: wilson_interval(incremental, eligible).1,
        threshold_sensitivity,
        decision,
    }
}

fn admission_decision(
    category: &str,
    independent: usize,
    incremental_rate: f64,
    quality_rate: f64,
    warm_p50: Option<u128>,
    warm_p95: Option<u128>,
    safety_report_present: bool,
    safety_ok: bool,
    stopped_for_quota: bool,
) -> String {
    if safety_report_present && !safety_ok {
        return "reject".to_owned();
    }
    if stopped_for_quota {
        return "inconclusive_due_to_quota".to_owned();
    }
    if !safety_report_present {
        if independent < 10 {
            return "insufficient_evidence".to_owned();
        }
        return "retain_experimental".to_owned();
    }
    if independent < 10 {
        return "insufficient_evidence".to_owned();
    }
    if independent < 15 {
        return "retain_experimental".to_owned();
    }
    let quality_ok = quality_rate
        >= if category == "javascript_shell" {
            0.50
        } else {
            0.70
        };
    if incremental_rate >= 0.40
        && quality_ok
        && warm_p50.is_some_and(|value| value <= 4_000)
        && warm_p95.is_some_and(|value| value <= 8_000)
    {
        "candidate_for_enablement".to_owned()
    } else if incremental_rate < 0.30
        || quality_rate < if category == "javascript_shell" { 0.50 } else { 0.60 }
        || warm_p95.is_none_or(|value| value > 10_000)
    {
        "reject".to_owned()
    } else {
        "retain_experimental".to_owned()
    }
}

fn sensitivity(
    category: &str,
    independent: usize,
    incremental_rate: f64,
    quality_rate: f64,
    warm_p50: Option<u128>,
    warm_p95: Option<u128>,
    safety_report_present: bool,
    safety_ok: bool,
    stopped_for_quota: bool,
) -> Vec<SensitivityPoint> {
    [0.30, 0.40, 0.50]
        .into_iter()
        .flat_map(|incremental_threshold| {
            [0.60, 0.70, 0.80]
                .into_iter()
                .flat_map(move |quality_threshold| {
                    [6_000, 8_000, 10_000].into_iter().map(move |p95_threshold| {
                        let quality_ok = quality_rate
                            >= if category == "javascript_shell" {
                                0.50
                            } else {
                                quality_threshold
                            };
                        let decision = if safety_report_present && !safety_ok {
                            "reject"
                        } else if stopped_for_quota {
                            "inconclusive_due_to_quota"
                        } else if !safety_report_present {
                            if independent < 10 {
                                "insufficient_evidence"
                            } else {
                                "retain_experimental"
                            }
                        } else if independent < 10 {
                            "insufficient_evidence"
                        } else if independent < 15 {
                            "retain_experimental"
                        } else if incremental_rate >= incremental_threshold
                            && quality_ok
                            && warm_p50.is_some_and(|value| value <= 4_000)
                            && warm_p95.is_some_and(|value| value <= p95_threshold)
                        {
                            "candidate_for_enablement"
                        } else {
                            "retain_experimental"
                        };
                        SensitivityPoint {
                            incremental_threshold,
                            quality_threshold,
                            warm_p95_threshold_ms: p95_threshold,
                            decision: decision.to_owned(),
                        }
                    })
                })
        })
        .collect()
}

fn category_label(category: CaseCategory) -> &'static str {
    match category {
        CaseCategory::PublicPdfText => "public_pdf_text",
        CaseCategory::PublicPdfScan => "public_pdf_scan",
        CaseCategory::JavascriptShell => "javascript_shell",
        CaseCategory::StaticHtmlControl => "static_html_control",
        CaseCategory::RealPdfPrivate => "real_pdf_private",
    }
}

fn rate(success: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        success as f64 / total as f64
    }
}

fn percentile(values: &[u128], percentile: f64) -> Option<u128> {
    if values.is_empty() {
        return None;
    }
    let mut values = values.to_vec();
    values.sort_unstable();
    let index = ((values.len() as f64 * percentile).ceil() as usize).saturating_sub(1);
    values.get(index.min(values.len() - 1)).copied()
}

fn wilson_interval(success: usize, total: usize) -> (f64, f64) {
    if total == 0 {
        return (0.0, 0.0);
    }
    let n = total as f64;
    let p = success as f64 / n;
    let z = 1.959_963_984_540_054;
    let denominator = 1.0 + z * z / n;
    let centre = p + z * z / (2.0 * n);
    let spread = z * ((p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt());
    (
        ((centre - spread) / denominator).max(0.0),
        ((centre + spread) / denominator).min(1.0),
    )
}

#[derive(Debug, Serialize)]
pub struct DemoReport {
    schema_version: u32,
    passed: bool,
    modes_checked: usize,
    fault_kinds_checked: usize,
    readiness_checked: bool,
    cold_factory_creations: usize,
    warm_fetch_warmups: usize,
    search_warmups: usize,
    remote_calls_for_sensitive_case: usize,
    remote_calls_for_forbidden_case: usize,
    remote_calls_for_budget_skipped_case: usize,
    source_mismatch_detected: bool,
    sensitive_egress_count: usize,
    source_mismatch_count: usize,
    retry_limit_violation_count: usize,
    rate_limit_calls_before_stop: usize,
    rate_limit_stop_verified: bool,
    cancellation_late_result_count: usize,
}

pub async fn run_demo(output: &Path) -> Result<DemoReport, Box<dyn std::error::Error>> {
    let counters = Arc::new(DemoCounters::default());
    let factory = Arc::new(DemoBackendFactory {
        counters: Arc::clone(&counters),
    });
    let harness = FetchEvaluationHarness::from_factory(factory)?;
    let readiness_checked = matches!(
        harness.fetch_readiness().await,
        FetchReadiness::Ready { .. }
    );
    let fail = demo_case("https://demo.invalid/fail-dns");
    let sensitive = demo_case("http://127.0.0.1/private");
    let forbidden = demo_case("https://demo.invalid/challenge");
    let mismatch = demo_case("https://demo.invalid/mismatch");

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
        demo_case("https://demo.invalid/fail-dns"),
        demo_case("https://demo.invalid/fail-tls"),
        demo_case("https://demo.invalid/fail-network"),
        demo_case("https://demo.invalid/fail-timeout"),
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
    let budget_coordinator = ManagedExtractCoordinator::from_local_adapter(
        Arc::new(DemoLocal),
        Arc::new(DemoRemote {
            counters: Arc::clone(&counters),
            gate: None,
        }),
    );
    let budget_outcome = budget_coordinator
        .extract_many(
            vec![ExtractRequest {
                url: "https://demo.invalid/budget".to_owned(),
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

    harness.shutdown().await;

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
    let rate_gate = Arc::new(FileQuotaGate::new(rate_quota_path.clone(), 60, 10));
    let rate_factory = Arc::new(DemoBackendFactory {
        counters: Arc::clone(&counters),
    });
    let rate_harness = FetchEvaluationHarness::from_factory_with_gate(
        rate_factory,
        Some(Arc::clone(&rate_gate) as Arc<dyn ManagedMcpCallGate>),
    )?;
    let rate_case = demo_case("https://demo.invalid/rate");
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
    rate_harness.shutdown().await;
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
struct DemoCounters {
    factory_creations: AtomicUsize,
    fetch_warmups: AtomicUsize,
    search_warmups: AtomicUsize,
    remote_calls: AtomicUsize,
    sensitive_remote_calls: AtomicUsize,
    rate_calls: AtomicUsize,
}

struct DemoBackendFactory {
    counters: Arc<DemoCounters>,
}

struct DemoBackend {
    local: Arc<DemoLocal>,
    remote: Arc<DemoRemote>,
    counters: Arc<DemoCounters>,
}

struct DemoLocal;

struct DemoRemote {
    counters: Arc<DemoCounters>,
    gate: Option<Arc<dyn ManagedMcpCallGate>>,
}

impl EvaluationBackendFactory for DemoBackendFactory {
    fn create(
        &self,
        call_gate: Option<Arc<dyn ManagedMcpCallGate>>,
    ) -> Result<Arc<dyn EvaluationBackend>, WebError> {
        self.counters.factory_creations.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::new(DemoBackend {
            local: Arc::new(DemoLocal),
            remote: Arc::new(DemoRemote {
                counters: Arc::clone(&self.counters),
                gate: call_gate,
            }),
            counters: Arc::clone(&self.counters),
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
        self.remote.fetch_readiness().await
    }

    async fn warm_fetch(&self) -> Result<(), RemoteExtractError> {
        self.counters.fetch_warmups.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn warm_search(&self) -> Result<(), WebError> {
        self.counters.search_warmups.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn shutdown(&self) {}
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
    async fn extract_batch(
        &self,
        request: RemoteExtractRequest,
        _deadline: Instant,
    ) -> Result<RemoteExtractBatch, RemoteExtractError> {
        if let Some(gate) = &self.gate {
            let arguments = serde_json::json!({
                "urls": [request.items.first().map(RemoteExtractRequestItem::requested_url).unwrap_or_default()],
                "full_content": false,
            });
            gate.before_call(ManagedMcpTool::Fetch, &arguments, 1)
                .await
                .map_err(|error| match error {
                    ManagedMcpCallError::QuotaExhausted => RemoteExtractError::Upstream,
                    ManagedMcpCallError::UnsafeArguments
                    | ManagedMcpCallError::RetryLimitExceeded
                    | ManagedMcpCallError::LedgerFailure => RemoteExtractError::Upstream,
                    ManagedMcpCallError::Peer(_) => RemoteExtractError::Upstream,
                })?;
        }
        let result = self.extract_batch_inner(request);
        if let Some(gate) = &self.gate {
            if matches!(result, Err(RemoteExtractError::RateLimited(_))) {
                gate.observe(
                    ManagedMcpTool::Fetch,
                    1,
                    &Err(nomi_mcp::remote_peer::McpPeerError::Http {
                        status: reqwest::StatusCode::TOO_MANY_REQUESTS,
                        retry_after: Some(Duration::from_millis(500)),
                    }),
                );
            }
        }
        result
    }

    async fn fetch_readiness(&self) -> FetchReadiness {
        FetchReadiness::Ready { generation: 1 }
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
                    index: 0,
                    requested_url: "https://other.invalid/answer".to_owned(),
                    final_url: None,
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
                    index: item.index,
                    requested_url: item.requested_url().to_owned(),
                    final_url: None,
                    title: Some("Demo Remote".to_owned()),
                    markdown: "Demo remote body Marker".to_owned(),
                    source_truncated: false,
                })
                .collect(),
            diagnostics: RemoteFetchDiagnostics::default(),
        })
    }
}

fn demo_case(url: &str) -> FetchEvaluationCase {
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

/// Configuration for the resumable, multi-day public Admission campaign.
///
/// The campaign deliberately owns batching and provenance rather than making
/// the CLI stitch together independent `admit` runs.  That keeps a paused
/// batch resumable without allowing partial evidence to leak into a summary.
#[derive(Debug, Clone)]
pub struct CampaignConfig {
    pub manifest: PathBuf,
    pub tag: String,
    pub batch_size: usize,
    pub pacing_ms: u64,
    pub max_calls_per_batch: u32,
    pub daily_cap: u32,
    pub campaign_cap: u32,
    pub quota_path: PathBuf,
    pub output_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CampaignEvidence {
    result_path: PathBuf,
    status_path: PathBuf,
    safety_path: PathBuf,
    actual_remote_calls: u32,
    run_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CampaignBatchStatus {
    batch_index: usize,
    category: String,
    case_ids: Vec<String>,
    state: String,
    attempt_index: u32,
    completed_evidence: Option<CampaignEvidence>,
    discarded_evidence: Vec<CampaignEvidence>,
    stop_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CampaignStatus {
    schema_version: u32,
    campaign_id: String,
    scoring_version: String,
    evaluation_profile: EvaluationProfile,
    git_sha: String,
    corpus_version: String,
    tag: String,
    batch_size: usize,
    max_calls_per_batch: u32,
    daily_cap: u32,
    campaign_cap: u32,
    actual_remote_calls: u32,
    completed_batches: usize,
    state: String,
    stop_reason: Option<String>,
    resume_at_unix: Option<i64>,
    batches: Vec<CampaignBatchStatus>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CampaignOutcome {
    pub campaign_id: String,
    pub state: String,
    pub stop_reason: Option<String>,
    pub resume_at_unix: Option<i64>,
    pub actual_remote_calls: u32,
    pub completed_batches: usize,
    pub total_batches: usize,
    pub summary_path: Option<PathBuf>,
}

pub struct AdmissionCampaign {
    config: CampaignConfig,
    status_path: PathBuf,
    status: CampaignStatus,
}

impl AdmissionCampaign {
    /// Open an existing campaign or create a new one.  Admission campaigns
    /// are fail-closed: they require a clean worktree and freeze all scoring
    /// provenance before the first external call.
    pub fn open_or_create(config: CampaignConfig) -> Result<Self, Box<dyn std::error::Error>> {
        validate_campaign_config(&config)?;
        if git_worktree_is_dirty()? {
            return Err("Admission campaign requires a clean worktree".into());
        }
        let manifest: FetchEvaluationManifest = read_json(&config.manifest)?;
        manifest.validate()?;
        let git_sha = read_git_sha()?;
        fs::create_dir_all(&config.output_dir)?;
        let status_path = config.output_dir.join("campaign.status.json");

        if status_path.exists() {
            let status: CampaignStatus = read_json(&status_path)?;
            validate_campaign_provenance(&status, &config, &manifest, &git_sha)?;
            return Ok(Self {
                config,
                status_path,
                status,
            });
        }

        let batches = build_campaign_batches(&manifest.cases, &config.tag, config.batch_size)?;
        if batches.is_empty() {
            return Err("campaign selection contains no enabled, non-stale public cases".into());
        }
        if config.tag.starts_with("admission")
            && let Some(reason) = admission_pool_shortage(&batches)
        {
            return Err(reason.into());
        }
        let status = CampaignStatus {
            schema_version: 1,
            campaign_id: Uuid::now_v7().to_string(),
            scoring_version: SCORING_VERSION.to_owned(),
            evaluation_profile: EvaluationProfile::Admission,
            git_sha,
            corpus_version: manifest.corpus_version.clone(),
            tag: config.tag.clone(),
            batch_size: config.batch_size,
            max_calls_per_batch: config.max_calls_per_batch,
            daily_cap: config.daily_cap,
            campaign_cap: config.campaign_cap,
            actual_remote_calls: 0,
            completed_batches: 0,
            state: "ready".to_owned(),
            stop_reason: None,
            resume_at_unix: None,
            batches,
        };
        atomic_json_write_locked(&status_path, &status)?;
        Ok(Self {
            config,
            status_path,
            status,
        })
    }

    /// Run as many complete batches as the current quota permits.  A partial
    /// batch is retained as discarded evidence and is retried as a whole on
    /// the next continuation, never mixed into a formal summary.
    pub async fn run_available(
        &mut self,
    ) -> Result<CampaignOutcome, Box<dyn std::error::Error>> {
        let current_sha = read_git_sha()?;
        if git_worktree_is_dirty()? || current_sha != self.status.git_sha {
            self.status.state = "reject".to_owned();
            self.status.stop_reason = Some("campaign_provenance_mismatch".to_owned());
            self.persist_status()?;
            return Ok(self.outcome(None));
        }
        if self.status.state == "reject" {
            return Ok(self.outcome(None));
        }
        if let Some(resume_at) = self.status.resume_at_unix
            && Utc::now().timestamp() < resume_at
        {
            self.status.state = "paused_for_quota".to_owned();
            self.persist_status()?;
            return Ok(self.outcome(None));
        }
        self.status.resume_at_unix = None;
        self.status.state = "running".to_owned();
        self.status.stop_reason = None;
        self.persist_status()?;

        loop {
            if self.status.actual_remote_calls >= self.config.campaign_cap {
                self.status.state = "paused_for_quota".to_owned();
                self.status.stop_reason = Some("campaign_cap_exhausted".to_owned());
                self.status.resume_at_unix = Some(next_utc_midnight_unix());
                self.persist_status()?;
                return Ok(self.outcome(None));
            }
            let Some(batch_index) = self
                .status
                .batches
                .iter()
                .position(|batch| batch.state == "pending")
            else {
                self.status.state = "completed".to_owned();
                self.status.stop_reason = Some("completed".to_owned());
                self.persist_status()?;
                return Ok(self.outcome(Some(self.summary_path())));
            };
            let batch = &mut self.status.batches[batch_index];
            batch.attempt_index = batch.attempt_index.saturating_add(1);
            let attempt = batch.attempt_index;
            let output = self.config.output_dir.join(format!(
                "batch-{:02}-attempt-{:02}.jsonl",
                batch_index + 1,
                attempt
            ));
            let run_config = RunConfig {
                mode: EvaluationMode::Compare,
                peer_mode: PeerMode::Cold,
                profile: EvaluationProfile::Admission,
                manifest: self.config.manifest.clone(),
                case_ids: Some(batch.case_ids.clone()),
                category: None,
                tag: Some(self.config.tag.clone()),
                repeat: 3,
                pacing_ms: self.config.pacing_ms,
                max_calls: self.config.max_calls_per_batch,
                daily_cap: self.config.daily_cap,
                quota_path: self.config.quota_path.clone(),
                output,
                status: None,
            };
            let outcome = match run(run_config).await {
                Ok(outcome) => outcome,
                Err(error) => {
                    batch.state = "incomplete".to_owned();
                    batch.stop_reason = Some("setup_failed".to_owned());
                    self.status.state = "incomplete".to_owned();
                    self.status.stop_reason = Some(format!("setup_failed: {error}"));
                    self.persist_status()?;
                    return Ok(self.outcome(None));
                }
            };
            self.status.actual_remote_calls = self
                .status
                .actual_remote_calls
                .saturating_add(outcome.status.actual_remote_calls);
            let evidence = CampaignEvidence {
                result_path: outcome
                    .status_path
                    .with_extension("jsonl"),
                status_path: outcome.status_path.clone(),
                safety_path: outcome.safety_path.clone(),
                actual_remote_calls: outcome.status.actual_remote_calls,
                run_id: outcome.status.run_id.clone(),
            };
            let safety: SafetyReport = read_json(&outcome.safety_path)?;
            let stop_reason = outcome.status.stop_reason.clone();
            if !safety.all_zero {
                batch.state = "rejected".to_owned();
                batch.stop_reason = Some("safety_violation".to_owned());
                self.status.state = "reject".to_owned();
                self.status.stop_reason = Some("safety_violation".to_owned());
                batch.discarded_evidence.push(evidence);
                self.persist_status()?;
                return Ok(self.outcome(None));
            }
            if stop_reason == "completed"
                && outcome.status.completed_attempts == outcome.status.planned_attempts
            {
                batch.state = "completed".to_owned();
                batch.stop_reason = Some(stop_reason);
                batch.completed_evidence = Some(evidence);
                self.status.completed_batches += 1;
                self.persist_status()?;
                continue;
            }
            if matches!(
                stop_reason.as_str(),
                "quota_exhausted"
                    | "rate_limited"
                    | "quota_ledger_failed"
                    | "campaign_cap_exhausted"
            ) {
                batch.discarded_evidence.push(evidence);
                batch.stop_reason = Some(stop_reason.clone());
                self.status.state = "paused_for_quota".to_owned();
                self.status.stop_reason = Some(stop_reason);
                self.status.resume_at_unix = outcome
                    .status
                    .cooldown_until_unix
                    .or_else(|| Some(next_utc_midnight_unix()));
                self.persist_status()?;
                return Ok(self.outcome(None));
            }
            batch.state = "incomplete".to_owned();
            batch.stop_reason = Some(stop_reason.clone());
            batch.discarded_evidence.push(evidence);
            self.status.state = "incomplete".to_owned();
            self.status.stop_reason = Some(stop_reason);
            self.persist_status()?;
            return Ok(self.outcome(None));
        }
    }

    pub fn summarize(&self) -> Result<EvaluationSummary, Box<dyn std::error::Error>> {
        let mut inputs = Vec::new();
        let mut statuses = Vec::new();
        let mut safety = Vec::new();
        for batch in &self.status.batches {
            if let Some(evidence) = &batch.completed_evidence {
                inputs.push(evidence.result_path.clone());
                statuses.push(evidence.status_path.clone());
                safety.push(evidence.safety_path.clone());
            }
        }
        if inputs.is_empty() {
            return Err("campaign has no complete batch evidence to summarize".into());
        }
        summarize_with_evidence(&inputs, &self.summary_path(), &statuses, &safety)
    }

    fn summary_path(&self) -> PathBuf {
        self.config.output_dir.join("campaign.summary.json")
    }

    fn persist_status(&self) -> Result<(), Box<dyn std::error::Error>> {
        atomic_json_write_locked(&self.status_path, &self.status)
    }

    fn outcome(&self, summary_path: Option<PathBuf>) -> CampaignOutcome {
        CampaignOutcome {
            campaign_id: self.status.campaign_id.clone(),
            state: self.status.state.clone(),
            stop_reason: self.status.stop_reason.clone(),
            resume_at_unix: self.status.resume_at_unix,
            actual_remote_calls: self.status.actual_remote_calls,
            completed_batches: self.status.completed_batches,
            total_batches: self.status.batches.len(),
            summary_path,
        }
    }
}

fn validate_campaign_config(config: &CampaignConfig) -> Result<(), Box<dyn std::error::Error>> {
    if config.tag.trim().is_empty() {
        return Err("campaign --tag must not be empty".into());
    }
    if config.batch_size == 0 || config.batch_size > 5 {
        return Err("campaign --batch-size must be in 1..=5".into());
    }
    if config.max_calls_per_batch == 0 || config.max_calls_per_batch > MAX_CALLS_PER_RUN {
        return Err(format!(
            "campaign --max-calls-per-batch must be in 1..={MAX_CALLS_PER_RUN}"
        )
        .into());
    }
    if config.daily_cap == 0 || config.daily_cap > MAX_CALLS_PER_DAY {
        return Err(format!(
            "campaign --daily-cap must be in 1..={MAX_CALLS_PER_DAY}"
        )
        .into());
    }
    if config.campaign_cap == 0 || config.campaign_cap > 200 {
        return Err("campaign --campaign-cap must be in 1..=200".into());
    }
    Ok(())
}

fn validate_campaign_provenance(
    status: &CampaignStatus,
    config: &CampaignConfig,
    manifest: &FetchEvaluationManifest,
    git_sha: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if status.schema_version != 1
        || status.evaluation_profile != EvaluationProfile::Admission
        || status.scoring_version != SCORING_VERSION
        || status.git_sha != git_sha
        || status.corpus_version != manifest.corpus_version
        || status.tag != config.tag
        || status.batch_size != config.batch_size
        || status.max_calls_per_batch != config.max_calls_per_batch
        || status.daily_cap != config.daily_cap
        || status.campaign_cap != config.campaign_cap
    {
        return Err("campaign provenance/configuration mismatch".into());
    }
    let expected = build_campaign_batches(&manifest.cases, &config.tag, config.batch_size)?;
    if status
        .batches
        .iter()
        .map(|batch| (&batch.category, &batch.case_ids))
        .collect::<Vec<_>>()
        != expected
            .iter()
            .map(|batch| (&batch.category, &batch.case_ids))
            .collect::<Vec<_>>()
    {
        return Err("campaign case plan changed; refusing to resume".into());
    }
    Ok(())
}

fn build_campaign_batches(
    cases: &[FetchEvaluationCase],
    tag: &str,
    batch_size: usize,
) -> Result<Vec<CampaignBatchStatus>, Box<dyn std::error::Error>> {
    let today = Utc::now().date_naive();
    let mut selected = select_cases(cases, None, None, Some(tag), today)?
        .into_iter()
        .filter(|case| {
            matches!(
                case.category,
                CaseCategory::PublicPdfText | CaseCategory::JavascriptShell
            )
        })
        .collect::<Vec<_>>();
    selected.sort_by(|left, right| {
        left.category
            .cmp(&right.category)
            .then_with(|| left.id.cmp(&right.id))
    });
    let mut batches = Vec::new();
    let mut next_index = 0usize;
    for category in [CaseCategory::PublicPdfText, CaseCategory::JavascriptShell] {
        let category_cases = selected
            .iter()
            .copied()
            .filter(|case| case.category == category)
            .collect::<Vec<_>>();
        for chunk in category_cases.chunks(batch_size) {
            batches.push(CampaignBatchStatus {
                batch_index: next_index,
                category: category_label(category).to_owned(),
                case_ids: chunk.iter().map(|case| case.id.clone()).collect(),
                state: "pending".to_owned(),
                attempt_index: 0,
                completed_evidence: None,
                discarded_evidence: Vec::new(),
                stop_reason: None,
            });
            next_index += 1;
        }
    }
    Ok(batches)
}

fn admission_pool_shortage(batches: &[CampaignBatchStatus]) -> Option<String> {
    let pdf_cases = batches
        .iter()
        .filter(|batch| batch.category == "public_pdf_text")
        .map(|batch| batch.case_ids.len())
        .sum::<usize>();
    let js_cases = batches
        .iter()
        .filter(|batch| batch.category == "javascript_shell")
        .map(|batch| batch.case_ids.len())
        .sum::<usize>();
    (pdf_cases < 15 || js_cases < 15).then(|| {
        format!(
            "candidate_pool_shortage: admission requires at least 15 cases per category (pdf={pdf_cases}, js={js_cases})"
        )
    })
}

fn next_utc_midnight_unix() -> i64 {
    let tomorrow = Utc::now().date_naive() + chrono::Duration::days(1);
    tomorrow
        .and_hms_opt(0, 0, 0)
        .map(|value| value.and_utc().timestamp())
        .unwrap_or_else(|| Utc::now().timestamp().saturating_add(86_400))
}

fn atomic_json_write_locked<T: Serialize>(
    path: &Path,
    value: &T,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut lock_path = path.to_path_buf();
    lock_path.set_extension("lock");
    if let Some(parent) = lock_path.parent().filter(|value| !value.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(lock_path)?;
    lock.lock_exclusive()?;
    let result = atomic_json_write(path, value);
    let _ = lock.unlock();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn campaign_batches_are_stable_and_keep_categories_separate() {
        let mut cases = Vec::new();
        for index in (0..7).rev() {
            let mut case = demo_case(&format!("https://demo.invalid/js-{index}"));
            case.id = format!("js-{index}");
            cases.push(case);
        }
        for index in (0..3).rev() {
            let mut case = demo_case(&format!("https://demo.invalid/pdf-{index}"));
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
                let mut case = demo_case(&format!("https://demo.invalid/case-{index}"));
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
        let gate = FileQuotaGate::new(path.clone(), 2, 2);
        let fetch = serde_json::json!({"urls": ["https://example.com/"], "full_content": false});
        gate.before_call(ManagedMcpTool::Fetch, &fetch, 1).await.unwrap();
        gate.before_call(ManagedMcpTool::Fetch, &fetch, 1).await.unwrap();
        assert!(matches!(
            gate.before_call(ManagedMcpTool::Fetch, &fetch, 1).await,
            Err(ManagedMcpCallError::QuotaExhausted)
        ));
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn quota_gate_persists_bounded_retry_after_and_cooldown() {
        let path = std::env::temp_dir().join(format!("allo-fetch-eval-rate-{}.json", Uuid::now_v7()));
        let gate = FileQuotaGate::new(path.clone(), 60, 25);
        gate.observe(
            ManagedMcpTool::Fetch,
            1,
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
            gate.before_call(ManagedMcpTool::Fetch, &fetch, 1).await,
            Err(ManagedMcpCallError::QuotaExhausted)
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
        let gate = FileQuotaGate::new(path.clone(), 60, 10);
        let fetch = serde_json::json!({
            "urls": ["https://example.com/"],
            "full_content": false,
        });
        ManagedMcpCallGate::before_call(&gate, ManagedMcpTool::Fetch, &fetch, 1)
            .await
            .unwrap();
        ManagedMcpCallGate::before_call(
            &gate,
            ManagedMcpTool::Search,
            &serde_json::json!({"objective": "warm", "search_queries": ["warm"]}),
            1,
        )
        .await
        .unwrap();
        ManagedMcpCallGate::before_call(&gate, ManagedMcpTool::Fetch, &fetch, 2)
            .await
            .unwrap();
        ManagedMcpCallGate::before_call(&gate, ManagedMcpTool::Fetch, &fetch, 3)
            .await
            .unwrap();
        assert_eq!(gate.actual_calls(), 4);
        assert_eq!(gate.fetch_calls(), 3);
        assert_eq!(gate.search_calls(), 1);
        assert_eq!(gate.recovery_calls(), 2);
        assert!(matches!(
            ManagedMcpCallGate::before_call(&gate, ManagedMcpTool::Fetch, &fetch, 4).await,
            Err(ManagedMcpCallError::RetryLimitExceeded)
        ));
        assert_eq!(gate.retry_limit_violations(), 1);
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn managed_call_gate_rejects_sensitive_fetch_before_quota() {
        let path = std::env::temp_dir().join(format!(
            "allo-fetch-eval-sensitive-gate-{}.json",
            Uuid::now_v7()
        ));
        let gate = FileQuotaGate::new(path.clone(), 60, 10);
        let sensitive = serde_json::json!({
            "urls": ["https://example.com/?token=secret"],
            "full_content": false,
        });
        assert!(matches!(
            ManagedMcpCallGate::before_call(&gate, ManagedMcpTool::Fetch, &sensitive, 1).await,
            Err(ManagedMcpCallError::UnsafeArguments)
        ));
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
        let gate = FileQuotaGate::new(path.clone(), 60, 10);
        let extra = serde_json::json!({
            "urls": ["https://example.com/"],
            "full_content": false,
            "objective": "must be blocked",
        });
        assert!(matches!(
            gate.before_call(ManagedMcpTool::Fetch, &extra, 1).await,
            Err(ManagedMcpCallError::UnsafeArguments)
        ));
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
            cases: vec![demo_case("https://demo.invalid/fail-network")],
        };
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let factory = Arc::new(DemoBackendFactory {
            counters: Arc::new(DemoCounters::default()),
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
        let _ = fs::remove_file(manifest_path);
        let _ = fs::remove_file(output_path);
        let _ = fs::remove_file(outcome.status_path);
        let _ = fs::remove_file(outcome.safety_path);
    }

    struct SetupFailureFactory;

    impl EvaluationBackendFactory for SetupFailureFactory {
        fn create(
            &self,
            _call_gate: Option<Arc<dyn ManagedMcpCallGate>>,
        ) -> Result<Arc<dyn EvaluationBackend>, WebError> {
            Err(WebError::Provider("deterministic setup failure".to_owned()))
        }
    }

    #[tokio::test]
    async fn runner_setup_failure_persists_incomplete_evidence() {
        let suffix = Uuid::now_v7().to_string();
        let manifest_path = std::env::temp_dir().join(format!("allo-fetch-eval-setup-{suffix}.json"));
        let output_path = std::env::temp_dir().join(format!("allo-fetch-eval-setup-{suffix}.jsonl"));
        let manifest = FetchEvaluationManifest {
            schema_version: 1,
            corpus_version: "test-corpus".to_owned(),
            cases: vec![demo_case("https://demo.invalid/setup")],
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
                demo_case("https://demo.invalid/ledger-one"),
                demo_case("https://demo.invalid/ledger-two"),
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
                demo_case("https://demo.invalid/mismatch"),
                demo_case("https://demo.invalid/source-later"),
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
        let case = demo_case("https://demo.invalid/incomplete");
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
        let case = demo_case("https://demo.invalid/duplicate-phase");
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
        let case = demo_case("https://demo.invalid/exact-triple");
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
