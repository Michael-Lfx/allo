//! Run / summarize / demo drivers with sanitized JSONL evidence.

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use thiserror::Error;
use uuid::Uuid;

use crate::corpus::{load_manifest, CorpusError};
use crate::harness::{ConversationEvalHarness, HarnessError, OfflineDemoHarness};
use crate::scorer::score_all;
use crate::types::{
    Case, CategorySummary, EvalResult, Manifest, RunProgress, RunProgressPhase, Summary,
    TurnTranscript, SCORING_VERSION, SCHEMA_VERSION,
};

#[derive(Debug, Error)]
pub enum RunnerError {
    #[error(transparent)]
    Corpus(#[from] CorpusError),
    #[error(transparent)]
    Harness(#[from] HarnessError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Clone)]
pub struct RunConfig {
    pub manifest: PathBuf,
    pub output: PathBuf,
    pub tag: Option<String>,
    /// When true (default), skip case_ids already present in the output JSONL.
    pub resume: bool,
    pub cancel: Option<Arc<AtomicBool>>,
    pub case_limit: Option<usize>,
    pub model: Option<String>,
    pub provider_id: Option<String>,
    pub harness_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunReport {
    pub run_id: String,
    pub corpus_version: String,
    pub suite: String,
    pub planned: usize,
    pub completed: usize,
    pub skipped_resume: usize,
    pub passed: usize,
    pub failed: usize,
    pub cancelled: bool,
    pub output: PathBuf,
}

/// Redact secrets (including `sk-…`) before storing prompt evidence.
pub fn sanitize_prompt(prompt: &str) -> String {
    nomi_redact::redact_secrets_owned(prompt.to_owned())
}

/// Append one JSONL evidence line (creates parent dirs).
pub fn append_evidence(path: &Path, result: &EvalResult) -> Result<(), RunnerError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    serde_json::to_writer(&mut file, result)?;
    file.write_all(b"\n")?;
    Ok(())
}

/// Read completed case_ids from an existing JSONL evidence file (for resume).
pub fn completed_case_ids(path: &Path) -> Result<HashSet<String>, RunnerError> {
    let mut ids = HashSet::new();
    if !path.exists() {
        return Ok(ids);
    }
    let file = File::open(path)?;
    for line in BufReader::new(file).lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value: EvalResult = serde_json::from_str(trimmed)?;
        ids.insert(value.case_id);
    }
    Ok(ids)
}

/// Run all enabled cases through `harness`, writing sanitized JSONL evidence.
pub async fn run(
    config: RunConfig,
    harness: Arc<dyn ConversationEvalHarness>,
) -> Result<RunReport, RunnerError> {
    run_with_progress(config, harness, None).await
}

/// Same as [`run`], with optional per-case progress callbacks.
pub async fn run_with_progress(
    config: RunConfig,
    harness: Arc<dyn ConversationEvalHarness>,
    on_progress: Option<&(dyn Fn(RunProgress) + Send + Sync)>,
) -> Result<RunReport, RunnerError> {
    let manifest = load_manifest(&config.manifest)?;
    run_manifest(config, &manifest, harness, on_progress).await
}

/// Run an in-memory manifest (used by the desktop eval lab).
pub async fn run_loaded_manifest(
    config: RunConfig,
    manifest: &Manifest,
    harness: Arc<dyn ConversationEvalHarness>,
    on_progress: Option<&(dyn Fn(RunProgress) + Send + Sync)>,
) -> Result<RunReport, RunnerError> {
    run_manifest(config, manifest, harness, on_progress).await
}

async fn run_manifest(
    config: RunConfig,
    manifest: &Manifest,
    harness: Arc<dyn ConversationEvalHarness>,
    on_progress: Option<&(dyn Fn(RunProgress) + Send + Sync)>,
) -> Result<RunReport, RunnerError> {
    let run_id = Uuid::now_v7().to_string();
    let already = if config.resume {
        completed_case_ids(&config.output)?
    } else {
        if let Some(parent) = config.output.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&config.output, "")?;
        HashSet::new()
    };

    let mut enabled: Vec<_> = manifest.cases.iter().filter(|c| c.enabled).collect();
    if let Some(limit) = config.case_limit {
        enabled.truncate(limit);
    }
    let mut skipped_resume = 0usize;
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut completed = 0usize;
    let mut cancelled = false;

    for (offset, case) in enabled.iter().enumerate() {
        if config
            .cancel
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
        {
            cancelled = true;
            if let Some(cb) = on_progress {
                cb(RunProgress {
                    run_id: run_id.clone(),
                    case_id: case.id.clone(),
                    category: case.category.clone(),
                    index: offset + 1,
                    total: enabled.len(),
                    phase: RunProgressPhase::Cancelled,
                    success: None,
                });
            }
            break;
        }
        if already.contains(&case.id) {
            skipped_resume += 1;
            continue;
        }

        if let Some(cb) = on_progress {
            cb(RunProgress {
                run_id: run_id.clone(),
                case_id: case.id.clone(),
                category: case.category.clone(),
                index: offset + 1,
                total: enabled.len(),
                phase: RunProgressPhase::Started,
                success: None,
            });
        }

        let started = Instant::now();
        let result = match harness.run_case(case).await {
            Ok(transcript) => {
                let (success, scorer_results) = score_all(&case.scorers, &transcript);
                let budget_error = episode_budget_error(case, &transcript);
                let budget_ok = budget_error.is_none();
                EvalResult {
                    schema_version: SCHEMA_VERSION,
                    scoring_version: SCORING_VERSION.to_owned(),
                    run_id: run_id.clone(),
                    corpus_version: manifest.corpus_version.clone(),
                    suite: manifest.suite.clone(),
                    case_id: case.id.clone(),
                    category: case.category.clone(),
                    prompt: sanitize_prompt(&case.prompt),
                    success: success && budget_ok,
                    scorer_results,
                    elapsed_ms: started.elapsed().as_millis(),
                    turns: transcript.turns,
                    tool_call_count: transcript.tool_names.len() as u32,
                    tag: config.tag.clone(),
                    error: budget_error.map(str::to_owned),
                    model: config.model.clone(),
                    provider_id: config.provider_id.clone(),
                    harness_profile: config
                        .harness_profile
                        .clone()
                        .or_else(|| case.task_profile.clone()),
                    stop_reason: transcript.stop_reason.clone(),
                    input_tokens: transcript.input_tokens,
                    output_tokens: transcript.output_tokens,
                    tool_error_count: transcript.tool_error_count,
                    trajectory_event_count: transcript.trajectory.len() as u32,
                    artifact_count: transcript.artifacts.len() as u32,
                    conversation_id: transcript.conversation_id.clone(),
                }
            }
            Err(err) => EvalResult {
                schema_version: SCHEMA_VERSION,
                scoring_version: SCORING_VERSION.to_owned(),
                run_id: run_id.clone(),
                corpus_version: manifest.corpus_version.clone(),
                suite: manifest.suite.clone(),
                case_id: case.id.clone(),
                category: case.category.clone(),
                prompt: sanitize_prompt(&case.prompt),
                success: false,
                scorer_results: vec![],
                elapsed_ms: started.elapsed().as_millis(),
                turns: 0,
                tool_call_count: 0,
                tag: config.tag.clone(),
                error: Some(err.to_string()),
                model: config.model.clone(),
                provider_id: config.provider_id.clone(),
                harness_profile: config
                    .harness_profile
                    .clone()
                    .or_else(|| case.task_profile.clone()),
                stop_reason: None,
                input_tokens: 0,
                output_tokens: 0,
                tool_error_count: 0,
                trajectory_event_count: 0,
                artifact_count: 0,
                conversation_id: None,
            },
        };

        if result.success {
            passed += 1;
        } else {
            failed += 1;
        }
        append_evidence(&config.output, &result)?;
        completed += 1;
        if let Some(cb) = on_progress {
            cb(RunProgress {
                run_id: run_id.clone(),
                case_id: result.case_id.clone(),
                category: result.category.clone(),
                index: offset + 1,
                total: enabled.len(),
                phase: RunProgressPhase::Scored,
                success: Some(result.success),
            });
        }
    }

    Ok(RunReport {
        run_id,
        corpus_version: manifest.corpus_version.clone(),
        suite: manifest.suite.clone(),
        planned: enabled.len(),
        completed,
        skipped_resume,
        passed,
        failed,
        cancelled,
        output: config.output,
    })
}

/// Runaway caps. `max_tokens` is cumulative **output** tokens, not context/input.
fn episode_budget_error(case: &Case, transcript: &TurnTranscript) -> Option<&'static str> {
    if let Some(max) = case.budgets.max_turns {
        if transcript.turns > max {
            return Some("budget_max_turns_exceeded");
        }
    }
    if let Some(max) = case.budgets.max_tokens {
        if transcript.output_tokens > u64::from(max) {
            return Some("budget_max_tokens_exceeded");
        }
    }
    None
}

