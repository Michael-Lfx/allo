//! Marketplace fetch orchestration (roadmap Phase 2, phase B).
//!
//! Bridges the low-level `market_source` primitives (clone / download /
//! promote) with the directory probe installed in `app_server_marketplace`:
//! fetch a remote source into a staging dir, validate + probe the content,
//! and atomically promote it into the live root. `refresh` reuses the same
//! path with a *new* staging dir so the live root is last-good during any
//! failure window.
//!
//! Layout under the marketplace root:
//!   {root}/{marketplace_id}/live            # promoted, validated content
//!   {root}/{marketplace_id}/staging-<ts>    # in-flight fetch (removed on
//!                                             failure or after promotion)

use std::path::{Path, PathBuf};
use std::sync::Arc;

use nomifun_common::AppError;
use nomifun_db::{IMarketplaceRepository, MarketplaceEntry};

use crate::market_source;

/// One resolved + validated marketplace root after `fetch_remote`.
pub struct FetchedEntrySet {
    /// The live root (already promoted; immutable for this cycle).
    pub live_root: PathBuf,
    /// Resolved revision (git commit / freshness marker).
    pub revision: String,
    /// Probed entries (relative sources inside the live root).
    pub entries: Vec<(MarketplaceEntry, String)>, // (entry, relative source)
}

/// Remote fetch outcome: fresh content or a 304-style no-op.
pub enum RemoteFetchOutcome {
    /// New content was fetched, validated and promoted.
    Fresh(FetchedEntrySet),
    /// The server answered 304 / unchanged revision — last-good stays.
    Unchanged { revision: String },
}

/// Resolve a remote source into a local, validated tree.
///
/// `source_kind` is `github` | `git` | `url`; directory sources are handled
/// by the caller before this point. When `current_revision` is provided, an
/// unchanged server revision short-circuits to `Unchanged` (git: same HEAD
/// commit; http: `304 Not Modified`).
pub async fn fetch_remote(
    source_kind: &str,
    source: &str,
    marketplace_root: PathBuf,
    current_revision: Option<&str>,
) -> Result<RemoteFetchOutcome, AppError> {
    let normalized = market_source::normalize_source_url(source_kind, source)
        .map_err(AppError::BadRequest)?;
    std::fs::create_dir_all(&marketplace_root)
        .map_err(|error| AppError::Internal(format!("create market root: {error}")))?;

    let staging = marketplace_root.join(format!("staging-{}", nomifun_common::generate_id()));
    let live_root = marketplace_root.join("live");

    // Fetch into staging; validate the tree looks like a market after the
    // fetch (git clones the whole repo; http downloads the manifest only).
    let (revision, manifest_only) = match source_kind {
        "github" | "git" => {
            let (hash, _repo) = market_source::clone_git(&normalized, &staging)
                .map_err(|error| AppError::Internal(error))?;
            if !market_source::looks_like_market(&staging) {
                let _ = std::fs::remove_dir_all(&staging);
                return Err(AppError::BadRequest(format!(
                    "source {source} does not look like a marketplace (no manifest in the checkout)"
                )));
            }
            if current_revision == Some(hash.as_str()) {
                // Same HEAD commit: no content change; the clone in staging is
                // discarded, last-good stays.
                let _ = std::fs::remove_dir_all(&staging);
                return Ok(RemoteFetchOutcome::Unchanged { revision: hash });
            }
            (hash, false)
        }
        "url" => {
            let outcome = market_source::fetch_http_market(&normalized, &staging, current_revision)
                .await
                .map_err(|error| AppError::Internal(error))?;
            match outcome {
                market_source::HttpFetchOutcome::NotModified => {
                    let _ = std::fs::remove_dir_all(&staging);
                    return Ok(RemoteFetchOutcome::Unchanged {
                        revision: current_revision.unwrap_or("http").to_owned(),
                    });
                }
                market_source::HttpFetchOutcome::Fresh { revision, .. } => {
                    if current_revision == Some(revision.as_str()) {
                        // Same etag/last-modified marker: content unchanged;
                        // the freshly fetched body is discarded, last-good stays.
                        let _ = std::fs::remove_dir_all(&staging);
                        return Ok(RemoteFetchOutcome::Unchanged { revision });
                    }
                    (revision, true)
                }
            }
        }
        other => {
            return Err(AppError::BadRequest(format!(
                "unsupported remote source kind `{other}`"
            )));
        }
    };

    // Probe the staged content (URL markets have only marketplace.json; their
    // entries are inlined relative paths and must resolve inside the same
    // fetch — CodeBuddy document semantics).
    let entries = if manifest_only {
        probe_url_entries(&staging)?
    } else {
        let (kind, scanned) = crate::app_server_marketplace::probe_directory(&staging)?;
        scan_to_entries(scanned, kind.as_str())
    };

    // Atomic promotion; on failure the staging is cleaned and last-good stays.
    market_source::promote(&staging, &live_root).map_err(|error| {
        let _ = std::fs::remove_dir_all(&staging);
        AppError::Internal(error)
    })?;

    Ok(RemoteFetchOutcome::Fresh(FetchedEntrySet { live_root, revision, entries }))
}

