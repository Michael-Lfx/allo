//! Suite catalog and download adapters for public agent-eval datasets.

mod aider;
mod classeval;

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::corpus::{load_bundled_manifest, CorpusError};
use crate::types::Manifest;

pub use aider::{aider_zip_to_manifest, SUITE_AIDER_POLYGLOT};
pub use classeval::{classeval_json_to_manifest, SUITE_CLASSEVAL};

pub const SUITE_OFFICE_TASKS: &str = "office_tasks";
pub const SUITE_AGENT_WORKFLOWS: &str = "agent_workflows";
pub const SUITE_HARNESS_CONTROL: &str = "harness_control";

/// Legacy offline-demo corpus id. Not listed in the live lab catalog.
pub const SUITE_SESSION_DIALOGUE: &str = "session_dialogue";

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
            id: SUITE_AGENT_WORKFLOWS.into(),
            title: "Agent workflows".into(),
            kind: "bundled".into(),
            default_task_profile: "coding".into(),
            source_url: None,
            default_limit: 5,
            max_limit: 5,
            notes: "Multi-step agent KPI: multi-file briefing, debug+pytest, CSV→JSON pipeline, refactor+docs, constrained policy edit. Replaces marker-style Q&A floors.".into(),
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
            notes: "Class-level Python (100 tasks). Agent edits the skeleton in solution.py; hidden unittests are the oracle. Harder than HumanEval-style floors; still closer to codegen than SWE-bench.".into(),
            requires_download: true,
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
        SUITE_OFFICE_TASKS | SUITE_AGENT_WORKFLOWS | SUITE_HARNESS_CONTROL => {
            let mut manifest = load_bundled_manifest(suite)?;
            apply_limit(&mut manifest, limit);
            Ok(manifest)
        }
        SUITE_AIDER_POLYGLOT => aider::load_aider_polyglot(cache_dir, limit).await,
        SUITE_CLASSEVAL => classeval::load_classeval(cache_dir, limit).await,
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

/// Cache directory used by the desktop eval lab.
pub fn cache_dir(data_dir: impl AsRef<Path>) -> PathBuf {
    data_dir.as_ref().join("diagnostics/agent-evals/datasets")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_lists_agent_suites_not_unit_floors() {
        let ids: Vec<_> = list_suites().into_iter().map(|s| s.id).collect();
        assert_eq!(ids[0], SUITE_OFFICE_TASKS);
        assert!(ids.contains(&SUITE_AGENT_WORKFLOWS.to_string()));
        assert!(ids.contains(&SUITE_AIDER_POLYGLOT.to_string()));
        assert!(ids.contains(&SUITE_CLASSEVAL.to_string()));
        assert!(!ids.iter().any(|id| id == "humaneval" || id == "mbpp"));
        assert!(!ids.iter().any(|id| id == SUITE_SESSION_DIALOGUE));
        load_bundled_manifest(SUITE_HARNESS_CONTROL).unwrap();
        load_bundled_manifest(SUITE_OFFICE_TASKS).unwrap();
        load_bundled_manifest(SUITE_AGENT_WORKFLOWS).unwrap();
    }

    #[test]
    fn download_cache_detects_zip_and_limit_files() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_download_cached(SUITE_AIDER_POLYGLOT, dir.path()));
        std::fs::write(dir.path().join("aider-polyglot.zip"), b"pk").unwrap();
        assert!(is_download_cached(SUITE_AIDER_POLYGLOT, dir.path()));
        assert!(is_download_cached(SUITE_AGENT_WORKFLOWS, dir.path()));
    }
}
