//! Native installation for ordinary Skills from the supported markets.
//!
//! This module deliberately owns only the market-to-archive adapters. The
//! filesystem transaction remains in [`crate::skill_service`], so expert
//! packages and ordinary Skills share the same validation and commit rules.

use std::io;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nomifun_api_types::{
    SkillCatalogSource, SkillHubMarketContentSource, SkillId, SkillMarketInstallStatus,
    SkillMarketSkillInstallRequest, SkillMarketSkillInstallResponse,
};
use nomifun_common::AppError;
use reqwest::Url;
use reqwest::header::CONTENT_TYPE;

use crate::error::ExtensionError;
use crate::skill_service::{self, SkillPaths};

use super::client::{
    MARKET_REQUEST_TIMEOUT, MAX_MARKET_SKILL_ARCHIVE_BYTES, SKILLHUB_DOWNLOAD_TIMEOUT,
    build_market_client, read_market_response,
    send_skillhub_get_with_retry,
};
use super::parse::{is_market_slug, json_text};
use super::staging::{MarketStaging, create_market_staging};

const SKILLHUB_SOURCE: &str = "skillhub";
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

fn now_epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

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
    SkillHub { owner: String, slug: String },
}

#[derive(Debug)]
struct DownloadedArtifact {
    bytes: Vec<u8>,
    version: Option<String>,
}

#[async_trait::async_trait]
trait MarketSkillDownloader: Send + Sync {
    async fn prepare(
        &self,
        target: &NativeMarketSkill,
    ) -> Result<RemoteSkillDetail, MarketSkillInstallError>;

