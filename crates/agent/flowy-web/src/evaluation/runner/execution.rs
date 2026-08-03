//! Run coordination and manifest selection.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant as StdInstant};

use chrono::Utc;
use uuid::Uuid;

use super::evidence::{
    atomic_json_write, default_safety_path, default_status_path, read_json,
};
use super::quota::FileQuotaControl;
use super::{
    CaseCategory, EvaluationBackendFactory, EvaluationMode, EvaluationProfile,
    FetchEvaluationCase, FetchEvaluationHarness, FetchEvaluationManifest, PeerMode, RunConfig,
    RunOutcome, RunStatus, SafetyReport, TestRunMetadata, MAX_CALLS_PER_DAY, MAX_CALLS_PER_RUN,
    SCORING_VERSION,
};
use crate::managed::ManagedMcpCallControl;

pub async fn run(config: RunConfig) -> Result<RunOutcome, Box<dyn std::error::Error>> {
    run_inner(&config, None, None).await
}

#[cfg(test)]
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

pub(crate) async fn run_inner(
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

    let control = if config.mode == EvaluationMode::Local {
        None
    } else {
        Some(Arc::new(FileQuotaControl::new(
            config.quota_path.clone(),
            config.daily_cap,
            config.max_calls,
        )))
    };
    let harness = match injected_factory {
        Some(factory) => FetchEvaluationHarness::from_factory_with_control(
            factory,
            control.as_ref()
                .map(|value| Arc::clone(value) as Arc<dyn ManagedMcpCallControl>),
        ),
        None => match control.as_ref() {
            Some(gate) => FetchEvaluationHarness::keyless_production_with_call_control(
                Arc::clone(gate) as Arc<dyn ManagedMcpCallControl>,
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
    // Keep the shutdown action outside the fallible execution block. Every
    // ordinary success or error path therefore closes the Harness before the
    // Runner returns; cancellation drops the in-flight future and leaves the
    // already-persisted evidence incomplete for recovery.
    let mut source_mismatch_count = 0usize;
    let mut dropped_remote_item_count = 0usize;
    let run_result: Result<RunOutcome, Box<dyn std::error::Error>> = async {
    let mut last_call: Option<StdInstant> = None;
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
            let calls_before_attempt = control.as_ref().map(|value| value.actual_calls());
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
            status.actual_remote_calls = control.as_ref().map_or(0, |value| value.actual_calls());
            if let Some(control) = control.as_ref() {
                status.actual_fetch_calls = control.fetch_calls();
                status.actual_search_calls = control.search_calls();
                status.recovery_retry_calls = control.recovery_calls();
                status.retry_limit_violation_count = control.retry_limit_violations();
                status.sensitive_egress_count = control.sensitive_egress_violations();
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
            // Keep an atomic, incomplete Safety checkpoint in lockstep with
            // every flushed RunStatus.  A cancelled attempt can therefore be
            // reconciled from disk without treating the initial zero-valued
            // safety file as authoritative.
            write_safety_checkpoint(
                &safety_path,
                &status,
                source_mismatch_count,
                dropped_remote_item_count,
                false,
            )?;

            if result.source_mismatch_count > 0 || result.dropped_remote_item_count > 0 {
                status.stop_reason = "safety_violation".to_owned();
                break 'cases;
            }
            if let Some(control) = control.as_ref()
                && matches!(control.state().0.as_deref(), Some("safety_violation"))
            {
                status.stop_reason = "safety_violation".to_owned();
                break 'cases;
            }
            if let Some(control) = control.as_ref()
                && matches!(
                    control.state().0.as_deref(),
                    Some("quota_ledger_failed" | "quota_exhausted" | "rate_limited")
                )
            {
                status.stop_reason = control.state().0.unwrap_or_default();
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
            if result.outcome_class == "shutdown_failed" {
                status.stop_reason = "shutdown_failed".to_owned();
                break 'cases;
            }
        }
    }
    if status.stop_reason == "running" {
        status.stop_reason = "completed".to_owned();
    }
    if let Some(control) = control {
        let (reason, retry_after_ms, cooldown) = control.state();
        status.actual_remote_calls = control.actual_calls();
        status.actual_fetch_calls = control.fetch_calls();
        status.actual_search_calls = control.search_calls();
        status.recovery_retry_calls = control.recovery_calls();
        status.retry_limit_violation_count = control.retry_limit_violations();
        status.sensitive_egress_count = control.sensitive_egress_violations();
        status.retry_after_ms = retry_after_ms.or(status.retry_after_ms);
        status.cooldown_until_unix = cooldown;
        if status.stop_reason == "completed" {
            if let Some(reason) = reason {
                status.stop_reason = reason;
            }
        }
    }
    write_status(&status_path, &status)?;
    write_safety_checkpoint(
        &safety_path,
        &status,
        source_mismatch_count,
        dropped_remote_item_count,
        status.stop_reason != "shutdown_failed",
    )?;
    Ok(RunOutcome {
        status: status.clone(),
        output_path: config.output.clone(),
        status_path: status_path.clone(),
        safety_path: safety_path.clone(),
    })
    }.await;
    let shutdown_result = harness.shutdown().await;
    match (run_result, shutdown_result) {
        (Ok(outcome), Ok(())) => Ok(outcome),
        (Err(primary), Ok(())) => Err(primary),
        (Ok(mut outcome), Err(shutdown_error)) => {
            outcome.status.stop_reason = "shutdown_failed".to_owned();
            if let Err(error) = write_status(&status_path, &outcome.status) {
                tracing::error!(
                    target: "managed_fetch_evaluation",
                    error = %error,
                    "failed to persist shutdown failure status"
                );
            }
            let (source_mismatch_count, dropped_remote_item_count) = read_json::<SafetyReport>(
                &safety_path,
            )
            .map(|safety| {
                (
                    safety.source_mismatch_count,
                    safety.dropped_remote_item_count,
                )
            })
            .unwrap_or_default();
            if let Err(error) = write_safety_checkpoint(
                &safety_path,
                &outcome.status,
                source_mismatch_count,
                dropped_remote_item_count,
                false,
            ) {
                tracing::error!(
                    target: "managed_fetch_evaluation",
                    error = %error,
                    "failed to persist incomplete shutdown evidence"
                );
            }
            Err(Box::new(shutdown_error))
        }
        (Err(primary), Err(shutdown_error)) => {
            status.stop_reason = "shutdown_failed".to_owned();
            if let Err(error) = write_status(&status_path, &status) {
                tracing::error!(
                    target: "managed_fetch_evaluation",
                    error = %error,
                    "failed to persist shutdown failure status"
                );
            }
            if let Err(error) = write_safety_checkpoint(
                &safety_path,
                &status,
                source_mismatch_count,
                dropped_remote_item_count,
                false,
            ) {
                tracing::error!(
                    target: "managed_fetch_evaluation",
                    error = %error,
                    "failed to persist incomplete shutdown evidence"
                );
            }
            tracing::warn!(
                target: "managed_fetch_evaluation",
                error = %shutdown_error,
                "evaluation cleanup failed after a primary execution error"
            );
            Err(primary)
        }
    }
}

pub(crate) fn validate_run_config(config: &RunConfig) -> Result<(), Box<dyn std::error::Error>> {
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

pub(crate) fn select_cases<'a>(
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

pub(crate) fn create_new_output(path: &Path) -> Result<File, Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent().filter(|value| !value.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    Ok(OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?)
}

pub(crate) fn write_status(path: &Path, status: &RunStatus) -> Result<(), Box<dyn std::error::Error>> {
    atomic_json_write(path, status)
}

pub(crate) fn write_safety_report(
    path: &Path,
    report: &SafetyReport,
) -> Result<(), Box<dyn std::error::Error>> {
    atomic_json_write(path, report)
}

pub(crate) fn write_safety_checkpoint(
    path: &Path,
    status: &RunStatus,
    source_mismatch_count: usize,
    dropped_remote_item_count: usize,
    complete: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let cancellation_late_result_count = 0usize;
    let all_zero = complete
        && source_mismatch_count == 0
        && dropped_remote_item_count == 0
        && status.sensitive_egress_count == 0
        && status.retry_limit_violation_count == 0
        && cancellation_late_result_count == 0;
    write_safety_report(
        path,
        &SafetyReport {
            schema_version: 2,
            scoring_version: status.scoring_version.clone(),
            run_id: status.run_id.clone(),
            git_sha: status.git_sha.clone(),
            corpus_version: status.corpus_version.clone(),
            evaluation_profile: status.evaluation_profile,
            dirty_worktree: status.dirty_worktree,
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
            complete,
            all_zero,
        },
    )
}

pub(crate) fn read_git_sha() -> Result<String, Box<dyn std::error::Error>> {
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

pub(crate) fn git_worktree_is_dirty() -> Result<bool, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=all"])
        .output()?;
    if !output.status.success() {
        return Err("git status failed".into());
    }
    Ok(!output.stdout.is_empty())
}
