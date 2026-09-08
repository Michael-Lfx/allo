//! Native installation for ordinary Skills from the supported markets.
//!
//! This module deliberately owns only the market-to-archive adapters. The
//! filesystem transaction remains in [`crate::skill_service`], so expert
//! packages and ordinary Skills share the same validation and commit rules.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use nomifun_api_types::{
    SkillMarketInstallStatus, SkillMarketSkillInstallRequest, SkillMarketSkillInstallResponse,
};
use nomifun_common::AppError;
use reqwest::Url;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde::Deserialize;

use crate::error::ExtensionError;
use crate::skill_service::{self, SkillPaths};

use super::client::{
    MARKET_REQUEST_TIMEOUT, MAX_MARKET_SKILL_ARCHIVE_BYTES, SKILLHUB_DOWNLOAD_TIMEOUT,
    build_market_client, map_market_fetch_error, read_market_bytes, read_market_response,
    send_skillhub_get_with_retry,
};
use super::parse::{is_market_slug, json_text};
use super::staging::{MarketStaging, create_market_staging};

const CLAWHUB_SOURCE: &str = "clawhub";
const SKILLHUB_SOURCE: &str = "skillhub";
const LOOPHUB_SOURCE: &str = "loophub";
const CLAWHUB_DOWNLOAD_URL: &str = "https://clawhub.ai/api/v1/download";
const GITHUB_ARCHIVE_HOST: &str = "codeload.github.com";
/// Exact single-Skill detail endpoint (`skills`, not the `skillsets` base in
/// `package.rs`). Loose rate limits, clean 404s.
const SKILLHUB_SKILL_DETAIL_BASE_URL: &str = "https://api.skillhub.cn/api/v1/skills/";
/// Same upstream contract as `package.rs`'s `SKILLHUB_SKILL_DOWNLOAD_URL`;
/// kept local so this module does not depend on the expert-package installer.
const SKILLHUB_SKILL_DOWNLOAD_URL: &str = "https://api.skillhub.cn/api/v1/download";
/// Manifest-declared Skill names larger than this are rejected.
const MAX_MARKET_SKILL_NAME_BYTES: usize = 96;
/// Stream-phase retries for one archive download (send-phase transient
/// failures — 429/5xx/connect/timeout — are already retried inside
/// [`send_skillhub_get_with_retry`]).
const MAX_DOWNLOAD_STREAM_ATTEMPTS: u32 = 2;

/// Stable, endpoint-local installation failure categories. The HTTP mapper in
/// `skill_routes` owns their public status/code representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MarketSkillInstallError {
    #[error("the requested market source is unsupported")]
    SourceUnsupported,
    #[error("the market Skill id is invalid")]
    IdInvalid,
    #[error("the market Skill was not found")]
    NotFound,
    #[error("a Skill with this name already exists and cannot be replaced")]
    NameConflict,
    #[error("the downloaded Skill artifact is invalid")]
    ArtifactInvalid,
    #[error("the downloaded Skill manifest is invalid")]
    ManifestInvalid,
    #[error("the downloaded Skill is a multi-skill bundle")]
    BundleUnsupported,
    #[error("the Skill market request failed")]
    Network,
    #[error("the Skill market request timed out")]
    Timeout,
    #[error("the local Skill installation failed")]
    LocalIo,
}

#[derive(Debug)]
enum NativeMarketSkill {
    ClawHub { owner: String, slug: String },
    SkillHub { owner: String, slug: String },
    LoopHub { artifact_url: Url },
}

impl NativeMarketSkill {
    /// The name the installed directory must declare, when the source knows
    /// it upfront. ClawHub archives are keyed by slug, so the manifest must
    /// match it exactly. SkillHub/LoopHub install under the manifest's
    /// declared name instead: SkillHub's public slug diverges from the
    /// manifest `name` for a large share of the catalog (e.g. slug
    /// `baozheng` ships `name: baozheng-skills`), and the runtime resolves
    /// Skills by directory name, so pinning the slug would silently drop
    /// those Skills from sessions.
    fn expected_name(&self) -> Option<&str> {
        match self {
            Self::ClawHub { slug, .. } => Some(slug),
            Self::SkillHub { .. } | Self::LoopHub { .. } => None,
        }
    }
}

#[derive(Debug)]
struct DownloadedArtifact {
    bytes: Vec<u8>,
    /// A GitHub descriptor can point at a Skill directory inside the repo.
    /// The path is used only to narrow the extracted archive search.
    descriptor_path: Option<PathBuf>,
}

#[async_trait::async_trait]
trait MarketSkillDownloader: Send + Sync {
    async fn download(
        &self,
        target: &NativeMarketSkill,
    ) -> Result<DownloadedArtifact, MarketSkillInstallError>;
}

/// SkillHub endpoints used by the native single-Skill pipeline. Fields are
/// injectable so integration tests can point the downloader at a local stub.
#[derive(Debug)]
struct SkillHubEndpoints {
    detail_base_url: String,
    download_url: String,
}

impl SkillHubEndpoints {
    fn production() -> Self {
        Self {
            detail_base_url: SKILLHUB_SKILL_DETAIL_BASE_URL.into(),
            download_url: SKILLHUB_SKILL_DOWNLOAD_URL.into(),
        }
    }

    #[cfg(test)]
    fn for_test(base_url: &str) -> Self {
        Self {
            detail_base_url: format!("{base_url}/api/v1/skills/"),
            download_url: format!("{base_url}/api/v1/download"),
        }
    }
}

struct HttpMarketSkillDownloader {
    client: reqwest::Client,
    skillhub: SkillHubEndpoints,
}

impl HttpMarketSkillDownloader {
    #[cfg(test)]
    fn for_test(base_url: &str) -> Self {
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("test market client should build");
        Self {
            client,
            skillhub: SkillHubEndpoints::for_test(base_url),
        }
    }
}

#[async_trait::async_trait]
impl MarketSkillDownloader for HttpMarketSkillDownloader {
    async fn download(
        &self,
        target: &NativeMarketSkill,
    ) -> Result<DownloadedArtifact, MarketSkillInstallError> {
        match target {
            NativeMarketSkill::ClawHub { owner, slug } => {
                let slug_param = format!("{owner}/{slug}");
                let url = Url::parse_with_params(CLAWHUB_DOWNLOAD_URL, &[("slug", &slug_param)])
                    .map_err(|_| MarketSkillInstallError::LocalIo)?;
                let bytes = download_http_bytes(&self.client, url, "ClawHub skill archive").await?;
                if is_zip_bytes(&bytes) {
                    return Ok(DownloadedArtifact {
                        bytes,
                        descriptor_path: None,
                    });
                }

                let descriptor = parse_public_github_descriptor(&bytes)?;
                let bytes = download_http_bytes(
                    &self.client,
                    descriptor.archive_url,
                    "ClawHub GitHub skill archive",
                )
                .await?;
                Ok(DownloadedArtifact {
                    bytes,
                    descriptor_path: descriptor.path,
                })
            }
            NativeMarketSkill::SkillHub { owner, slug } => {
                let detail = self.fetch_skillhub_detail(owner, slug).await?;
                let bytes = self.download_skillhub_archive(&detail).await?;
                Ok(DownloadedArtifact {
                    bytes,
                    descriptor_path: None,
                })
            }
            NativeMarketSkill::LoopHub { artifact_url } => {
                let mut response = self
                    .client
                    .get(artifact_url.clone())
                    .header(ACCEPT, "application/zip,application/octet-stream,*/*")
                    .send()
                    .await
                    .map_err(|error| map_app_market_error(map_market_fetch_error(error)))?;
                validate_loophub_artifact_url(response.url().as_str()).map_err(|_| {
                    MarketSkillInstallError::ArtifactInvalid
                })?;
                Ok(DownloadedArtifact {
                    bytes: read_market_bytes(
                        &mut response,
                        MAX_MARKET_SKILL_ARCHIVE_BYTES,
                        "LoopHub skill archive",
                    )
                    .await
                    .map_err(map_app_market_error)?,
                    descriptor_path: None,
                })
            }
        }
    }
}

