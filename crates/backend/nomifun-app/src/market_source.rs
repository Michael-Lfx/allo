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
use git2::Repository;
use reqwest::StatusCode;
use serde_json::Value;

pub const MARKET_MANIFEST_A: &str = ".codebuddy-skill/marketplace.json";
pub const MARKET_MANIFEST_B: &str = ".codebuddy-connector/connectors.json";
pub const MARKET_MANIFEST_PLUGIN: &str = ".codebuddy-plugin/plugin.json";

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
        other => Err(format!("unsupported remote source kind `{other}`")),
    }
}

/// Directories that make a checkout look like a valid marketplace catalog.
pub fn looks_like_market(root: &Path) -> bool {
    root.join(MARKET_MANIFEST_A).is_file()
        || root.join(MARKET_MANIFEST_B).is_file()
        || root.join(MARKET_MANIFEST_PLUGIN).is_file()
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
        .user_agent("allo-agent-store/1.0")
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
    // a refresh take tens of seconds over a local server.
    const BATCH: usize = 8;
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

/// Download `url` (expected to be a marketplace.json) into `staging`, parsing
/// and validating the manifest. Returns (revision marker, etag).
///
/// When `if_none_match` is provided, the request carries the conditional
/// header; a `304 Not Modified` response maps to `ContentMissing` so callers
/// can short-circuit without downloading the body again (documented freshness
/// semantics: ETag/Last-Modified check before re-fetch).
pub enum HttpFetchOutcome {
    /// Fresh content was downloaded and validated into `staging`.
    Fresh {
        /// Revision marker (etag/last-modified digest or "http").
        revision: String,
        etag: Option<String>,
    },
    /// Server answered `304 Not Modified` — caller keeps last-good.
    NotModified,
}

pub async fn fetch_http_market(
    url: &str,
    staging: &Path,
    if_none_match: Option<&str>,
) -> Result<HttpFetchOutcome, String> {
    let client = reqwest::Client::builder()
        .user_agent("allo-agent-store/1.0")
        .build()
        .map_err(|error| format!("build http client: {error}"))?;
    let mut request = client.get(url);
    if let Some(etag) = if_none_match {
        request = request.header("if-none-match", etag);
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
    let etag = response
        .headers()
        .get("etag")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let last_modified = response
        .headers()
        .get("last-modified")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let text = response
        .text()
        .await
        .map_err(|error| format!("read body {url}: {error}"))?;
    validate_http_manifest(&text, url)?;
    fs::create_dir_all(staging).map_err(|error| format!("create staging {}: {error}", staging.display()))?;
    fs::write(staging.join("marketplace.json"), text)
        .map_err(|error| format!("write manifest: {error}"))?;
    let marker = etag
        .as_deref()
        .or(last_modified.as_deref())
        .map(|value| nomifun_importer::digest::sha256_hex(value.as_bytes()))
        .unwrap_or_else(|| "http".to_owned());
    Ok(HttpFetchOutcome::Fresh { revision: marker, etag })
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
        let HttpFetchOutcome::Fresh { revision, etag } = outcome else {
            panic!("expected fresh content");
        };
        assert_eq!(etag.as_deref(), Some("\"abc-1\""));
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
        let outcome = fetch_http_market(&url, &temp.path().join("staging"), Some("\"abc-1\""))
            .await
            .unwrap();
        assert!(matches!(outcome, HttpFetchOutcome::NotModified));
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
}
