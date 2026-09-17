//! Marketplace remote-source fetch layer (roadmap Phase 2, phase B).
//!
//! Implements the CodeBuddy marketplace fetch semantics in a local-first way:
//! - Git sources (GitHub `owner/repo`, any Git URL) are cloned into a staging
//!   directory, resolved to their HEAD commit, and then atomically promoted
//!   into the live root (backup + rename). An unchanged commit is a no-op.
//! - HTTP sources fetch `marketplace.json` into staging, validate the full
//!   manifest, and promote atomically; ETag/Last-Modified are used for
//!   freshness short-circuiting (documented CodeBuddy semantics).
//!
//! Failures never touch the last-good live root: promotion happens only after
//! the staged content passed full validation, and the backup is removed only
//! when the new live root is in place.

use std::fs;
use std::path::{Path, PathBuf};

use futures_util::future::join_all;
use futures_util::StreamExt;
use git2::Repository;
use reqwest::StatusCode;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

pub const MARKET_MANIFEST_A: &str = ".codebuddy-skill/marketplace.json";
pub const MARKET_MANIFEST_B: &str = ".codebuddy-connector/connectors.json";
pub const MARKET_MANIFEST_PLUGIN: &str = ".codebuddy-plugin/plugin.json";
/// Plugin-**market** manifest (`.codebuddy-plugin/marketplace.json`) — a market
/// root listing `plugins[]`. Distinct from [`MARKET_MANIFEST_PLUGIN`], which is
/// a single plugin's own manifest.
pub const MARKET_MANIFEST_PLUGIN_MARKET: &str = ".codebuddy-plugin/marketplace.json";

/// Total request budget for one `zip` market archive.
///
/// Deliberately *not* [`http_client`]'s 15s: that is a total-request timeout,
/// so it aborts every real archive (the official `experts` bundle is ~289 MiB).
/// Matches the managed-runtime precedent for large archives
/// (`nomi-config/src/runtime_dep_install/ffmpeg.rs`, 300s); raised because a
/// market archive is bigger than a toolchain build and is fetched on a user's
/// first store open.
pub const ARCHIVE_DOWNLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(900);

/// Compressed-size cap for one archive (both the declared `Content-Length` and
/// the bytes actually written). Far above any real market (the three official
/// bundles are 17-289 MiB) and far below "filled the user's disk".
pub const MAX_ARCHIVE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Uncompressed-size cap for one market archive extraction.
///
/// [`nomifun_common::zip_safe::ZipExtractionBudget`]'s default is 256 MiB,
/// which the official `experts` bundle already exceeds (611 MiB of JSON, CSV
/// and DuckDB payloads) — a zip source that kept the default would fail on
/// every real market.
pub const MAX_MARKET_ZIP_UNCOMPRESSED_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// Entry-count cap for one market archive. `experts` ships 14,714 entries.
pub const MAX_MARKET_ZIP_ENTRIES: usize = 200_000;


/// The fetched + validated materialization for one remote marketplace.
#[derive(Debug, Clone)]
pub struct FetchedMarket {
    /// Resolved revision: git HEAD commit hash, or a traceability marker for
    /// HTTP sources (ETag/Last-Modified or the file digest).
    pub revision: String,
    /// Absolute path of the *validated* content root (the staging dir).
    pub content_root: PathBuf,
}

/// Parse a marketplace source into a normalized fetchable URL.
///
/// - `github`: `owner/repo` → `https://github.com/{owner}/{repo}.git`
/// - `git`: any Git URL (`https://…/repo.git`, `git@host:org/repo.git`)
/// - `url`: HTTP(S) `marketplace.json` URL
///
/// Phase B rejects `directory` here (the caller handles it separately).
pub fn normalize_source_url(source_kind: &str, source: &str) -> Result<String, String> {
    match source_kind {
        "github" => {
            let parts: Vec<&str> = source.trim().trim_end_matches('/').split('/').collect();
            if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
                return Ok(format!("https://github.com/{}/{}.git", parts[0], parts[1]));
            }
            Err(format!("expected `owner/repo` for a GitHub source, got `{source}`"))
        }
        "git" => {
            if source.starts_with("https://")
                || source.starts_with("http://")
                || source.starts_with("git@")
                || source.starts_with("ssh://")
                || source.starts_with("git://")
            {
                return Ok(source.to_owned());
            }
            // Local filesystem git paths are legitimate for tests/dev: a
            // `*.git` suffix or an existing repository directory both work
            // (git2 requires a path git2 can open).
            let path = std::path::Path::new(source);
            if source.ends_with(".git") || path.is_dir() {
                return Ok(source.to_owned());
            }
            Err(format!("expected a Git URL or local git path, got `{source}`"))
        }
        "url" => {
            if source.starts_with("https://") || source.starts_with("http://") {
                return Ok(source.to_owned());
            }
            Err(format!("expected an HTTP(S) URL, got `{source}`"))
        }
        // Doc 30: an archive whose root is the market root. Same URL shape as
        // `url` — the kind, not the suffix, decides how it is fetched, so a
        // signed/extension-less archive URL keeps working.
        "zip" => {
            if source.starts_with("https://") || source.starts_with("http://") {
                return Ok(source.to_owned());
            }
            Err(format!("expected an HTTP(S) URL to a market archive, got `{source}`"))
        }
        other => Err(format!("unsupported remote source kind `{other}`")),
    }
}

