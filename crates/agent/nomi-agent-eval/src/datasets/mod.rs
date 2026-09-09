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
pub const SUITE_OFFICE_CORE: &str = "office_core";
pub const SUITE_AGENT_WORKFLOWS: &str = "agent_workflows";
pub const SUITE_CODING_LOCAL: &str = "coding_local";
pub const SUITE_HARNESS_CONTROL: &str = "harness_control";
pub const SUITE_HARNESS_SMOKE: &str = "harness_smoke";
pub const SUITE_BROWSER_SMOKE: &str = "browser_smoke";
pub const SUITE_MCP_FIXTURE: &str = "mcp_fixture";
pub const SUITE_PRIVATE_BADCASES: &str = "private_badcases";
pub const SUITE_HARBOR: &str = "harbor_terminal_bench";

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
    #[error("suite {0} requires a sandbox runner (not implemented)")]
    RequiresSandbox(String),
    #[error("suite {0} has no cases")]
    EmptySuite(String),
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
    /// `smoke` | `capability` | `advanced` | `sandbox`.
    #[serde(default = "default_tier")]
    pub tier: String,
    #[serde(default = "default_trials")]
    pub default_trials: u32,
    #[serde(default)]
    pub requires_sandbox: bool,
}

fn default_tier() -> String {
    "capability".into()
}

fn default_trials() -> u32 {
    1
}

/// Map legacy suite ids onto the current catalog.
pub fn canonical_suite_id(id: &str) -> &str {
    match id.trim() {
        SUITE_OFFICE_TASKS => SUITE_OFFICE_CORE,
        SUITE_HARNESS_CONTROL => SUITE_HARNESS_SMOKE,
        SUITE_AGENT_WORKFLOWS => SUITE_CODING_LOCAL,
        other => other,
    }
}

pub fn default_trials_for_suite(id: &str) -> u32 {
    suite_descriptor(id)
        .map(|s| s.default_trials)
        .unwrap_or(1)
}

fn desc(
    id: &str,
    title: &str,
    kind: &str,
    profile: &str,
    source_url: Option<String>,
    default_limit: usize,
    max_limit: usize,
    notes: &str,
    requires_download: bool,
    tier: &str,
    default_trials: u32,
    requires_sandbox: bool,
) -> SuiteDescriptor {
    SuiteDescriptor {
        id: id.into(),
        title: title.into(),
        kind: kind.into(),
        default_task_profile: profile.into(),
        source_url,
        default_limit,
        max_limit,
        notes: notes.into(),
        requires_download,
        tier: tier.into(),
        default_trials,
        requires_sandbox,
    }
}

pub fn list_suites() -> Vec<SuiteDescriptor> {
    vec![
        desc(
            SUITE_HARNESS_SMOKE,
            "Harness smoke",
            "bundled",
            "coding",
            None,
            2,
            2,
            "Runtime regression: Write/Edit + write_root. Not a capability KPI.",
            false,
            "smoke",
            1,
            false,
        ),
        desc(
            SUITE_OFFICE_CORE,
            "Office core",
            "bundled",
            "office",
            None,
            7,
            7,
            "Office capability: memo, minutes, CSV budget, email, rewrite, briefing, policy. Structural oracles, no magic tokens.",
            false,
            "capability",
            3,
            false,
        ),
        desc(
            SUITE_CODING_LOCAL,
            "Coding local",
            "bundled",
            "coding",
            None,
            3,
            3,
            "Local coding loop: pytest, CSV→JSON, refactor. No Docker.",
            false,
            "capability",
            3,
            false,
        ),
        desc(
            SUITE_BROWSER_SMOKE,
            "Browser smoke",
            "bundled",
            "office",
            None,
            2,
            2,
            "Opens the browser tool against a local HTML fixture. No public internet.",
            false,
            "capability",
            1,
            false,
        ),
        desc(
            SUITE_MCP_FIXTURE,
            "MCP fixture",
            "bundled",
            "office",
            None,
            2,
            2,
            "Injects a stdio CRM MCP server. Suite-level overlay; host MCP stays off.",
            false,
            "capability",
            1,
            false,
        ),
        desc(
            SUITE_AIDER_POLYGLOT,
            "Aider Polyglot (Python)",
            "agent",
            "coding",
            Some(aider::POLYGLOT_ZIP_URL.into()),
            DEFAULT_DOWNLOAD_LIMIT,
            MAX_DOWNLOAD_LIMIT,
            "Advanced coding-agent suite. Not a default KPI and not an official Aider leaderboard score.",
            true,
            "advanced",
            1,
            false,
        ),
        desc(
            SUITE_CLASSEVAL,
            "ClassEval",
            "agent",
            "coding",
            Some(classeval::CLASSEVAL_URL.into()),
            DEFAULT_DOWNLOAD_LIMIT,
            MAX_DOWNLOAD_LIMIT,
            "Advanced class-level Python. Hidden unittests. Not a default KPI.",
            true,
            "advanced",
            1,
            false,
        ),
        desc(
            SUITE_HARBOR,
            "Harbor / Terminal-Bench",
            "sandbox",
            "coding",
            None,
            0,
            0,
            "Placeholder. Official Harbor scores need a Docker farm (P2). This suite cannot run.",
            false,
            "sandbox",
            1,
            true,
        ),
        desc(
            SUITE_PRIVATE_BADCASES,
            "Private badcases",
            "private",
            "office",
            None,
            20,
            50,
            "Promoted cloud badcases synced to diagnostics/agent-evals/private/.",
            false,
            "capability",
            1,
            false,
        ),
    ]
}

