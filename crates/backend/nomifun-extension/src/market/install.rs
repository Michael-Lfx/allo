//! Product-owned installation of one SkillHub Skill.
//!
//! The route layer only sees [`MarketSkillInstaller::install`] and
//! [`MarketSkillInstaller::list_installations`]. URL construction, source
//! revalidation, streaming, archive extraction, deterministic hashing,
//! provenance, staging cleanup, and the commit/rollback protocol stay here.

use std::fs::{self, Metadata, symlink_metadata};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use nomifun_api_types::{
    SkillMarketInstallRequest, SkillMarketInstallResponse, SkillMarketInstallStatus,
    SkillMarketInstallationResponse,
};
use nomifun_common::{AppError, dir_config::write_atomic_replace};
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::error::ExtensionError;
use crate::constants::SKILL_MANIFEST_FILE;
use crate::skill_service::{self, MARKET_IMPORT_SCAN_DEPTH, SkillPaths};

use super::client::{
    MAX_SKILLHUB_SKILL_ZIP_BYTES, build_skillhub_install_client, read_market_detail_body,
};
use super::parse::{is_market_slug, json_text};

const SKILLHUB_SOURCE: &str = "skillhub";
const SKILLHUB_SKILL_DOWNLOAD_URL: &str = "https://api.skillhub.cn/api/v1/download";
const SKILLHUB_SKILL_SEARCH_URL: &str = "https://api.skillhub.cn/api/v1/search";
const INSTALLATIONS_DIR: &str = "skill-market/installations";
const MARKET_STAGING_DIR: &str = ".market-import";
const IMPORT_STAGING_DIR: &str = ".import-tmp";
const SKILL_STAGING_PREFIX: &str = "skill-";
const PACKAGE_STAGING_PREFIX: &str = "package-";
const IMPORT_STAGING_PREFIX: &str = "skills-";
const STAGING_RETENTION: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_INSTALLATION_RECORD_BYTES: usize = 64 * 1024;

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
    #[error("the Skill market request failed")]
    Network,
    #[error("the Skill market request timed out")]
    Timeout,
    #[error("the local Skill installation failed")]
    LocalIo,
}

/// Deep module seam used by the HTTP implementation and unit-test fakes.
/// Consumers of the installer do not receive a URL or archive body.
#[async_trait]
trait SkillHubArtifactSource: Send + Sync {
    async fn find_skill(&self, owner: &str, slug: &str) -> Result<RemoteSkill, MarketSkillInstallError>;
    async fn download_skill(
        &self,
        skill: &RemoteSkill,
        archive_path: &Path,
    ) -> Result<String, MarketSkillInstallError>;
}

#[derive(Debug, Clone)]
struct RemoteSkill {
    owner: String,
    slug: String,
    revision: Option<String>,
}

struct HttpSkillHubArtifactSource {
    client: reqwest::Client,
    search_url: String,
    download_url: String,
}

impl HttpSkillHubArtifactSource {
    fn new() -> Result<Self, MarketSkillInstallError> {
        build_skillhub_install_client()
            .map(|client| Self {
                client,
                search_url: SKILLHUB_SKILL_SEARCH_URL.into(),
                download_url: SKILLHUB_SKILL_DOWNLOAD_URL.into(),
            })
            .map_err(|_| MarketSkillInstallError::LocalIo)
    }

    #[cfg(test)]
    fn for_test(base_url: &str) -> Self {
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("test SkillHub client should build");
        Self {
            client,
            search_url: format!("{base_url}/api/v1/search"),
            download_url: format!("{base_url}/api/v1/download"),
        }
    }
}

#[async_trait]
impl SkillHubArtifactSource for HttpSkillHubArtifactSource {
    async fn find_skill(&self, owner: &str, slug: &str) -> Result<RemoteSkill, MarketSkillInstallError> {
        let query = format!("{owner}/{slug}");
        let url = reqwest::Url::parse_with_params(
            &self.search_url,
            [("q", query.as_str()), ("limit", "20")],
        )
        .map_err(|_| MarketSkillInstallError::LocalIo)?;
        let body = read_market_detail_body(&self.client, url.as_str(), "SkillHub search")
            .await
            .map_err(map_app_market_error)?;
        let root = serde_json::from_str::<serde_json::Value>(&body)
            .map_err(|_| MarketSkillInstallError::ArtifactInvalid)?;
        find_exact_search_result(&root, owner, slug).ok_or(MarketSkillInstallError::NotFound)
    }

    async fn download_skill(
        &self,
        skill: &RemoteSkill,
        archive_path: &Path,
    ) -> Result<String, MarketSkillInstallError> {
        // Keep the owner bound all the way through the artifact request. The
        // search response is not sufficient when two namespaces publish the
        // same slug.
        let url = skillhub_download_url(&self.download_url, skill)?;
        let mut response = self
            .client
            .get(url)
            .header(ACCEPT, "application/zip,application/octet-stream")
            .send()
            .await
            .map_err(map_reqwest_market_error)?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(MarketSkillInstallError::NotFound);
        }
        if !response.status().is_success() {
            return Err(MarketSkillInstallError::Network);
        }

        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.split(';').next().unwrap_or_default().trim().to_ascii_lowercase());
        if !matches!(content_type.as_deref(), Some("application/zip" | "application/octet-stream")) {
            return Err(MarketSkillInstallError::ArtifactInvalid);
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_SKILLHUB_SKILL_ZIP_BYTES)
        {
            return Err(MarketSkillInstallError::ArtifactInvalid);
        }

        let mut archive = tokio::fs::File::create(archive_path)
            .await
            .map_err(|_| MarketSkillInstallError::LocalIo)?;
        let mut hasher = Sha256::new();
        let mut total = 0_u64;
        let mut magic = [0_u8; 2];
        let mut magic_len = 0_usize;

        while let Some(chunk) = response.chunk().await.map_err(map_reqwest_market_error)? {
            let chunk_len = u64::try_from(chunk.len()).unwrap_or(u64::MAX);
            if total.saturating_add(chunk_len) > MAX_SKILLHUB_SKILL_ZIP_BYTES {
                return Err(MarketSkillInstallError::ArtifactInvalid);
            }
            let copy_len = (magic.len() - magic_len).min(chunk.len());
            magic[magic_len..magic_len + copy_len].copy_from_slice(&chunk[..copy_len]);
            magic_len += copy_len;
            hasher.update(&chunk);
            archive
                .write_all(&chunk)
                .await
                .map_err(|_| MarketSkillInstallError::LocalIo)?;
            total += chunk_len;
        }
        archive
            .flush()
            .await
            .map_err(|_| MarketSkillInstallError::LocalIo)?;

        if magic_len < magic.len() || magic != [b'P', b'K'] {
            return Err(MarketSkillInstallError::ArtifactInvalid);
        }
        Ok(format!("{:x}", hasher.finalize()))
    }
}