    async fn download(
        &self,
        target: &NativeMarketSkill,
        detail: &RemoteSkillDetail,
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
    async fn prepare(
        &self,
        target: &NativeMarketSkill,
    ) -> Result<RemoteSkillDetail, MarketSkillInstallError> {
        match target {
            NativeMarketSkill::SkillHub { owner, slug } => {
                self.fetch_skillhub_detail(owner, slug).await
            }
        }
    }

    async fn download(
        &self,
        _target: &NativeMarketSkill,
        detail: &RemoteSkillDetail,
    ) -> Result<DownloadedArtifact, MarketSkillInstallError> {
        let bytes = self.download_skillhub_archive(detail).await?;
        Ok(DownloadedArtifact {
            bytes,
            version: Some(detail.version.clone()),
        })
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
    #[cfg(test)]
    async fn download(
        &self,
        target: &NativeMarketSkill,
    ) -> Result<DownloadedArtifact, MarketSkillInstallError> {
        let detail = <Self as MarketSkillDownloader>::prepare(self, target).await?;
        <Self as MarketSkillDownloader>::download(self, target, &detail).await
    }

    /// Exact detail lookup for one market entry. Verifies the documented
    /// `owner.handle` and `skill.slug` before returning the pinned version to
    /// download. The public namespace handle is authoritative when the API
    /// returns one; the account owner is only the fallback for entries without
    /// a namespace.
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
/// The official detail contract identifies the account with `owner.handle` and
/// the Skill with `skill.slug`. When `namespace.handle` is present it is the
/// public identity and must agree with the requested owner. Without a
/// namespace, `owner.handle` is used as the public identity. Missing or
/// conflicting identity fields fail closed as `NotFound`, so a stale list entry
/// cannot be substituted by another Skill.
fn parse_skill_detail(
    body: &str,
    expected_owner: &str,
    expected_slug: &str,
) -> Result<RemoteSkillDetail, MarketSkillInstallError> {
    let root = serde_json::from_str::<serde_json::Value>(body)
        .map_err(|_| MarketSkillInstallError::ArtifactInvalid)?;
    if root.get("code").is_some_and(|code| {
        !(code.as_i64().is_some_and(|value| value == 0)
            || code.as_str().is_some_and(|value| value == "0"))
    }) {
        return Err(MarketSkillInstallError::Network);
    }
    let root = root.get("data").unwrap_or(&root);
    let skill = root
        .get("skill")
        .ok_or(MarketSkillInstallError::NotFound)?;
    let resolved_slug = json_text(skill, "slug", 96).ok_or(MarketSkillInstallError::NotFound)?;
    if !is_market_slug(&resolved_slug)
        || !resolved_slug.eq_ignore_ascii_case(expected_slug)
    {
        return Err(MarketSkillInstallError::NotFound);
    }
    let owner = root
        .get("owner")
        .and_then(|value| json_text(value, "handle", 96))
        .ok_or(MarketSkillInstallError::NotFound)?;
    let public_owner = match root.get("namespace") {
        None | Some(serde_json::Value::Null) => owner.clone(),
        Some(namespace) => json_text(namespace, "handle", 96).ok_or(MarketSkillInstallError::NotFound)?,
    };
    if !is_market_slug(&owner)
        || !is_market_slug(&public_owner)
        || !public_owner.eq_ignore_ascii_case(expected_owner)
    {
        return Err(MarketSkillInstallError::NotFound);
    }
    let version = root
        .get("latestVersion")
        .and_then(|value| json_text(value, "version", 64))
        .ok_or(MarketSkillInstallError::ArtifactInvalid)?;
    Ok(RemoteSkillDetail {
        slug: resolved_slug,
        version,
    })
}

fn download_retry_backoff(stream_attempt: u32) -> Duration {
    Duration::from_millis(500 * u64::from(stream_attempt))
}

/// Install one ordinary Skill without starting OpenClaw or another external
/// CLI. The backend derives the SkillHub detail and download URLs from the
/// validated canonical id.
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
    install_market_skill_with_downloader_and_source(
        paths,
        &req.source,
        target,
        req.market_source
            .map(market_source_name)
            .unwrap_or("unknown"),
        &downloader,
    )
    .await
}

#[cfg(test)]
async fn install_market_skill_with_downloader<D: MarketSkillDownloader>(
    paths: &SkillPaths,
    source: &str,
    target: NativeMarketSkill,
    downloader: &D,
) -> Result<SkillMarketSkillInstallResponse, MarketSkillInstallError> {
    install_market_skill_with_downloader_and_source(paths, source, target, "unknown", downloader).await
}

async fn install_market_skill_with_downloader_and_source<D: MarketSkillDownloader>(
    paths: &SkillPaths,
    source: &str,
    target: NativeMarketSkill,
    market_source: &str,
    downloader: &D,
) -> Result<SkillMarketSkillInstallResponse, MarketSkillInstallError> {
    // The lock covers the complete idempotent transaction. In particular,
    // mapping lookup happens before download so two concurrent requests for a
    // SkillHub entry cannot both fetch the same archive.
    let _commit_guard = super::market_commit_lock().lock().await;
    let detail = downloader.prepare(&target).await?;
    let market_id = market_skill_id(&target);
    let mut mappings = skill_service::load_market_skill_mappings(paths).await;
    if let Some(mapping) = mappings.get(&market_id) {
        let Some(skill_name) = mapped_skill_name(mapping, &target)? else {
            return Err(MarketSkillInstallError::NameConflict);
        };
        if reuse_existing_skill(paths, &skill_name).await?.is_some() {
            return Ok(install_response(
                source,
                &skill_name,
                SkillMarketInstallStatus::Reused,
            ));
        }
    }

    let artifact = downloader.download(&target, &detail).await?;
    if artifact.bytes.len() as u64 > MAX_MARKET_SKILL_ARCHIVE_BYTES {
        return Err(MarketSkillInstallError::ArtifactInvalid);
    }

    let staging = create_market_staging(paths, "skill")
        .await
        .map_err(|_| MarketSkillInstallError::LocalIo)?;
    let artifact_version = artifact.version.clone();
    let (staged_dir, skill_name) = stage_market_skill(&staging, artifact).await?;

    if reuse_existing_skill(paths, &skill_name).await?.is_some() {
        let mapping = build_market_skill_mapping(&target, &skill_name, artifact_version, market_source);
        mappings.insert(market_id, mapping);
        skill_service::save_market_skill_mappings(paths, &mappings)
            .await
            .map_err(|_| MarketSkillInstallError::LocalIo)?;
        return Ok(install_response(
            source,
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
    let mapping = build_market_skill_mapping(&target, &skill_name, artifact_version, market_source);
    mappings.insert(market_id, mapping);
    skill_service::save_market_skill_mappings(paths, &mappings)
        .await
        .map_err(|_| MarketSkillInstallError::LocalIo)?;
    Ok(install_response(source, &skill_name, status))
}

fn market_skill_id(target: &NativeMarketSkill) -> String {
    match target {
        NativeMarketSkill::SkillHub { owner, slug } => {
            format!("skillhub:{owner}/skills/{slug}")
        }
    }
}

fn mapped_skill_name(
    mapping: &skill_service::MarketSkillMapping,
    target: &NativeMarketSkill,
) -> Result<Option<String>, MarketSkillInstallError> {
    let (expected_owner, expected_slug) = match target {
        NativeMarketSkill::SkillHub { owner, slug } => (owner, slug),
    };
    if mapping.source != SKILLHUB_SOURCE
        || mapping.owner != *expected_owner
        || mapping.slug != *expected_slug
    {
        return Err(MarketSkillInstallError::NameConflict);
    }
    let parsed_skill_id = SkillId::parse(&mapping.installed_skill_id)
        .map_err(|_| MarketSkillInstallError::NameConflict)?;
    if parsed_skill_id.source() != SkillCatalogSource::User
        || mapping.installed_skill_id.matches(':').count() != 1
    {
        return Err(MarketSkillInstallError::NameConflict);
    }
    let Some(skill_name) = parsed_skill_id.local_key() else {
        return Err(MarketSkillInstallError::NameConflict);
    };
    validate_market_skill_name(&skill_name)?;
    Ok(Some(skill_name))
}

fn build_market_skill_mapping(
    target: &NativeMarketSkill,
    skill_name: &str,
    version: Option<String>,
    market_source: &str,
) -> skill_service::MarketSkillMapping {
    let (owner, slug) = match target {
        NativeMarketSkill::SkillHub { owner, slug } => (owner, slug),
    };
    skill_service::MarketSkillMapping {
        source: SKILLHUB_SOURCE.to_owned(),
        market_source: market_source.to_owned(),
        owner: owner.clone(),
        slug: slug.clone(),
        installed_skill_id: SkillId::new(SkillCatalogSource::User, None, skill_name)
            .as_str()
            .to_owned(),
        version,
        installed_at: now_epoch_ms(),
    }
}

fn market_source_name(source: SkillHubMarketContentSource) -> &'static str {
    match source {
        SkillHubMarketContentSource::Skillhub => "skillhub",
        SkillHubMarketContentSource::Clawhub => "clawhub",
        SkillHubMarketContentSource::Unknown => "unknown",
    }
}

fn install_response(
    source: &str,
    skill_name: &str,
    status: SkillMarketInstallStatus,
) -> SkillMarketSkillInstallResponse {
    SkillMarketSkillInstallResponse {
        source: source.to_owned(),
        skill_id: SkillId::new(SkillCatalogSource::User, None, skill_name)
            .as_str()
            .to_owned(),
        skill_name: skill_name.to_owned(),
        status,
    }
}

fn parse_install_target(
    req: &SkillMarketSkillInstallRequest,
) -> Result<NativeMarketSkill, MarketSkillInstallError> {
    match req.source.as_str() {
        SKILLHUB_SOURCE => {
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
        _ => Err(MarketSkillInstallError::SourceUnsupported),
    }
}

fn is_zip_bytes(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") || bytes.starts_with(b"PK\x07\x08")
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
    let skill_name = skill_service::validate_market_skill_directory_name(&skill_dir)
        .await
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
        async fn prepare(
            &self,
            target: &NativeMarketSkill,
        ) -> Result<RemoteSkillDetail, MarketSkillInstallError> {
            let NativeMarketSkill::SkillHub { slug, .. } = target;
            Ok(RemoteSkillDetail {
                slug: slug.clone(),
                version: "test-version".into(),
            })
        }

        async fn download(
            &self,
            target: &NativeMarketSkill,
            _detail: &RemoteSkillDetail,
        ) -> Result<DownloadedArtifact, MarketSkillInstallError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert!(matches!(target, NativeMarketSkill::SkillHub { .. }));
            Ok(DownloadedArtifact {
                bytes: self.archive.clone(),
                version: None,
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

    fn request(source: &str, id: &str) -> SkillMarketSkillInstallRequest {
        SkillMarketSkillInstallRequest {
            source: source.into(),
            id: id.into(),
            market_source: None,
        }
    }

    #[test]
    fn parses_only_canonical_skillhub_ids_and_rejects_external_sources() {
        match parse_install_target(&request(SKILLHUB_SOURCE, "skillhub:owner/skills/skill-skill"))
            .unwrap()
        {
            NativeMarketSkill::SkillHub { owner, slug } => {
                assert_eq!(owner, "owner");
                assert_eq!(slug, "skill-skill");
            }
        }
        assert_eq!(
            parse_install_target(&request("mcpworld", "mcpworld:x")).unwrap_err(),
            MarketSkillInstallError::SourceUnsupported
        );
        assert_eq!(
            parse_install_target(&request(SKILLHUB_SOURCE, "skillhub:owner/skills/../x"))
                .unwrap_err(),
            MarketSkillInstallError::IdInvalid
        );
        assert_eq!(
            parse_install_target(&request(SKILLHUB_SOURCE, "skillhub:owner/other/skill-skill"))
                .unwrap_err(),
            MarketSkillInstallError::IdInvalid
        );
    }

    #[test]
    fn rejects_external_sources_and_keeps_the_install_request_contract_strict() {
        assert_eq!(
            parse_install_target(&request("clawhub", "clawhub:owner/demo")).unwrap_err(),
            MarketSkillInstallError::SourceUnsupported
        );
    }

    #[tokio::test]
    async fn installs_skillhub_and_reuses_valid_existing_skill() {
        let (_tmp, paths) = make_paths();
        let calls = Arc::new(AtomicUsize::new(0));
        let downloader = FakeDownloader {
            archive: make_archive(&["skill-skill"]),
            calls: calls.clone(),
        };

        let installed = install_market_skill_with_downloader(
            &paths,
            SKILLHUB_SOURCE,
            NativeMarketSkill::SkillHub {
                owner: "owner".into(),
                slug: "skill-skill".into(),
            },
            &downloader,
        )
        .await
        .unwrap();
        assert_eq!(installed.status, SkillMarketInstallStatus::Installed);
        assert_eq!(installed.skill_id, "user:skill-skill");
        assert!(paths.user_skills_dir.join("skill-skill").is_dir());

        let reused = install_market_skill_with_downloader(
            &paths,
            SKILLHUB_SOURCE,
            NativeMarketSkill::SkillHub {
                owner: "owner".into(),
                slug: "skill-skill".into(),
            },
            &downloader,
        )
        .await
        .unwrap();
        assert_eq!(reused.status, SkillMarketInstallStatus::Reused);
        assert_eq!(reused.skill_id, "user:skill-skill");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(skill_service::market_skill_mappings_path(&paths).is_file());

        let second_downloader = FakeDownloader {
            archive: make_archive(&["second-skill"]),
            calls: calls.clone(),
        };
        let second_skill = install_market_skill_with_downloader(
            &paths,
            SKILLHUB_SOURCE,
            NativeMarketSkill::SkillHub {
                owner: "owner".into(),
                slug: "second".into(),
            },
            &second_downloader,
        )
        .await
        .unwrap();
        assert_eq!(second_skill.status, SkillMarketInstallStatus::Installed);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn strict_market_identity_does_not_guess_legacy_local_slug() {
        let (_tmp, paths) = make_paths();
        let legacy_dir = paths.user_skills_dir.join("skill-skill");
        tokio::fs::create_dir_all(&legacy_dir).await.unwrap();
        tokio::fs::write(
            legacy_dir.join("SKILL.md"),
            "---\nname: skill-skill\ndescription: Existing local skill\n---\n",
        )
        .await
        .unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        let downloader = FakeDownloader {
            archive: make_archive(&["skill-skill"]),
            calls: calls.clone(),
        };
        let response = install_market_skill_with_downloader(
            &paths,
            SKILLHUB_SOURCE,
            NativeMarketSkill::SkillHub {
                owner: "new-owner".into(),
                slug: "skill-skill".into(),
            },
            &downloader,
        )
        .await
        .unwrap();

        assert_eq!(response.status, SkillMarketInstallStatus::Reused);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn rejects_multiple_skills_and_preserves_invalid_existing_directory() {
        let (_tmp, paths) = make_paths();
        tokio::fs::create_dir_all(paths.user_skills_dir.join("skill-skill"))
            .await
            .unwrap();
        tokio::fs::write(paths.user_skills_dir.join("skill-skill").join("README.md"), "invalid")
            .await
            .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let downloader = FakeDownloader {
            archive: make_archive(&["skill-skill"]),
            calls: calls.clone(),
        };
        let result = install_market_skill_with_downloader(
            &paths,
            SKILLHUB_SOURCE,
            NativeMarketSkill::SkillHub {
                owner: "owner".into(),
                slug: "skill-skill".into(),
            },
            &downloader,
        )
        .await;
        assert_eq!(result.unwrap_err(), MarketSkillInstallError::NameConflict);
        assert!(paths.user_skills_dir.join("skill-skill").join("README.md").is_file());
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        tokio::fs::remove_dir_all(paths.user_skills_dir.join("skill-skill"))
            .await
            .unwrap();
        let result = install_market_skill_with_downloader(
            &paths,
            SKILLHUB_SOURCE,
            NativeMarketSkill::SkillHub {
                owner: "owner".into(),
                slug: "skill-skill".into(),
            },
            &FakeDownloader {
                archive: make_archive(&["skill-skill", "another-skill"]),
                calls: calls.clone(),
            },
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

    /// Reinstalling a name≠slug entry uses the persisted market mapping, so it
    /// reuses the declared-name directory without downloading again.
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
            NativeMarketSkill::SkillHub {
                owner: "owner".into(),
                slug: "baozheng".into(),
            },
            &downloader,
        )
        .await
        .unwrap();
        assert_eq!(second.status, SkillMarketInstallStatus::Reused);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
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

    /// Detail payload in the documented SkillHub shape. The namespace is the
    /// public identity when present, while owner is the account identity.
    fn detail_json(owner: &str, namespace: Option<&str>, slug: &str, version: &str) -> Vec<u8> {
        let namespace = namespace.map(|handle| serde_json::json!({"handle": handle}));
        serde_json::json!({
            "latestVersion": { "version": version, "createdAt": 1, "changelog": "" },
            "skill": { "slug": slug },
            "owner": { "handle": owner, "displayName": "owner" },
            "namespace": namespace
        })
        .to_string()
        .into_bytes()
    }

    #[test]
    fn parse_skill_detail_binds_official_owner_and_version() {
        let body = String::from_utf8(detail_json("tencent-adm", Some("tencent-adm"), "tencent-docs", "1.0.41")).unwrap();
        let detail = parse_skill_detail(&body, "tencent-adm", "tencent-docs").unwrap();
        assert_eq!(detail.slug, "tencent-docs");
        assert_eq!(detail.version, "1.0.41");

        let body = String::from_utf8(detail_json("u_d95b6787", Some("tencent-adm"), "agently-mail", "1.0.13")).unwrap();
        let detail = parse_skill_detail(&body, "tencent-adm", "agently-mail").unwrap();
        assert_eq!(detail.slug, "agently-mail");
        assert_eq!(detail.version, "1.0.13");

        // Case-insensitive binding.
        let body = String::from_utf8(detail_json("tencent-adm", Some("tencent-adm"), "tencent-docs", "1.0.41")).unwrap();
        let detail = parse_skill_detail(&body, "Tencent-Adm", "TENCENT-DOCS").unwrap();
        assert_eq!(detail.version, "1.0.41");

        // The observed namespace assertion is optional when the official
        // owner and skill fields are present.
        let body = serde_json::json!({
            "skill": { "slug": "demo" },
            "latestVersion": { "version": "2.0.0" },
            "owner": { "handle": "owner" }
        })
        .to_string();
        let detail = parse_skill_detail(&body, "owner", "demo").unwrap();
        assert_eq!(detail.slug, "demo");
        assert_eq!(detail.version, "2.0.0");
    }

    #[test]
    fn parse_skill_detail_rejects_mismatch_and_missing_fields() {
        // Official owner mismatch → NotFound (the archive download is keyed
        // by slug alone, so a mismatched entry is uninstallable, never
        // substituted).
        let body = String::from_utf8(detail_json("other-owner", Some("other-owner"), "demo", "1.0.0")).unwrap();
        assert_eq!(
            parse_skill_detail(&body, "owner", "demo").unwrap_err(),
            MarketSkillInstallError::NotFound
        );
        // The optional observed namespace assertion must also agree.
        let body = serde_json::json!({
            "skill": { "slug": "tencent-docs" },
            "latestVersion": { "version": "1.0.41" },
            "owner": { "handle": "tencent-adm" },
            "namespace": { "handle": "other-owner" }
        })
        .to_string();
        assert_eq!(
            parse_skill_detail(&body, "tencent-adm", "tencent-docs").unwrap_err(),
            MarketSkillInstallError::NotFound
        );
        // Slug mismatch → NotFound.
        assert_eq!(
            parse_skill_detail(&body, "tencent-adm", "other-skill").unwrap_err(),
            MarketSkillInstallError::NotFound
        );

        // Missing identity fields are not found; missing version is an
        // invalid detail payload because the download cannot be pinned.
        for bad in [
            serde_json::json!({"latestVersion": {"version": "1.0.0"}}),
            serde_json::json!({"skill": {"slug": "demo"}, "latestVersion": {"version": "1.0.0"}}),
        ] {
            assert_eq!(
                parse_skill_detail(&bad.to_string(), "owner", "demo").unwrap_err(),
                MarketSkillInstallError::NotFound,
                "{bad}"
            );
        }
        assert_eq!(
            parse_skill_detail(
                &serde_json::json!({
                    "skill": {"slug": "demo"},
                    "owner": {"handle": "owner"},
                    "latestVersion": {"createdAt": 1}
                })
                .to_string(),
                "owner",
                "demo"
            )
            .unwrap_err(),
            MarketSkillInstallError::ArtifactInvalid
        );
        assert_eq!(
            parse_skill_detail(
                &serde_json::json!({
                    "skill": {"slug": "demo"},
                    "owner": {"handle": "not a handle!"},
                    "latestVersion": {"version": "1.0.0"}
                })
                .to_string(),
                "owner",
                "demo"
            )
            .unwrap_err(),
            MarketSkillInstallError::NotFound
        );
        assert_eq!(
            parse_skill_detail(
                &serde_json::json!({
                    "code": 17,
                    "skill": {"slug": "demo"},
                    "owner": {"handle": "owner"},
                    "latestVersion": {"version": "1.0.0"}
                })
                .to_string(),
                "owner",
                "demo"
            )
            .unwrap_err(),
            MarketSkillInstallError::Network
        );
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
            stub("200 OK", "application/json", detail_json("owner", Some("owner"), "demo", "1.2.3")),
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
            stub("200 OK", "application/json", detail_json("owner", Some("owner"), "demo", "1.2.3")),
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
            stub("200 OK", "application/json", detail_json("owner", Some("owner"), "demo", "1.2.3")),
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
            stub("200 OK", "application/json", detail_json("owner", Some("owner"), "demo", "1.2.3")),
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
            detail_json("owner", Some("owner"), "demo", "1.2.3"),
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
            detail_json("other-owner", Some("other-owner"), "demo", "1.0.0"),
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
                stub("200 OK", "application/json", detail_json("owner", Some("owner"), "demo", "1.2.3")),
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