/// Exact SkillHub detail record: the resolved public slug and the pinned
/// version to download.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RemoteSkillDetail {
    slug: String,
    version: String,
}

impl HttpMarketSkillDownloader {
    /// Exact detail lookup for one market entry. Verifies that the namespace
    /// SkillHub resolves for `slug` actually belongs to the requested `owner`
    /// and returns the pinned version to download. Replaces the old fuzzy
    /// `/api/v1/search` fallback, which missed valid entries outside the top
    /// search results and was aggressively rate limited.
    async fn fetch_skillhub_detail(
        &self,
        owner: &str,
        slug: &str,
    ) -> Result<RemoteSkillDetail, MarketSkillInstallError> {
        let url = skillhub_skill_detail_url(&self.skillhub.detail_base_url, slug)?;
        let mut response =
            send_skillhub_get_with_retry(&self.client, url, "application/json", MARKET_REQUEST_TIMEOUT)
                .await
                .map_err(map_app_market_error)?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(MarketSkillInstallError::NotFound);
        }
        // Exhausted 429/5xx and any other non-success land here.
        if !response.status().is_success() {
            return Err(MarketSkillInstallError::Network);
        }
        let body = read_market_response(&mut response)
            .await
            .map_err(map_app_market_error)?;
        parse_skill_detail(&body, owner, slug)
    }

    /// Download the version-pinned archive into memory, retrying only
    /// mid-stream transport failures (a fresh request per attempt; the partial
    /// buffer is simply dropped).
    async fn download_skillhub_archive(
        &self,
        skill: &RemoteSkillDetail,
    ) -> Result<Vec<u8>, MarketSkillInstallError> {
        // The version from the detail lookup is pinned on the download, so a
        // publish landing between the two requests cannot silently swap the
        // archived content. SkillHub's download contract is keyed by
        // slug + version only; an `owner` parameter is not honored upstream
        // and is deliberately not sent.
        let url = Url::parse_with_params(
            &self.skillhub.download_url,
            [
                ("slug", skill.slug.as_str()),
                ("version", skill.version.as_str()),
            ],
        )
        .map_err(|_| MarketSkillInstallError::LocalIo)?;

        let mut stream_attempt = 0_u32;
        loop {
            stream_attempt += 1;
            match self.download_attempt(&url).await {
                Ok(bytes) => return Ok(bytes),
                Err(DownloadAttemptError::Final(error)) => return Err(error),
                Err(DownloadAttemptError::StreamDied(error)) => {
                    // Only a mid-stream transport failure is retried here, with
                    // a fresh request per attempt. Status-gate failures
                    // (including an already-retried 429/5xx exhausted inside
                    // the send helper), validation failures, and local I/O are
                    // deterministic and returned immediately.
                    if stream_attempt >= MAX_DOWNLOAD_STREAM_ATTEMPTS {
                        return Err(error);
                    }
                    tokio::time::sleep(download_retry_backoff(stream_attempt)).await;
                }
            }
        }
    }

    /// One download attempt: send (with transient retry inside the helper),
    /// status/content-type/size gates, then stream the body into memory.
    async fn download_attempt(&self, url: &Url) -> Result<Vec<u8>, DownloadAttemptError> {
        let mut response = send_skillhub_get_with_retry(
            &self.client,
            url.clone(),
            "application/zip,application/octet-stream",
            SKILLHUB_DOWNLOAD_TIMEOUT,
        )
        .await
        .map_err(|error| DownloadAttemptError::Final(map_app_market_error(error)))?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(DownloadAttemptError::Final(MarketSkillInstallError::NotFound));
        }
        if !response.status().is_success() {
            return Err(DownloadAttemptError::Final(MarketSkillInstallError::Network));
        }

        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.split(';').next().unwrap_or_default().trim().to_ascii_lowercase());
        if !matches!(content_type.as_deref(), Some("application/zip" | "application/octet-stream")) {
            return Err(DownloadAttemptError::Final(MarketSkillInstallError::ArtifactInvalid));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_MARKET_SKILL_ARCHIVE_BYTES)
        {
            return Err(DownloadAttemptError::Final(MarketSkillInstallError::ArtifactInvalid));
        }

        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| DownloadAttemptError::StreamDied(MarketSkillInstallError::Network))?
        {
            if bytes.len().saturating_add(chunk.len()) as u64 > MAX_MARKET_SKILL_ARCHIVE_BYTES {
                return Err(DownloadAttemptError::Final(MarketSkillInstallError::ArtifactInvalid));
            }
            bytes.extend_from_slice(&chunk);
        }

        if !is_zip_bytes(&bytes) {
            return Err(DownloadAttemptError::Final(MarketSkillInstallError::ArtifactInvalid));
        }
        Ok(bytes)
    }
}

/// Internal outcome of one download attempt. The outer stream-retry loop must
/// only retry failures whose partial output can be safely discarded and whose
/// cause is transient — i.e. the connection dying while the body streams.
/// Send-phase failures were already retried inside
/// [`send_skillhub_get_with_retry`], and status/validation/local-I/O failures
/// are deterministic; both are `Final` so an exhausted 429 cannot be
/// double-retried into a second full round.
enum DownloadAttemptError {
    Final(MarketSkillInstallError),
    StreamDied(MarketSkillInstallError),
}

fn skillhub_skill_detail_url(base_url: &str, slug: &str) -> Result<Url, MarketSkillInstallError> {
    // `slug` has already passed `is_market_slug` via `parse_install_target`,
    // so it cannot escape the detail path segment.
    let mut url = Url::parse(base_url).map_err(|_| MarketSkillInstallError::LocalIo)?;
    url.path_segments_mut()
        .map_err(|_| MarketSkillInstallError::LocalIo)?
        .pop_if_empty()
        .push(slug);
    Ok(url)
}

/// Parse the exact detail response and bind it to the requested market entry.
///
/// Owner verification uses `namespace.handle` — NOT the documented
/// `owner.handle`, which is SkillHub's internal user handle (e.g.
/// `u_b0de8114`) rather than the ranking's namespace (e.g. `tencent-adm`).
/// `namespace` is not listed in the official response schema but is present
/// on every observed response; when it is missing we fail closed
/// (ArtifactInvalid) rather than install an unverified identity.
fn parse_skill_detail(
    body: &str,
    expected_owner: &str,
    expected_slug: &str,
) -> Result<RemoteSkillDetail, MarketSkillInstallError> {
    let root = serde_json::from_str::<serde_json::Value>(body)
        .map_err(|_| MarketSkillInstallError::ArtifactInvalid)?;
    let namespace = root
        .get("namespace")
        .ok_or(MarketSkillInstallError::ArtifactInvalid)?;
    let handle = json_text(namespace, "handle", 96).ok_or(MarketSkillInstallError::ArtifactInvalid)?;
    let public_slug = json_text(namespace, "publicSlug", 96)
        .or_else(|| {
            json_text(namespace, "canonicalName", 160).and_then(|canonical| {
                canonical
                    .trim()
                    .trim_start_matches('@')
                    .rsplit('/')
                    .next()
                    .map(str::to_string)
            })
        })
        .ok_or(MarketSkillInstallError::ArtifactInvalid)?;
    if !is_market_slug(&handle) || !is_market_slug(&public_slug) {
        return Err(MarketSkillInstallError::ArtifactInvalid);
    }
    // The download endpoint resolves archives by slug alone; a requested
    // entry whose namespace does not match what SkillHub resolves is
    // uninstallable and reported as not found rather than substituted.
    if !handle.eq_ignore_ascii_case(expected_owner) || !public_slug.eq_ignore_ascii_case(expected_slug) {
        return Err(MarketSkillInstallError::NotFound);
    }
    let version = root
        .get("latestVersion")
        .and_then(|value| json_text(value, "version", 64))
        .ok_or(MarketSkillInstallError::ArtifactInvalid)?;
    Ok(RemoteSkillDetail {
        slug: public_slug,
        version,
    })
}