fn skillhub_download_url(
    base_url: &str,
    skill: &RemoteSkill,
) -> Result<reqwest::Url, MarketSkillInstallError> {
    reqwest::Url::parse_with_params(
        base_url,
        [
            ("owner", skill.owner.as_str()),
            ("slug", skill.slug.as_str()),
        ],
    )
    .map_err(|_| MarketSkillInstallError::LocalIo)
}

/// Application-facing installation seam. Route and UI code only depend on
/// this small contract, never on SkillHub transport or filesystem details.
#[async_trait]
pub trait MarketSkillInstaller: Send + Sync {
    async fn install(
        &self,
        request: SkillMarketInstallRequest,
    ) -> Result<SkillMarketInstallResponse, MarketSkillInstallError>;

    async fn list_installations(
        &self,
    ) -> Result<Vec<SkillMarketInstallationResponse>, MarketSkillInstallError>;
}

/// Product-owned SkillHub installer.
#[derive(Clone)]
pub struct ManagedSkillInstaller {
    paths: SkillPaths,
}

impl ManagedSkillInstaller {
    pub fn new(paths: SkillPaths) -> Self {
        Self { paths }
    }

    async fn install_with_source(
        &self,
        request: SkillMarketInstallRequest,
        source: Arc<dyn SkillHubArtifactSource>,
    ) -> Result<SkillMarketInstallResponse, MarketSkillInstallError> {
        let market_skill = parse_market_skill_id(&request.source, &request.id)?;
        let remote = source.find_skill(&market_skill.owner, &market_skill.slug).await?;
        if !remote.owner.eq_ignore_ascii_case(&market_skill.owner)
            || !remote.slug.eq_ignore_ascii_case(&market_skill.slug)
        {
            return Err(MarketSkillInstallError::ArtifactInvalid);
        }

        let staging = MarketSkillStaging::create(&self.paths).await?;
        let result = async {
            let archive_path = staging.root.join("skill.zip");
            let extract_dir = staging.root.join("extract");
            let artifact_sha256 = source.download_skill(&remote, &archive_path).await?;

            skill_service::extract_skill_archive_to_staging(
                &archive_path,
                &extract_dir,
                staging.cleanup_handle.clone(),
            )
                .await
                .map_err(map_archive_error)?;
            let skill_dir = find_single_skill_directory(&extract_dir)?;
            skill_service::validate_market_skill_directory(&skill_dir, &market_skill.slug)
                .await
                .map_err(|_| MarketSkillInstallError::ManifestInvalid)?;
            validate_managed_skill_name(&market_skill.slug)?;
            let content_sha256 = hash_skill_directory(&skill_dir).await?;

            let _mutation_guard = skill_service::skill_mutation_lock().lock().await;
            commit_managed_skill(
                &self.paths,
                &market_skill,
                remote.revision,
                &skill_dir,
                artifact_sha256,
                content_sha256,
            )
            .await
        }
        .await;
        let cleanup = staging.cleanup().await;
        match (result, cleanup) {
            (Ok(response), Ok(())) => Ok(response),
            (Err(error), Ok(())) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Err(error), Err(cleanup_error)) => {
                tracing::warn!(error = %cleanup_error, "SkillHub staging cleanup failed after installation error");
                Err(error)
            }
        }
    }
}

#[async_trait]
impl MarketSkillInstaller for ManagedSkillInstaller {
    async fn install(
        &self,
        request: SkillMarketInstallRequest,
    ) -> Result<SkillMarketInstallResponse, MarketSkillInstallError> {
        let source = Arc::new(HttpSkillHubArtifactSource::new()?) as Arc<dyn SkillHubArtifactSource>;
        self.install_with_source(request, source).await
    }

    async fn list_installations(
        &self,
    ) -> Result<Vec<SkillMarketInstallationResponse>, MarketSkillInstallError> {
        list_valid_installations(&self.paths).await
    }
}

#[derive(Debug, Clone)]
struct MarketSkillId {
    owner: String,
    slug: String,
    market_id: String,
}

fn parse_market_skill_id(source: &str, id: &str) -> Result<MarketSkillId, MarketSkillInstallError> {
    if source != SKILLHUB_SOURCE {
        return Err(MarketSkillInstallError::SourceUnsupported);
    }
    let Some(suffix) = id.strip_prefix("skillhub:") else {
        return Err(MarketSkillInstallError::IdInvalid);
    };
    let mut parts = suffix.split('/');
    let (Some(owner), Some(skills_segment), Some(slug), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(MarketSkillInstallError::IdInvalid);
    };
    if skills_segment != "skills"
        || !is_market_slug(owner)
        || !is_market_slug(slug)
        || id != format!("skillhub:{owner}/skills/{slug}")
    {
        return Err(MarketSkillInstallError::IdInvalid);
    }
    validate_managed_skill_name(slug)?;
    Ok(MarketSkillId {
        owner: owner.to_string(),
        slug: slug.to_string(),
        market_id: id.to_string(),
    })
}

fn validate_managed_skill_name(name: &str) -> Result<(), MarketSkillInstallError> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.ends_with('.')
        || name.ends_with(' ')
        || name.chars().any(char::is_control)
        || name.contains('/')
        || name.contains('\\')
        || name.contains(':')
        || ["companion", "shared", "_drafts", MARKET_STAGING_DIR, IMPORT_STAGING_DIR]
            .iter()
            .any(|reserved| name.eq_ignore_ascii_case(reserved))
        || is_windows_device_name(name)
    {
        return Err(MarketSkillInstallError::IdInvalid);
    }
    Ok(())
}

