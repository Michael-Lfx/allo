//! Suite catalog and download adapters for public agent-eval datasets.

mod aider;
mod classeval;

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::corpus::{load_bundled_manifest, CorpusError};
use crate::types::{Case, CaseBudgets, Manifest, ScorerSpec, SCHEMA_VERSION};

pub use aider::{aider_zip_to_manifest, SUITE_AIDER_POLYGLOT};
pub use classeval::{classeval_json_to_manifest, SUITE_CLASSEVAL};

pub const SUITE_SESSION_DIALOGUE: &str = "session_dialogue";
pub const SUITE_OFFICE_TASKS: &str = "office_tasks";
pub const SUITE_HARNESS_CONTROL: &str = "harness_control";
pub const SUITE_HUMANEVAL: &str = "humaneval";
pub const SUITE_MBPP: &str = "mbpp";

const HUMANEVAL_URL: &str =
    "https://raw.githubusercontent.com/openai/human-eval/master/data/HumanEval.jsonl.gz";
const HUMANEVAL_URLS: &[&str] = &[
    HUMANEVAL_URL,
    "https://cdn.jsdelivr.net/gh/openai/human-eval@master/data/HumanEval.jsonl.gz",
];
const MBPP_URL: &str =
    "https://raw.githubusercontent.com/google-research/google-research/master/mbpp/sanitized-mbpp.json";
const MBPP_URLS: &[&str] = &[
    MBPP_URL,
    "https://cdn.jsdelivr.net/gh/google-research/google-research@master/mbpp/sanitized-mbpp.json",
];

pub(crate) const DEFAULT_DOWNLOAD_LIMIT: usize = 8;
pub(crate) const MAX_DOWNLOAD_LIMIT: usize = 20;

#[derive(Debug, Error)]
pub enum DatasetError {
    #[error(transparent)]
    Corpus(#[from] CorpusError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("unknown suite: {0}")]
    UnknownSuite(String),
    #[error("failed to download {url}: {message}")]
    Download { url: String, message: String },
    #[error("failed to read archive: {0}")]
    Archive(String),
}

/// Catalog row shown in the eval lab UI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SuiteDescriptor {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub default_task_profile: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    pub default_limit: usize,
    pub max_limit: usize,
    pub notes: String,
    pub requires_download: bool,
}

