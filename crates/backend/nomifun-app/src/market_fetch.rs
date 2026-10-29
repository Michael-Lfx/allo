//! Marketplace fetch coordination (roadmap Phase 2, phase B).
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

use nomifun_common::{AppError, collect_localized_variants};
use nomifun_db::{IMarketplaceRepository, MarketplaceEntry};
use serde_json::Value;

use crate::market_source;

/// One resolved + validated marketplace root after `fetch_remote`.
pub struct FetchedEntrySet {
    /// The live root (already promoted; immutable for this cycle).
    pub live_root: PathBuf,
    /// Resolved revision (git commit / freshness marker).
    pub revision: String,
    /// The HTTP source's raw conditional-request validators, when it sent any
    /// (doc 18 D4 ①). `None` for git sources.
    pub validators: Option<market_source::HttpValidators>,
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
/// `source_kind` is `github` | `git` | `url` | `zip`; directory sources are
/// handled by the caller before this point. When `current_revision` is
/// provided, an unchanged server revision short-circuits to `Unchanged` (git:
/// same HEAD commit; http: `304 Not Modified`; zip: same archive digest).
pub async fn fetch_remote(
    source_kind: &str,
    source: &str,
    marketplace_root: PathBuf,
    current_revision: Option<&str>,
    // The raw validators persisted from the previous fetch (doc 18 D4 ①).
    // `None` on a first fetch or for a source that never sent any.
    current_validators: Option<&market_source::HttpValidators>,
) -> Result<RemoteFetchOutcome, AppError> {
    let normalized = market_source::normalize_source_url(source_kind, source)
        .map_err(AppError::BadRequest)?;
    std::fs::create_dir_all(&marketplace_root)
        .map_err(|error| AppError::Internal(format!("create market root: {error}")))?;

    let staging = marketplace_root.join(format!("staging-{}", nomifun_common::generate_id()));
    let live_root = marketplace_root.join("live");
    // Staging is owned by this fetch regardless of how it ends: a normal
    // completion promotes it (and the guard disarms), an early return or an
    // outer `tokio::time::timeout` drop removes it. Without the guard a
    // timeout strand leaves staging-* dirs accumulating under
    // `{market_root}/{marketplace_id}/` (observed in the wild).
    let mut staging_guard = StagingGuard::new(staging.clone());

    // Fetch into staging; validate the tree looks like a market after the
    // fetch (git clones the whole repo; http downloads the manifest only; zip
    // downloads one archive and unpacks it).
    let (revision, manifest_only, validators) = match source_kind {
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
            (hash, false, None)
        }
        "url" => {
            // Real conditional request (doc 18 D4 ①): the raw validators, so a
            // compliant server answers 304 instead of shipping the body.
            let outcome = market_source::fetch_http_market(
                &normalized,
                &staging,
                current_validators,
            )
            .await
            .map_err(|error| AppError::Internal(error))?;
            match outcome {
                market_source::HttpFetchOutcome::NotModified => {
                    let _ = std::fs::remove_dir_all(&staging);
                    return Ok(RemoteFetchOutcome::Unchanged {
                        revision: current_revision.unwrap_or("http").to_owned(),
                    });
                }
                market_source::HttpFetchOutcome::Fresh {
                    revision,
                    validators,
                } => {
                    if current_revision == Some(revision.as_str()) {
                        // Same etag/last-modified marker: content unchanged;
                        // the freshly fetched body is discarded, last-good stays.
                        // The marker is derived from the validators, so an equal
                        // marker means an equal validator set — nothing to update.
                        let _ = std::fs::remove_dir_all(&staging);
                        return Ok(RemoteFetchOutcome::Unchanged { revision });
                    }
                    // Full-tree URL market: when the source exposes a file
                    // listing (`/_files.txt`), mirror every listed asset over
                    // HTTP so relative entry sources resolve inside the live
                    // tree (unlike a manifest-only market, whose entries are
                    // mirror-impossible and stay `external`).
                    let has_listing = market_source::has_file_listing(&normalized).await;
                    if has_listing {
                        market_source::mirror_http_tree(&normalized, &staging).await
                            .map_err(|error| AppError::Internal(error))?;
                    }
                    (revision, !has_listing, Some(validators))
                }
            }
        }
        // Doc 30: one archive whose root is the market root. Replaces the
        // `url` full-tree mirror for the official bundles, where `experts` cost
        // 14,714 requests / 611 MiB per first fetch.
        "zip" => {
            // Freshness first: when the host reports the archive digest (a
            // `HEAD`, no body), an unchanged archive short-circuits before a
            // multi-hundred-MiB transfer. `None` means "not told" — never
            // "unchanged"; the download below is the fallback.
            let probed = market_source::probe_archive_digest(&normalized).await;
            if let (Some(probed), Some(current)) = (probed.as_deref(), current_revision) {
                if probed == current {
                    return Ok(RemoteFetchOutcome::Unchanged {
                        revision: probed.to_owned(),
                    });
                }
            }
            // The archive is a sibling of staging, never inside it: `promote`
            // renames staging into the live root, so an archive placed inside
            // would be promoted as market content (289 MiB of it).
            let archive =
                marketplace_root.join(format!("download-{}.zip", nomifun_common::generate_id()));
            let mut archive_guard = ArchiveGuard::new(archive.clone());
            let (local_digest, server_digest) =
                market_source::download_archive(&normalized, &archive)
                    .await
                    .map_err(AppError::Internal)?;
            // The digest doubles as a free integrity check: when the host
            // advertised one, the bytes we received must hash to it.
            if let Some(server) = server_digest.as_deref() {
                if server != local_digest {
                    return Err(AppError::Internal(format!(
                        "archive digest mismatch for {source}: host reports {server}, \
                         downloaded bytes hash to {local_digest}"
                    )));
                }
            }
            if current_revision == Some(local_digest.as_str()) {
                return Ok(RemoteFetchOutcome::Unchanged {
                    revision: local_digest,
                });
            }
            std::fs::create_dir_all(&staging)
                .map_err(|error| AppError::Internal(format!("create staging: {error}")))?;
            market_source::extract_zip_market(&archive, &staging)
                .map_err(AppError::Internal)?;
            archive_guard.remove();
            if !market_source::looks_like_market(&staging) {
                let _ = std::fs::remove_dir_all(&staging);
                return Err(AppError::BadRequest(format!(
                    "source {source} does not look like a marketplace (no manifest in the archive)"
                )));
            }
            (local_digest, false, None)
        }
        // Doc 30 §6.1: index/content separation. The market root is just
        // `index.json` (+ optional `categories.json`); entry payloads are
        // fetched on demand by `import_entry`, never here.
        "index" => {
            let text = market_source::fetch_text(&normalized)
                .await
                .map_err(AppError::Internal)?;
            validate_index_json(&text)?;
            let revision = market_source::sha256_hex_bytes(text.as_bytes());
            if current_revision == Some(revision.as_str()) {
                let _ = std::fs::remove_dir_all(&staging);
                return Ok(RemoteFetchOutcome::Unchanged { revision });
            }
            let value: serde_json::Value = serde_json::from_str(&text)
                .map_err(|error| AppError::Internal(format!("parse index: {error}")))?;
            // The categories dictionary is optional but, when declared, its
            // digest is mandatory to verify if the index carries one: the two
            // files must be the same batch the index was built from.
            let categories = match value.get("categories_url").and_then(as_str) {
                Some(url) => {
                    if !(url.starts_with("https://") || url.starts_with("http://")) {
                        return Err(AppError::BadRequest(format!(
                            "index categories_url must be an absolute http(s) URL, got `{url}`"
                        )));
                    }
                    let bytes = market_source::fetch_bytes(url)
                        .await
                        .map_err(AppError::Internal)?;
                    if let Some(expected) = value.get("categories_sha256").and_then(as_str) {
                        let actual = market_source::sha256_hex_bytes(&bytes);
                        if actual != expected {
                            return Err(AppError::Internal(format!(
                                "categories digest mismatch for {url}: index declares {expected}, \
                                 downloaded bytes hash to {actual}"
                            )));
                        }
                    }
                    Some(bytes)
                }
                None => None,
            };
            std::fs::create_dir_all(&staging)
                .map_err(|error| AppError::Internal(format!("create staging: {error}")))?;
            std::fs::write(staging.join("index.json"), &text)
                .map_err(|error| AppError::Internal(format!("write index.json: {error}")))?;
            if let Some(bytes) = categories {
                std::fs::write(staging.join("categories.json"), bytes)
                    .map_err(|error| AppError::Internal(format!("write categories.json: {error}")))?;
            }
            (revision, false, None)
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
    let entries = if source_kind == "index" {
        probe_index_entries(&staging)?
    } else if manifest_only {
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
    staging_guard.disarm();

    Ok(RemoteFetchOutcome::Fresh(FetchedEntrySet {
        live_root,
        revision,
        validators,
        entries,
    }))
}

/// RAII cleanup for a `fetch_remote` staging dir: removes it on Drop unless
/// disarmed (promotion succeeded). Covers early returns, `?` failures, and —
/// critically — an outer `tokio::time::timeout` cancelling the future, where
/// the normal error paths never run.
struct StagingGuard {
    path: PathBuf,
    armed: bool,
}

impl StagingGuard {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for StagingGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

/// RAII cleanup for a downloaded market archive.
///
/// The archive is a *sibling* of the staging dir, not a child, because
/// `promote` renames staging into the live root — an archive inside staging
/// would be promoted as market content. Being outside staging, it also falls
/// outside `StagingGuard`'s reach, so it gets its own guard: an early return, a
/// `?` failure, or an outer `tokio::time::timeout` cancellation all have to
/// reclaim it, or every failed fetch strands a multi-hundred-MiB file under the
/// market root.
struct ArchiveGuard {
    path: PathBuf,
    armed: bool,
}

impl ArchiveGuard {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    /// Delete now (the success path) and disarm.
    fn remove(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        self.armed = false;
    }
}

impl Drop for ArchiveGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
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
            // `strict` is a market-entry field (`02` §8), but the rule it
            // carries is about a **plugin** source shipping its own
            // `plugin.json` — a `skills[]` / `connectors[]` row that happens to
            // declare it must not be refused for it. The directory prober
            // pins `false` for those kinds for the same reason.
            let strict = key_kind == "plugin"
                && item.get("strict").and_then(|value| value.as_bool()).unwrap_or(false);
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
                        published_at: crate::app_server_marketplace::published_at_of(item),
                        localized: collect_localized_variants(item),
                        strict,
                        // `source_kind = "external"` already says why this row
                        // cannot be installed; no second reason needed.
                        blocked_reason: None,
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
                        published_at: crate::app_server_marketplace::published_at_of(item),
                        localized: collect_localized_variants(item),
                        strict,
                        blocked_reason: None,
                    },
                    String::new(),
                ));
                continue;
            }
            // A directory row is the only one whose `strict` claim can be
            // checked here: the source is in the fetched tree.
            let blocked_reason = crate::app_server_marketplace::strict_entry_block(
                strict,
                staging
                    .join(&relative)
                    .join(".codebuddy-plugin/plugin.json")
                    .is_file(),
            );
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
                    published_at: crate::app_server_marketplace::published_at_of(item),
                    localized: collect_localized_variants(item),
                    strict,
                    blocked_reason,
                },
                relative,
            ));
        }
    }
    Ok(entries)
}