fn is_windows_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0')
}

fn find_exact_search_result(root: &serde_json::Value, owner: &str, slug: &str) -> Option<RemoteSkill> {
    let mut stack = vec![(root, 0_u8)];
    while let Some((value, depth)) = stack.pop() {
        if depth > 16 {
            continue;
        }
        if let Some((candidate_owner, candidate_slug)) = search_item_owner_slug(value)
            && candidate_owner.eq_ignore_ascii_case(owner)
            && candidate_slug.eq_ignore_ascii_case(slug)
        {
            let revision = json_text(value, "revision", 128)
                .or_else(|| json_text(value, "version", 128))
                .or_else(|| json_text(value, "updatedAt", 128));
            return Some(RemoteSkill {
                owner: candidate_owner,
                slug: candidate_slug,
                revision,
            });
        }
        match value {
            serde_json::Value::Array(items) => {
                stack.extend(items.iter().map(|item| (item, depth + 1)));
            }
            serde_json::Value::Object(object) => {
                stack.extend(object.values().map(|item| (item, depth + 1)));
            }
            serde_json::Value::Null
            | serde_json::Value::Bool(_)
            | serde_json::Value::Number(_)
            | serde_json::Value::String(_) => {}
        }
    }
    None
}

fn search_item_owner_slug(item: &serde_json::Value) -> Option<(String, String)> {
    let namespace = item.get("namespace");
    let canonical = namespace
        .and_then(|value| json_text(value, "canonicalName", 160))
        .or_else(|| json_text(item, "canonicalName", 160));
    if let Some(canonical) = canonical {
        let value = canonical.trim().trim_start_matches('@');
        let mut parts = value.split('/');
        let owner = parts.next()?.to_string();
        let slug = parts.next()?.to_string();
        if parts.next().is_none() && is_market_slug(&owner) && is_market_slug(&slug) {
            return Some((owner, slug));
        }
    }

    let owner = namespace
        .and_then(|value| json_text(value, "handle", 96))
        .or_else(|| json_text(item, "ownerHandle", 96))
        .or_else(|| item.get("owner").and_then(|value| json_text(value, "handle", 96)))?;
    let slug = namespace
        .and_then(|value| json_text(value, "publicSlug", 96))
        .or_else(|| json_text(item, "slug", 96))
        .or_else(|| item.get("skill").and_then(|value| json_text(value, "slug", 96)))?;
    if is_market_slug(&owner) && is_market_slug(&slug) {
        Some((owner, slug))
    } else {
        None
    }
}

async fn commit_managed_skill(
    paths: &SkillPaths,
    market_skill: &MarketSkillId,
    revision: Option<String>,
    staged_dir: &Path,
    artifact_sha256: String,
    content_sha256: String,
) -> Result<SkillMarketInstallResponse, MarketSkillInstallError> {
    ensure_regular_directory(&paths.user_skills_dir).await?;
    let target = paths.user_skills_dir.join(&market_skill.slug);
    let record = read_installation_record(paths, &market_skill.slug).await?;

    match tokio::fs::symlink_metadata(&target).await {
        Ok(metadata) => {
            if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
                return Err(MarketSkillInstallError::NameConflict);
            }
            // An existing entry that cannot be proven to be a regular,
            // deterministically hashable Skill is still a name conflict. Do
            // not turn a user-owned link/reparse/unsupported entry into a
            // generic local-I/O failure that invites a retry with overwrite
            // semantics.
            let current_hash = hash_skill_directory(&target)
                .await
                .map_err(|_| MarketSkillInstallError::NameConflict)?;
            if let Some(record) = record
                && record.source == SKILLHUB_SOURCE
                && record.source_id == market_skill.market_id
                && record.content_sha256 == current_hash
                && current_hash == content_sha256
            {
                return Ok(SkillMarketInstallResponse {
                    status: SkillMarketInstallStatus::Reused,
                    source: record.source,
                    market_id: record.source_id,
                    skill_name: record.skill_name,
                    revision: record.revision,
                    artifact_sha256: record.artifact_sha256,
                    content_sha256: record.content_sha256,
                    installed_at: record.installed_at,
                });
            }
            Err(MarketSkillInstallError::NameConflict)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let installed_at = now_epoch_ms();
            let record = MarketSkillInstallationRecord {
                schema_version: 1,
                skill_name: market_skill.slug.clone(),
                source: SKILLHUB_SOURCE.into(),
                source_id: market_skill.market_id.clone(),
                revision: revision.clone(),
                artifact_sha256: artifact_sha256.clone(),
                content_sha256: content_sha256.clone(),
                installed_at,
            };
            if let Err(error) = write_installation_record(paths, &record).await {
                // The target has not been renamed yet, so a provenance write
                // failure cannot expose a partially installed Skill. The
                // atomic writer may have replaced the record before failing
                // while syncing its parent; remove that uncertain result
                // instead of leaving stale provenance behind.
                if let Err(cleanup_error) = remove_installation_record(paths, &record.skill_name).await {
                    tracing::error!(
                        error = %cleanup_error,
                        "failed to remove incomplete SkillHub installation record"
                    );
                }
                return Err(error);
            }

            if let Err(error) = tokio::fs::rename(staged_dir, &target).await {
                let cleanup_result = remove_installation_record(paths, &record.skill_name).await;
                if let Err(cleanup_error) = cleanup_result {
                    tracing::error!(
                        error = %cleanup_error,
                        "failed to remove SkillHub installation record after directory commit failure"
                    );
                    return Err(MarketSkillInstallError::LocalIo);
                }
                return Err(if error.kind() == io::ErrorKind::AlreadyExists {
                    MarketSkillInstallError::NameConflict
                } else {
                    MarketSkillInstallError::LocalIo
                });
            }

            Ok(SkillMarketInstallResponse {
                status: SkillMarketInstallStatus::Created,
                source: SKILLHUB_SOURCE.into(),
                market_id: market_skill.market_id.clone(),
                skill_name: market_skill.slug.clone(),
                revision,
                artifact_sha256,
                content_sha256,
                installed_at,
            })
        }
        Err(_) => Err(MarketSkillInstallError::LocalIo),
    }
}