/// Offline demo: scripted harness over the bundled conversation corpus.
pub async fn run_demo(output: impl AsRef<Path>) -> Result<RunReport, RunnerError> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("evaluation/corpus.conversation.json");
    run(
        RunConfig {
            manifest,
            output: output.as_ref().to_path_buf(),
            tag: Some("offline-demo".into()),
            resume: false,
            cancel: None,
            case_limit: None,
            model: None,
            provider_id: None,
            harness_profile: Some("offline-demo".into()),
        },
        Arc::new(OfflineDemoHarness),
    )
    .await
}

/// Aggregate success rates from one or more JSONL evidence files.
pub fn summarize(inputs: &[PathBuf], output: Option<&Path>) -> Result<Summary, RunnerError> {
    let mut results = Vec::new();
    for path in inputs {
        if !path.exists() {
            return Err(RunnerError::Other(format!(
                "evidence file not found: {}",
                path.display()
            )));
        }
        let file = File::open(path)?;
        for line in BufReader::new(file).lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            results.push(serde_json::from_str::<EvalResult>(trimmed)?);
        }
    }

    let total = results.len();
    let passed = results.iter().filter(|r| r.success).count();
    let failed = total.saturating_sub(passed);
    let success_rate = if total == 0 {
        0.0
    } else {
        passed as f64 / total as f64
    };

    let mut by_cat: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for r in &results {
        let entry = by_cat.entry(r.category.clone()).or_insert((0, 0));
        entry.0 += 1;
        if r.success {
            entry.1 += 1;
        }
    }
    let by_category: Vec<CategorySummary> = by_cat
        .into_iter()
        .map(|(category, (total, passed))| CategorySummary {
            success_rate: if total == 0 {
                0.0
            } else {
                passed as f64 / total as f64
            },
            category,
            total,
            passed,
        })
        .collect();

    let corpus_version = results.first().map(|r| r.corpus_version.clone());
    let suite = results.first().map(|r| r.suite.clone());
    let avg_turns = mean(results.iter().map(|r| r.turns as f64), total);
    let avg_elapsed_ms = mean(results.iter().map(|r| r.elapsed_ms as f64), total);
    let avg_input_tokens = mean(results.iter().map(|r| r.input_tokens as f64), total);
    let avg_output_tokens = mean(results.iter().map(|r| r.output_tokens as f64), total);

    let summary = Summary {
        schema_version: SCHEMA_VERSION,
        scoring_version: SCORING_VERSION.to_owned(),
        total_cases: total,
        passed,
        failed,
        success_rate,
        by_category,
        corpus_version,
        suite,
        avg_turns,
        avg_elapsed_ms,
        avg_input_tokens,
        avg_output_tokens,
    };

    if let Some(path) = output {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&summary)?;
        fs::write(path, json)?;
    }

    Ok(summary)
}

