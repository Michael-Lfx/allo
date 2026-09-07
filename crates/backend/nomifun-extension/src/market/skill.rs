//! Native installation for ordinary Skills from the supported markets.
//!
//! This module deliberately owns only the market-to-archive adapters. The
//! filesystem transaction remains in [`crate::skill_service`], so expert
//! packages and ordinary Skills share the same validation and commit rules.

use std::io;
use std::path::{Path, PathBuf};

use nomifun_api_types::{
    SkillMarketInstallStatus, SkillMarketSkillInstallRequest, SkillMarketSkillInstallResponse,
};
use nomifun_common::AppError;
use reqwest::Url;
use reqwest::header::ACCEPT;
use serde::Deserialize;

use crate::error::ExtensionError;
use crate::skill_service::{self, SkillPaths};

use super::client::{
    MAX_MARKET_SKILL_ARCHIVE_BYTES, build_market_client, map_market_fetch_error, read_market_bytes,
};
use super::package::download_skillhub_skill_zip;
use super::parse::is_market_slug;
use super::staging::{MarketStaging, create_market_staging};

const CLAWHUB_SOURCE: &str = "clawhub";
const SKILLHUB_SOURCE: &str = "skillhub";
const LOOPHUB_SOURCE: &str = "loophub";
const CLAWHUB_DOWNLOAD_URL: &str = "https://clawhub.ai/api/v1/download";
const GITHUB_ARCHIVE_HOST: &str = "codeload.github.com";

#[derive(Debug)]
enum NativeMarketSkill {
    ClawHub { owner: String, slug: String },
    SkillHub { slug: String },
    LoopHub { artifact_url: Url },
}