async fn remove_installation_record(
    paths: &SkillPaths,
    skill_name: &str,
) -> Result<(), MarketSkillInstallError> {
    let path = installation_record_path(paths, skill_name)?;
    let metadata = match tokio::fs::symlink_metadata(&path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(MarketSkillInstallError::LocalIo),
    };
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err(MarketSkillInstallError::LocalIo);
    }
    tokio::fs::remove_file(path)
        .await
        .map_err(|_| MarketSkillInstallError::LocalIo)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct MarketSkillInstallationRecord {
    schema_version: u32,
    skill_name: String,
    source: String,
    source_id: String,
    #[serde(default)]
    revision: Option<String>,
    artifact_sha256: String,
    content_sha256: String,
    installed_at: i64,
}

async fn read_installation_record(
    paths: &SkillPaths,
    skill_name: &str,
) -> Result<Option<MarketSkillInstallationRecord>, MarketSkillInstallError> {
    let path = installation_record_path(paths, skill_name)?;
    let parent = path.parent().ok_or(MarketSkillInstallError::LocalIo)?;
    ensure_regular_directory(parent).await?;
    let metadata = match tokio::fs::symlink_metadata(&path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(MarketSkillInstallError::LocalIo),
    };
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err(MarketSkillInstallError::NameConflict);
    }
    let body = tokio::fs::read(&path).await.map_err(|_| MarketSkillInstallError::LocalIo)?;
    if body.len() > MAX_INSTALLATION_RECORD_BYTES {
        return Err(MarketSkillInstallError::NameConflict);
    }
    let record = serde_json::from_slice::<MarketSkillInstallationRecord>(&body)
        .map_err(|_| MarketSkillInstallError::NameConflict)?;
    if !valid_installation_record(&record) {
        return Err(MarketSkillInstallError::NameConflict);
    }
    Ok(Some(record))
}

fn valid_installation_record(record: &MarketSkillInstallationRecord) -> bool {
    record.schema_version == 1
        && record.source == SKILLHUB_SOURCE
        && validate_managed_skill_name(&record.skill_name).is_ok()
        && parse_market_skill_id(SKILLHUB_SOURCE, &record.source_id)
            .map(|market| market.slug.eq_ignore_ascii_case(&record.skill_name))
            .unwrap_or(false)
        && is_sha256(&record.artifact_sha256)
        && is_sha256(&record.content_sha256)
        && record.installed_at > 0
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn installation_record_path(paths: &SkillPaths, skill_name: &str) -> Result<PathBuf, MarketSkillInstallError> {
    validate_managed_skill_name(skill_name)?;
    Ok(paths.data_dir.join(INSTALLATIONS_DIR).join(format!("{skill_name}.json")))
}

async fn write_installation_record(
    paths: &SkillPaths,
    record: &MarketSkillInstallationRecord,
) -> Result<(), MarketSkillInstallError> {
    let path = installation_record_path(paths, &record.skill_name)?;
    let parent = path.parent().ok_or(MarketSkillInstallError::LocalIo)?;
    ensure_regular_directory(parent).await?;
    let bytes = serde_json::to_vec_pretty(record).map_err(|_| MarketSkillInstallError::LocalIo)?;
    tokio::task::spawn_blocking(move || write_atomic_replace(&path, &bytes))
        .await
        .map_err(|_| MarketSkillInstallError::LocalIo)?
        .map_err(|_| MarketSkillInstallError::LocalIo)
}

async fn ensure_regular_directory(path: &Path) -> Result<(), MarketSkillInstallError> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => {
            if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
                Err(MarketSkillInstallError::LocalIo)
            } else {
                Ok(())
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent()
                && parent != path
                && !parent.as_os_str().is_empty()
            {
                Box::pin(ensure_regular_directory(parent)).await?;
            }
            match tokio::fs::create_dir(path).await {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    let metadata = tokio::fs::symlink_metadata(path)
                        .await
                        .map_err(|_| MarketSkillInstallError::LocalIo)?;
                    if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
                        return Err(MarketSkillInstallError::LocalIo);
                    }
                    Ok(())
                }
                Err(_) => Err(MarketSkillInstallError::LocalIo),
            }
        }
        Err(_) => Err(MarketSkillInstallError::LocalIo),
    }
}

async fn list_valid_installations(
    paths: &SkillPaths,
) -> Result<Vec<SkillMarketInstallationResponse>, MarketSkillInstallError> {
    let root = paths.data_dir.join(INSTALLATIONS_DIR);
    ensure_regular_directory(&root).await?;

    let mut result = Vec::new();
    let mut entries = tokio::fs::read_dir(&root)
        .await
        .map_err(|_| MarketSkillInstallError::LocalIo)?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|_| MarketSkillInstallError::LocalIo)?
    {
        let path = entry.path();
        let metadata = match tokio::fs::symlink_metadata(&path).await {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if metadata_is_link_or_reparse(&metadata)
            || !metadata.is_file()
            || path.extension().and_then(|value| value.to_str()) != Some("json")
        {
            continue;
        }
        let body = match tokio::fs::read(&path).await {
            Ok(body) => body,
            Err(_) => continue,
        };
        if body.len() > MAX_INSTALLATION_RECORD_BYTES {
            continue;
        }
        let Ok(record) = serde_json::from_slice::<MarketSkillInstallationRecord>(&body) else {
            continue;
        };
        let file_stem = path.file_stem().and_then(|value| value.to_str());
        if file_stem != Some(record.skill_name.as_str()) || !valid_installation_record(&record) {
            continue;
        }
        let target = paths.user_skills_dir.join(&record.skill_name);
        let target_metadata = match tokio::fs::symlink_metadata(&target).await {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if metadata_is_link_or_reparse(&target_metadata) || !target_metadata.is_dir() {
            continue;
        }
        let expected_hash = record.content_sha256.clone();
        let Ok(actual_hash) = hash_skill_directory(&target).await else {
            continue;
        };
        if actual_hash != expected_hash {
            continue;
        }
        result.push(SkillMarketInstallationResponse {
            source: record.source,
            market_id: record.source_id,
            skill_name: record.skill_name,
            revision: record.revision,
            artifact_sha256: record.artifact_sha256,
            content_sha256: record.content_sha256,
            installed_at: record.installed_at,
        });
    }
    result.sort_by(|left, right| left.market_id.cmp(&right.market_id));
    Ok(result)
}

async fn hash_skill_directory(path: &Path) -> Result<String, MarketSkillInstallError> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || skill_service::hash_skill_directory_content_sync(&path))
        .await
        .map_err(|_| MarketSkillInstallError::LocalIo)?
        .map_err(|_| MarketSkillInstallError::LocalIo)
}