/// Directories that make a checkout look like a valid marketplace catalog.
///
/// Must stay in sync with the discovery order `probe_directory` implements
/// (doc 18 §3). `.codebuddy-plugin/marketplace.json` was missing here while
/// `probe_directory` has always accepted it — and that is *the* layout the
/// official `experts` market uses (`.codebuddy-plugin/marketplace.json` at the
/// root, nothing else), so a `github`/`git` checkout or a `zip` archive of that
/// market was refused as "not a marketplace" even though every entry in it is
/// perfectly importable.
pub fn looks_like_market(root: &Path) -> bool {
    root.join(MARKET_MANIFEST_A).is_file()
        || root.join(MARKET_MANIFEST_B).is_file()
        || root.join(MARKET_MANIFEST_PLUGIN).is_file()
        || root.join(MARKET_MANIFEST_PLUGIN_MARKET).is_file()
        || root.join("marketplace.json").is_file()
        || root.join("cli.json").is_file()
}

/// Derive the market root base URL from a manifest URL:
/// - `http://h/.codebuddy-plugin/marketplace.json` → `http://h` (preferred;
///   must be checked before the generic root suffix, which would otherwise
///   cut into the `.codebuddy-plugin/` segment)
/// - `http://h/.codebuddy-skill/marketplace.json` → `http://h`
/// - `http://h/.codebuddy-connector/connectors.json` → `http://h`
/// - `http://h/marketplace.json` → `http://h`
fn manifest_base_url(manifest_url: &str) -> Option<String> {
    let trimmed = manifest_url.trim_end_matches('/');
    let base = trimmed
        .strip_suffix("/.codebuddy-plugin/marketplace.json")
        .or_else(|| trimmed.strip_suffix("/.codebuddy-skill/marketplace.json"))
        .or_else(|| trimmed.strip_suffix("/.codebuddy-connector/connectors.json"))
        .or_else(|| trimmed.strip_suffix("/marketplace.json"))?;
    (!base.is_empty()).then(|| base.to_owned())
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("flowy-agent-store/1.0")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|error| format!("build http client: {error}"))
}

/// A listing is exposed at `{base}/_files.txt` with one relative path per
/// line; when present the URL market is a full-tree mirror (not manifest-only).
pub async fn has_file_listing(manifest_url: &str) -> bool {
    let Some(base) = manifest_base_url(manifest_url) else {
        return false;
    };
    let url = format!("{base}/_files.txt");
    let Ok(client) = http_client() else { return false };
    match client.get(&url).send().await {
        Ok(response) => response.status() == StatusCode::OK,
        Err(_) => false,
    }
}

/// Mirror every file listed in `{base}/_files.txt` into `staging`, preserving
/// relative paths. The listing itself is skipped; `..` traversal or absolute
/// entries are rejected (the listing is untrusted input).
pub async fn mirror_http_tree(manifest_url: &str, staging: &Path) -> Result<(), String> {
    let Some(base) = manifest_base_url(manifest_url) else {
        return Err(format!("cannot derive base URL from {manifest_url}"));
    };
    let client = http_client()?;
    let listing_url = format!("{base}/_files.txt");
    let response = client
        .get(&listing_url)
        .send()
        .await
        .map_err(|error| format!("fetch {listing_url}: {error}"))?;
    if response.status() != StatusCode::OK {
        return Err(format!("fetch {listing_url}: HTTP {}", response.status()));
    }
    let text = response
        .text()
        .await
        .map_err(|error| format!("read {listing_url}: {error}"))?;
    let files: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && *line != "_files.txt")
        // Untrusted listing: reject absolute / traversal paths.
        .filter(|relative| {
            !relative.starts_with('/') && !relative.contains("..") && !relative.contains('\\')
        })
        .map(str::to_owned)
        .collect();

    // Download concurrently in small batches: user markets routinely carry
    // hundreds of small assets (avatars, prompts), and serial GETs would make
    // a refresh take tens of seconds over a local server. 32 keeps a public
    // mirror saturated without overwhelming a modest VPS; the fixed 15s
    // client timeout keeps one slow asset from stalling a batch.
    const BATCH: usize = 32;
    for chunk in files.chunks(BATCH) {
        let futures = chunk.iter().map(|relative| {
            let client = client.clone();
            let base = base.clone();
            async move {
                let url = format!("{base}/{relative}");
                let file_response = client
                    .get(&url)
                    .send()
                    .await
                    .map_err(|error| format!("fetch {url}: {error}"))?;
                if file_response.status() != StatusCode::OK {
                    return Err(format!("fetch {url}: HTTP {}", file_response.status()));
                }
                let bytes = file_response
                    .bytes()
                    .await
                    .map_err(|error| format!("read {url}: {error}"))?;
                Ok::<_, String>((relative.clone(), bytes.to_vec()))
            }
        });
        let results = join_all(futures).await;
        for result in results {
            let (relative, bytes) = result?;
            let target = staging.join(&relative);
            let Some(parent) = target.parent() else { continue };
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("create {}: {error}", parent.display()))?;
            std::fs::write(&target, &bytes)
                .map_err(|error| format!("write {}: {error}", target.display()))?;
        }
    }
    Ok(())
}