/// Validate one `index.json` body (doc 30 §6.1 / schema_version 2).
///
/// Structural, not best-effort: an index that names an un-downloadable or
/// unverifiable entry is refused wholesale, because every consumer treats the
/// index as the market's complete truth.
pub(crate) fn validate_index_json(text: &str) -> Result<(), AppError> {
    fn bad(message: impl Into<String>) -> AppError {
        AppError::BadRequest(message.into())
    }
    fn is_lower_hex64(value: &str) -> bool {
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }

    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|error| bad(format!("index.json is not valid JSON: {error}")))?;
    if value.get("schema_version").and_then(Value::as_i64) != Some(2) {
        return Err(bad("index.json schema_version must be 2"));
    }
    if value
        .get("market")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|market| !market.is_empty())
        .is_none()
    {
        return Err(bad("index.json market must be a non-empty string"));
    }
    let entries = value
        .get("entries")
        .and_then(Value::as_array)
        .filter(|entries| !entries.is_empty())
        .ok_or_else(|| bad("index.json entries must be a non-empty array"))?;
    let mut seen = std::collections::HashSet::new();
    for (index, item) in entries.iter().enumerate() {
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("");
        if name.is_empty() {
            return Err(bad(format!("index.json entries[{index}].name is empty")));
        }
        if !seen.insert(name) {
            return Err(bad(format!("index.json has a duplicate entry name `{name}`")));
        }
        let download = item.get("download").and_then(Value::as_str).unwrap_or("");
        if !download.starts_with("https://") {
            return Err(bad(format!(
                "index.json entry `{name}` download must be an absolute https URL, got `{download}`"
            )));
        }
        let sha256 = item.get("sha256").and_then(Value::as_str).unwrap_or("");
        if !is_lower_hex64(sha256) {
            return Err(bad(format!(
                "index.json entry `{name}` sha256 must be 64 lowercase hex characters"
            )));
        }
        if item.get("size").and_then(Value::as_u64).unwrap_or(0) == 0 {
            return Err(bad(format!(
                "index.json entry `{name}` size must be a positive integer"
            )));
        }
        if let Some(available) = item.get("available") {
            if !available.is_boolean() {
                return Err(bad(format!(
                    "index.json entry `{name}` available must be a boolean"
                )));
            }
        }
    }
    if let Some(categories_sha256) = value.get("categories_sha256").and_then(Value::as_str) {
        if !is_lower_hex64(categories_sha256) {
            return Err(bad(
                "index.json categories_sha256 must be 64 lowercase hex characters",
            ));
        }
    }
    Ok(())
}