fn find_single_skill_directory(extract_dir: &Path) -> Result<PathBuf, MarketSkillInstallError> {
    let mut manifests = Vec::new();
    collect_skill_manifests(extract_dir, 0, &mut manifests)?;
    if manifests.len() != 1 {
        return Err(MarketSkillInstallError::ManifestInvalid);
    }
    manifests
        .pop()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .ok_or(MarketSkillInstallError::ManifestInvalid)
}

fn collect_skill_manifests(
    directory: &Path,
    depth: usize,
    manifests: &mut Vec<PathBuf>,
) -> Result<(), MarketSkillInstallError> {
    if depth > MARKET_IMPORT_SCAN_DEPTH {
        return Err(MarketSkillInstallError::ArtifactInvalid);
    }
    let mut entries = fs::read_dir(directory)
        .map_err(|_| MarketSkillInstallError::ArtifactInvalid)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| MarketSkillInstallError::ArtifactInvalid)?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let metadata = symlink_metadata(&path).map_err(|_| MarketSkillInstallError::ArtifactInvalid)?;
        if metadata_is_link_or_reparse(&metadata) {
            return Err(MarketSkillInstallError::ArtifactInvalid);
        }
        if metadata.is_file() {
            if path.file_name().and_then(|name| name.to_str()) == Some(SKILL_MANIFEST_FILE) {
                manifests.push(path);
            }
        } else if metadata.is_dir() {
            collect_skill_manifests(&path, depth + 1, manifests)?;
        } else {
            return Err(MarketSkillInstallError::ArtifactInvalid);
        }
        if manifests.len() > 1 {
            return Err(MarketSkillInstallError::ManifestInvalid);
        }
    }
    Ok(())
}

struct MarketSkillStaging {
    root: PathBuf,
    parent: PathBuf,
    cleanup_handle: skill_service::StagingCleanupHandle,
    armed: bool,
}

impl MarketSkillStaging {
    async fn create(paths: &SkillPaths) -> Result<Self, MarketSkillInstallError> {
        let parent = paths.user_skills_dir.join(MARKET_STAGING_DIR);
        ensure_regular_directory(&parent).await?;
        let root = parent.join(format!("{SKILL_STAGING_PREFIX}{}-{}", std::process::id(), unique_nonce()));
        if tokio::fs::create_dir(&root).await.is_err() {
            let _ = tokio::fs::remove_dir(&parent).await;
            return Err(MarketSkillInstallError::LocalIo);
        }
        let cleanup_handle = skill_service::StagingCleanupHandle::new(root.clone(), parent.clone());
        Ok(Self {
            root,
            parent,
            cleanup_handle,
            armed: true,
        })
    }

    async fn cleanup(mut self) -> Result<(), MarketSkillInstallError> {
        let root_removed = match tokio::fs::remove_dir_all(&self.root).await {
            Ok(()) => true,
            Err(error) if error.kind() == io::ErrorKind::NotFound => true,
            Err(_) => false,
        };
        let parent_removed = match tokio::fs::remove_dir(&self.parent).await {
            Ok(()) => true,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::DirectoryNotEmpty
                ) => true,
            Err(_) => false,
        };
        if root_removed && parent_removed {
            self.armed = false;
            Ok(())
        } else {
            Err(MarketSkillInstallError::LocalIo)
        }
    }

}

impl Drop for MarketSkillStaging {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        self.cleanup_handle.request_cleanup();
    }
}

/// Remove only old, operation-owned staging directories. This is intentionally
/// conservative: it never follows a link and never considers a final Skill
/// directory a cleanup candidate.
pub async fn cleanup_stale_market_staging(paths: &SkillPaths) -> Result<(), MarketSkillInstallError> {
    let _mutation_guard = skill_service::skill_mutation_lock().lock().await;
    cleanup_staging_parent(&paths.user_skills_dir.join(MARKET_STAGING_DIR), &[SKILL_STAGING_PREFIX, PACKAGE_STAGING_PREFIX]).await?;
    cleanup_staging_parent(&paths.user_skills_dir.join(IMPORT_STAGING_DIR), &[IMPORT_STAGING_PREFIX]).await?;
    Ok(())
}

async fn cleanup_staging_parent(parent: &Path, prefixes: &[&str]) -> Result<(), MarketSkillInstallError> {
    let metadata = match tokio::fs::symlink_metadata(parent).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(MarketSkillInstallError::LocalIo),
    };
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err(MarketSkillInstallError::LocalIo);
    }
    let cutoff = SystemTime::now().checked_sub(STAGING_RETENTION).unwrap_or(UNIX_EPOCH);
    let mut entries = tokio::fs::read_dir(parent).await.map_err(|_| MarketSkillInstallError::LocalIo)?;
    while let Some(entry) = entries.next_entry().await.map_err(|_| MarketSkillInstallError::LocalIo)? {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !prefixes.iter().any(|prefix| is_operation_staging_name(&name, prefix)) {
            continue;
        }
        let path = entry.path();
        let metadata = match tokio::fs::symlink_metadata(&path).await {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
            continue;
        }
        let modified = metadata.modified().unwrap_or(SystemTime::now());
        if modified < cutoff {
            tokio::fs::remove_dir_all(&path)
                .await
                .map_err(|_| MarketSkillInstallError::LocalIo)?;
        }
    }
    let _ = tokio::fs::remove_dir(parent).await;
    Ok(())
}