pub fn list_suites() -> Vec<SuiteDescriptor> {
    vec![
        SuiteDescriptor {
            id: SUITE_OFFICE_TASKS.into(),
            title: "Office tasks".into(),
            kind: "bundled".into(),
            default_task_profile: "office".into(),
            source_url: None,
            default_limit: 5,
            max_limit: 5,
            notes: "Primary office-agent suite: memo, minutes, CSV budget, client email, rewrite. Uses Office profile (Read/Write/Edit), not CodingHarness.".into(),
            requires_download: false,
        },
        SuiteDescriptor {
            id: SUITE_SESSION_DIALOGUE.into(),
            title: "Session dialogue".into(),
            kind: "bundled".into(),
            default_task_profile: "office".into(),
            source_url: None,
            default_limit: 5,
            max_limit: 5,
            notes: "Bundled conversation-loop contracts. Not a coding-agent benchmark.".into(),
            requires_download: false,
        },
        SuiteDescriptor {
            id: SUITE_HARNESS_CONTROL.into(),
            title: "Harness control".into(),
            kind: "bundled".into(),
            default_task_profile: "coding".into(),
            source_url: None,
            default_limit: 2,
            max_limit: 2,
            notes: "Smoke tests for CodingHarness + write_root. Not an agent capability KPI.".into(),
            requires_download: false,
        },
        SuiteDescriptor {
            id: SUITE_AIDER_POLYGLOT.into(),
            title: "Aider Polyglot (Python)".into(),
            kind: "agent".into(),
            default_task_profile: "coding".into(),
            source_url: Some(aider::POLYGLOT_ZIP_URL.into()),
            default_limit: DEFAULT_DOWNLOAD_LIMIT,
            max_limit: MAX_DOWNLOAD_LIMIT,
            notes: "Primary coding-agent suite: Exercism Python from Aider polyglot. Stub + tests in the workspace; pytest is the oracle. Canonical example.py is stripped. Needs Python + pytest. Not an official Aider leaderboard score (no Docker harness).".into(),
            requires_download: true,
        },
        SuiteDescriptor {
            id: SUITE_CLASSEVAL.into(),
            title: "ClassEval".into(),
            kind: "agent".into(),
            default_task_profile: "coding".into(),
            source_url: Some(classeval::CLASSEVAL_URL.into()),
            default_limit: DEFAULT_DOWNLOAD_LIMIT,
            max_limit: MAX_DOWNLOAD_LIMIT,
            notes: "Class-level Python (100 tasks). Agent edits the skeleton in solution.py; hidden unittests are the oracle. Harder than HumanEval, still closer to codegen than SWE-bench.".into(),
            requires_download: true,
        },
        SuiteDescriptor {
            id: SUITE_HUMANEVAL.into(),
            title: "HumanEval (unit floor)".into(),
            kind: "unit".into(),
            default_task_profile: "coding".into(),
            source_url: Some(HUMANEVAL_URL.into()),
            default_limit: DEFAULT_DOWNLOAD_LIMIT,
            max_limit: MAX_DOWNLOAD_LIMIT,
            notes: "Single-function codegen floor. Not an agent eval — do not use as the harness KPI.".into(),
            requires_download: true,
        },
        SuiteDescriptor {
            id: SUITE_MBPP.into(),
            title: "MBPP sanitized (unit floor)".into(),
            kind: "unit".into(),
            default_task_profile: "coding".into(),
            source_url: Some(MBPP_URL.into()),
            default_limit: DEFAULT_DOWNLOAD_LIMIT,
            max_limit: MAX_DOWNLOAD_LIMIT,
            notes: "Short programming prompts + hidden asserts. Unit-coding floor, not an agent eval.".into(),
            requires_download: true,
        },
    ]
}

pub fn suite_descriptor(id: &str) -> Option<SuiteDescriptor> {
    list_suites().into_iter().find(|s| s.id == id)
}

/// Whether a downloadable suite already has a cache file under `cache_dir`.
pub fn is_download_cached(suite_id: &str, cache_dir: &Path) -> bool {
    let Some(suite) = suite_descriptor(suite_id) else {
        return false;
    };
    if !suite.requires_download {
        return true;
    }
    let prefix = format!("{suite_id}.limit");
    let extras: &[&str] = match suite_id {
        SUITE_AIDER_POLYGLOT => &["aider-polyglot.zip"],
        SUITE_CLASSEVAL => &["classeval.json"],
        _ => &[],
    };
    cache_dir
        .read_dir()
        .ok()
        .map(|entries| {
            entries.filter_map(|e| e.ok()).any(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                name.starts_with(&prefix) || extras.iter().any(|extra| name == *extra)
            })
        })
        .unwrap_or(false)
}

/// Load a suite, downloading and caching remote datasets when needed.
pub async fn load_suite_manifest(
    suite: &str,
    cache_dir: &Path,
    limit: Option<usize>,
) -> Result<Manifest, DatasetError> {
    match suite.trim() {
        SUITE_SESSION_DIALOGUE | SUITE_HARNESS_CONTROL | SUITE_OFFICE_TASKS => {
            let mut manifest = load_bundled_manifest(suite)?;
            apply_limit(&mut manifest, limit);
            Ok(manifest)
        }
        SUITE_AIDER_POLYGLOT => aider::load_aider_polyglot(cache_dir, limit).await,
        SUITE_CLASSEVAL => classeval::load_classeval(cache_dir, limit).await,
        SUITE_HUMANEVAL => load_humaneval(cache_dir, clamp_limit(limit)).await,
        SUITE_MBPP => load_mbpp(cache_dir, clamp_limit(limit)).await,
        other => Err(DatasetError::UnknownSuite(other.to_owned())),
    }
}