fn download_retry_backoff(stream_attempt: u32) -> Duration {
    Duration::from_millis(500 * u64::from(stream_attempt))
}

#[derive(Debug)]
struct PublicGithubDescriptor {
    archive_url: Url,
    path: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct RawPublicGithubDescriptor {
    source: String,
    repo: String,
    #[serde(rename = "ref")]
    git_ref: String,
    #[serde(default)]
    path: Option<String>,
}

/// Install one ordinary Skill without starting OpenClaw or another external
/// CLI. The backend derives all ClawHub/SkillHub URLs from the validated id;
/// only LoopHub receives a source-owned artifact hint from the ranking feed.
pub async fn install_market_skill(
    paths: &SkillPaths,
    req: SkillMarketSkillInstallRequest,
) -> Result<SkillMarketSkillInstallResponse, MarketSkillInstallError> {
    let target = parse_install_target(&req)?;
    let client = build_market_client().map_err(|_| MarketSkillInstallError::LocalIo)?;
    let downloader = HttpMarketSkillDownloader {
        client,
        skillhub: SkillHubEndpoints::production(),
    };
    install_market_skill_with_downloader(paths, &req.source, &req.id, target, &downloader).await
}

async fn install_market_skill_with_downloader<D: MarketSkillDownloader>(
    paths: &SkillPaths,
    source: &str,
    id: &str,
    target: NativeMarketSkill,
    downloader: &D,
) -> Result<SkillMarketSkillInstallResponse, MarketSkillInstallError> {
    if let Some(expected_name) = target.expected_name()
        && reuse_existing_skill(paths, expected_name).await?.is_some()
    {
        return Ok(install_response(
            source,
            id,
            expected_name,
            SkillMarketInstallStatus::Reused,
        ));
    }

    let artifact = downloader.download(&target).await?;
    if artifact.bytes.len() as u64 > MAX_MARKET_SKILL_ARCHIVE_BYTES {
        return Err(MarketSkillInstallError::ArtifactInvalid);
    }

    let staging = create_market_staging(paths, "skill")
        .await
        .map_err(|_| MarketSkillInstallError::LocalIo)?;
    let (staged_dir, skill_name) = stage_market_skill(&staging, artifact, target.expected_name()).await?;

    // Re-check under the same lock used by expert packages. A second window
    // may have installed this Skill while this request was downloading it.
    // This is also the idempotency fence for declared-name installs: a
    // name≠slug SkillHub entry skips the pre-download check (its declared
    // name is unknown until extraction) and is caught here instead.
    let _commit_guard = super::market_commit_lock().lock().await;
    if reuse_existing_skill(paths, &skill_name).await?.is_some() {
        return Ok(install_response(
            source,
            id,
            &skill_name,
            SkillMarketInstallStatus::Reused,
        ));
    }

    let commit = skill_service::commit_market_skill_directory(paths, &staged_dir, &skill_name)
        .await
        .map_err(map_commit_error)?;
    let status = match commit {
        skill_service::MarketSkillCommit::Created => SkillMarketInstallStatus::Installed,
        skill_service::MarketSkillCommit::Reused => SkillMarketInstallStatus::Reused,
    };
    Ok(install_response(source, id, &skill_name, status))
}

fn install_response(
    source: &str,
    id: &str,
    skill_name: &str,
    status: SkillMarketInstallStatus,
) -> SkillMarketSkillInstallResponse {
    SkillMarketSkillInstallResponse {
        source: source.to_owned(),
        id: id.to_owned(),
        skill_name: skill_name.to_owned(),
        status,
    }
}

fn parse_install_target(
    req: &SkillMarketSkillInstallRequest,
) -> Result<NativeMarketSkill, MarketSkillInstallError> {
    match req.source.as_str() {
        CLAWHUB_SOURCE => {
            if req.artifact_url.is_some() {
                return Err(MarketSkillInstallError::IdInvalid);
            }
            let (owner, slug) = parse_two_part_id(&req.id, CLAWHUB_SOURCE)?;
            Ok(NativeMarketSkill::ClawHub { owner, slug })
        }
        SKILLHUB_SOURCE => {
            if req.artifact_url.is_some() {
                return Err(MarketSkillInstallError::IdInvalid);
            }
            let suffix = req
                .id
                .strip_prefix("skillhub:")
                .ok_or(MarketSkillInstallError::IdInvalid)?;
            let mut parts = suffix.split('/');
            let owner = parts.next().unwrap_or_default();
            let marker = parts.next().unwrap_or_default();
            let slug = parts.next().unwrap_or_default();
            if parts.next().is_some()
                || marker != "skills"
                || !is_market_slug(owner)
                || !is_market_slug(slug)
            {
                return Err(MarketSkillInstallError::IdInvalid);
            }
            // The owner is kept: the detail lookup binds the resolved entry to
            // this namespace before any archive is downloaded.
            Ok(NativeMarketSkill::SkillHub {
                owner: owner.to_owned(),
                slug: slug.to_owned(),
            })
        }
        LOOPHUB_SOURCE => {
            let suffix = req
                .id
                .strip_prefix("loophub:")
                .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
                .ok_or(MarketSkillInstallError::IdInvalid)?;
            let id = suffix
                .parse::<u64>()
                .map_err(|_| MarketSkillInstallError::IdInvalid)?;
            if id == 0 {
                return Err(MarketSkillInstallError::IdInvalid);
            }
            let artifact_url = req
                .artifact_url
                .as_deref()
                .ok_or(MarketSkillInstallError::IdInvalid)?;
            Ok(NativeMarketSkill::LoopHub {
                artifact_url: validate_loophub_artifact_url(artifact_url)?,
            })
        }
        _ => Err(MarketSkillInstallError::SourceUnsupported),
    }
}

fn parse_two_part_id(id: &str, prefix: &str) -> Result<(String, String), MarketSkillInstallError> {
    let suffix = id
        .strip_prefix(&format!("{prefix}:"))
        .ok_or(MarketSkillInstallError::IdInvalid)?;
    let mut parts = suffix.split('/');
    let first = parts.next().unwrap_or_default();
    let second = parts.next().unwrap_or_default();
    if parts.next().is_some() || !is_market_slug(first) || !is_market_slug(second) {
        return Err(MarketSkillInstallError::IdInvalid);
    }
    Ok((first.to_owned(), second.to_owned()))
}

fn validate_loophub_artifact_url(value: &str) -> Result<Url, MarketSkillInstallError> {
    let url = Url::parse(value).map_err(|_| MarketSkillInstallError::IdInvalid)?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || !url
            .host_str()
            .is_some_and(|host| host.eq_ignore_ascii_case("dl.cocoloop.cn"))
        || !url.path().starts_with("/bss/skills/")
    {
        return Err(MarketSkillInstallError::IdInvalid);
    }
    Ok(url)
}

async fn download_http_bytes(
    client: &reqwest::Client,
    url: Url,
    label: &str,
) -> Result<Vec<u8>, MarketSkillInstallError> {
    let mut response = client
        .get(url)
        .header(ACCEPT, "application/zip,application/octet-stream,application/json,*/*")
        .send()
        .await
        .map_err(|error| map_app_market_error(map_market_fetch_error(error)))?;
    read_market_bytes(&mut response, MAX_MARKET_SKILL_ARCHIVE_BYTES, label)
        .await
        .map_err(map_app_market_error)
}

fn is_zip_bytes(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") || bytes.starts_with(b"PK\x07\x08")
}

fn parse_public_github_descriptor(bytes: &[u8]) -> Result<PublicGithubDescriptor, MarketSkillInstallError> {
    let raw = serde_json::from_slice::<RawPublicGithubDescriptor>(bytes)
        .map_err(|_| MarketSkillInstallError::ArtifactInvalid)?;
    if raw.source != "public-github" {
        return Err(MarketSkillInstallError::ArtifactInvalid);
    }
    let (owner, repo) = parse_github_repo(&raw.repo)?;
    if !is_safe_github_ref(&raw.git_ref) {
        return Err(MarketSkillInstallError::ArtifactInvalid);
    }
    let path = raw
        .path
        .as_deref()
        .map(parse_descriptor_path)
        .transpose()?;
    let archive_url = Url::parse(&format!(
        "https://{GITHUB_ARCHIVE_HOST}/{owner}/{repo}/zip/{}",
        raw.git_ref
    ))
    .map_err(|_| MarketSkillInstallError::LocalIo)?;
    Ok(PublicGithubDescriptor { archive_url, path })
}

fn parse_github_repo(repo: &str) -> Result<(String, String), MarketSkillInstallError> {
    let mut parts = repo.split('/');
    let owner = parts.next().unwrap_or_default();
    let name = parts.next().unwrap_or_default();
    if parts.next().is_some() || !is_market_slug(owner) || !is_market_slug(name) {
        return Err(MarketSkillInstallError::ArtifactInvalid);
    }
    Ok((owner.to_owned(), name.to_owned()))
}

fn is_safe_github_ref(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && !value.contains("..")
        && !value.chars().any(|character| {
            character.is_control() || matches!(character, '?' | '#' | '\\' | '%')
        })
}

fn parse_descriptor_path(value: &str) -> Result<PathBuf, MarketSkillInstallError> {
    if value.is_empty()
        || value.len() > 320
        || value.starts_with('/')
        || value.contains('\\')
        || value.contains(':')
    {
        return Err(MarketSkillInstallError::ArtifactInvalid);
    }
    let mut path = PathBuf::new();
    let mut depth = 0;
    for component in value.split('/') {
        if component.is_empty() || component == "." || component == ".." || component.contains('\0') {
            return Err(MarketSkillInstallError::ArtifactInvalid);
        }
        depth += 1;
        if depth > skill_service::MARKET_IMPORT_SCAN_DEPTH {
            return Err(MarketSkillInstallError::ArtifactInvalid);
        }
        path.push(component);
    }
    Ok(path)
}

async fn reuse_existing_skill(
    paths: &SkillPaths,
    name: &str,
) -> Result<Option<()>, MarketSkillInstallError> {
    let target = paths.user_skills_dir.join(name);
    match tokio::fs::symlink_metadata(&target).await {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(MarketSkillInstallError::NameConflict);
            }
            skill_service::validate_market_skill_directory(&target, name)
                .await
                .map_err(|_| MarketSkillInstallError::NameConflict)?;
            Ok(Some(()))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(MarketSkillInstallError::LocalIo),
    }
}