/// Client for archive transfers: same UA as the manifest client (doc 18 §5.2),
/// but with a budget a multi-hundred-MiB body can actually finish inside.
fn archive_http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("flowy-agent-store/1.0")
        .timeout(ARCHIVE_DOWNLOAD_TIMEOUT)
        .build()
        .map_err(|error| format!("build http client: {error}"))
}

/// A server-supplied content digest for an archive, when it sent one.
///
/// ModelScope answers the stable download URL with the archive's **sha256** in
/// `X-Linked-Etag` (verified against a locally computed digest), and answers
/// `HEAD` with it directly — no redirect, no body. That makes it both the
/// freshness marker and a free integrity check for the downloaded archive.
///
/// Only a 64-hex value is accepted: the header is not a standard contract, and
/// a host that puts something else there must not be able to poison the
/// revision marker.
fn linked_etag_digest(headers: &reqwest::header::HeaderMap) -> Option<String> {
    let value = headers.get("x-linked-etag")?.to_str().ok()?.trim().trim_matches('"');
    let lowered = value.to_ascii_lowercase();
    (lowered.len() == 64 && lowered.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then_some(lowered)
}

/// Cheap freshness probe for a `zip` source: the digest the server reports for
/// the archive, without transferring it.
///
/// `None` means "the server did not tell us" — the caller must then fall back
/// to downloading and hashing locally, never to "assume unchanged".
///
/// Conditional requests are deliberately unused: the origin ignores
/// `If-None-Match` for non-LFS files, and for LFS files a `304` only exists
/// after following the cross-origin redirect, which is a much narrower
/// contract than a plain `HEAD`.
pub async fn probe_archive_digest(url: &str) -> Option<String> {
    let client = http_client().ok()?;
    let response = client.head(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    linked_etag_digest(response.headers())
}

/// Stream `url` into `dest`, returning `(local sha256, server digest)`.
///
/// Streamed rather than buffered: the official bundles are 17-289 MiB, and
/// hashing while writing means the archive never has to be read twice.
pub async fn download_archive(
    url: &str,
    dest: &Path,
) -> Result<(String, Option<String>), String> {
    let client = archive_http_client()?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("fetch {url}: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("fetch {url}: HTTP {status}"));
    }
    let server_digest = linked_etag_digest(response.headers());
    if let Some(declared) = response.content_length() {
        if declared > MAX_ARCHIVE_BYTES {
            return Err(format!(
                "archive {url} declares {declared} bytes, over the {MAX_ARCHIVE_BYTES}-byte cap"
            ));
        }
    }
    let Some(parent) = dest.parent() else {
        return Err(format!("archive destination {} has no parent", dest.display()));
    };
    fs::create_dir_all(parent).map_err(|error| format!("create {}: {error}", parent.display()))?;

    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|error| format!("create {}: {error}", dest.display()))?;
    let mut hasher = Sha256::new();
    let mut written: u64 = 0;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("read {url}: {error}"))?;
        written = written.saturating_add(chunk.len() as u64);
        // Enforced on bytes actually received, not on the declared length: a
        // chunked or lying response must not be able to fill the disk.
        if written > MAX_ARCHIVE_BYTES {
            drop(file);
            let _ = fs::remove_file(dest);
            return Err(format!(
                "archive {url} passed the {MAX_ARCHIVE_BYTES}-byte cap while downloading"
            ));
        }
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|error| format!("write {}: {error}", dest.display()))?;
    }
    file.flush()
        .await
        .map_err(|error| format!("flush {}: {error}", dest.display()))?;
    drop(file);
    Ok((hex::encode(hasher.finalize()), server_digest))
}

/// Extract a market archive into `staging`, returning how many files were
/// written.
///
/// Safety comes from [`nomifun_common::zip_safe`]: zip-slip names, symlink
/// entries and drive-prefixed names are rejected, and a decompression-bomb
/// budget caps both entry count and bytes *actually written*. The policies the
/// callers own are chosen here: [`ZipColonPolicy::RejectDrivePrefix`] (market
/// trees embed real upstream file names, so a non-prefix colon is a legal Unix
/// name — but it is an alternate-data-stream reference on Windows), and
/// last-entry-wins for a duplicate name.
pub fn extract_zip_market(archive_path: &Path, staging: &Path) -> Result<usize, String> {
    use nomifun_common::zip_safe::ZipExtractionBudget;

    extract_zip_market_with_budget(
        archive_path,
        staging,
        ZipExtractionBudget::new(MAX_MARKET_ZIP_UNCOMPRESSED_BYTES, MAX_MARKET_ZIP_ENTRIES),
    )
}