fn is_operation_staging_name(name: &str, prefix: &str) -> bool {
    let Some(suffix) = name.strip_prefix(prefix) else {
        return false;
    };
    let mut parts = suffix.split('-');
    let (Some(pid), Some(nonce), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    !pid.is_empty()
        && !nonce.is_empty()
        && pid.bytes().all(|byte| byte.is_ascii_digit())
        && nonce.bytes().all(|byte| byte.is_ascii_digit())
}

fn map_app_market_error(error: AppError) -> MarketSkillInstallError {
    match error {
        AppError::NotFound(_) => MarketSkillInstallError::NotFound,
        AppError::Timeout(_) => MarketSkillInstallError::Timeout,
        AppError::BadGateway(_) => MarketSkillInstallError::Network,
        _ => MarketSkillInstallError::LocalIo,
    }
}

fn map_archive_error(error: ExtensionError) -> MarketSkillInstallError {
    match error {
        ExtensionError::Io(_) => MarketSkillInstallError::LocalIo,
        _ => MarketSkillInstallError::ArtifactInvalid,
    }
}

fn map_reqwest_market_error(error: reqwest::Error) -> MarketSkillInstallError {
    if error.is_timeout() {
        MarketSkillInstallError::Timeout
    } else {
        MarketSkillInstallError::Network
    }
}

fn now_epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

fn unique_nonce() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn metadata_is_link_or_reparse(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
        metadata.file_type().is_symlink()
            || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomifun_api_types::{SkillMarketInstallRequest, SkillMarketInstallStatus};
    use std::io::Write;
    use std::sync::{Arc, Barrier};
    use serial_test::serial;
    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct FakeSkillHub {
        archive: Vec<u8>,
        owner: String,
        slug: String,
        revision: Option<String>,
    }

    #[async_trait]
    impl SkillHubArtifactSource for FakeSkillHub {
        async fn find_skill(&self, owner: &str, slug: &str) -> Result<RemoteSkill, MarketSkillInstallError> {
            if owner != self.owner || slug != self.slug {
                return Err(MarketSkillInstallError::NotFound);
            }
            Ok(RemoteSkill {
                owner: self.owner.clone(),
                slug: self.slug.clone(),
                revision: self.revision.clone(),
            })
        }

        async fn download_skill(
            &self,
            _skill: &RemoteSkill,
            archive_path: &Path,
        ) -> Result<String, MarketSkillInstallError> {
            tokio::fs::write(archive_path, &self.archive)
                .await
                .map_err(|_| MarketSkillInstallError::LocalIo)?;
            Ok(format!("{:x}", Sha256::digest(&self.archive)))
        }
    }

    struct SlowSkillHub {
        inner: FakeSkillHub,
        delay: Duration,
    }

    #[async_trait]
    impl SkillHubArtifactSource for SlowSkillHub {
        async fn find_skill(&self, owner: &str, slug: &str) -> Result<RemoteSkill, MarketSkillInstallError> {
            self.inner.find_skill(owner, slug).await
        }

        async fn download_skill(
            &self,
            skill: &RemoteSkill,
            archive_path: &Path,
        ) -> Result<String, MarketSkillInstallError> {
            tokio::time::sleep(self.delay).await;
            self.inner.download_skill(skill, archive_path).await
        }
    }

    fn test_paths(root: &Path) -> SkillPaths {
        SkillPaths {
            data_dir: root.to_path_buf(),
            user_skills_dir: root.join("skills"),
            cron_skills_dir: root.join("cron/skills"),
            builtin_skills_dir: root.join("builtin-skills"),
            builtin_rules_dir: root.join("builtin-rules"),
            preset_rules_dir: root.join("preset-rules"),
            preset_skills_dir: root.join("preset-skills"),
            catalog_roots: Default::default(),
        }
    }

    fn archive_for(slug: &str, name: &str) -> Vec<u8> {
        let mut output = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut output);
            let mut writer = zip::ZipWriter::new(cursor);
            let options = zip::write::SimpleFileOptions::default();
            writer.start_file(format!("{slug}/SKILL.md"), options).unwrap();
            writeln!(writer, "---\nname: {name}\ndescription: Demo\n---").unwrap();
            writer.start_file(format!("{slug}/README.md"), options).unwrap();
            writer.write_all(b"demo").unwrap();
            writer.finish().unwrap();
        }
        output
    }

    async fn spawn_http_fixture(
        responses: Vec<(&'static str, &'static str, Vec<u8>)>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for (status, content_type, body) in responses {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0_u8; 4096];
                let _ = socket.read(&mut request).await;
                let headers = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                socket.write_all(headers.as_bytes()).await.unwrap();
                socket.write_all(&body).await.unwrap();
            }
        });
        (format!("http://{address}"), server)
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

    #[tokio::test]
    async fn managed_install_creates_then_reuses_without_overwriting() {
        let tmp = TempDir::new().unwrap();
        let paths = test_paths(tmp.path());
        let installer = ManagedSkillInstaller::new(paths.clone());
        let fake = Arc::new(FakeSkillHub {
            archive: archive_for("demo", "demo"),
            owner: "owner".into(),
            slug: "demo".into(),
            revision: Some("r1".into()),
        });
        let request = SkillMarketInstallRequest {
            source: SKILLHUB_SOURCE.into(),
            id: "skillhub:owner/skills/demo".into(),
        };
        let first = installer.install_with_source(request.clone(), fake.clone()).await.unwrap();
        assert_eq!(first.status, SkillMarketInstallStatus::Created);
        assert!(paths.user_skills_dir.join("demo/SKILL.md").is_file());
        assert!(paths.data_dir.join("skill-market/installations/demo.json").is_file());
        assert!(!paths.user_skills_dir.join(".market-import").exists());
        let listed = installer.list_installations().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].market_id, "skillhub:owner/skills/demo");

        let second = installer.install_with_source(request, fake).await.unwrap();
        assert_eq!(second.status, SkillMarketInstallStatus::Reused);
        assert_eq!(second.content_sha256, first.content_sha256);

        tokio::fs::write(
            paths.user_skills_dir.join("demo/README.md"),
            "user edit",
        )
        .await
        .unwrap();
        assert!(installer.list_installations().await.unwrap().is_empty());
        let error = installer
            .install_with_source(
                SkillMarketInstallRequest {
                    source: SKILLHUB_SOURCE.into(),
                    id: "skillhub:owner/skills/demo".into(),
                },
                Arc::new(FakeSkillHub {
                    archive: archive_for("demo", "demo"),
                    owner: "owner".into(),
                    slug: "demo".into(),
                    revision: Some("r2".into()),
                }),
            )
            .await
            .unwrap_err();
        assert_eq!(error, MarketSkillInstallError::NameConflict);
    }

    #[tokio::test]
    async fn concurrent_managed_installs_create_once_and_reuse_the_winner() {
        let tmp = TempDir::new().unwrap();
        let paths = test_paths(tmp.path());
        let installer = Arc::new(ManagedSkillInstaller::new(paths.clone()));
        let fake = Arc::new(FakeSkillHub {
            archive: archive_for("parallel", "parallel"),
            owner: "owner".into(),
            slug: "parallel".into(),
            revision: Some("r1".into()),
        });
        let request = SkillMarketInstallRequest {
            source: SKILLHUB_SOURCE.into(),
            id: "skillhub:owner/skills/parallel".into(),
        };

        let (left, right) = tokio::join!(
            installer.install_with_source(request.clone(), fake.clone()),
            installer.install_with_source(request, fake),
        );
        let statuses = [left.unwrap().status, right.unwrap().status];
        assert!(statuses.contains(&SkillMarketInstallStatus::Created));
        assert!(statuses.contains(&SkillMarketInstallStatus::Reused));
        assert!(paths.user_skills_dir.join("parallel/SKILL.md").is_file());
        assert_eq!(installer.list_installations().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn managed_install_conflicts_with_unprovenance_or_modified_directory() {
        let tmp = TempDir::new().unwrap();
        let paths = test_paths(tmp.path());
        let installer = ManagedSkillInstaller::new(paths.clone());
        tokio::fs::create_dir_all(paths.user_skills_dir.join("demo")).await.unwrap();
        tokio::fs::write(
            paths.user_skills_dir.join("demo/SKILL.md"),
            "---\nname: demo\ndescription: local\n---\n",
        )
        .await
        .unwrap();
        let fake = Arc::new(FakeSkillHub {
            archive: archive_for("demo", "demo"),
            owner: "owner".into(),
            slug: "demo".into(),
            revision: None,
        });
        let error = installer
            .install_with_source(
                SkillMarketInstallRequest {
                    source: SKILLHUB_SOURCE.into(),
                    id: "skillhub:owner/skills/demo".into(),
                },
                fake,
            )
            .await
            .unwrap_err();
        assert_eq!(error, MarketSkillInstallError::NameConflict);
    }

    #[tokio::test]
    async fn managed_install_rolls_back_skill_when_provenance_commit_fails() {
        let tmp = TempDir::new().unwrap();
        let paths = test_paths(tmp.path());
        let installer = ManagedSkillInstaller::new(paths.clone());
        tokio::fs::create_dir_all(&paths.data_dir).await.unwrap();
        tokio::fs::write(paths.data_dir.join("skill-market"), "not a directory")
            .await
            .unwrap();
        let fake = Arc::new(FakeSkillHub {
            archive: archive_for("rollback", "rollback"),
            owner: "owner".into(),
            slug: "rollback".into(),
            revision: None,
        });

        let error = installer
            .install_with_source(
                SkillMarketInstallRequest {
                    source: SKILLHUB_SOURCE.into(),
                    id: "skillhub:owner/skills/rollback".into(),
                },
                fake,
            )
            .await
            .unwrap_err();
        assert_eq!(error, MarketSkillInstallError::LocalIo);
        assert!(!paths.user_skills_dir.join("rollback").exists());
    }

    #[tokio::test]
    async fn managed_install_cleans_staging_after_archive_failure() {
        let tmp = TempDir::new().unwrap();
        let paths = test_paths(tmp.path());
        let installer = ManagedSkillInstaller::new(paths.clone());
        let error = installer
            .install_with_source(
                SkillMarketInstallRequest {
                    source: SKILLHUB_SOURCE.into(),
                    id: "skillhub:owner/skills/invalid-archive".into(),
                },
                Arc::new(FakeSkillHub {
                    archive: b"not a zip".to_vec(),
                    owner: "owner".into(),
                    slug: "invalid-archive".into(),
                    revision: None,
                }),
            )
            .await
            .unwrap_err();

        assert_eq!(error, MarketSkillInstallError::ArtifactInvalid);
        assert!(!paths.user_skills_dir.join(MARKET_STAGING_DIR).exists());
    }

    #[tokio::test]
    async fn managed_install_cleans_staging_when_request_is_cancelled() {
        let tmp = TempDir::new().unwrap();
        let paths = test_paths(tmp.path());
        let installer = ManagedSkillInstaller::new(paths.clone());
        let result = tokio::time::timeout(
            Duration::from_millis(20),
            installer.install_with_source(
                SkillMarketInstallRequest {
                    source: SKILLHUB_SOURCE.into(),
                    id: "skillhub:owner/skills/cancelled-skill".into(),
                },
                Arc::new(SlowSkillHub {
                    inner: FakeSkillHub {
                        archive: archive_for("cancelled-skill", "cancelled-skill"),
                        owner: "owner".into(),
                        slug: "cancelled-skill".into(),
                        revision: None,
                    },
                    delay: Duration::from_secs(1),
                }),
            ),
        )
        .await;

        assert!(result.is_err(), "the slow download should be cancelled");
        assert!(!paths.user_skills_dir.join(MARKET_STAGING_DIR).exists());
    }

    #[tokio::test]
    #[serial]
    async fn managed_install_cleans_staging_when_cancelled_during_extraction() {
        let tmp = TempDir::new().unwrap();
        let paths = test_paths(tmp.path());
        let installer = ManagedSkillInstaller::new(paths.clone());
        let started = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let _gate = crate::skill_service::test_overrides::pause_archive_extraction(
            started.clone(),
            release.clone(),
        );
        let task = tokio::spawn({
            let installer = installer.clone();
            async move {
                installer
                    .install_with_source(
                        SkillMarketInstallRequest {
                            source: SKILLHUB_SOURCE.into(),
                            id: "skillhub:owner/skills/cancelled-during-extraction".into(),
                        },
                        Arc::new(FakeSkillHub {
                            archive: archive_for(
                                "cancelled-during-extraction",
                                "cancelled-during-extraction",
                            ),
                            owner: "owner".into(),
                            slug: "cancelled-during-extraction".into(),
                            revision: None,
                        }),
                    )
                    .await
            }
        });

        tokio::time::timeout(
            Duration::from_secs(5),
            tokio::task::spawn_blocking(move || started.wait()),
        )
        .await
        .unwrap()
        .unwrap();
        task.abort();
        tokio::task::spawn_blocking(move || release.wait()).await.unwrap();
        assert!(task.await.unwrap_err().is_cancelled());

        for _ in 0..50 {
            if !paths.user_skills_dir.join(MARKET_STAGING_DIR).exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(!paths.user_skills_dir.join(MARKET_STAGING_DIR).exists());
    }

    #[tokio::test]
    async fn http_source_binds_search_and_download_to_exact_skill() {
        let archive = archive_for("demo", "demo");
        let (base_url, server) = spawn_http_fixture(vec![
            (
                "200 OK",
                "application/json",
                br#"{"results":[{"namespace":{"canonicalName":"@owner/demo"},"revision":"r1"}]}"#.to_vec(),
            ),
            ("200 OK", "application/zip", archive.clone()),
        ])
        .await;
        let source = HttpSkillHubArtifactSource::for_test(&base_url);
        let remote = source.find_skill("owner", "demo").await.unwrap();
        assert_eq!(remote.owner, "owner");
        assert_eq!(remote.slug, "demo");

        let tmp = TempDir::new().unwrap();
        let archive_path = tmp.path().join("skill.zip");
        let sha = source.download_skill(&remote, &archive_path).await.unwrap();
        assert_eq!(sha, format!("{:x}", Sha256::digest(&archive)));
        assert_eq!(tokio::fs::read(archive_path).await.unwrap(), archive);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn http_source_rejects_wrong_content_type_and_magic() {
        for (content_type, body) in [("text/html", archive_for("demo", "demo")), ("application/zip", b"not zip".to_vec())] {
            let (base_url, server) = spawn_http_fixture(vec![
                (
                    "200 OK",
                    "application/json",
                    br#"{"results":[{"namespace":{"canonicalName":"@owner/demo"}}]}"#.to_vec(),
                ),
                ("200 OK", content_type, body),
            ])
            .await;
            let source = HttpSkillHubArtifactSource::for_test(&base_url);
            let remote = source.find_skill("owner", "demo").await.unwrap();
            let tmp = TempDir::new().unwrap();
            let error = source
                .download_skill(&remote, &tmp.path().join("skill.zip"))
                .await
                .unwrap_err();
            assert_eq!(error, MarketSkillInstallError::ArtifactInvalid);
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn http_source_rejects_chunked_archive_over_limit() {
        let mut body = vec![0_u8; MAX_SKILLHUB_SKILL_ZIP_BYTES as usize + 1];
        body[0] = b'P';
        body[1] = b'K';
        let (base_url, server) = spawn_chunked_http_fixture("application/zip", body).await;
        let source = HttpSkillHubArtifactSource::for_test(&base_url);
        let tmp = TempDir::new().unwrap();
        let error = source
            .download_skill(
                &RemoteSkill {
                    owner: "owner".into(),
                    slug: "demo".into(),
                    revision: None,
                },
                &tmp.path().join("skill.zip"),
            )
            .await
            .unwrap_err();

        assert_eq!(error, MarketSkillInstallError::ArtifactInvalid);
        server.await.unwrap();
    }

    #[test]
    fn managed_name_rejects_portable_and_windows_reserved_names() {
        for name in ["", ".", "..", "CON", "nul.txt", "skill.", "skill ", "skill/name"] {
            assert!(validate_managed_skill_name(name).is_err(), "{name:?}");
        }
        assert!(validate_managed_skill_name("normal-skill").is_ok());
        assert!(parse_market_skill_id(SKILLHUB_SOURCE, "skillhub:owner/skills/normal-skill").is_ok());
        assert!(parse_market_skill_id(SKILLHUB_SOURCE, "skillhub:owner/other/normal-skill").is_err());
    }

    #[test]
    fn search_result_requires_exact_owner_and_slug() {
        let root = serde_json::json!({
            "results": [
                {"namespace": {"canonicalName": "@owner/other"}},
                {"namespace": {"canonicalName": "@owner/demo"}, "revision": "r2"}
            ]
        });
        let found = find_exact_search_result(&root, "owner", "demo").unwrap();
        assert_eq!(found.slug, "demo");
        assert_eq!(found.revision.as_deref(), Some("r2"));
        assert!(find_exact_search_result(&root, "other", "demo").is_none());
    }

    #[test]
    fn download_url_keeps_owner_and_slug_bound() {
        let url = skillhub_download_url(
            SKILLHUB_SKILL_DOWNLOAD_URL,
            &RemoteSkill {
                owner: "owner-a".into(),
                slug: "demo".into(),
                revision: None,
            },
        )
        .unwrap();
        let pairs: Vec<_> = url.query_pairs().into_owned().collect();
        assert_eq!(pairs, vec![("owner".into(), "owner-a".into()), ("slug".into(), "demo".into())]);
    }

    #[test]
    fn staging_gc_only_accepts_operation_owned_names() {
        assert!(is_operation_staging_name("skill-123-456", "skill-"));
        assert!(is_operation_staging_name("package-123-456", "package-"));
        assert!(is_operation_staging_name("skills-123-456", "skills-"));
        for name in [
            "skill-user",
            "skill-123",
            "skill-123-abc",
            "skill-123-456-extra",
            "skill--456",
            "skills-123-456.tmp",
        ] {
            assert!(!is_operation_staging_name(name, "skill-"), "must reject {name:?}");
        }
    }
}