async fn stage_market_skill(
    staging: &MarketStaging,
    artifact: DownloadedArtifact,
    expected_name: Option<&str>,
) -> Result<(PathBuf, String), MarketSkillInstallError> {
    let archive_path = staging.root.join("skill.zip");
    let extract_dir = staging.root.join("extract");
    tokio::fs::write(&archive_path, &artifact.bytes)
        .await
        .map_err(|_| MarketSkillInstallError::LocalIo)?;
    skill_service::extract_skill_archive_to_staging(&archive_path, &extract_dir)
        .await
        .map_err(map_archive_error)?;
    tokio::fs::remove_file(&archive_path)
        .await
        .map_err(|_| MarketSkillInstallError::LocalIo)?;

    let mut skill_dirs = Vec::new();
    skill_service::collect_skill_dirs_recursive(
        &extract_dir,
        &mut skill_dirs,
        skill_service::MARKET_IMPORT_SCAN_DEPTH,
    )
    .await
    .map_err(map_archive_error)?;
    if let Some(descriptor_path) = artifact.descriptor_path.as_deref() {
        skill_dirs.retain(|path| path_ends_with(path, descriptor_path));
    }
    // A single-Skill install cannot guess which manifest the user wanted:
    // zero manifests means the archive is broken, more than one means it is a
    // multi-Skill bundle (e.g. SkillHub's `ima-skills`) this endpoint does
    // not support.
    if skill_dirs.is_empty() {
        return Err(MarketSkillInstallError::ManifestInvalid);
    }
    if skill_dirs.len() > 1 {
        return Err(MarketSkillInstallError::BundleUnsupported);
    }

    let skill_dir = skill_dirs.pop().expect("length checked above");
    let skill_name = match expected_name {
        Some(expected_name) => skill_service::validate_market_skill_directory(&skill_dir, expected_name).await,
        None => skill_service::validate_market_skill_directory_name(&skill_dir).await,
    }
    .map_err(|_| MarketSkillInstallError::ManifestInvalid)?;
    validate_market_skill_name(&skill_name)?;
    Ok((skill_dir, skill_name))
}

/// Bound a manifest-declared Skill name to a directory name that is safe on
/// every supported OS. `skill_service::validate_filename` has no length cap
/// and knows nothing about Windows device names or trailing dot/space, all of
/// which produce undeletable or misresolved directories; the reserved sibling
/// directories under the user Skills root (companion/shared/_drafts and the
/// market staging parent) must never be install targets either.
fn validate_market_skill_name(name: &str) -> Result<(), MarketSkillInstallError> {
    if name.len() > MAX_MARKET_SKILL_NAME_BYTES
        || name.ends_with('.')
        || name.ends_with(' ')
        || name.chars().any(char::is_control)
        || [".market-import", "companion", "shared", "_drafts"]
            .iter()
            .any(|reserved| name.eq_ignore_ascii_case(reserved))
        || is_windows_device_name(name)
    {
        return Err(MarketSkillInstallError::ManifestInvalid);
    }
    skill_service::validate_filename(name).map_err(|_| MarketSkillInstallError::ManifestInvalid)
}

fn is_windows_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0')
}

fn path_ends_with(path: &Path, suffix: &Path) -> bool {
    let path_components = path
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>();
    let suffix_components = suffix
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>();
    path_components.len() >= suffix_components.len()
        && path_components[path_components.len() - suffix_components.len()..] == suffix_components[..]
}

fn map_commit_error(error: ExtensionError) -> MarketSkillInstallError {
    match error {
        ExtensionError::InvalidSkillPath(message) if message.contains("user Skill target") => {
            MarketSkillInstallError::NameConflict
        }
        // The staged directory failed revalidation inside the commit.
        ExtensionError::InvalidSkillPath(_) => MarketSkillInstallError::ManifestInvalid,
        ExtensionError::Io(_) => MarketSkillInstallError::LocalIo,
        _ => MarketSkillInstallError::LocalIo,
    }
}

fn map_archive_error(error: ExtensionError) -> MarketSkillInstallError {
    match error {
        ExtensionError::Io(_) => MarketSkillInstallError::LocalIo,
        _ => MarketSkillInstallError::ArtifactInvalid,
    }
}