impl NativeMarketSkill {
    fn expected_name(&self) -> Option<&str> {
        match self {
            Self::ClawHub { slug, .. } | Self::SkillHub { slug } => Some(slug),
            Self::LoopHub { .. } => None,
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
    async fn download(&self, target: &NativeMarketSkill) -> Result<DownloadedArtifact, AppError>;
}

struct HttpMarketSkillDownloader {
    client: reqwest::Client,
}

#[async_trait::async_trait]
impl MarketSkillDownloader for HttpMarketSkillDownloader {
    async fn download(&self, target: &NativeMarketSkill) -> Result<DownloadedArtifact, AppError> {
        match target {
            NativeMarketSkill::ClawHub { owner, slug } => {
                let slug_param = format!("{owner}/{slug}");
                let url = Url::parse_with_params(CLAWHUB_DOWNLOAD_URL, &[("slug", &slug_param)])
                    .map_err(|error| AppError::Internal(format!("invalid ClawHub download URL: {error}")))?;
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
            NativeMarketSkill::SkillHub { slug } => {
                let (_resolved_slug, bytes) = download_skillhub_skill_zip(&self.client, slug).await?;
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
                    .map_err(map_market_fetch_error)?;
                validate_loophub_artifact_url(response.url().as_str()).map_err(|_| {
                    AppError::BadGateway(
                        "LoopHub artifact redirect left the trusted download path".into(),
                    )
                })?;
                Ok(DownloadedArtifact {
                    bytes: read_market_bytes(
                        &mut response,
                        MAX_MARKET_SKILL_ARCHIVE_BYTES,
                        "LoopHub skill archive",
                    )
                    .await?,
                    descriptor_path: None,
                })
            }
        }
    }
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
) -> Result<SkillMarketSkillInstallResponse, AppError> {
    let target = parse_install_target(&req)?;
    let client = build_market_client()?;
    let downloader = HttpMarketSkillDownloader { client };
    install_market_skill_with_downloader(paths, &req.source, &req.id, target, &downloader).await
}

async fn install_market_skill_with_downloader<D: MarketSkillDownloader>(
    paths: &SkillPaths,
    source: &str,
    id: &str,
    target: NativeMarketSkill,
    downloader: &D,
) -> Result<SkillMarketSkillInstallResponse, AppError> {
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
        return Err(AppError::BadGateway("market Skill archive is too large".into()));
    }

    let staging = create_market_staging(paths, "skill").await?;
    let (staged_dir, skill_name) = stage_market_skill(&staging, artifact, target.expected_name()).await?;

    // Re-check under the same lock used by expert packages. A second window
    // may have installed this Skill while this request was downloading it.
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

fn parse_install_target(req: &SkillMarketSkillInstallRequest) -> Result<NativeMarketSkill, AppError> {
    match req.source.as_str() {
        CLAWHUB_SOURCE => {
            if req.artifact_url.is_some() {
                return Err(AppError::BadRequest(
                    "artifact_url is only accepted for LoopHub Skills".into(),
                ));
            }
            let (owner, slug) = parse_two_part_id(&req.id, CLAWHUB_SOURCE, "ClawHub")?;
            Ok(NativeMarketSkill::ClawHub { owner, slug })
        }
        SKILLHUB_SOURCE => {
            if req.artifact_url.is_some() {
                return Err(AppError::BadRequest(
                    "artifact_url is only accepted for LoopHub Skills".into(),
                ));
            }
            let suffix = req
                .id
                .strip_prefix("skillhub:")
                .ok_or_else(|| AppError::BadRequest("invalid SkillHub Skill id".into()))?;
            let mut parts = suffix.split('/');
            let owner = parts.next().unwrap_or_default();
            let marker = parts.next().unwrap_or_default();
            let slug = parts.next().unwrap_or_default();
            if parts.next().is_some()
                || marker != "skills"
                || !is_market_slug(owner)
                || !is_market_slug(slug)
            {
                return Err(AppError::BadRequest("invalid SkillHub Skill id".into()));
            }
            Ok(NativeMarketSkill::SkillHub {
                slug: slug.to_owned(),
            })
        }
        LOOPHUB_SOURCE => {
            let suffix = req
                .id
                .strip_prefix("loophub:")
                .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
                .ok_or_else(|| AppError::BadRequest("invalid LoopHub Skill id".into()))?;
            let id = suffix
                .parse::<u64>()
                .map_err(|_| AppError::BadRequest("invalid LoopHub Skill id".into()))?;
            if id == 0 {
                return Err(AppError::BadRequest("invalid LoopHub Skill id".into()));
            }
            let artifact_url = req
                .artifact_url
                .as_deref()
                .ok_or_else(|| AppError::BadRequest("LoopHub artifact_url is required".into()))?;
            Ok(NativeMarketSkill::LoopHub {
                artifact_url: validate_loophub_artifact_url(artifact_url)?,
            })
        }
        _ => Err(AppError::BadRequest(
            "ordinary Skill installation supports only clawhub, skillhub, and loophub".into(),
        )),
    }
}

fn parse_two_part_id(id: &str, prefix: &str, label: &str) -> Result<(String, String), AppError> {
    let suffix = id
        .strip_prefix(&format!("{prefix}:"))
        .ok_or_else(|| AppError::BadRequest(format!("invalid {label} Skill id")))?;
    let mut parts = suffix.split('/');
    let first = parts.next().unwrap_or_default();
    let second = parts.next().unwrap_or_default();
    if parts.next().is_some() || !is_market_slug(first) || !is_market_slug(second) {
        return Err(AppError::BadRequest(format!("invalid {label} Skill id")));
    }
    Ok((first.to_owned(), second.to_owned()))
}

fn validate_loophub_artifact_url(value: &str) -> Result<Url, AppError> {
    let url = Url::parse(value).map_err(|_| AppError::BadRequest("invalid LoopHub artifact_url".into()))?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || !url
            .host_str()
            .is_some_and(|host| host.eq_ignore_ascii_case("dl.cocoloop.cn"))
        || !url.path().starts_with("/bss/skills/")
    {
        return Err(AppError::BadRequest(
            "LoopHub artifact_url is outside the trusted download path".into(),
        ));
    }
    Ok(url)
}

async fn download_http_bytes(
    client: &reqwest::Client,
    url: Url,
    label: &str,
) -> Result<Vec<u8>, AppError> {
    let mut response = client
        .get(url)
        .header(ACCEPT, "application/zip,application/octet-stream,application/json,*/*")
        .send()
        .await
        .map_err(map_market_fetch_error)?;
    read_market_bytes(&mut response, MAX_MARKET_SKILL_ARCHIVE_BYTES, label).await
}

fn is_zip_bytes(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") || bytes.starts_with(b"PK\x07\x08")
}

fn parse_public_github_descriptor(bytes: &[u8]) -> Result<PublicGithubDescriptor, AppError> {
    let raw = serde_json::from_slice::<RawPublicGithubDescriptor>(bytes)
        .map_err(|error| AppError::BadGateway(format!("ClawHub GitHub descriptor is invalid JSON: {error}")))?;
    if raw.source != "public-github" {
        return Err(AppError::BadGateway(
            "ClawHub download returned an unsupported artifact descriptor".into(),
        ));
    }
    let (owner, repo) = parse_github_repo(&raw.repo)?;
    if !is_safe_github_ref(&raw.git_ref) {
        return Err(AppError::BadGateway("ClawHub GitHub descriptor has an invalid ref".into()));
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
    .map_err(|error| AppError::Internal(format!("invalid GitHub archive URL: {error}")))?;
    Ok(PublicGithubDescriptor { archive_url, path })
}

fn parse_github_repo(repo: &str) -> Result<(String, String), AppError> {
    let mut parts = repo.split('/');
    let owner = parts.next().unwrap_or_default();
    let name = parts.next().unwrap_or_default();
    if parts.next().is_some() || !is_market_slug(owner) || !is_market_slug(name) {
        return Err(AppError::BadGateway(
            "ClawHub GitHub descriptor has an invalid repo".into(),
        ));
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

fn parse_descriptor_path(value: &str) -> Result<PathBuf, AppError> {
    if value.is_empty()
        || value.len() > 320
        || value.starts_with('/')
        || value.contains('\\')
        || value.contains(':')
    {
        return Err(AppError::BadGateway(
            "ClawHub GitHub descriptor has an invalid path".into(),
        ));
    }
    let mut path = PathBuf::new();
    let mut depth = 0;
    for component in value.split('/') {
        if component.is_empty() || component == "." || component == ".." || component.contains('\0') {
            return Err(AppError::BadGateway(
                "ClawHub GitHub descriptor has an invalid path".into(),
            ));
        }
        depth += 1;
        if depth > skill_service::MARKET_IMPORT_SCAN_DEPTH {
            return Err(AppError::BadGateway(
                "ClawHub GitHub descriptor path is too deep".into(),
            ));
        }
        path.push(component);
    }
    Ok(path)
}

async fn reuse_existing_skill(paths: &SkillPaths, name: &str) -> Result<Option<()>, AppError> {
    let target = paths.user_skills_dir.join(name);
    match tokio::fs::symlink_metadata(&target).await {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(AppError::Conflict(format!(
                    "user Skill target '{name}' already exists and is not a valid directory"
                )));
            }
            skill_service::validate_market_skill_directory(&target, name)
                .await
                .map_err(|error| {
                    AppError::Conflict(format!(
                        "user Skill target '{name}' exists but is invalid: {error}"
                    ))
                })?;
            Ok(Some(()))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(AppError::Internal(format!("inspect existing Skill: {error}"))),
    }
}

async fn stage_market_skill(
    staging: &MarketStaging,
    artifact: DownloadedArtifact,
    expected_name: Option<&str>,
) -> Result<(PathBuf, String), AppError> {
    let archive_path = staging.root.join("skill.zip");
    let extract_dir = staging.root.join("extract");
    tokio::fs::write(&archive_path, &artifact.bytes)
        .await
        .map_err(|error| AppError::Internal(format!("write market Skill archive: {error}")))?;
    skill_service::extract_skill_archive_to_staging(&archive_path, &extract_dir)
        .await
        .map_err(AppError::from)?;
    tokio::fs::remove_file(&archive_path).await.map_err(|error| {
        AppError::Internal(format!("remove staged market Skill archive: {error}"))
    })?;

    let mut skill_dirs = Vec::new();
    skill_service::collect_skill_dirs_recursive(
        &extract_dir,
        &mut skill_dirs,
        skill_service::MARKET_IMPORT_SCAN_DEPTH,
    )
    .await
    .map_err(AppError::from)?;
    if let Some(descriptor_path) = artifact.descriptor_path.as_deref() {
        skill_dirs.retain(|path| path_ends_with(path, descriptor_path));
    }
    if skill_dirs.len() != 1 {
        return Err(AppError::BadRequest(format!(
            "market Skill archive must contain exactly one Skill, found {}",
            skill_dirs.len()
        )));
    }

    let skill_dir = skill_dirs.pop().expect("length checked above");
    let skill_name = match expected_name {
        Some(expected_name) => skill_service::validate_market_skill_directory(&skill_dir, expected_name).await,
        None => skill_service::validate_market_skill_directory_name(&skill_dir).await,
    }
    .map_err(AppError::from)?;
    Ok((skill_dir, skill_name))
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

fn map_commit_error(error: ExtensionError) -> AppError {
    match error {
        ExtensionError::InvalidSkillPath(message) if message.contains("user Skill target") => {
            AppError::Conflict(message)
        }
        other => other.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tempfile::TempDir;

    struct FakeDownloader {
        archive: Vec<u8>,
        calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl MarketSkillDownloader for FakeDownloader {
        async fn download(&self, target: &NativeMarketSkill) -> Result<DownloadedArtifact, AppError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match target {
                NativeMarketSkill::ClawHub { owner, slug } => {
                    assert_eq!(owner, "owner");
                    assert_eq!(slug, "claw-skill");
                }
                NativeMarketSkill::SkillHub { slug } => assert_eq!(slug, "skill-skill"),
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
        assert!(matches!(
            parse_install_target(&request(SKILLHUB_SOURCE, "skillhub:owner/skills/skill-skill", None)).unwrap(),
            NativeMarketSkill::SkillHub { .. }
        ));
        assert!(matches!(
            parse_install_target(&request(
                LOOPHUB_SOURCE,
                "loophub:12277",
                Some("https://dl.cocoloop.cn/bss/skills/skill.zip")
            ))
            .unwrap(),
            NativeMarketSkill::LoopHub { .. }
        ));
        assert!(parse_install_target(&request("mcpworld", "mcpworld:x", None)).is_err());
        assert!(parse_install_target(&request(SKILLHUB_SOURCE, "skillhub:owner/skills/../x", None)).is_err());
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
            assert!(validate_loophub_artifact_url(url).is_err(), "must reject {url}");
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
            assert!(parse_public_github_descriptor(descriptor).is_err());
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
        assert!(matches!(result, Err(AppError::Conflict(_))));
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
        assert!(matches!(result, Err(AppError::BadRequest(message)) if message.contains("exactly one")));
    }
}