/// Probe entries from an HTTP-manifest-only market: each declared entry must
/// be fully inlined (its source is a *relative* path that resolves inside the
/// same fetched directory tree, or a `data:`-style inline payload). Phase B
/// accepts relative paths only and validates them against the live tree.
fn probe_url_entries(staging: &Path) -> Result<Vec<(MarketplaceEntry, String)>, AppError> {
    let manifest_path = staging.join("marketplace.json");
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|error| AppError::Internal(format!("read manifest: {error}")))?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| AppError::Internal(format!("parse manifest: {error}")))?;
    let mut entries = Vec::new();
    for (key_list, key_kind) in [("skills", "skill"), ("connectors", "connector"), ("plugins", "plugin")] {
        let Some(items) = value.get(key_list).and_then(|value| value.as_array()) else {
            continue;
        };
        for (index, item) in items.iter().enumerate() {
            let name = item
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_owned();
            if name.is_empty() {
                continue;
            }
            let source = item
                .get("source")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .trim_start_matches("./")
                .to_owned();
            // URL markets: only inlined/relative entries are resolvable.
            // A `source` that is itself an absolute URL or git reference is
            // recorded but flagged via a marker source kind.
            let relative = if source.starts_with("http") || source.contains("github.com") {
                // Entries with a fetchable external source cannot be mirrored
                // by a URL marketplace; keep the entry discoverable but mark it
                // as `external` so the UI can explain the limitation.
                entries.push((
                    MarketplaceEntry {
                        name,
                        source_kind: "external".into(),
                        source_uri: source,
                        version: item.get("version").and_then(as_str).map(str::to_owned),
                        description: item.get("description").and_then(as_str).map(str::to_owned),
                        keywords: Vec::new(),
                        category: item.get("category").and_then(as_str).map(str::to_owned),
                    },
                    String::new(),
                ));
                continue;
            } else {
                source.clone()
            };
            if !staging.join(&relative).exists() && key_kind != "plugin" {
                // Declared but missing on the live tree (directory not
                // mirrored) — keep discoverable with an empty source marker.
                entries.push((
                    MarketplaceEntry {
                        name,
                        source_kind: "external".into(),
                        source_uri: relative,
                        version: None,
                        description: item.get("description").and_then(as_str).map(str::to_owned),
                        keywords: Vec::new(),
                        category: None,
                    },
                    String::new(),
                ));
                continue;
            }
            let _ = index;
            entries.push((
                MarketplaceEntry {
                    name,
                    source_kind: "directory".into(),
                    source_uri: relative.clone(),
                    version: item.get("version").and_then(as_str).map(str::to_owned),
                    description: item.get("description").and_then(as_str).map(str::to_owned),
                    keywords: Vec::new(),
                    category: item.get("category").and_then(as_str).map(str::to_owned),
                },
                relative,
            ));
        }
    }
    Ok(entries)
}

fn as_str(value: &serde_json::Value) -> Option<&str> {
    value.as_str()
}

/// Convert probed directory entries into marketplace entries (relative
/// sources inside the live tree), plus the (entry, relative) pair.
fn scan_to_entries(
    scanned: Vec<crate::app_server_marketplace::ScannedEntry>,
    _kind: &str,
) -> Vec<(MarketplaceEntry, String)> {
    scanned
        .into_iter()
        .map(|entry| {
            let relative = entry.relative.clone();
            (
                MarketplaceEntry {
                    name: entry.name,
                    source_kind: "directory".into(),
                    source_uri: relative.clone(),
                    version: None,
                    description: entry.description,
                    keywords: entry.keywords,
                    category: entry.category,
                },
                relative,
            )
        })
        .collect()
}

/// The live root for a marketplace (when materialized), `None` for directory
/// sources (which read their source in place).
pub fn live_root_for(root: &Path, marketplace_id: &str) -> PathBuf {
    root.join(marketplace_id).join("live")
}

/// Resolve an entry's source path inside the marketplace tree. Directory
/// sources use the source directory directly; remote sources resolve inside
/// the live root.
pub fn entry_source_path(
    source_kind: &str,
    row_source: &str,
    marketplace_root: &Path,
    marketplace_id: &str,
    entry_relative: &str,
) -> PathBuf {
    if source_kind == "directory" {
        let root = PathBuf::from(row_source);
        if entry_relative == "." {
            root
        } else {
            root.join(entry_relative)
        }
    } else {
        let live = live_root_for(marketplace_root, marketplace_id);
        if entry_relative.is_empty() {
            live
        } else {
            live.join(entry_relative)
        }
    }
}

/// Register a marketplace row for a remote source after a successful fetch.
pub async fn register_remote(
    markets: Arc<dyn IMarketplaceRepository>,
    marketplace_id: &str,
    name: &str,
    source_kind: &str,
    source: &str,
    fetched: &FetchedEntrySet,
) -> Result<(), AppError> {
    let entries: Vec<MarketplaceEntry> = fetched.entries.iter().map(|(entry, _)| entry.clone()).collect();
    markets
        .insert_marketplace(nomifun_db::NewPluginMarketplace {
            marketplace_id,
            name,
            description: None,
            source_kind,
            source_uri: source,
            owner_json: None,
            version: None,
            content_digest: None,
            entries,
            auto_update: false,
        })
        .await
        .map_err(AppError::from)?;
    markets
        .record_resolved_revision(
            marketplace_id,
            &fetched.revision,
            fetched.live_root.to_str().unwrap_or(""),
        )
        .await
        .map_err(AppError::from)?;
    Ok(())
}