/// Map an index's `entries[]` to probe rows. Entry payloads stay remote: the
/// `source_uri` is the absolute `download` URL and `source_kind = "remote"`, the
/// marker `import_entry` uses to fetch + verify the archive on demand.
fn probe_index_entries(staging: &Path) -> Result<Vec<(MarketplaceEntry, String)>, AppError> {
    let text = std::fs::read_to_string(staging.join("index.json"))
        .map_err(|error| AppError::Internal(format!("read index.json: {error}")))?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| AppError::Internal(format!("parse index.json: {error}")))?;
    let items = value
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::Internal("index.json has no entries array".into()))?;
    let localized = |item: &serde_json::Value, key: &str| -> Option<String> {
        let value = item.get(key)?;
        ["zh", "en"]
            .into_iter()
            .filter_map(|lang| value.get(lang).and_then(Value::as_str))
            .map(str::trim)
            .find(|text| !text.is_empty())
            .map(str::to_owned)
    };
    let strings = |item: &serde_json::Value, key: &str| -> Vec<String> {
        item.get(key)
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut entries = Vec::new();
    for item in items {
        let name = item.get("name").and_then(Value::as_str).unwrap_or("").to_owned();
        let download = item.get("download").and_then(Value::as_str).unwrap_or("").to_owned();
        let available = item.get("available").and_then(Value::as_bool).unwrap_or(true);
        let blocked_reason = (!available).then(|| {
            item.get("unavailable_reason")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| "市场标记该条目不可用".to_owned())
        });
        let mut keywords = strings(item, "tags_zh");
        if keywords.is_empty() {
            keywords = strings(item, "tags_en");
        }
        let category = item
            .get("category_zh")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .or_else(|| item.get("category_en").and_then(Value::as_str).filter(|text| !text.is_empty()))
            .or_else(|| item.get("category_id").and_then(Value::as_str).filter(|text| !text.is_empty()))
            .map(str::to_owned);
        entries.push((
            MarketplaceEntry {
                name,
                source_kind: "remote".into(),
                source_uri: download,
                version: item.get("version").and_then(Value::as_str).map(str::to_owned),
                description: localized(item, "description"),
                keywords,
                category,
                published_at: None,
                localized: std::collections::BTreeMap::new(),
                strict: false,
                blocked_reason,
            },
            String::new(),
        ));
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
                    published_at: entry.published_at,
                    localized: entry.localized,
                    strict: entry.strict,
                    blocked_reason: entry.blocked_reason,
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
/// A previously soft-removed row (same `marketplace_id`) is reactivated
/// instead of inserted (the id column is unique).
pub async fn register_remote(
    markets: Arc<dyn IMarketplaceRepository>,
    marketplace_id: &str,
    name: &str,
    source_kind: &str,
    source: &str,
    fetched: &FetchedEntrySet,
) -> Result<(), AppError> {
    let entries: Vec<MarketplaceEntry> = fetched.entries.iter().map(|(entry, _)| entry.clone()).collect();
    let existing = markets
        .get_marketplace(marketplace_id)
        .await
        .map_err(AppError::from)?;
    // Any existing row is reactivated with the current source — a re-source
    // (same id, new source, e.g. default sources re-pointed in config.toml)
    // must update the row instead of tripping the unique id on insert.
    if let Some(_) = existing {
        markets
            .reactivate_marketplace(
                marketplace_id,
                source_kind,
                source,
                &entries,
                &fetched.revision,
                None,
                // A re-pointed row takes the *current* source's class default
                // (doc 18 D1): keeping the old flag would leave a market that
                // just moved to an official source out of the auto-update sweep.
                crate::app_server_marketplace::is_official_source(source_kind, source),
            )
            .await
            .map_err(AppError::from)?;
    } else {
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
                auto_update: crate::app_server_marketplace::is_official_source(source_kind, source),
            })
            .await
            .map_err(AppError::from)?;
    }
    markets
        .record_resolved_revision(
            marketplace_id,
            &fetched.revision,
            fetched.live_root.to_str().unwrap_or(""),
        )
        .await
        .map_err(AppError::from)?;
    // D4 ①: keep the validators so the next refresh can be conditional.
    if let Some(validators) = fetched.validators.as_ref() {
        markets
            .record_source_validators(
                marketplace_id,
                validators.etag.as_deref(),
                validators.last_modified.as_deref(),
            )
            .await
            .map_err(AppError::from)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, download: &str, sha256: &str, size: i64) -> serde_json::Value {
        serde_json::json!({
            "name": name,
            "kind": "agent",
            "download": download,
            "sha256": sha256,
            "size": size,
            "available": true
        })
    }

    fn document(entries: serde_json::Value) -> String {
        serde_json::json!({
            "schema_version": 2,
            "market": "experts",
            "market_kind": "plugin",
            "entries": entries
        })
        .to_string()
    }

    #[test]
    fn index_validation_accepts_a_well_formed_document() {
        let ok = document(serde_json::json!([
            entry("a", "https://h/e/a.tar.gz", &"a".repeat(64), 10)
        ]));
        assert!(validate_index_json(&ok).is_ok());
    }

    #[test]
    fn index_validation_rejects_a_bad_sha_relative_download_and_duplicates() {
        let bad_sha = document(serde_json::json!([
            entry("a", "https://h/e/a.tar.gz", "XYZ", 10)
        ]));
        assert!(validate_index_json(&bad_sha).is_err(), "a non-hex sha must be refused");

        let upper = document(serde_json::json!([
            entry("a", "https://h/e/a.tar.gz", &"A".repeat(64), 10)
        ]));
        assert!(validate_index_json(&upper).is_err(), "uppercase hex must be refused");

        let relative = document(serde_json::json!([
            entry("a", "entries/a/1.tar.gz", &"a".repeat(64), 10)
        ]));
        assert!(validate_index_json(&relative).is_err(), "a relative download must be refused");

        let duplicate = document(serde_json::json!([
            entry("a", "https://h/e/a.tar.gz", &"a".repeat(64), 10),
            entry("a", "https://h/e/a2.tar.gz", &"b".repeat(64), 10)
        ]));
        assert!(validate_index_json(&duplicate).is_err(), "duplicate names must be refused");

        let zero = document(serde_json::json!([
            entry("a", "https://h/e/a.tar.gz", &"a".repeat(64), 0)
        ]));
        assert!(validate_index_json(&zero).is_err(), "size 0 must be refused");
    }

    #[test]
    fn index_validation_rejects_the_wrong_schema_and_empty_entries() {
        let wrong = serde_json::json!({"schema_version": 1, "market": "x", "entries": []}).to_string();
        assert!(validate_index_json(&wrong).is_err());
        let empty = document(serde_json::json!([]));
        assert!(validate_index_json(&empty).is_err());
        let no_market = serde_json::json!({"schema_version": 2, "market": " ", "entries": [
            entry("a", "https://h/e/a.tar.gz", &"a".repeat(64), 10)
        ]}).to_string();
        assert!(validate_index_json(&no_market).is_err());
    }
}