pub fn suite_descriptor(id: &str) -> Option<SuiteDescriptor> {
    let canonical = canonical_suite_id(id);
    list_suites().into_iter().find(|s| s.id == canonical)
}

pub fn private_corpus_dir(data_dir: impl AsRef<Path>) -> PathBuf {
    data_dir.as_ref().join("diagnostics/agent-evals/private")
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
    let suite = canonical_suite_id(suite.trim());
    match suite {
        SUITE_HARNESS_SMOKE
        | SUITE_OFFICE_CORE
        | SUITE_CODING_LOCAL
        | SUITE_BROWSER_SMOKE
        | SUITE_MCP_FIXTURE => {
            let mut manifest = load_bundled_manifest(suite)?;
            apply_limit(&mut manifest, limit);
            Ok(manifest)
        }
        SUITE_PRIVATE_BADCASES => load_private_manifest(cache_dir, limit),
        SUITE_HARBOR => Err(DatasetError::RequiresSandbox(suite.to_owned())),
        SUITE_AIDER_POLYGLOT => aider::load_aider_polyglot(cache_dir, limit).await,
        SUITE_CLASSEVAL => classeval::load_classeval(cache_dir, limit).await,
        other => Err(DatasetError::UnknownSuite(other.to_owned())),
    }
}

fn load_private_manifest(cache_dir: &Path, limit: Option<usize>) -> Result<Manifest, DatasetError> {
    let dir = cache_dir
        .parent()
        .unwrap_or(cache_dir)
        .join("private");
    let mut cases = Vec::new();
    if dir.is_dir() {
        let mut files: Vec<_> = fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
            .collect();
        files.sort();
        for path in files {
            let mut manifest = crate::corpus::load_manifest(&path)?;
            cases.append(&mut manifest.cases);
        }
    }
    if cases.is_empty() {
        return Err(DatasetError::EmptySuite(SUITE_PRIVATE_BADCASES.into()));
    }
    let mut manifest = Manifest {
        schema_version: crate::types::SCHEMA_VERSION,
        corpus_version: "private-badcases".into(),
        suite: SUITE_PRIVATE_BADCASES.into(),
        cases,
    };
    crate::corpus::validate_manifest(&manifest)?;
    apply_limit(&mut manifest, limit);
    Ok(manifest)
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
        assert_eq!(ids[0], SUITE_HARNESS_SMOKE);
        assert!(ids.contains(&SUITE_OFFICE_CORE.to_string()));
        assert!(ids.contains(&SUITE_CODING_LOCAL.to_string()));
        assert!(ids.contains(&SUITE_BROWSER_SMOKE.to_string()));
        assert!(ids.contains(&SUITE_MCP_FIXTURE.to_string()));
        assert!(ids.contains(&SUITE_AIDER_POLYGLOT.to_string()));
        assert!(ids.contains(&SUITE_CLASSEVAL.to_string()));
        assert!(ids.contains(&SUITE_HARBOR.to_string()));
        assert!(!ids.iter().any(|id| id == "humaneval" || id == "mbpp"));
        assert!(!ids.iter().any(|id| id == SUITE_SESSION_DIALOGUE));
        load_bundled_manifest(SUITE_HARNESS_CONTROL).unwrap();
        load_bundled_manifest(SUITE_OFFICE_TASKS).unwrap();
        load_bundled_manifest(SUITE_AGENT_WORKFLOWS).unwrap();
        assert_eq!(canonical_suite_id(SUITE_OFFICE_TASKS), SUITE_OFFICE_CORE);
        assert!(list_suites().iter().any(|s| s.id == SUITE_AIDER_POLYGLOT && s.tier == "advanced"));
        assert!(list_suites().iter().any(|s| s.id == SUITE_HARBOR && s.requires_sandbox));
    }

    #[test]
    fn download_cache_detects_zip_and_limit_files() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_download_cached(SUITE_AIDER_POLYGLOT, dir.path()));
        std::fs::write(dir.path().join("aider-polyglot.zip"), b"pk").unwrap();
        assert!(is_download_cached(SUITE_AIDER_POLYGLOT, dir.path()));
        assert!(is_download_cached(SUITE_OFFICE_CORE, dir.path()));
    }
}
