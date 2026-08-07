//! Run / summarize / demo orchestration with sanitized JSONL evidence.

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use thiserror::Error;
use uuid::Uuid;

use crate::corpus::{load_manifest, CorpusError};
use crate::harness::{ConversationEvalHarness, HarnessError, OfflineDemoHarness};
use crate::scorer::score_all;
use crate::types::{
    CategorySummary, EvalResult, Manifest, Summary, SCORING_VERSION, SCHEMA_VERSION,
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
    let manifest = load_manifest(&config.manifest)?;
    run_manifest(config, &manifest, harness).await
}

async fn run_manifest(
    config: RunConfig,
    manifest: &Manifest,
    harness: Arc<dyn ConversationEvalHarness>,
) -> Result<RunReport, RunnerError> {
    let run_id = Uuid::now_v7().to_string();
    let already = if config.resume {
        completed_case_ids(&config.output)?
    } else {
        // Fresh run: truncate existing output if present.
        if config.output.exists() {
            fs::write(&config.output, "")?;
        }
        HashSet::new()
    };

    let enabled: Vec<_> = manifest.cases.iter().filter(|c| c.enabled).collect();
    let mut skipped_resume = 0usize;
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut completed = 0usize;

    for case in &enabled {
        if already.contains(&case.id) {
            skipped_resume += 1;
            continue;
        }

        let started = Instant::now();
        let result = match harness.run_case(case).await {
            Ok(transcript) => {
                let (success, scorer_results) = score_all(&case.scorers, &transcript);
                // Enforce budget max_turns as an implicit gate when set.
                let budget_ok = case
                    .budgets
                    .max_turns
                    .map(|max| transcript.turns <= max)
                    .unwrap_or(true);
                let success = success && budget_ok;
                EvalResult {
                    schema_version: SCHEMA_VERSION,
                    scoring_version: SCORING_VERSION.to_owned(),
                    run_id: run_id.clone(),
                    corpus_version: manifest.corpus_version.clone(),
                    suite: manifest.suite.clone(),
                    case_id: case.id.clone(),
                    category: case.category.clone(),
                    prompt: sanitize_prompt(&case.prompt),
                    success,
                    scorer_results,
                    elapsed_ms: started.elapsed().as_millis(),
                    turns: transcript.turns,
                    tool_call_count: transcript.tool_names.len() as u32,
                    tag: config.tag.clone(),
                    error: if budget_ok {
                        None
                    } else {
                        Some("budget_max_turns_exceeded".into())
                    },
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
            },
        };

        if result.success {
            passed += 1;
        } else {
            failed += 1;
        }
        append_evidence(&config.output, &result)?;
        completed += 1;
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
        output: config.output,
    })
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
            },
            Arc::new(OfflineDemoHarness),
        )
        .await
        .unwrap();
        assert_eq!(second.completed, 0);
        assert_eq!(second.skipped_resume, first.planned);
    }
}