fn mean(values: impl Iterator<Item = f64>, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        values.sum::<f64>() / total as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn sanitize_strips_sk_keys() {
        let raw = "The API key is sk-TESTSECRETKEY1234567890ABCD. Do not repeat.";
        let cleaned = sanitize_prompt(raw);
        assert!(!cleaned.contains("sk-TESTSECRETKEY"));
        assert!(cleaned.contains("[REDACTED_SECRET]"));
    }

    #[tokio::test]
    async fn demo_and_summarize_aggregate_success_rate() {
        let dir = tempdir().unwrap();
        let evidence = dir.path().join("demo.jsonl");
        let report = run_demo(&evidence).await.unwrap();
        assert_eq!(report.failed, 0);
        assert!(report.passed >= 5);

        let summary_path = dir.path().join("summary.json");
        let summary = summarize(&[evidence], Some(&summary_path)).unwrap();
        assert_eq!(summary.total_cases, report.passed + report.failed);
        assert!((summary.success_rate - 1.0).abs() < f64::EPSILON);
        assert!(summary_path.exists());
    }

    #[tokio::test]
    async fn resume_skips_completed_cases() {
        let dir = tempdir().unwrap();
        let evidence = dir.path().join("run.jsonl");
        let first = run_demo(&evidence).await.unwrap();
        let second = run(
            RunConfig {
                manifest: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("evaluation/corpus.conversation.json"),
                output: evidence.clone(),
                tag: Some("resume".into()),
                resume: true,
                cancel: None,
                case_limit: None,
                model: None,
                provider_id: None,
                harness_profile: None,
            },
            Arc::new(OfflineDemoHarness),
        )
        .await
        .unwrap();
        assert_eq!(second.completed, 0);
        assert_eq!(second.skipped_resume, first.planned);
    }

    #[tokio::test]
    async fn cancel_flag_skips_remaining_cases() {
        let dir = tempdir().unwrap();
        let evidence = dir.path().join("cancelled.jsonl");
        let cancel = Arc::new(AtomicBool::new(true));
        let report = run(
            RunConfig {
                manifest: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("evaluation/corpus.conversation.json"),
                output: evidence,
                tag: Some("cancel".into()),
                resume: false,
                cancel: Some(cancel),
                case_limit: None,
                model: None,
                provider_id: None,
                harness_profile: None,
            },
            Arc::new(OfflineDemoHarness),
        )
        .await
        .unwrap();
        assert!(report.cancelled);
        assert_eq!(report.completed, 0);
    }

    #[test]
    fn token_budget_ignores_input_context() {
        let case = Case {
            id: "t".into(),
            category: "x".into(),
            prompt: "p".into(),
            enabled: true,
            budgets: crate::types::CaseBudgets {
                max_turns: None,
                max_tokens: Some(8192),
            },
            scorers: vec![crate::types::ScorerSpec::MaxTurns { max: 99 }],
            notes: None,
            task_profile: None,
            workspace_files: Default::default(),
            timeout_secs: None,
        };
        let input_heavy = TurnTranscript {
            input_tokens: 50_000,
            output_tokens: 100,
            ..TurnTranscript::default()
        };
        assert!(episode_budget_error(&case, &input_heavy).is_none());
        let output_heavy = TurnTranscript {
            output_tokens: 9_000,
            ..TurnTranscript::default()
        };
        assert_eq!(
            episode_budget_error(&case, &output_heavy),
            Some("budget_max_tokens_exceeded")
        );
    }
}