pub(crate) fn clamp_limit(limit: Option<usize>) -> usize {
    limit
        .unwrap_or(DEFAULT_DOWNLOAD_LIMIT)
        .clamp(1, MAX_DOWNLOAD_LIMIT)
}

fn apply_limit(manifest: &mut Manifest, limit: Option<usize>) {
    let Some(limit) = limit else {
        return;
    };
    let mut kept = 0usize;
    manifest.cases.retain(|case| {
        if !case.enabled {
            return false;
        }
        if kept >= limit {
            return false;
        }
        kept += 1;
        true
    });
}

async fn load_humaneval(cache_dir: &Path, limit: usize) -> Result<Manifest, DatasetError> {
    let cached = cache_dir.join(format!("humaneval.limit{limit}.json"));
    if cached.exists() {
        return Ok(crate::corpus::load_manifest(&cached)?);
    }
    let bytes = http_get_first_ok(HUMANEVAL_URLS, 60).await?;
    let mut decoder = GzDecoder::new(bytes.as_slice());
    let mut jsonl = String::new();
    decoder.read_to_string(&mut jsonl)?;
    let manifest = humaneval_jsonl_to_manifest(&jsonl, limit)?;
    write_cached_manifest(&cached, &manifest)?;
    Ok(manifest)
}

async fn load_mbpp(cache_dir: &Path, limit: usize) -> Result<Manifest, DatasetError> {
    let cached = cache_dir.join(format!("mbpp.limit{limit}.json"));
    if cached.exists() {
        return Ok(crate::corpus::load_manifest(&cached)?);
    }
    let bytes = http_get_first_ok(MBPP_URLS, 60).await?;
    let text = String::from_utf8(bytes).map_err(|e| DatasetError::Download {
        url: MBPP_URL.into(),
        message: format!("invalid utf-8: {e}"),
    })?;
    let manifest = mbpp_json_to_manifest(&text, limit)?;
    write_cached_manifest(&cached, &manifest)?;
    Ok(manifest)
}

pub(crate) fn write_cached_manifest(path: &Path, manifest: &Manifest) -> Result<(), DatasetError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(manifest)?;
    fs::write(path, json)?;
    Ok(())
}

/// Try each URL until one returns a non-empty body (GitHub raw → CDN mirrors).
pub(crate) async fn http_get_first_ok(
    urls: &[&str],
    timeout_secs: u64,
) -> Result<Vec<u8>, DatasetError> {
    let mut last = None;
    for url in urls {
        match http_get_bytes_timed(url, timeout_secs).await {
            Ok(bytes) if !bytes.is_empty() => return Ok(bytes),
            Ok(_) => {
                last = Some(DatasetError::Download {
                    url: (*url).to_owned(),
                    message: "empty body".into(),
                });
            }
            Err(error) => last = Some(error),
        }
    }
    Err(last.unwrap_or_else(|| DatasetError::Download {
        url: urls.first().copied().unwrap_or("").to_owned(),
        message: "no download URLs configured".into(),
    }))
}

pub(crate) async fn http_get_bytes_timed(
    url: &str,
    timeout_secs: u64,
) -> Result<Vec<u8>, DatasetError> {
    let client = reqwest::Client::builder()
        .user_agent("nomifun-agent-eval/1.0")
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| DatasetError::Download {
            url: url.to_owned(),
            message: e.to_string(),
        })?;
    let response = client.get(url).send().await.map_err(|e| DatasetError::Download {
        url: url.to_owned(),
        message: e.to_string(),
    })?;
    if !response.status().is_success() {
        return Err(DatasetError::Download {
            url: url.to_owned(),
            message: format!("http {}", response.status()),
        });
    }
    response.bytes().await.map(|b| b.to_vec()).map_err(|e| DatasetError::Download {
        url: url.to_owned(),
        message: e.to_string(),
    })
}