/// [`extract_zip_market`] with an injectable budget.
///
/// The caps are a parameter for the same reason
/// [`nomifun_common::zip_safe::ZipExtractionBudget::new`] accepts them: the
/// guards must be testable without a multi-hundred-MiB fixture.
fn extract_zip_market_with_budget(
    archive_path: &Path,
    staging: &Path,
    mut budget: nomifun_common::zip_safe::ZipExtractionBudget,
) -> Result<usize, String> {
    use nomifun_common::zip_safe::{
        safe_zip_entry_path, zip_entry_is_symlink, ZipColonPolicy,
    };

    let file = fs::File::open(archive_path)
        .map_err(|error| format!("open {}: {error}", archive_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| format!("read archive {}: {error}", archive_path.display()))?;
    budget
        .check_entry_count(archive.len())
        .map_err(|error| error.to_string())?;

    let mut written = 0usize;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("read archive entry {index}: {error}"))?;
        if entry.is_dir() {
            continue;
        }
        if zip_entry_is_symlink(entry.unix_mode()) {
            return Err(format!(
                "archive entry `{}` is a symlink; refusing to extract",
                entry.name()
            ));
        }
        let Some(relative) = safe_zip_entry_path(entry.name(), ZipColonPolicy::RejectDrivePrefix)
        else {
            return Err(format!(
                "archive entry `{}` is not a safe relative path",
                entry.name()
            ));
        };
        let target = staging.join(&relative);
        let Some(parent) = target.parent() else {
            continue;
        };
        fs::create_dir_all(parent).map_err(|error| format!("create {}: {error}", parent.display()))?;
        let mut out = fs::File::create(&target)
            .map_err(|error| format!("create {}: {error}", target.display()))?;
        // `io::copy`'s byte count, never the entry's self-declared size.
        let bytes = std::io::copy(&mut entry, &mut out)
            .map_err(|error| format!("extract {}: {error}", target.display()))?;
        budget
            .record_written(bytes)
            .map_err(|error| error.to_string())?;
        written += 1;
    }
    Ok(written)
}

/// Shallow-clone `url` into `staging` (a fresh directory) and return the raw
/// HEAD commit hash. The clone is a *single* fetch — we never re-clone after
/// promotion; a refresh reuses this same path with a new staging dir.
pub fn clone_git(url: &str, staging: &Path) -> Result<(String, Repository), String> {
    fs::create_dir_all(staging).map_err(|error| format!("create staging {}: {error}", staging.display()))?;
    let repo = Repository::clone(url, staging).map_err(|error| format!("git clone {url}: {error}"))?;
    // Compute the HEAD commit hash while the repo is alive; return the owned
    // hash plus the repo so the caller can keep it open for later reads.
    let hash = {
        let head = repo.head().map_err(|error| format!("resolve HEAD: {error}"))?;
        let commit = head.peel_to_commit().map_err(|error| format!("peel commit: {error}"))?;
        commit.id().to_string()
    };
    Ok((hash, repo))
}

/// The HTTP source's conditional-request validators, exactly as the server
/// sent them.
///
/// Kept **raw** on purpose (doc 18 D4 ①): the earlier code stored only their
/// digest and then sent that digest as `If-None-Match`, which no server can
/// match against its own ETag — so the 304 branch was unreachable in practice
/// and every refresh re-downloaded the manifest just to compare digests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HttpValidators {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

impl HttpValidators {
    /// Rebuild validators from the values persisted on the marketplace row.
    pub fn from_stored(etag: Option<&str>, last_modified: Option<&str>) -> Option<Self> {
        if etag.is_none() && last_modified.is_none() {
            return None;
        }
        Some(Self {
            etag: etag.map(str::to_owned),
            last_modified: last_modified.map(str::to_owned),
        })
    }

    /// The `resolved_revision` marker for these validators.
    ///
    /// The rule is deliberately unchanged (ETag preferred, `Last-Modified` as
    /// the fallback, `"http"` when the server sends neither) so the same
    /// response keeps producing the same marker — switching to raw validators
    /// must not make every URL market look "changed" exactly once.
    pub fn marker(&self) -> String {
        self.etag
            .as_deref()
            .or(self.last_modified.as_deref())
            .map(|value| nomifun_importer::digest::sha256_hex(value.as_bytes()))
            .unwrap_or_else(|| "http".to_owned())
    }
}

/// Outcome of downloading a `url` marketplace manifest into `staging`.
///
/// A `304 Not Modified` response maps to [`HttpFetchOutcome::NotModified`] so
/// the caller can short-circuit without re-reading the body (doc 18 §5.2).
pub enum HttpFetchOutcome {
    /// Fresh content was downloaded and validated into `staging`.
    Fresh {
        /// Revision marker (etag/last-modified digest, or `"http"`).
        revision: String,
        /// The validators to persist for the next conditional request.
        validators: HttpValidators,
    },
    /// Server answered `304 Not Modified` — caller keeps last-good.
    NotModified,
}

pub async fn fetch_http_market(
    url: &str,
    staging: &Path,
    validators: Option<&HttpValidators>,
) -> Result<HttpFetchOutcome, String> {
    // Same client contract as the listing probe / mirror (doc 18 §5.2 step 2:
    // UA + 15s). Building a second client here used to drop the timeout, so a
    // hung manifest server held the whole refresh open with no bound.
    let client = http_client()?;
    let mut request = client.get(url);
    // A real conditional request (doc 18 D4 ①): the raw values the server sent
    // last time, not their digest.
    if let Some(etag) = validators.and_then(|value| value.etag.as_deref()) {
        request = request.header("if-none-match", etag);
    }
    if let Some(last_modified) = validators.and_then(|value| value.last_modified.as_deref()) {
        request = request.header("if-modified-since", last_modified);
    }
    let response = request
        .send()
        .await
        .map_err(|error| format!("fetch {url}: {error}"))?;
    let status = response.status();
    if status == StatusCode::NOT_MODIFIED {
        return Ok(HttpFetchOutcome::NotModified);
    }
    if status != StatusCode::OK {
        return Err(format!("fetch {url}: HTTP {status}"));
    }
    // Read the headers before `text()` consumes the response.
    let sent = HttpValidators {
        etag: response
            .headers()
            .get("etag")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned),
        last_modified: response
            .headers()
            .get("last-modified")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned),
    };
    let text = response
        .text()
        .await
        .map_err(|error| format!("read body {url}: {error}"))?;
    validate_http_manifest(&text, url)?;
    fs::create_dir_all(staging).map_err(|error| format!("create staging {}: {error}", staging.display()))?;
    fs::write(staging.join("marketplace.json"), text)
        .map_err(|error| format!("write manifest: {error}"))?;
    let revision = sent.marker();
    Ok(HttpFetchOutcome::Fresh {
        revision,
        validators: sent,
    })
}

