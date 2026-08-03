//! Evidence scoring and admission decisions.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use super::evidence::{
    atomic_json_write, default_safety_path, default_status_path, read_json,
};
use super::{
    CaseCategory, EvaluationMode, EvaluationProfile, FetchEvaluationResult, PeerMode, QualityGrade,
    RunStatus, SafetyReport,
};

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
    let result_records_by_run = results.iter().fold(
        BTreeMap::<&str, Vec<&FetchEvaluationResult>>::new(),
        |mut records, result| {
            records.entry(result.run_id.as_str()).or_default().push(result);
            records
        },
    );
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
            let run_records = result_records_by_run
                .get(status.run_id.as_str())
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            provenance_consistent &= run_records.iter().all(|result| {
                result.scoring_version == status.scoring_version
                    && result.corpus_version == status.corpus_version
                    && result.evaluation_profile == status.evaluation_profile
                    && result.git_sha == status.git_sha
            });
            counter_consistent &= run_records.len() == status.planned_attempts
                && run_records.len() == status.completed_attempts;
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
            let run_records = result_records_by_run
                .get(report.run_id.as_str())
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            provenance_consistent &= run_records.iter().all(|result| {
                result.scoring_version == report.scoring_version
                    && result.corpus_version == report.corpus_version
                    && result.evaluation_profile == report.evaluation_profile
                    && result.git_sha == report.git_sha
            });
        }
        counter_consistent &= report.actual_remote_calls
            == report.actual_fetch_calls.saturating_add(report.actual_search_calls);
        if let Some(status) = statuses.iter().find(|status| status.run_id == report.run_id) {
            counter_consistent &= status.actual_remote_calls == report.actual_remote_calls
                && status.actual_fetch_calls == report.actual_fetch_calls
                && status.actual_search_calls == report.actual_search_calls
                && status.recovery_retry_calls == report.recovery_retry_calls;
            counter_consistent &= status.sensitive_egress_count == report.sensitive_egress_count
                && status.retry_limit_violation_count == report.retry_limit_violation_count;
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
        || statuses.iter().any(|status| {
            status.sensitive_egress_count > 0 || status.retry_limit_violation_count > 0
        })
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

pub(crate) fn validate_admission_composition(
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

pub(crate) fn summarize_category(
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

pub(crate) fn admission_decision(
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

pub(crate) fn sensitivity(
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

pub(crate) fn category_label(category: CaseCategory) -> &'static str {
    match category {
        CaseCategory::PublicPdfText => "public_pdf_text",
        CaseCategory::PublicPdfScan => "public_pdf_scan",
        CaseCategory::JavascriptShell => "javascript_shell",
        CaseCategory::StaticHtmlControl => "static_html_control",
        CaseCategory::RealPdfPrivate => "real_pdf_private",
    }
}

pub(crate) fn rate(success: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        success as f64 / total as f64
    }
}

pub(crate) fn percentile(values: &[u128], percentile: f64) -> Option<u128> {
    if values.is_empty() {
        return None;
    }
    let mut values = values.to_vec();
    values.sort_unstable();
    let index = ((values.len() as f64 * percentile).ceil() as usize).saturating_sub(1);
    values.get(index.min(values.len() - 1)).copied()
}

pub(crate) fn wilson_interval(success: usize, total: usize) -> (f64, f64) {
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