/// Map transport-layer [`AppError`]s from the shared market HTTP helpers onto
/// the typed install categories. New code should raise typed errors at the
/// point of failure instead of relying on this conversion.
fn map_app_market_error(error: AppError) -> MarketSkillInstallError {
    match error {
        AppError::NotFound(_) => MarketSkillInstallError::NotFound,
        AppError::Timeout(_) => MarketSkillInstallError::Timeout,
        AppError::BadGateway(_) => MarketSkillInstallError::Network,
        AppError::Conflict(_) => MarketSkillInstallError::NameConflict,
        AppError::BadRequest(_) => MarketSkillInstallError::ManifestInvalid,
        _ => MarketSkillInstallError::LocalIo,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct FakeDownloader {
        archive: Vec<u8>,
        calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl MarketSkillDownloader for FakeDownloader {
        async fn download(
            &self,
            target: &NativeMarketSkill,
        ) -> Result<DownloadedArtifact, MarketSkillInstallError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match target {
                NativeMarketSkill::ClawHub { owner, slug } => {
                    assert_eq!(owner, "owner");
                    assert_eq!(slug, "claw-skill");
                }
                // SkillHub routing (owner/slug propagation) is asserted by the
                // recorded-HTTP pipeline tests below; this fake only serves
                // bytes for any SkillHub target.
                NativeMarketSkill::SkillHub { .. } => {}
                NativeMarketSkill::LoopHub { artifact_url } => {
                    assert_eq!(artifact_url.host_str(), Some("dl.cocoloop.cn"));
                }
            }
            Ok(DownloadedArtifact {
                bytes: self.archive.clone(),
                descriptor_path: None,
            })
        }
    }

    fn make_paths() -> (TempDir, SkillPaths) {
        let tmp = TempDir::new().unwrap();
        let paths = SkillPaths {
            data_dir: tmp.path().to_path_buf(),
            user_skills_dir: tmp.path().join("skills"),
            cron_skills_dir: tmp.path().join("cron").join("skills"),
            builtin_skills_dir: tmp.path().join("builtin-skills"),
            builtin_rules_dir: tmp.path().join("builtin-rules"),
            preset_rules_dir: tmp.path().join("preset-rules"),
            preset_skills_dir: tmp.path().join("preset-skills"),
            catalog_roots: Default::default(),
        };
        (tmp, paths)
    }

    fn make_archive(skill_names: &[&str]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for name in skill_names {
            writer
                .start_file(format!("{name}/SKILL.md"), options)
                .unwrap();
            writer
                .write_all(format!("---\nname: {name}\ndescription: Test skill\n---\n").as_bytes())
                .unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    /// Archive whose directory prefix differs from the manifest-declared name,
    /// mirroring SkillHub entries like slug `baozheng` → `name: baozheng-skills`.
    fn archive_for(dir_prefix: &str, name: &str) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        writer
            .start_file(format!("{dir_prefix}/SKILL.md"), options)
            .unwrap();
        writer
            .write_all(format!("---\nname: {name}\ndescription: Test skill\n---\n").as_bytes())
            .unwrap();
        writer
            .start_file(format!("{dir_prefix}/README.md"), options)
            .unwrap();
        writer.write_all(b"demo").unwrap();
        writer.finish().unwrap().into_inner()
    }

    /// Multi-manifest archive mirroring SkillHub bundles like `ima-skills`:
    /// sibling Skill directories under one wrapper, no root manifest.
    fn bundle_archive() -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("bundle/alpha/SKILL.md", options).unwrap();
        writer
            .write_all(b"---\nname: bundle-alpha\ndescription: Alpha\n---\n")
            .unwrap();
        writer.start_file("bundle/beta/SKILL.md", options).unwrap();
        writer
            .write_all(b"---\nname: bundle-beta\ndescription: Beta\n---\n")
            .unwrap();
        writer.finish().unwrap().into_inner()
    }

    fn no_manifest_archive() -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("skill/README.md", options).unwrap();
        writer.write_all(b"no manifest here").unwrap();
        writer.finish().unwrap().into_inner()
    }

    fn request(source: &str, id: &str, artifact_url: Option<&str>) -> SkillMarketSkillInstallRequest {
        SkillMarketSkillInstallRequest {
            source: source.into(),
            id: id.into(),
            artifact_url: artifact_url.map(str::to_owned),
        }
    }

    #[test]
    fn parses_three_native_source_ids_and_rejects_external_sources() {
        assert!(matches!(
            parse_install_target(&request(CLAWHUB_SOURCE, "clawhub:owner/claw-skill", None)).unwrap(),
            NativeMarketSkill::ClawHub { .. }
        ));
        // The SkillHub owner is preserved for namespace binding at detail time.
        match parse_install_target(&request(SKILLHUB_SOURCE, "skillhub:owner/skills/skill-skill", None))
            .unwrap()
        {
            NativeMarketSkill::SkillHub { owner, slug } => {
                assert_eq!(owner, "owner");
                assert_eq!(slug, "skill-skill");
            }
            other => panic!("expected SkillHub target, got {other:?}"),
        }
        assert!(matches!(
            parse_install_target(&request(
                LOOPHUB_SOURCE,
                "loophub:12277",
                Some("https://dl.cocoloop.cn/bss/skills/skill.zip")
            ))
            .unwrap(),
            NativeMarketSkill::LoopHub { .. }
        ));
        assert_eq!(
            parse_install_target(&request("mcpworld", "mcpworld:x", None)).unwrap_err(),
            MarketSkillInstallError::SourceUnsupported
        );
        assert_eq!(
            parse_install_target(&request(SKILLHUB_SOURCE, "skillhub:owner/skills/../x", None))
                .unwrap_err(),
            MarketSkillInstallError::IdInvalid
        );
        assert_eq!(
            parse_install_target(&request(SKILLHUB_SOURCE, "skillhub:owner/other/skill-skill", None))
                .unwrap_err(),
            MarketSkillInstallError::IdInvalid
        );
    }

    #[test]
    fn validates_loophub_artifact_allowlist() {
        assert!(validate_loophub_artifact_url("https://dl.cocoloop.cn/bss/skills/a.zip").is_ok());
        for url in [
            "http://dl.cocoloop.cn/bss/skills/a.zip",
            "https://evil.example/bss/skills/a.zip",
            "https://dl.cocoloop.cn/other/a.zip",
            "https://user:pass@dl.cocoloop.cn/bss/skills/a.zip",
        ] {
            assert_eq!(
                validate_loophub_artifact_url(url).unwrap_err(),
                MarketSkillInstallError::IdInvalid,
                "must reject {url}"
            );
        }
    }

    #[test]
    fn validates_public_github_descriptor_without_accepting_arbitrary_urls() {
        let descriptor = br#"{"source":"public-github","repo":"owner/repo","ref":"main","path":"skills/demo"}"#;
        let parsed = parse_public_github_descriptor(descriptor).unwrap();
        assert_eq!(parsed.archive_url.host_str(), Some(GITHUB_ARCHIVE_HOST));
        assert_eq!(parsed.archive_url.path(), "/owner/repo/zip/main");
        assert_eq!(parsed.path.as_deref(), Some(Path::new("skills/demo")));

        for descriptor in [
            br#"{"source":"public-github","repo":"https://evil.example/repo","ref":"main"}"#
                .as_slice(),
            br#"{"source":"public-github","repo":"owner/repo","ref":"../main"}"#.as_slice(),
            br#"{"source":"public-github","repo":"owner/repo","ref":"main","path":"../secret"}"#
                .as_slice(),
        ] {
            assert_eq!(
                parse_public_github_descriptor(descriptor).unwrap_err(),
                MarketSkillInstallError::ArtifactInvalid
            );
        }
    }

    #[tokio::test]
    async fn installs_all_three_sources_and_reuses_valid_existing_skill() {
        let (_tmp, paths) = make_paths();
        let calls = Arc::new(AtomicUsize::new(0));
        let downloader = FakeDownloader {
            archive: make_archive(&["claw-skill"]),
            calls: calls.clone(),
        };

        let claw = install_market_skill_with_downloader(
            &paths,
            CLAWHUB_SOURCE,
            "clawhub:owner/claw-skill",
            parse_install_target(&request(CLAWHUB_SOURCE, "clawhub:owner/claw-skill", None)).unwrap(),
            &downloader,
        )
        .await
        .unwrap();
        assert_eq!(claw.status, SkillMarketInstallStatus::Installed);
        assert!(paths.user_skills_dir.join("claw-skill").is_dir());

        let skillhub = install_market_skill_with_downloader(
            &paths,
            SKILLHUB_SOURCE,
            "skillhub:owner/skills/skill-skill",
            NativeMarketSkill::SkillHub {
                owner: "owner".into(),
                slug: "skill-skill".into(),
            },
            &FakeDownloader {
                archive: make_archive(&["skill-skill"]),
                calls: calls.clone(),
            },
        )
        .await
        .unwrap();
        assert_eq!(skillhub.status, SkillMarketInstallStatus::Installed);

        let loophub = install_market_skill_with_downloader(
            &paths,
            LOOPHUB_SOURCE,
            "loophub:12277",
            NativeMarketSkill::LoopHub {
                artifact_url: validate_loophub_artifact_url("https://dl.cocoloop.cn/bss/skills/loop.zip")
                    .unwrap(),
            },
            &FakeDownloader {
                archive: make_archive(&["loop-skill"]),
                calls: calls.clone(),
            },
        )
        .await
        .unwrap();
        assert_eq!(loophub.status, SkillMarketInstallStatus::Installed);

        let reused = install_market_skill_with_downloader(
            &paths,
            CLAWHUB_SOURCE,
            "clawhub:owner/claw-skill",
            parse_install_target(&request(CLAWHUB_SOURCE, "clawhub:owner/claw-skill", None)).unwrap(),
            &downloader,
        )
        .await
        .unwrap();
        assert_eq!(reused.status, SkillMarketInstallStatus::Reused);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn rejects_multiple_skills_and_preserves_invalid_existing_directory() {
        let (_tmp, paths) = make_paths();
        tokio::fs::create_dir_all(paths.user_skills_dir.join("claw-skill"))
            .await
            .unwrap();
        tokio::fs::write(paths.user_skills_dir.join("claw-skill").join("README.md"), "invalid")
            .await
            .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let downloader = FakeDownloader {
            archive: make_archive(&["claw-skill", "another-skill"]),
            calls: calls.clone(),
        };
        let result = install_market_skill_with_downloader(
            &paths,
            CLAWHUB_SOURCE,
            "clawhub:owner/claw-skill",
            parse_install_target(&request(CLAWHUB_SOURCE, "clawhub:owner/claw-skill", None)).unwrap(),
            &downloader,
        )
        .await;
        assert_eq!(result.unwrap_err(), MarketSkillInstallError::NameConflict);
        assert!(paths.user_skills_dir.join("claw-skill").join("README.md").is_file());
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        tokio::fs::remove_dir_all(paths.user_skills_dir.join("claw-skill"))
            .await
            .unwrap();
        let result = install_market_skill_with_downloader(
            &paths,
            CLAWHUB_SOURCE,
            "clawhub:owner/claw-skill",
            parse_install_target(&request(CLAWHUB_SOURCE, "clawhub:owner/claw-skill", None)).unwrap(),
            &downloader,
        )
        .await;
        assert_eq!(result.unwrap_err(), MarketSkillInstallError::BundleUnsupported);
    }

    /// A SkillHub entry whose manifest name differs from its public slug must
    /// install under the declared name: the runtime resolves Skills by
    /// directory name, so a slug-named directory would never materialize.
    #[tokio::test]
    async fn skillhub_install_commits_under_manifest_declared_name() {
        let (_tmp, paths) = make_paths();
        let downloader = FakeDownloader {
            archive: archive_for("baozheng", "baozheng-skills"),
            calls: Arc::new(AtomicUsize::new(0)),
        };

        let response = install_market_skill_with_downloader(
            &paths,
            SKILLHUB_SOURCE,
            "skillhub:owner/skills/baozheng",
            NativeMarketSkill::SkillHub {
                owner: "owner".into(),
                slug: "baozheng".into(),
            },
            &downloader,
        )
        .await
        .unwrap();

        assert_eq!(response.status, SkillMarketInstallStatus::Installed);
        assert_eq!(response.skill_name, "baozheng-skills");
        assert!(paths.user_skills_dir.join("baozheng-skills").join("SKILL.md").is_file());
        assert!(!paths.user_skills_dir.join("baozheng").exists());
    }

    /// Reinstalling a name≠slug entry cannot hit the pre-download reuse check
    /// (the declared name is unknown until extraction), so it downloads again
    /// and is caught by the in-lock recheck: Reused, never overwritten.
    #[tokio::test]
    async fn skillhub_reinstall_with_differing_name_reuses_via_commit_recheck() {
        let (_tmp, paths) = make_paths();
        let calls = Arc::new(AtomicUsize::new(0));
        let downloader = FakeDownloader {
            archive: archive_for("baozheng", "baozheng-skills"),
            calls: calls.clone(),
        };

        let first = install_market_skill_with_downloader(
            &paths,
            SKILLHUB_SOURCE,
            "skillhub:owner/skills/baozheng",
            NativeMarketSkill::SkillHub {
                owner: "owner".into(),
                slug: "baozheng".into(),
            },
            &downloader,
        )
        .await
        .unwrap();
        assert_eq!(first.status, SkillMarketInstallStatus::Installed);

        let second = install_market_skill_with_downloader(
            &paths,
            SKILLHUB_SOURCE,
            "skillhub:owner/skills/baozheng",
            NativeMarketSkill::SkillHub {
                owner: "owner".into(),
                slug: "baozheng".into(),
            },
            &downloader,
        )
        .await
        .unwrap();
        assert_eq!(second.status, SkillMarketInstallStatus::Reused);
        // The second install paid one extra download; documented trade-off.
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(paths.user_skills_dir.join("baozheng-skills").join("SKILL.md").is_file());
    }

    #[tokio::test]
    async fn stage_rejects_zero_manifest_and_multi_manifest_bundle() {
        let (_tmp, paths) = make_paths();

        let downloader = FakeDownloader {
            archive: no_manifest_archive(),
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let result = install_market_skill_with_downloader(
            &paths,
            SKILLHUB_SOURCE,
            "skillhub:owner/skills/demo",
            NativeMarketSkill::SkillHub {
                owner: "owner".into(),
                slug: "demo".into(),
            },
            &downloader,
        )
        .await;
        assert_eq!(result.unwrap_err(), MarketSkillInstallError::ManifestInvalid);

        let downloader = FakeDownloader {
            archive: bundle_archive(),
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let result = install_market_skill_with_downloader(
            &paths,
            SKILLHUB_SOURCE,
            "skillhub:owner/skills/demo",
            NativeMarketSkill::SkillHub {
                owner: "owner".into(),
                slug: "demo".into(),
            },
            &downloader,
        )
        .await;
        assert_eq!(result.unwrap_err(), MarketSkillInstallError::BundleUnsupported);

        // Nothing was committed for either failure.
        assert!(!paths.user_skills_dir.join("bundle-alpha").exists());
        assert!(!paths.user_skills_dir.join("demo").exists());
        // The staging Drop guard reclaims the working directory (spawned
        // blocking task — allow it a moment to land).
        let staging_parent = paths.user_skills_dir.join(".market-import");
        let mut leftovers = usize::MAX;
        for _ in 0..50 {
            leftovers = std::fs::read_dir(&staging_parent)
                .map(|entries| entries.count())
                .unwrap_or(0);
            if leftovers == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(leftovers, 0, "staging directories must be cleaned up");
    }

    #[test]
    fn validate_market_skill_name_rejects_overlong_and_reserved_names() {
        let overlong = "a".repeat(MAX_MARKET_SKILL_NAME_BYTES + 1);
        for name in [
            overlong.as_str(),
            "CON",
            "nul.txt",
            "lpt1",
            "skill.",
            "skill ",
            "skill/name",
            "skill\\name",
            "companion",
            "shared",
            "_drafts",
            ".market-import",
        ] {
            assert_eq!(
                validate_market_skill_name(name).unwrap_err(),
                MarketSkillInstallError::ManifestInvalid,
                "must reject {name:?}"
            );
        }
        let at_cap = "a".repeat(MAX_MARKET_SKILL_NAME_BYTES);
        assert!(validate_market_skill_name(&at_cap).is_ok());
        assert!(validate_market_skill_name("normal-skill").is_ok());
    }

    // -----------------------------------------------------------------------
    // SkillHub exact-detail pipeline
    // -----------------------------------------------------------------------

    /// Detail payload in the live SkillHub shape. Note the documented
    /// `owner.handle` is the internal user handle (`u_internal`), NOT the
    /// ranking namespace — binding must only trust `namespace`.
    fn detail_json(owner: &str, slug: &str, version: &str) -> Vec<u8> {
        serde_json::json!({
            "namespace": {
                "canonicalName": format!("@{owner}/{slug}"),
                "handle": owner,
                "publicSlug": slug,
            },
            "latestVersion": { "version": version, "createdAt": 1, "changelog": "" },
            "skill": { "slug": slug },
            "owner": { "handle": "u_internal", "displayName": "internal" }
        })
        .to_string()
        .into_bytes()
    }

    #[test]
    fn parse_skill_detail_binds_namespace_and_version() {
        let body = String::from_utf8(detail_json("tencent-adm", "tencent-docs", "1.0.41")).unwrap();
        let detail = parse_skill_detail(&body, "tencent-adm", "tencent-docs").unwrap();
        assert_eq!(detail.slug, "tencent-docs");
        assert_eq!(detail.version, "1.0.41");

        // Case-insensitive binding.
        let detail = parse_skill_detail(&body, "Tencent-Adm", "TENCENT-DOCS").unwrap();
        assert_eq!(detail.version, "1.0.41");

        // canonicalName fallback when publicSlug is absent.
        let body = serde_json::json!({
            "namespace": { "canonicalName": "@owner/demo", "handle": "owner" },
            "latestVersion": { "version": "2.0.0" }
        })
        .to_string();
        let detail = parse_skill_detail(&body, "owner", "demo").unwrap();
        assert_eq!(detail.slug, "demo");
        assert_eq!(detail.version, "2.0.0");
    }

    #[test]
    fn parse_skill_detail_rejects_mismatch_and_missing_fields() {
        // Namespace owner mismatch → NotFound (the archive download is keyed
        // by slug alone, so a mismatched entry is uninstallable, never
        // substituted).
        let body = String::from_utf8(detail_json("other-owner", "demo", "1.0.0")).unwrap();
        assert_eq!(
            parse_skill_detail(&body, "owner", "demo").unwrap_err(),
            MarketSkillInstallError::NotFound
        );
        // The documented `owner.handle` is an internal user handle; binding
        // against it must fail (only `namespace.handle` is trusted).
        let body = String::from_utf8(detail_json("tencent-adm", "tencent-docs", "1.0.41")).unwrap();
        assert_eq!(
            parse_skill_detail(&body, "u_internal", "tencent-docs").unwrap_err(),
            MarketSkillInstallError::NotFound
        );
        // Slug mismatch → NotFound.
        assert_eq!(
            parse_skill_detail(&body, "tencent-adm", "other-skill").unwrap_err(),
            MarketSkillInstallError::NotFound
        );

        // Missing namespace / latestVersion / version → ArtifactInvalid
        // (fail closed: identity or version cannot be verified).
        for bad in [
            serde_json::json!({"latestVersion": {"version": "1.0.0"}}),
            serde_json::json!({"namespace": {"handle": "owner", "publicSlug": "demo"}}),
            serde_json::json!({
                "namespace": {"handle": "owner", "publicSlug": "demo"},
                "latestVersion": {"createdAt": 1}
            }),
            serde_json::json!({
                "namespace": {"handle": "not a handle!", "publicSlug": "demo"},
                "latestVersion": {"version": "1.0.0"}
            }),
        ] {
            let body = bad.to_string();
            assert_eq!(
                parse_skill_detail(&body, "owner", "demo").unwrap_err(),
                MarketSkillInstallError::ArtifactInvalid,
                "{body}"
            );
        }
        assert_eq!(
            parse_skill_detail("not json", "owner", "demo").unwrap_err(),
            MarketSkillInstallError::ArtifactInvalid
        );
    }

    #[test]
    fn detail_url_targets_one_validated_slug() {
        assert_eq!(
            skillhub_skill_detail_url(SKILLHUB_SKILL_DETAIL_BASE_URL, "demo")
                .unwrap()
                .as_str(),
            "https://api.skillhub.cn/api/v1/skills/demo"
        );
        // A base without the trailing slash normalizes to the same URL.
        assert_eq!(
            skillhub_skill_detail_url("https://api.skillhub.cn/api/v1/skills", "demo")
                .unwrap()
                .as_str(),
            "https://api.skillhub.cn/api/v1/skills/demo"
        );
    }

    struct StubResponse {
        status: &'static str,
        content_type: &'static str,
        extra_headers: Vec<(&'static str, &'static str)>,
        body: Vec<u8>,
        /// Announce a larger Content-Length than `body`, simulating a
        /// connection that dies mid-stream.
        announced_length: Option<usize>,
    }

    fn stub(status: &'static str, content_type: &'static str, body: Vec<u8>) -> StubResponse {
        StubResponse {
            status,
            content_type,
            extra_headers: Vec::new(),
            body,
            announced_length: None,
        }
    }

    struct RecordedSkillHub {
        base_url: String,
        requests: Arc<Mutex<Vec<String>>>,
        server: tokio::task::JoinHandle<()>,
    }

    /// Serve one stubbed response per accepted connection and record each raw
    /// request head.
    async fn spawn_recorded_fixture(responses: Vec<StubResponse>) -> RecordedSkillHub {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = requests.clone();
        let server = tokio::spawn(async move {
            for response in responses {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0_u8; 4096];
                let n = socket.read(&mut request).await.unwrap_or(0);
                log.lock().unwrap().push(String::from_utf8_lossy(&request[..n]).into_owned());
                let mut headers = format!("HTTP/1.1 {}\r\nContent-Type: {}\r\n", response.status, response.content_type);
                for (name, value) in response.extra_headers {
                    headers.push_str(&format!("{name}: {value}\r\n"));
                }
                let length = response.announced_length.unwrap_or(response.body.len());
                headers.push_str(&format!("Content-Length: {length}\r\nConnection: close\r\n\r\n"));
                socket.write_all(headers.as_bytes()).await.unwrap();
                socket.write_all(&response.body).await.unwrap();
            }
        });
        RecordedSkillHub {
            base_url: format!("http://{address}"),
            requests,
            server,
        }
    }

    async fn spawn_chunked_http_fixture(
        content_type: &'static str,
        body: Vec<u8>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 4096];
            let _ = socket.read(&mut request).await;
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n"
            );
            socket.write_all(headers.as_bytes()).await.unwrap();
            for chunk in body.chunks(64 * 1024) {
                socket
                    .write_all(format!("{:x}\r\n", chunk.len()).as_bytes())
                    .await
                    .unwrap();
                socket.write_all(chunk).await.unwrap();
                socket.write_all(b"\r\n").await.unwrap();
            }
            socket.write_all(b"0\r\n\r\n").await.unwrap();
        });
        (format!("http://{address}"), server)
    }

    fn skillhub_target(owner: &str, slug: &str) -> NativeMarketSkill {
        NativeMarketSkill::SkillHub {
            owner: owner.into(),
            slug: slug.into(),
        }
    }

    #[tokio::test]
    async fn http_skillhub_fetches_detail_then_downloads_pinned_version() {
        let archive = archive_for("demo", "demo");
        let fixture = spawn_recorded_fixture(vec![
            stub("200 OK", "application/json", detail_json("owner", "demo", "1.2.3")),
            stub("200 OK", "application/zip", archive.clone()),
        ])
        .await;
        let downloader = HttpMarketSkillDownloader::for_test(&fixture.base_url);

        let artifact = downloader
            .download(&skillhub_target("owner", "demo"))
            .await
            .unwrap();
        assert_eq!(artifact.bytes, archive);

        let requests = fixture.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].starts_with("GET /api/v1/skills/demo "), "{}", requests[0]);
        assert!(requests[1].starts_with("GET /api/v1/download?"), "{}", requests[1]);
        assert!(requests[1].contains("slug=demo"), "{}", requests[1]);
        assert!(requests[1].contains("version=1.2.3"), "{}", requests[1]);
        assert!(!requests[1].contains("owner="), "{}", requests[1]);
        drop(requests);
        fixture.server.await.unwrap();
    }

    #[tokio::test]
    async fn http_skillhub_retries_429_download_then_succeeds() {
        let archive = archive_for("demo", "demo");
        let fixture = spawn_recorded_fixture(vec![
            stub("200 OK", "application/json", detail_json("owner", "demo", "1.2.3")),
            StubResponse {
                extra_headers: vec![("Retry-After", "0")],
                ..stub("429 Too Many Requests", "application/json", b"{\"error\":\"too many requests\"}".to_vec())
            },
            stub("200 OK", "application/zip", archive.clone()),
        ])
        .await;
        let downloader = HttpMarketSkillDownloader::for_test(&fixture.base_url);

        let artifact = downloader
            .download(&skillhub_target("owner", "demo"))
            .await
            .unwrap();
        assert_eq!(artifact.bytes, archive);
        assert_eq!(fixture.requests.lock().unwrap().len(), 3);
        fixture.server.await.unwrap();
    }

    #[tokio::test]
    async fn http_skillhub_retries_died_stream_with_fresh_request() {
        let archive = archive_for("demo", "demo");
        let fixture = spawn_recorded_fixture(vec![
            stub("200 OK", "application/json", detail_json("owner", "demo", "1.2.3")),
            // Announces far more bytes than it delivers: the connection close
            // surfaces as a mid-stream error, which must be retried once with
            // a fresh request.
            StubResponse {
                announced_length: Some(archive.len() + 100_000),
                ..stub("200 OK", "application/zip", archive.clone())
            },
            stub("200 OK", "application/zip", archive.clone()),
        ])
        .await;
        let downloader = HttpMarketSkillDownloader::for_test(&fixture.base_url);

        let artifact = downloader
            .download(&skillhub_target("owner", "demo"))
            .await
            .unwrap();
        assert_eq!(artifact.bytes, archive);
        assert_eq!(fixture.requests.lock().unwrap().len(), 3);
        fixture.server.await.unwrap();
    }

    #[tokio::test]
    async fn http_skillhub_maps_download_404_to_not_found_without_retry() {
        // A publish landing between the detail lookup and the download
        // (version drift) surfaces as a download 404. It must map to NotFound
        // and must not be retried: exactly one detail + one download request.
        let fixture = spawn_recorded_fixture(vec![
            stub("200 OK", "application/json", detail_json("owner", "demo", "1.2.3")),
            stub(
                "404 Not Found",
                "application/json",
                b"{\"error\":\"version no longer available\"}".to_vec(),
            ),
        ])
        .await;
        let downloader = HttpMarketSkillDownloader::for_test(&fixture.base_url);

        let error = downloader
            .download(&skillhub_target("owner", "demo"))
            .await
            .unwrap_err();
        assert_eq!(error, MarketSkillInstallError::NotFound);
        assert_eq!(fixture.requests.lock().unwrap().len(), 2);
        fixture.server.await.unwrap();
    }

    #[tokio::test]
    async fn http_skillhub_does_not_stream_retry_an_exhausted_429_download() {
        // The send helper already retries a 429 download internally; once it
        // returns the exhausted response, the status gate's Network mapping is
        // Final and the stream loop must NOT start a second round. Extra
        // scripted 429s make a regression observable as extra requests.
        let mut responses = vec![stub(
            "200 OK",
            "application/json",
            detail_json("owner", "demo", "1.2.3"),
        )];
        for _ in 0..6 {
            responses.push(StubResponse {
                extra_headers: vec![("Retry-After", "0")],
                ..stub("429 Too Many Requests", "application/json", b"{\"error\":\"too many requests\"}".to_vec())
            });
        }
        let fixture = spawn_recorded_fixture(responses).await;
        let downloader = HttpMarketSkillDownloader::for_test(&fixture.base_url);

        let error = downloader
            .download(&skillhub_target("owner", "demo"))
            .await
            .unwrap_err();
        assert_eq!(error, MarketSkillInstallError::Network);
        // 1 detail + 3 download attempts (the send helper's attempt budget).
        assert_eq!(fixture.requests.lock().unwrap().len(), 4);
    }

    #[tokio::test]
    async fn http_skillhub_maps_detail_404_and_namespace_mismatch_to_not_found() {
        // Detail 404 → NotFound, exactly one request.
        let fixture = spawn_recorded_fixture(vec![stub(
            "404 Not Found",
            "application/json",
            b"{\"error\":\"Skill not found\"}".to_vec(),
        )])
        .await;
        let downloader = HttpMarketSkillDownloader::for_test(&fixture.base_url);
        let error = downloader
            .download(&skillhub_target("owner", "missing"))
            .await
            .unwrap_err();
        assert_eq!(error, MarketSkillInstallError::NotFound);
        assert_eq!(fixture.requests.lock().unwrap().len(), 1);
        fixture.server.await.unwrap();

        // Detail resolves to a different namespace → NotFound.
        let fixture = spawn_recorded_fixture(vec![stub(
            "200 OK",
            "application/json",
            detail_json("other-owner", "demo", "1.0.0"),
        )])
        .await;
        let downloader = HttpMarketSkillDownloader::for_test(&fixture.base_url);
        let error = downloader
            .download(&skillhub_target("owner", "demo"))
            .await
            .unwrap_err();
        assert_eq!(error, MarketSkillInstallError::NotFound);
        fixture.server.await.unwrap();
    }

    #[tokio::test]
    async fn http_skillhub_rejects_wrong_content_type_and_magic() {
        for (content_type, body) in [
            ("text/html", archive_for("demo", "demo")),
            ("application/zip", b"not zip".to_vec()),
        ] {
            let fixture = spawn_recorded_fixture(vec![
                stub("200 OK", "application/json", detail_json("owner", "demo", "1.2.3")),
                stub("200 OK", content_type, body),
            ])
            .await;
            let downloader = HttpMarketSkillDownloader::for_test(&fixture.base_url);
            let error = downloader
                .download(&skillhub_target("owner", "demo"))
                .await
                .unwrap_err();
            assert_eq!(error, MarketSkillInstallError::ArtifactInvalid);
            fixture.server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn http_skillhub_rejects_chunked_archive_over_limit() {
        let mut body = vec![0_u8; MAX_MARKET_SKILL_ARCHIVE_BYTES as usize + 1];
        body[0] = b'P';
        body[1] = b'K';
        let (base_url, server) = spawn_chunked_http_fixture("application/zip", body).await;
        let downloader = HttpMarketSkillDownloader {
            client: reqwest::Client::builder().no_proxy().build().unwrap(),
            skillhub: SkillHubEndpoints::for_test(&base_url),
        };
        let error = downloader
            .download_skillhub_archive(&RemoteSkillDetail {
                slug: "demo".into(),
                version: "1.0.0".into(),
            })
            .await
            .unwrap_err();

        assert_eq!(error, MarketSkillInstallError::ArtifactInvalid);
        server.await.unwrap();
    }
}