fn validate_http_manifest(text: &str, url: &str) -> Result<(), String> {
    let value: Value = serde_json::from_str(text).map_err(|error| format!("parse {url}: {error}"))?;
    let has_container = value.get("name").and_then(Value::as_str).is_some();
    let has_entries = value.get("skills").is_some()
        || value.get("connectors").is_some()
        || value.get("plugins").is_some();
    if !has_container || !has_entries {
        return Err(format!(
            "{url} is not a marketplace manifest: requires `name` plus `skills`/`connectors`/`plugins`"
        ));
    }
    Ok(())
}

/// Atomically promote a fully validated `staging` dir into `live_root`:
/// rename the existing live root to a backup first, then rename staging into
/// place; any failure restores the backup. Returns the backup path (already
/// removed on success).
pub fn promote(staging: &Path, live_root: &Path) -> Result<(), String> {
    let backup = live_root.with_extension(format!("backup-{}", nomifun_common::now_ms()));
    if live_root.exists() {
        fs::rename(live_root, &backup)
            .map_err(|error| format!("backup live root {}: {error}", live_root.display()))?;
    }
    fs::rename(staging, live_root).map_err(|error| {
        // Restore the previous live root so last-good survives failed promotion.
        if backup.exists() {
            let _ = fs::rename(&backup, live_root);
        }
        format!("promote staging -> {}: {error}", live_root.display())
    })?;
    if backup.exists() {
        let _ = fs::remove_dir_all(&backup);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// D4 ① must not change the marker rule: identical response headers have to
    /// keep producing the same `resolved_revision`, otherwise switching to raw
    /// validators would make every URL market look "changed" exactly once.
    #[test]
    fn http_validators_keep_the_pre_existing_marker_rule() {
        let etag_only = HttpValidators {
            etag: Some("\"v1\"".into()),
            last_modified: None,
        };
        assert_eq!(etag_only.marker(), nomifun_importer::digest::sha256_hex(b"\"v1\""));

        // ETag wins over Last-Modified, exactly as before.
        let both = HttpValidators {
            etag: Some("\"v1\"".into()),
            last_modified: Some("Wed, 21 Oct 2015 07:28:00 GMT".into()),
        };
        assert_eq!(both.marker(), etag_only.marker());

        // Last-Modified is the fallback.
        let last_modified_only = HttpValidators {
            etag: None,
            last_modified: Some("Wed, 21 Oct 2015 07:28:00 GMT".into()),
        };
        assert_eq!(
            last_modified_only.marker(),
            nomifun_importer::digest::sha256_hex(b"Wed, 21 Oct 2015 07:28:00 GMT")
        );

        // Neither header → the literal fallback, unchanged.
        assert_eq!(HttpValidators::default().marker(), "http");
    }

    #[test]
    fn stored_validators_rebuild_from_the_row_columns() {
        assert!(HttpValidators::from_stored(None, None).is_none());
        let restored = HttpValidators::from_stored(Some("\"abc\""), None).expect("an etag is enough");
        assert_eq!(restored.etag.as_deref(), Some("\"abc\""));
        assert_eq!(restored.last_modified, None);
    }

    #[test]
    fn normalize_github_and_git_and_url() {
        assert_eq!(
            normalize_source_url("github", "Tencent/codebuddy-plugins").unwrap(),
            "https://github.com/Tencent/codebuddy-plugins.git"
        );
        assert_eq!(
            normalize_source_url("git", "https://gitlab.com/company/plugins.git").unwrap(),
            "https://gitlab.com/company/plugins.git"
        );
        assert_eq!(
            normalize_source_url("url", "https://example.com/marketplace.json").unwrap(),
            "https://example.com/marketplace.json"
        );
        assert!(normalize_source_url("github", "not-a-repo").is_err());
        assert!(normalize_source_url("git", "relative/path").is_err());
        assert!(normalize_source_url("url", "/local/file.json").is_err());
    }

    #[test]
    fn validate_http_manifest_accepts_and_rejects() {
        let ok = r#"{"name":"market","skills":[{"name":"a","source":"./skills/a"}]}"#;
        assert!(validate_http_manifest(ok, "https://x/marketplace.json").is_ok());
        let bad = r#"{"name":"market"}"#;
        assert!(validate_http_manifest(bad, "https://x/marketplace.json").is_err());
        let not_json = "hello";
        assert!(validate_http_manifest(not_json, "https://x/marketplace.json").is_err());
    }

    #[test]
    fn promote_preserves_last_good_on_new_content() {
        let temp = tempfile::tempdir().unwrap();
        let staging = temp.path().join("staging");
        let live = temp.path().join("live");
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("marketplace.json"), "v2").unwrap();
        fs::create_dir_all(&live).unwrap();
        fs::write(live.join("marketplace.json"), "v1").unwrap();

        promote(&staging, &live).unwrap();
        assert_eq!(fs::read_to_string(live.join("marketplace.json")).unwrap(), "v2");
    }

    #[test]
    fn promote_failure_restores_backup() {
        let temp = tempfile::tempdir().unwrap();
        let staging = temp.path().join("staging");
        let live = temp.path().join("live");
        fs::create_dir_all(&live).unwrap();
        fs::write(live.join("marketplace.json"), "v1").unwrap();
        // staging missing -> promote fails; live must still hold v1.
        let err = promote(&staging, &live).unwrap_err();
        assert!(err.contains("promote"), "{err}");
        assert_eq!(fs::read_to_string(live.join("marketplace.json")).unwrap(), "v1");
    }

    #[tokio::test]
    async fn http_fetch_roundtrip_via_local_server() {
        // A tiny in-process HTTP server serving a valid marketplace manifest.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let body = r#"{"name":"http-market","skills":[{"name":"a","source":"./skills/a"}]}"#;
        let server = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { break };
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = [0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\netag: \"abc-1\"\r\ncontent-length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });

        let temp = tempfile::tempdir().unwrap();
        let staging = temp.path().join("staging");
        let url = format!("http://{addr}/marketplace.json");
        let outcome = fetch_http_market(&url, &staging, None).await.unwrap();
        let HttpFetchOutcome::Fresh {
            revision,
            validators,
        } = outcome
        else {
            panic!("expected fresh content");
        };
        assert_eq!(validators.etag.as_deref(), Some("\"abc-1\""));
        let new_revision = nomifun_importer::digest::sha256_hex("\"abc-1\"".as_bytes());
        assert_eq!(revision, new_revision);
        assert!(staging.join("marketplace.json").is_file());

        server.abort();
    }

    #[tokio::test]
    async fn http_fetch_304_with_if_none_match_short_circuits() {
        // Use wiremock for deterministic conditional-request handling.
        let mock = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::header("if-none-match", "\"abc-1\""))
            .respond_with(wiremock::ResponseTemplate::new(304))
            .mount(&mock)
            .await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(
                r#"{"name":"http-market","skills":[]}"#,
            ))
            .mount(&mock)
            .await;

        let temp = tempfile::tempdir().unwrap();
        let url = format!("{}/marketplace.json", mock.uri());
        // The stored validator is offered as-is (doc 18 D4 ①): this is the raw
        // server ETag, not its digest — which is why the mock can match it.
        let stored = HttpValidators {
            etag: Some("\"abc-1\"".into()),
            last_modified: None,
        };
        let outcome = fetch_http_market(&url, &temp.path().join("staging"), Some(&stored))
            .await
            .unwrap();
        assert!(matches!(outcome, HttpFetchOutcome::NotModified));
    }

    #[tokio::test]
    async fn http_fetch_sends_if_modified_since_when_only_last_modified_is_known() {
        // D4 ① second half: `Last-Modified` used to feed the digest only and was
        // never sent as a conditional header, so this branch was unreachable.
        let mock = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(
                r#"{"name":"http-market","skills":[]}"#,
            ))
            .mount(&mock)
            .await;

        let temp = tempfile::tempdir().unwrap();
        let url = format!("{}/marketplace.json", mock.uri());
        let stored = HttpValidators {
            etag: None,
            last_modified: Some("Wed, 21 Oct 2015 07:28:00 GMT".into()),
        };
        fetch_http_market(&url, &temp.path().join("staging"), Some(&stored))
            .await
            .unwrap();

        // Assert on what actually went out: the stored `Last-Modified` must be
        // offered as a conditional header.
        let requests = mock
            .received_requests()
            .await
            .expect("the mock records received requests");
        let sent = requests
            .first()
            .expect("one request was made")
            .headers
            .get("if-modified-since")
            .expect("If-Modified-Since must be sent");
        assert_eq!(sent.to_str().unwrap(), "Wed, 21 Oct 2015 07:28:00 GMT");
    }

    #[tokio::test]
    async fn file_listing_and_tree_mirror_download_every_asset() {
        let mock = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::path("/_files.txt"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(
                ".codebuddy-plugin/marketplace.json\nplugins/demo/.codebuddy-plugin/plugin.json\nplugins/demo/agents/demo.md\n",
            ))
            .mount(&mock)
            .await;
        wiremock::Mock::given(wiremock::matchers::path("/.codebuddy-plugin/marketplace.json"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(
                r#"{"name":"tree-market","plugins":[{"name":"demo","source":"./plugins/demo"}]}"#,
            ))
            .mount(&mock)
            .await;
        wiremock::Mock::given(wiremock::matchers::path("/plugins/demo/.codebuddy-plugin/plugin.json"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(
                r#"{"name":"demo","displayName":"Demo","agents":["./agents"]}"#,
            ))
            .mount(&mock)
            .await;
        wiremock::Mock::given(wiremock::matchers::path("/plugins/demo/agents/demo.md"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string("---\nname: demo\n---\n"))
            .mount(&mock)
            .await;

        // The manifest URL uses the `.codebuddy-plugin/` sub-path, as the
        // locally served experts market does.
        let manifest_url = format!("{}/.codebuddy-plugin/marketplace.json", mock.uri());
        assert!(
            has_file_listing(&manifest_url).await,
            "listing must be detected"
        );

        let temp = tempfile::tempdir().unwrap();
        let staging = temp.path().join("staging");
        fs::create_dir_all(&staging).unwrap();
        mirror_http_tree(&manifest_url, &staging).await.unwrap();
        assert!(staging.join(".codebuddy-plugin/marketplace.json").is_file());
        assert!(staging.join("plugins/demo/.codebuddy-plugin/plugin.json").is_file());
        assert!(staging.join("plugins/demo/agents/demo.md").is_file());
    }

    #[test]
    #[cfg(unix)]
    fn git_clone_roundtrip_against_local_bare_repo() {
        let temp = tempfile::tempdir().unwrap();
        let bare = temp.path().join("origin.git");
        let work = temp.path().join("work");
        let remote = temp.path().join("remote");

        fs::create_dir_all(&work).unwrap();
        let repo = Repository::init(&work).unwrap();
        let sig = git2::Signature::now("tester", "tester@example.com").unwrap();
        fs::write(work.join("marketplace.json"), r#"{"name":"git-market","skills":[]}"#).unwrap();
        {
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("marketplace.json")).unwrap();
            let tree_id = index.write_tree().unwrap();
            let tree = repo.find_tree(tree_id).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();
        }

        let origin = git2::Repository::init_bare(&bare).unwrap();
        {
            let mut remote_cfg = origin.remote("push-origin", work.to_str().unwrap()).unwrap();
            let _ = remote_cfg;
            let mut push = repo.remote("origin", bare.to_str().unwrap()).unwrap();
            push.push(&["refs/heads/main:refs/heads/main"], None).unwrap();
        }

        // Now clone from the bare repo.
        let (hash, cloned) = clone_git(bare.to_str().unwrap(), &remote).unwrap();
        assert!(hash.len() == 40, "{hash}");
        assert!(looks_like_market(&remote));
        assert!(cloned.is_bare() == false);

        // Second clone resolves the same commit (idempotent revision).
        let (hash2, _) = clone_git(bare.to_str().unwrap(), &remote).unwrap();
        assert_eq!(hash, hash2);
    }

    // ---------------------------------------------------------------- zip (doc 30)

    /// Build a market archive off-disk. `unix_permissions` is separate so the
    /// symlink case can set `S_IFLNK` without a second helper.
    fn write_zip(path: &Path, entries: &[(&str, &str)]) {
        use std::io::Write;
        let file = fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        for (name, body) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn normalize_zip_requires_http() {
        assert_eq!(
            normalize_source_url("zip", "https://host.example/experts.zip").unwrap(),
            "https://host.example/experts.zip"
        );
        assert_eq!(
            normalize_source_url("zip", "http://127.0.0.1:8080/m.zip").unwrap(),
            "http://127.0.0.1:8080/m.zip"
        );
        // A local path is `directory`'s job; the kind decides, not the suffix.
        assert!(normalize_source_url("zip", "/tmp/experts.zip").is_err());
        assert!(normalize_source_url("zip", "experts.zip").is_err());
    }

    #[test]
    fn linked_etag_digest_accepts_only_a_sha256() {
        let sha = "18e18afcaccafade98daf13a54092927904649e1dd4eba8299ab717d5d94ff45";
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("x-linked-etag", sha.parse().unwrap());
        assert_eq!(linked_etag_digest(&headers).as_deref(), Some(sha));

        // A quoted, upper-case digest is the same digest.
        headers.insert(
            "x-linked-etag",
            format!("\"{}\"", sha.to_uppercase()).parse().unwrap(),
        );
        assert_eq!(linked_etag_digest(&headers).as_deref(), Some(sha));

        // Anything that is not 64 hex chars is refused rather than trusted as a
        // revision marker (the header is not a standard contract).
        for bad in [
            "",
            "abc",
            // 63 hex chars.
            "18e18afcaccafade98daf13a54092927904649e1dd4eba8299ab717d5d94ff4",
            // 64 chars, not hex.
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
            // An OSS multipart etag is not a content digest.
            "\"CB1C0FF4FE3E122F0F501EE704A54655-377\"",
        ] {
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert("x-linked-etag", bad.parse().unwrap());
            assert_eq!(linked_etag_digest(&headers), None, "must refuse {bad:?}");
        }

        // Absent header → not told, which the caller must not read as "unchanged".
        assert_eq!(linked_etag_digest(&reqwest::header::HeaderMap::new()), None);
    }

    #[test]
    fn extract_zip_market_unpacks_a_tree_at_the_archive_root() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("market.zip");
        write_zip(
            &archive,
            &[
                (
                    ".codebuddy-plugin/marketplace.json",
                    r#"{"name":"zip-market","plugins":[{"name":"demo","source":"./plugins/demo"}]}"#,
                ),
                ("plugins/demo/.codebuddy-plugin/plugin.json", r#"{"name":"demo"}"#),
                ("plugins/demo/agents/demo.md", "---\nname: demo\n---\n"),
            ],
        );
        let staging = temp.path().join("staging");
        let written = extract_zip_market(&archive, &staging).unwrap();
        assert_eq!(written, 3);
        assert!(looks_like_market(&staging), "the archive root must be the market root");
        assert!(staging.join("plugins/demo/agents/demo.md").is_file());
    }

    /// Regression: the official `experts` market root carries **only**
    /// `.codebuddy-plugin/marketplace.json`, and `looks_like_market` used to
    /// omit it while `probe_directory` accepted it. Every `github`/`git`/`zip`
    /// fetch of that market therefore failed validation before this was fixed.
    #[test]
    fn looks_like_market_accepts_a_plugin_market_root() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("experts");
        fs::create_dir_all(root.join(".codebuddy-plugin")).unwrap();
        fs::write(
            root.join(".codebuddy-plugin/marketplace.json"),
            r#"{"name":"experts","plugins":[]}"#,
        )
        .unwrap();
        assert!(
            looks_like_market(&root),
            "a `.codebuddy-plugin/marketplace.json` root is a market"
        );

        // A bare plugin manifest is also accepted (unchanged behaviour).
        let plugin_root = temp.path().join("plugin");
        fs::create_dir_all(plugin_root.join(".codebuddy-plugin")).unwrap();
        fs::write(plugin_root.join(".codebuddy-plugin/plugin.json"), r#"{"name":"p"}"#).unwrap();
        assert!(looks_like_market(&plugin_root));

        // And a directory with neither stays rejected.
        let empty = temp.path().join("empty");
        fs::create_dir_all(&empty).unwrap();
        assert!(!looks_like_market(&empty));
    }

    #[test]
    fn extract_zip_market_rejects_traversal() {
        let temp = tempfile::tempdir().unwrap();

        // Zip-slip: the entry must be refused and, above all, nothing written
        // outside the destination.
        let escape = temp.path().join("escape.zip");
        write_zip(&escape, &[("../evil.txt", "owned")]);
        let staging = temp.path().join("staging-escape");
        let err = extract_zip_market(&escape, &staging).unwrap_err();
        assert!(err.contains("not a safe relative path"), "{err}");
        assert!(
            !temp.path().join("evil.txt").exists(),
            "a traversal entry escaped the destination"
        );

        // A symlink entry would be refused the same way, but it cannot be
        // constructed here: `SimpleFileOptions::unix_permissions` masks the
        // mode to `mode & 0o777` (zip 2.4.2 `src/write.rs`), so the `S_IFLNK`
        // type bits never reach the archive. The detection itself is pinned by
        // `nomifun_common::zip_safe::symlink_mode_detection`.
    }

    #[test]
    fn extract_zip_market_refuses_a_decompression_bomb() {
        use nomifun_common::zip_safe::ZipExtractionBudget;

        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("bomb.zip");
        write_zip(&archive, &[("big.bin", "0123456789abcdef")]);

        // Written bytes, not the entry's self-declared size: an 8-byte cap
        // trips on a 16-byte payload.
        let err = extract_zip_market_with_budget(
            &archive,
            &temp.path().join("staging-bytes"),
            ZipExtractionBudget::new(8, 100),
        )
        .unwrap_err();
        assert!(err.contains("expands beyond"), "{err}");

        // Entry count is checked up front, before anything is written.
        let err = extract_zip_market_with_budget(
            &archive,
            &temp.path().join("staging-entries"),
            ZipExtractionBudget::new(u64::MAX, 0),
        )
        .unwrap_err();
        assert!(err.contains("too many entries"), "{err}");
        assert!(!temp.path().join("staging-entries").exists());
    }

    /// Live check of the whole `zip` client path against the **real** market
    /// host (public internet, ~18 MiB).
    ///
    /// Ignored by default. Run it whenever the market host or this fetch path
    /// changes:
    ///
    /// ```text
    /// cargo test -p nomifun-app --lib market_source -- --ignored
    /// ```
    ///
    /// It deliberately pins **no digest**: a market is a moving target, so a
    /// pinned value would fail on the next publish. What it pins is the
    /// contract this client depends on — the URL in
    /// `builtin_default_marketplaces()` answers `HEAD` with a 64-hex content
    /// digest, the body is reachable through the host's redirect, the received
    /// bytes hash to exactly that digest, and what comes out of the archive is
    /// a market. That last assertion is the one that would have caught
    /// `looks_like_market` omitting `.codebuddy-plugin/marketplace.json`.
    #[tokio::test]
    #[ignore = "live network test: needs the public market host"]
    async fn live_official_zip_market_probe_download_and_extract_agree() {
        let official = nomifun_app_server::AgentStoreConfig::builtin_default_marketplaces();
        assert_eq!(official.len(), 3, "three official markets are configured");

        // Every official source must be a zip and must report a digest; only the
        // smallest one is downloaded (the other two are 18x and 16x its size).
        let mut target = None;
        for (id, kind, url) in &official {
            assert_eq!(kind, "zip", "{id} must be a zip source");
            let probed = probe_archive_digest(url)
                .await
                .unwrap_or_else(|| panic!("{url} must report X-Linked-Etag"));
            assert_eq!(probed.len(), 64, "{url} digest must be a sha256");
            if id == "connectors" {
                target = Some((url.clone(), probed));
            }
        }
        let (url, probed) = target.expect("the official connectors market is configured");

        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("market.zip");
        let (local, server) = download_archive(&url, &archive).await.expect("download archive");
        assert_eq!(local, probed, "downloaded bytes must hash to the HEAD digest");
        if let Some(server) = server {
            assert_eq!(server, local, "the GET response's digest must agree as well");
        }

        let staging = temp.path().join("staging");
        let written = extract_zip_market(&archive, &staging).unwrap();
        assert!(written > 0, "the archive must contain files");
        assert!(
            looks_like_market(&staging),
            "an official archive must be recognised as a market"
        );
    }
}