#[derive(Debug, Deserialize)]
struct HumanEvalRow {
    task_id: String,
    prompt: String,
    entry_point: String,
    test: String,
}

pub fn humaneval_jsonl_to_manifest(jsonl: &str, limit: usize) -> Result<Manifest, DatasetError> {
    let mut cases = Vec::new();
    for line in jsonl.lines() {
        if cases.len() >= limit {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let row: HumanEvalRow = serde_json::from_str(trimmed)?;
        let id = row
            .task_id
            .replace('/', "-")
            .replace(' ', "_");
        let prompt = format!(
            "Implement the following Python function in a file named solution.py at the workspace root.\n\
The file must define `{entry}`. Write a complete, correct implementation. Do not create test files.\n\n\
```python\n{body}```",
            entry = row.entry_point,
            body = row.prompt
        );
        cases.push(Case {
            id,
            category: "humaneval".into(),
            prompt,
            enabled: true,
            budgets: CaseBudgets {
                max_turns: Some(12),
                max_tokens: Some(65536),
            },
            scorers: vec![ScorerSpec::PythonHiddenCheck {
                entry_point: row.entry_point,
                test: row.test,
            }],
            notes: Some(row.task_id),
            task_profile: Some("coding".into()),
            workspace_files: Default::default(),
            timeout_secs: Some(180),
        });
    }
    if cases.is_empty() {
        return Err(DatasetError::Corpus(CorpusError::Invalid(
            "HumanEval JSONL produced no cases".into(),
        )));
    }
    let manifest = Manifest {
        schema_version: SCHEMA_VERSION,
        corpus_version: format!("humaneval-openai-{limit}"),
        suite: SUITE_HUMANEVAL.into(),
        cases,
    };
    crate::corpus::validate_manifest(&manifest)?;
    Ok(manifest)
}

#[derive(Debug, Deserialize)]
struct MbppRow {
    #[serde(default)]
    task_id: serde_json::Value,
    /// Full MBPP uses `text`; sanitized MBPP uses `prompt`.
    #[serde(default)]
    text: String,
    #[serde(default)]
    prompt: String,
    #[serde(default)]
    test_list: Vec<String>,
}

impl MbppRow {
    fn description(&self) -> &str {
        let prompt = self.prompt.trim();
        if !prompt.is_empty() {
            prompt
        } else {
            self.text.trim()
        }
    }
}

pub fn mbpp_json_to_manifest(json: &str, limit: usize) -> Result<Manifest, DatasetError> {
    let rows: Vec<MbppRow> = serde_json::from_str(json)?;
    let mut cases = Vec::new();
    for row in rows {
        if cases.len() >= limit {
            break;
        }
        let description = row.description();
        if description.is_empty() || row.test_list.is_empty() {
            continue;
        }
        let id = match &row.task_id {
            serde_json::Value::Number(n) => format!("mbpp-{n}"),
            serde_json::Value::String(s) => format!("mbpp-{s}"),
            _ => format!("mbpp-{}", cases.len()),
        };
        let asserts = row.test_list.join("\n");
        let prompt = format!(
            "Write a Python module solution.py that solves the following problem.\n\
The hidden tests import names defined in solution.py. Do not write the test file.\n\n{}\n",
            description
        );
        cases.push(Case {
            id,
            category: "mbpp".into(),
            prompt,
            enabled: true,
            budgets: CaseBudgets {
                max_turns: Some(12),
                max_tokens: Some(65536),
            },
            scorers: vec![ScorerSpec::PythonHiddenCheck {
                entry_point: "_module".into(),
                test: format!(
                    "def check(module):\n    exec({asserts:?}, module.__dict__, module.__dict__)\n"
                ),
            }],
            notes: None,
            task_profile: Some("coding".into()),
            workspace_files: Default::default(),
            timeout_secs: Some(180),
        });
    }
    if cases.is_empty() {
        return Err(DatasetError::Corpus(CorpusError::Invalid(
            "MBPP JSON produced no cases (expected sanitized rows with prompt/text + test_list)"
                .into(),
        )));
    }
    let manifest = Manifest {
        schema_version: SCHEMA_VERSION,
        corpus_version: format!("mbpp-sanitized-{limit}"),
        suite: SUITE_MBPP.into(),
        cases,
    };
    crate::corpus::validate_manifest(&manifest)?;
    Ok(manifest)
}

/// Cache directory used by the desktop eval lab.
pub fn cache_dir(data_dir: impl AsRef<Path>) -> PathBuf {
    data_dir.as_ref().join("diagnostics/agent-evals/datasets")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humaneval_adapter_builds_python_oracle_cases() {
        let jsonl = r#"{"task_id":"HumanEval/0","prompt":"def add(a, b):\n    \"\"\"add\"\"\"\n","entry_point":"add","test":"def check(candidate):\n    assert candidate(1,2)==3\n"}
{"task_id":"HumanEval/1","prompt":"def sub(a, b):\n    pass\n","entry_point":"sub","test":"def check(candidate):\n    assert candidate(3,1)==2\n"}"#;
        let manifest = humaneval_jsonl_to_manifest(jsonl, 1).unwrap();
        assert_eq!(manifest.suite, SUITE_HUMANEVAL);
        assert_eq!(manifest.cases.len(), 1);
        assert_eq!(manifest.cases[0].id, "HumanEval-0");
        assert!(manifest.cases[0].prompt.contains("solution.py"));
        assert!(matches!(
            manifest.cases[0].scorers[0],
            ScorerSpec::PythonHiddenCheck { .. }
        ));
    }

    #[test]
    fn mbpp_adapter_skips_empty_rows() {
        let json = r#"[{"task_id":1,"text":"Write add","test_list":["assert add(1,2)==3"]},{"task_id":2,"text":"","test_list":[]}]"#;
        let manifest = mbpp_json_to_manifest(json, 8).unwrap();
        assert_eq!(manifest.cases.len(), 1);
        assert_eq!(manifest.cases[0].id, "mbpp-1");
    }

    #[test]
    fn mbpp_adapter_accepts_sanitized_prompt_field() {
        let json = r#"[{"task_id":2,"prompt":"Write similar_elements","test_list":["assert similar_elements((1,2),(2,3))==(2,)"]}]"#;
        let manifest = mbpp_json_to_manifest(json, 8).unwrap();
        assert_eq!(manifest.cases.len(), 1);
        assert!(manifest.cases[0].prompt.contains("similar_elements"));
    }

    #[test]
    fn catalog_puts_agent_suites_before_unit_floors() {
        let ids: Vec<_> = list_suites().into_iter().map(|s| s.id).collect();
        let office = ids.iter().position(|id| id == SUITE_OFFICE_TASKS).unwrap();
        let aider = ids.iter().position(|id| id == SUITE_AIDER_POLYGLOT).unwrap();
        let humaneval = ids.iter().position(|id| id == SUITE_HUMANEVAL).unwrap();
        assert_eq!(office, 0);
        assert!(aider < humaneval);
        assert!(ids.contains(&SUITE_CLASSEVAL.to_string()));
        load_bundled_manifest(SUITE_HARNESS_CONTROL).unwrap();
        load_bundled_manifest(SUITE_OFFICE_TASKS).unwrap();
    }

    #[test]
    fn download_cache_detects_zip_and_limit_files() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_download_cached(SUITE_AIDER_POLYGLOT, dir.path()));
        std::fs::write(dir.path().join("aider-polyglot.zip"), b"pk").unwrap();
        assert!(is_download_cached(SUITE_AIDER_POLYGLOT, dir.path()));
        assert!(is_download_cached(SUITE_SESSION_DIALOGUE, dir.path()));
    }
}