#[cfg(test)]
mod index_fetch_tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn entry(name: &str) -> serde_json::Value {
        serde_json::json!({
            "name": name,
            "kind": "agent",
            "download": format!("https://cdn.example/{name}.tar.gz"),
            "sha256": "a".repeat(64),
            "size": 10,
            "available": true,
        })
    }

    #[tokio::test]
    async fn index_fetch_stages_index_and_categories_then_short_circuits() {
        let mock = MockServer::start().await;
        let categories = br#"{"schema_version":1,"market":"experts","taxonomy":[],"assignments":{},"counts":{}}"#;
        let categories_sha = market_source::sha256_hex_bytes(categories);
        let index = serde_json::json!({
            "schema_version": 2,
            "market": "experts",
            "market_kind": "plugin",
            "categories_url": format!("{}/categories.json", mock.uri()),
            "categories_sha256": categories_sha,
            "entries": [entry("a")],
        })
        .to_string();

        Mock::given(method("GET"))
            .and(path("/index.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(index))
            .mount(&mock)
            .await;
        Mock::given(method("GET"))
            .and(path("/categories.json"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(categories.to_vec()))
            .mount(&mock)
            .await;

        let root = tempfile::tempdir().unwrap();
        let source = format!("{}/index.json", mock.uri());
        let first = fetch_remote("index", &source, root.path().to_path_buf(), None, None)
            .await
            .expect("first index fetch");
        let (revision, entries) = match first {
            RemoteFetchOutcome::Fresh(fetched) => (fetched.revision, fetched.entries),
            RemoteFetchOutcome::Unchanged { .. } => panic!("first fetch must be fresh"),
        };
        assert_eq!(revision.len(), 64);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0.source_kind, "remote");
        assert_eq!(entries[0].0.source_uri, "https://cdn.example/a.tar.gz");
        let live = root.path().join("live");
        assert!(live.join("index.json").is_file());
        assert_eq!(std::fs::read(live.join("categories.json")).unwrap(), categories);

        let second = fetch_remote(
            "index",
            &source,
            root.path().to_path_buf(),
            Some(revision.as_str()),
            None,
        )
        .await
        .expect("second index fetch");
        match second {
            RemoteFetchOutcome::Unchanged { revision: same } => assert_eq!(same, revision),
            RemoteFetchOutcome::Fresh(_) => panic!("same index content must be Unchanged"),
        }
        assert!(live.join("index.json").is_file(), "last-good stays");
    }

    #[tokio::test]
    async fn index_fetch_rejects_a_bad_categories_digest() {
        let mock = MockServer::start().await;
        let categories = b"{}";
        let index = serde_json::json!({
            "schema_version": 2,
            "market": "experts",
            "categories_url": format!("{}/categories.json", mock.uri()),
            "categories_sha256": "b".repeat(64),
            "entries": [entry("a")],
        })
        .to_string();
        Mock::given(method("GET"))
            .and(path("/index.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(index))
            .mount(&mock)
            .await;
        Mock::given(method("GET"))
            .and(path("/categories.json"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(categories.to_vec()))
            .mount(&mock)
            .await;
        let root = tempfile::tempdir().unwrap();
        let result = fetch_remote(
            "index",
            &format!("{}/index.json", mock.uri()),
            root.path().to_path_buf(),
            None,
            None,
        )
        .await;
        assert!(result.is_err(), "a mismatched categories digest must fail the fetch");
        assert!(!root.path().join("live").exists(), "failed fetch must not promote");
    }

    /// Manual live smoke: the real published experts index must fetch, validate
    /// and promote. Ignored by default (network); run with
    /// `cargo test -p nomifun-app --no-default-features --lib live_published -- --ignored`.
    #[tokio::test]
    #[ignore = "network: fetch the published experts index.json from ModelScope"]
    async fn live_published_experts_index_fetches_and_promotes() {
        let url = std::env::var("WB_MARKET_INDEX_URL").unwrap_or_else(|_| {
            "https://www.modelscope.cn/models/me9rez/flowy-marketplace/resolve/master/wb-market/experts/index.json"
                .to_owned()
        });
        let root = tempfile::tempdir().unwrap();
        let outcome = fetch_remote("index", &url, root.path().to_path_buf(), None, None)
            .await
            .expect("published index must fetch");
        match outcome {
            RemoteFetchOutcome::Fresh(set) => {
                assert!(set.entries.len() >= 400, "entries: {}", set.entries.len());
                assert!(set.live_root.join("index.json").is_file());
                assert!(set.live_root.join("categories.json").is_file());
                assert!(
                    set.entries.iter().all(|(entry, _)| entry.source_kind == "remote"),
                    "index entries are remote downloads"
                );
                // End-to-end: pick the first entry from the promoted index,
                // download its archive, verify the declared digest/size and
                // extract it exactly like `import_entry` does.
                let text = std::fs::read_to_string(set.live_root.join("index.json")).unwrap();
                let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
                let row = &doc["entries"][0];
                let name = row["name"].as_str().unwrap();
                let url = row["download"].as_str().unwrap();
                let sha = row["sha256"].as_str().unwrap();
                let size = row["size"].as_u64().unwrap();
                let archive = root.path().join("entry.tar.gz");
                let (digest, server) =
                    market_source::download_archive(url, &archive).await.expect("entry download");
                assert_eq!(digest, sha, "downloaded digest matches the index");
                if let Some(server) = server {
                    assert_eq!(server, sha);
                }
                assert_eq!(std::fs::metadata(&archive).unwrap().len(), size);
                let extracted = root.path().join("entry");
                market_source::extract_tar_gz_market(&archive, &extracted).expect("safe extract");
                let nested = extracted.join(name).join(".codebuddy-plugin/plugin.json");
                assert!(
                    nested.is_file() || extracted.join(".codebuddy-plugin/plugin.json").is_file(),
                    "entry payload must carry plugin.json"
                );
            }
            RemoteFetchOutcome::Unchanged { .. } => panic!("first fetch cannot be Unchanged"),
        }
    }
}

#[cfg(test)]
mod small_file_tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn small_file_download_enforces_the_icon_cap() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ok.webp"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"small".to_vec()))
            .mount(&mock)
            .await;
        Mock::given(method("GET"))
            .and(path("/big.webp"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![0u8; 64]))
            .mount(&mock)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let ok = dir.path().join("ok.webp");
        market_source::download_small_file(&format!("{}/ok.webp", mock.uri()), &ok, 16)
            .await
            .expect("small asset");
        assert_eq!(std::fs::read(&ok).unwrap(), b"small");

        let big = dir.path().join("big.webp");
        assert!(
            market_source::download_small_file(&format!("{}/big.webp", mock.uri()), &big, 16)
                .await
                .is_err()
        );
        assert!(!big.exists(), "an over-cap asset must not be written");
    }
}
