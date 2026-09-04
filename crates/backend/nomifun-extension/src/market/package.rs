//! SkillHub expert-package support: resolve a package entry into its
//! name/instructions/child-skill slugs, stage and validate every child Skill,
//! then commit the complete package and its preset as one logical operation.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use nomifun_api_types::{
    SkillCatalogSource, SkillId, SkillMarketPackageInstallError, SkillMarketPackageInstallResponse,
    SkillMarketPackageRequest, SkillMarketPackageResponse,
};
use nomifun_common::AppError;
use reqwest::header::ACCEPT;

use crate::error::ExtensionError;
use crate::skill_service::{self, SkillPaths};

use super::client::{
    MAX_SKILLHUB_SKILL_ZIP_BYTES, build_market_client, map_market_fetch_error, read_market_body,
    read_market_bytes, read_market_detail_body,
};
use super::parse::{
    dedup_strings, is_market_slug, json_text, json_text_preserve,
    last_url_segment, market_https_image_url, market_ref_suffix, title_from_slug,
};
use super::SKILLHUB_PACKAGES_SOURCE;

const SKILLHUB_SKILL_DOWNLOAD_URL: &str = "https://api.skillhub.cn/api/v1/download";
const SKILLHUB_SKILL_SEARCH_URL: &str = "https://api.skillhub.cn/api/v1/search";
const SKILLHUB_PACKAGE_DETAIL_BASE_URL: &str = "https://api.skillhub.cn/api/v1/skillsets/";

/// Bridge implemented by `PresetService` without introducing a dependency
/// from the extension crate back to the preset crate. The market installer
/// owns the Skill transaction; the bridge is called only after every child
/// Skill has passed validation and has been committed.
#[async_trait::async_trait]
pub trait MarketPackagePresetInstaller: Send + Sync {
    async fn install_market_package_preset(
        &self,
        preset_id: Option<String>,
        package: SkillMarketPackageResponse,
        skill_ids: Vec<String>,
    ) -> Result<String, MarketPackagePresetInstallFailure>;
}

/// Failure returned by the preset bridge. When an idempotent install found an
/// existing preset, newly downloaded Skills must be preserved even if the
/// state refresh fails; removing them would break that existing preset.
#[derive(Debug)]
pub struct MarketPackagePresetInstallFailure {
    pub error: AppError,
    pub preserve_committed_skills: bool,
}

impl MarketPackagePresetInstallFailure {
    pub fn new(error: AppError, preserve_committed_skills: bool) -> Self {
        Self {
            error,
            preserve_committed_skills,
        }
    }
}

struct MarketPackageStaging {
    root: PathBuf,
    parent: PathBuf,
}

// A package can stage its network payloads concurrently, but the commit and
// preset handoff must be serialized. Without this small process-local fence,
// two windows could both observe a missing target, and a failed first preset
// handoff could roll back a Skill that the second window already reused.
static MARKET_PACKAGE_COMMIT_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

fn market_package_commit_lock() -> &'static tokio::sync::Mutex<()> {
    MARKET_PACKAGE_COMMIT_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

impl Drop for MarketPackageStaging {
    fn drop(&mut self) {
        // Drop is the final cancellation/unwind guard. The extracted archive
        // is untrusted input and must not survive an interrupted install. Do
        // not recurse synchronously on a Tokio worker: package archives are
        // bounded but can still make cancellation block the runtime.
        let root = self.root.clone();
        let parent = self.parent.clone();
        let cleanup = move || {
            let _ = std::fs::remove_dir_all(root);
            let _ = std::fs::remove_dir(parent);
        };
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn_blocking(cleanup);
        } else {
            let _ = std::thread::Builder::new()
                .name("market-package-cleanup".into())
                .spawn(cleanup);
        }
    }
}

async fn create_market_package_staging(paths: &SkillPaths) -> Result<MarketPackageStaging, AppError> {
    let parent = paths.user_skills_dir.join(".market-import");
    tokio::fs::create_dir_all(&parent)
        .await
        .map_err(|error| AppError::Internal(format!("create market staging directory: {error}")))?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let root = parent.join(format!("package-{}-{nonce}", std::process::id()));
    if let Err(error) = tokio::fs::create_dir(&root).await {
        // No staging guard exists until the root is constructed. Remove only
        // the empty parent we created for this attempt; a concurrent package
        // install keeps it alive when its own child directory is present.
        let _ = tokio::fs::remove_dir(&parent).await;
        return Err(AppError::Internal(format!("create market staging root: {error}")));
    }
    Ok(MarketPackageStaging { root, parent })
}

/// Resolve a SkillHub expert package and install its child skills. This is
/// the `POST /api/skills/market/package/install` implementation; resolving
/// without installing has no frontend caller, so [`resolve_market_package`]
/// stays internal.
pub async fn install_market_package(
    paths: &SkillPaths,
    req: SkillMarketPackageRequest,
    preset_installer: Option<&dyn MarketPackagePresetInstaller>,
) -> Result<SkillMarketPackageInstallResponse, AppError> {
    let requested_preset_id = req.preset_id.clone();
    let slug = resolve_skillhub_package_slug(&req)?;
    if super::is_skillhub_package_blacklisted(&slug)? {
        return Err(AppError::NotFound(format!(
            "SkillHub expert package '{slug}' is unavailable"
        )));
    }
    let package = match resolve_market_package(req).await {
        Ok(package) => package,
        Err(error) => return resolve_package_failure_response(&slug, error),
    };
    let install_result = install_skillhub_package_skills(paths, &package.skill_slugs).await?;
    if !install_result.errors.is_empty() {
        let (failure_class, failure_code) = classify_package_failures(&install_result.errors);
        return Ok(SkillMarketPackageInstallResponse {
            package,
            preset_id: None,
            installed_skill_ids: Vec::new(),
            installed_skill_names: Vec::new(),
            errors: install_result.errors,
            failure_class: Some(failure_class.into()),
            failure_code: Some(failure_code.into()),
        });
    }

    let Some(preset_installer) = preset_installer else {
        return Err(AppError::Internal(
            "market package preset installer is not configured".into(),
        ));
    };
    let _commit_guard = market_package_commit_lock().lock().await;
    let mut committed = Vec::new();
    for staged in &install_result.staged_skills {
        if let Some(staged_dir) = staged.staged_dir.as_deref() {
            match skill_service::commit_market_skill_directory(paths, staged_dir, &staged.name).await {
                Ok(skill_service::MarketSkillCommit::Created) => committed.push(staged.name.clone()),
                Ok(skill_service::MarketSkillCommit::Reused) => {}
                Err(error) => {
                    rollback_committed_skills(paths, &committed).await;
                    return Ok(package_install_failure_response(
                        package,
                        staged.name.clone(),
                        error.to_string(),
                        "local",
                        "SKILL_COMMIT_FAILED",
                        None,
                    ));
                }
            }
        }
    }

    let skill_ids = install_result
        .installed_skill_names
        .iter()
        .map(|name| SkillId::new(SkillCatalogSource::User, None, name).as_str().to_owned())
        .collect::<Vec<_>>();
    match preset_installer
        .install_market_package_preset(requested_preset_id, package.clone(), skill_ids.clone())
        .await
    {
        Ok(preset_id) => Ok(SkillMarketPackageInstallResponse {
            package,
            preset_id: Some(preset_id),
            installed_skill_ids: skill_ids,
            installed_skill_names: install_result.installed_skill_names,
            errors: Vec::new(),
            failure_class: None,
            failure_code: None,
        }),
        Err(failure) => {
            if !failure.preserve_committed_skills {
                rollback_committed_skills(paths, &committed).await;
            }
            Ok(package_install_failure_response(
                package,
                "<preset>".into(),
                failure.error.to_string(),
                "local",
                "PRESET_COMMIT_FAILED",
                None,
            ))
        }
    }
}

async fn rollback_committed_skills(paths: &SkillPaths, names: &[String]) {
    for name in names {
        if let Err(error) = skill_service::rollback_market_skill(paths, name).await {
            tracing::error!(skill = %name, %error, "failed to roll back market Skill");
        }
    }
}

fn package_install_failure_response(
    package: SkillMarketPackageResponse,
    skill_slug: String,
    error: String,
    failure_class: &str,
    failure_code: &str,
    http_status: Option<u16>,
) -> SkillMarketPackageInstallResponse {
    SkillMarketPackageInstallResponse {
        package,
        preset_id: None,
        installed_skill_ids: Vec::new(),
        installed_skill_names: Vec::new(),
        errors: vec![SkillMarketPackageInstallError {
            skill_slug,
            error,
            http_status,
        }],
        failure_class: Some(failure_class.to_owned()),
        failure_code: Some(failure_code.to_owned()),
    }
}

fn resolve_package_failure_response(
    slug: &str,
    error: AppError,
) -> Result<SkillMarketPackageInstallResponse, AppError> {
    let (failure_class, failure_code) = classify_package_resolution_error(&error);
    let http_status = Some(error.status_code().as_u16());
    match error {
        AppError::NotFound(message) | AppError::BadGateway(message) | AppError::Timeout(message) => {
            Ok(package_install_failure_response(
                unresolved_package_response(slug),
                "<package>".into(),
                message,
                failure_class,
                failure_code,
                http_status,
            ))
        }
        other => Err(other),
    }
}

fn unresolved_package_response(slug: &str) -> SkillMarketPackageResponse {
    SkillMarketPackageResponse {
        name: title_from_slug(slug),
        description: String::new(),
        instructions: String::new(),
        skill_slugs: Vec::new(),
        avatar: None,
    }
}

fn classify_package_resolution_error(error: &AppError) -> (&'static str, &'static str) {
    match error {
        AppError::NotFound(_) => ("deterministic", "PACKAGE_NOT_FOUND"),
        AppError::Timeout(_) => ("transient", "PACKAGE_DETAIL_NETWORK"),
        AppError::BadGateway(message) => {
            let lower = message.to_ascii_lowercase();
            if lower.contains("json parse")
                || lower.contains("missing")
                || lower.contains("mismatch")
                || lower.contains("content")
            {
                ("deterministic", "PACKAGE_DETAIL_INVALID")
            } else {
                ("transient", "PACKAGE_DETAIL_NETWORK")
            }
        }
        _ => ("local", "PACKAGE_DETAIL_LOCAL")
    }
}

fn map_market_local_error(error: impl std::fmt::Display) -> AppError {
    AppError::Internal(format!("local: {error}"))
}

fn map_market_extension_error(error: ExtensionError) -> AppError {
    match error {
        ExtensionError::Io(_) => map_market_local_error(error),
        other => other.into(),
    }
}

/// Fetch one package by slug from the SkillHub detail endpoint and build its
/// response (name, instructions, child skill slugs).
async fn resolve_market_package(req: SkillMarketPackageRequest) -> Result<SkillMarketPackageResponse, AppError> {
    if req.source != SKILLHUB_PACKAGES_SOURCE {
        return Err(AppError::BadRequest(format!(
            "unsupported package market source: {}",
            req.source
        )));
    }
    let slug = resolve_skillhub_package_slug(&req)?;

    let client = build_market_client()?;
    let url = skillhub_package_detail_url(&slug)?;
    let body = read_market_detail_body(&client, url.as_str(), &format!("SkillHub package '{slug}'")).await?;
    parse_skillhub_package_detail(&body, &slug)
}

/// Resolve a package slug from the backend-issued id and URL. A stale cache
/// may contain one malformed field, so a valid peer can recover it; two valid
/// but conflicting fields are rejected rather than silently selecting one.
fn resolve_skillhub_package_slug(req: &SkillMarketPackageRequest) -> Result<String, AppError> {
    let id_slug = market_ref_suffix(&req.id, SKILLHUB_PACKAGES_SOURCE).filter(|slug| is_market_slug(slug));
    let url_slug = last_url_segment(&req.url).filter(|slug| is_market_slug(slug));

    match (id_slug, url_slug) {
        (Some(id_slug), Some(url_slug)) if !id_slug.eq_ignore_ascii_case(&url_slug) => Err(AppError::BadRequest(
            "SkillHub package id and URL refer to different packages".into(),
        )),
        (Some(slug), _) | (_, Some(slug)) => Ok(slug),
        (None, None) => Err(AppError::BadRequest("invalid SkillHub package slug".into())),
    }
}

fn skillhub_package_detail_url(slug: &str) -> Result<reqwest::Url, AppError> {
    if !is_market_slug(slug) {
        return Err(AppError::BadRequest("invalid SkillHub package slug".into()));
    }
    let mut url = reqwest::Url::parse(SKILLHUB_PACKAGE_DETAIL_BASE_URL)
        .map_err(|e| AppError::Internal(format!("invalid SkillHub package detail URL: {e}")))?;
    url.path_segments_mut()
        .map_err(|_| AppError::Internal("invalid SkillHub package detail URL base".into()))?
        .pop_if_empty()
        .push(slug);
    Ok(url)
}

fn parse_skillhub_package_detail(body: &str, slug: &str) -> Result<SkillMarketPackageResponse, AppError> {
    let package = serde_json::from_str::<serde_json::Value>(body)
        .map_err(|e| AppError::BadGateway(format!("SkillHub package JSON parse failed: {e}")))?;
    let returned_slug = package
        .get("slug")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| is_market_slug(value))
        .ok_or_else(|| AppError::BadGateway("SkillHub package response missing slug".into()))?;
    if !returned_slug.eq_ignore_ascii_case(slug) {
        return Err(AppError::BadGateway(format!(
            "SkillHub package response slug mismatch: expected '{slug}', got '{returned_slug}'"
        )));
    }

    build_skillhub_package_response(&package, slug)
}

fn build_skillhub_package_response(
    package: &serde_json::Value,
    slug: &str,
) -> Result<SkillMarketPackageResponse, AppError> {
    let name = json_text(package, "displayName", 96)
        .or_else(|| json_text(package, "displayNameEn", 96))
        .unwrap_or_else(|| title_from_slug(slug));
    let description = json_text(package, "summary", 500)
        .or_else(|| json_text(package, "summaryEn", 500))
        .unwrap_or_default();
    let instructions = json_text_preserve(package, "content", 120_000)
        .or_else(|| json_text_preserve(package, "contentEn", 120_000))
        .ok_or_else(|| AppError::BadGateway("SkillHub package content missing".into()))?;
    let skill_slugs = package_skill_slugs(package, &instructions);
    let avatar = json_text(package, "iconUrl", 260).and_then(|url| market_https_image_url(&url));

    Ok(SkillMarketPackageResponse {
        name,
        description,
        instructions,
        skill_slugs,
        avatar,
    })
}

// ---------------------------------------------------------------------------
// Child skill install
// ---------------------------------------------------------------------------

#[derive(Default)]
struct SkillMarketPackageSkillInstallOutcome {
    installed_skill_names: Vec<String>,
    staged_skills: Vec<StagedMarketSkill>,
    errors: Vec<SkillMarketPackageInstallError>,
    _staging: Option<MarketPackageStaging>,
}

struct StagedMarketSkill {
    name: String,
    staged_dir: Option<PathBuf>,
}

/// Prepare every child Skill of a package without touching the user Skill
/// root. A package is only eligible for commit when every child has been
/// downloaded (or is a valid existing custom Skill), extracted, and strictly
/// validated.
async fn install_skillhub_package_skills(
    paths: &SkillPaths,
    skill_slugs: &[String],
) -> Result<SkillMarketPackageSkillInstallOutcome, AppError> {
    let slugs = normalize_package_skill_install_slugs(skill_slugs.to_vec());
    if slugs.is_empty() {
        return Ok(SkillMarketPackageSkillInstallOutcome {
            errors: vec![SkillMarketPackageInstallError {
                skill_slug: "<package>".into(),
                error: "expert package declares no installable Skills".into(),
                http_status: None,
            }],
            ..Default::default()
        });
    }

    let mut declared_errors = Vec::new();
    let mut seen_slugs = HashSet::new();
    for slug in &slugs {
        if slug.is_empty() {
            declared_errors.push(SkillMarketPackageInstallError {
                skill_slug: "<empty>".into(),
                error: "invalid SkillHub skill slug: empty slug".into(),
                http_status: None,
            });
        } else if !is_market_slug(slug) {
            declared_errors.push(SkillMarketPackageInstallError {
                skill_slug: slug.clone(),
                error: "invalid SkillHub skill slug".into(),
                http_status: None,
            });
        } else if !seen_slugs.insert(slug.to_ascii_lowercase()) {
            declared_errors.push(SkillMarketPackageInstallError {
                skill_slug: slug.clone(),
                error: format!("duplicate SkillHub skill slug '{slug}'"),
                http_status: None,
            });
        }
    }
    if !declared_errors.is_empty() {
        return Ok(SkillMarketPackageSkillInstallOutcome {
            errors: declared_errors,
            ..Default::default()
        });
    }

    let client = build_market_client()?;
    let staging = create_market_package_staging(paths).await?;
    let mut installed_skill_names = Vec::new();
    let mut staged_skills = Vec::new();
    let mut errors = Vec::new();

    for (index, slug) in slugs.into_iter().enumerate() {
        // A canonical market binding is user-owned. Inspect the exact target
        // before downloading so a malformed same-name directory is reported
        // as a local failure and is never overwritten by an install retry.
        let target = paths.user_skills_dir.join(&slug);
        match tokio::fs::symlink_metadata(&target).await {
            Ok(_) => match skill_service::validate_market_skill_directory(&target, &slug).await {
                Ok(validated_name) => {
                    installed_skill_names.push(validated_name.clone());
                    staged_skills.push(StagedMarketSkill {
                        name: validated_name,
                        staged_dir: None,
                    });
                    continue;
                }
                Err(error) => {
                    errors.push(SkillMarketPackageInstallError {
                        skill_slug: slug,
                        error: format!("local: existing Skill is invalid: {error}"),
                        http_status: None,
                    });
                    continue;
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                errors.push(SkillMarketPackageInstallError {
                    skill_slug: slug,
                    error: format!("local: cannot inspect existing Skill target: {error}"),
                    http_status: None,
                });
                continue;
            }
        }

        let child_result: Result<PathBuf, AppError> = async {
            let (_download_slug, archive) = download_skillhub_skill_zip(&client, &slug).await?;
            let child_root = staging.root.join(format!("{index:03}-{slug}"));
            let archive_path = child_root.join("skill.zip");
            let extract_dir = child_root.join("extract");
            tokio::fs::create_dir_all(&child_root)
                .await
                .map_err(map_market_local_error)?;
            tokio::fs::write(&archive_path, archive)
                .await
                .map_err(map_market_local_error)?;
            skill_service::extract_skill_archive_to_staging(&archive_path, &extract_dir)
                .await
                .map_err(map_market_extension_error)?;
            tokio::fs::remove_file(&archive_path)
                .await
                .map_err(map_market_local_error)?;
            let mut skill_dirs = Vec::new();
            skill_service::collect_skill_dirs_recursive(
                &extract_dir,
                &mut skill_dirs,
                skill_service::MARKET_IMPORT_SCAN_DEPTH,
            )
            .await
            .map_err(map_market_extension_error)?;
            if skill_dirs.len() != 1 {
                return Err(AppError::BadRequest(format!(
                    "SkillHub Skill '{slug}' archive contains {} Skill directories; expected exactly one",
                    skill_dirs.len()
                )));
            }
            let skill_dir = skill_dirs
                .pop()
                .expect("skill_dirs length was checked to be one");
            skill_service::validate_market_skill_directory(&skill_dir, &slug)
                .await
                .map_err(map_market_extension_error)?;
            Ok(skill_dir)
        }
        .await;

        match child_result {
            Ok(staged_dir) => {
                installed_skill_names.push(slug.clone());
                staged_skills.push(StagedMarketSkill {
                    name: slug,
                    staged_dir: Some(staged_dir),
                });
            }
            Err(error) => errors.push(SkillMarketPackageInstallError {
                skill_slug: slug,
                error: error.to_string(),
                http_status: Some(error.status_code().as_u16()),
            }),
        }
    }

    dedup_strings(&mut installed_skill_names);
    Ok(SkillMarketPackageSkillInstallOutcome {
        installed_skill_names,
        staged_skills,
        errors,
        _staging: Some(staging),
    })
}

/// Download a skill zip by slug, falling back to an exact-match search when
/// the direct download 404s. The slug is validated BEFORE any URL or temp
/// path is built from it.
async fn download_skillhub_skill_zip(
    client: &reqwest::Client,
    skill_slug: &str,
) -> Result<(String, Vec<u8>), AppError> {
    if !is_market_slug(skill_slug) {
        return Err(AppError::BadRequest("invalid SkillHub skill slug".into()));
    }

    match request_skillhub_skill_zip(client, skill_slug).await {
        Ok(bytes) => Ok((skill_slug.to_string(), bytes)),
        Err(AppError::NotFound(_)) => {
            let found_slug = search_skillhub_skill_slug(client, skill_slug).await?;
            let bytes = request_skillhub_skill_zip(client, &found_slug).await?;
            Ok((found_slug, bytes))
        }
        Err(error) => Err(error),
    }
}

async fn request_skillhub_skill_zip(client: &reqwest::Client, skill_slug: &str) -> Result<Vec<u8>, AppError> {
    let url = skillhub_skill_download_url(skill_slug)?;
    let mut response = client
        .get(url)
        .header(ACCEPT, "application/zip,application/octet-stream,*/*")
        .send()
        .await
        .map_err(map_market_fetch_error)?;
    read_market_bytes(&mut response, MAX_SKILLHUB_SKILL_ZIP_BYTES, "SkillHub skill archive").await
}

fn skillhub_skill_download_url(skill_slug: &str) -> Result<reqwest::Url, AppError> {
    if !is_market_slug(skill_slug) {
        return Err(AppError::BadRequest("invalid SkillHub skill slug".into()));
    }
    reqwest::Url::parse_with_params(SKILLHUB_SKILL_DOWNLOAD_URL, &[("slug", skill_slug)])
        .map_err(|e| AppError::Internal(format!("invalid SkillHub download URL: {e}")))
}

async fn search_skillhub_skill_slug(client: &reqwest::Client, skill_slug: &str) -> Result<String, AppError> {
    let url = reqwest::Url::parse_with_params(
        SKILLHUB_SKILL_SEARCH_URL,
        &[("q", skill_slug), ("limit", "20")],
    )
    .map_err(|e| AppError::Internal(format!("invalid SkillHub search URL: {e}")))?;
    let body = read_market_body(client, url.as_str()).await?;
    let root = serde_json::from_str::<serde_json::Value>(&body)
        .map_err(|e| AppError::BadGateway(format!("SkillHub search JSON parse failed: {e}")))?;
    select_skillhub_search_slug(&root, skill_slug)
        .ok_or_else(|| AppError::NotFound(format!("SkillHub skill '{skill_slug}' not found")))
}

fn select_skillhub_search_slug(root: &serde_json::Value, requested_slug: &str) -> Option<String> {
    let results = root
        .get("results")
        .and_then(serde_json::Value::as_array)
        .or_else(|| root.as_array())?;

    results.iter().find_map(|item| {
        let slug = json_text(item, "slug", 96)
            .or_else(|| item.get("skill").and_then(|skill| json_text(skill, "slug", 96)))?;
        if is_market_slug(&slug) && slug.eq_ignore_ascii_case(requested_slug) {
            Some(slug)
        } else {
            None
        }
    })
}

fn classify_package_failures(errors: &[SkillMarketPackageInstallError]) -> (&'static str, &'static str) {
    let mut deterministic = true;
    let mut transient = true;
    let mut local = true;
    for error in errors {
        let lower = error.error.to_ascii_lowercase();
        let is_transient = lower.contains("timeout")
            || lower.contains("timed out")
            || lower.contains("bad gateway")
            || lower.contains("rate")
            || lower.contains("dns")
            || lower.contains("tls")
            || lower.contains("connection");
        let is_local = lower.starts_with("local:")
            || lower.contains("permission denied")
            || lower.contains("access is denied");
        let is_deterministic = !is_local
            && (lower.contains("invalid")
                || lower.contains("not found")
                || lower.contains("expected exactly one")
                || lower.contains("duplicate")
                || lower.contains("no installable"));
        deterministic &= is_deterministic;
        transient &= is_transient;
        local &= is_local;
    }
    if local {
        ("local", "LOCAL_SKILL_INVALID")
    } else if deterministic {
        ("deterministic", "PACKAGE_SKILL_INVALID")
    } else if transient {
        ("transient", "PACKAGE_SKILL_NETWORK")
    } else {
        ("mixed", "PACKAGE_SKILL_FAILURE")
    }
}

// ---------------------------------------------------------------------------
// Child slug extraction (skillSlugs field + frontmatter children)
// ---------------------------------------------------------------------------

fn package_skill_slugs(package: &serde_json::Value, instructions: &str) -> Vec<String> {
    let mut slugs = package
        .get("skillSlugs")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    slugs.extend(frontmatter_child_slugs(instructions));
    normalize_package_skill_slugs(slugs)
}

fn normalize_package_skill_slugs(slugs: Vec<String>) -> Vec<String> {
    slugs
        .into_iter()
        .map(|value| value.trim().to_string())
        // Metadata echoes are known non-Skill fields. Every other declared
        // value is retained, including invalid/duplicate slugs, so the
        // atomic installer can report the package as incomplete instead of
        // silently shrinking its required Skill set.
        .filter(|value| !is_package_metadata_field(value))
        .collect()
}

/// Looser variant used at install time: keeps invalid slugs so the install
/// loop can report a per-slug error instead of silently dropping them.
fn normalize_package_skill_install_slugs(slugs: Vec<String>) -> Vec<String> {
    normalize_package_skill_slugs(slugs)
}

/// SkillHub package `skillSlugs` arrays sometimes echo frontmatter field
/// names; those are metadata, not installable skills.
fn is_package_metadata_field(value: &str) -> bool {
    const FIELDS: &[&str] = &[
        "aliases",
        "author",
        "children",
        "compatibility",
        "description",
        "display_name",
        "metadata",
        "name",
        "orchestration",
        "package_type",
        "version",
    ];
    FIELDS.iter().any(|field| value.eq_ignore_ascii_case(field))
}

fn frontmatter_child_slugs(markdown: &str) -> Vec<String> {
    let Some(frontmatter) = markdown_frontmatter(markdown) else {
        return Vec::new();
    };
    let Ok(root) = serde_yaml::from_str::<serde_yaml::Value>(frontmatter) else {
        return Vec::new();
    };
    let Some(children) = root.get("orchestration").and_then(|value| value.get("children")) else {
        return Vec::new();
    };

    children
        .as_sequence()
        .into_iter()
        .flatten()
        .filter_map(serde_yaml::Value::as_str)
        .map(str::trim)
        .map(str::to_string)
        .collect()
}

/// Return the YAML frontmatter body of a markdown document: the content
/// between a leading `---` line and the closing `---`/`...` line. `None`
/// when the document has no (closed) frontmatter block.
fn markdown_frontmatter(markdown: &str) -> Option<&str> {
    let markdown = markdown.strip_prefix('\u{feff}').unwrap_or(markdown);
    let rest = markdown
        .strip_prefix("---\r\n")
        .or_else(|| markdown.strip_prefix("---\n"))?;

    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed == "---" || trimmed == "..." {
            return Some(rest[..offset].trim());
        }
        offset += line.len();
    }

    None
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_paths() -> SkillPaths {
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
        std::mem::forget(tmp);
        paths
    }

    #[test]
    fn build_skillhub_package_response_uses_real_child_skills() {
        let package = serde_json::json!({
            "slug": "tech-test-automation",
            "displayName": "Test Automation",
            "summary": "End-to-end automated testing workflow.",
            "skillSlugs": ["name", "superpowers-tdd", "description"],
            "iconUrl": "https://cloudcache.tencent-cloud.com/qcloud/tea/app/skillhub/assets/source/ai-buddy-decouple/expert-profiles/tech-test-automation.v20260625.avif",
            "content": "---\nname: tech-test-automation\ndescription: Test package\nmetadata:\n  author: SkillHub\norchestration:\n  children:\n    - test-case-generator\n    - metadata\n---\n# Test Automation\nUse this package."
        });

        let response = build_skillhub_package_response(&package, "tech-test-automation").unwrap();

        assert_eq!(response.skill_slugs, vec!["superpowers-tdd", "test-case-generator"]);
        assert_eq!(
            response.avatar.as_deref(),
            Some("https://cloudcache.tencent-cloud.com/qcloud/tea/app/skillhub/assets/source/ai-buddy-decouple/expert-profiles/tech-test-automation.v20260625.avif")
        );
        assert!(response.instructions.starts_with("---\nname: tech-test-automation"));
        assert!(response.instructions.contains("metadata:"));
        assert!(response.instructions.contains("# Test Automation"));
    }

    #[test]
    fn package_detail_url_targets_one_validated_slug() {
        assert_eq!(
            skillhub_package_detail_url("tech-test-automation")
                .unwrap()
                .as_str(),
            "https://api.skillhub.cn/api/v1/skillsets/tech-test-automation"
        );
        assert!(skillhub_package_detail_url("../tech-test-automation").is_err());
    }

    #[test]
    fn package_slug_recovers_one_stale_field_but_rejects_conflicts() {
        let valid = SkillMarketPackageRequest {
            source: SKILLHUB_PACKAGES_SOURCE.into(),
            id: "skillhub_packages:tech-test-automation".into(),
            url: "https://skillhub.cn/skillspackage/tech-test-automation".into(),
            preset_id: None,
        };
        assert_eq!(
            resolve_skillhub_package_slug(&valid).unwrap(),
            "tech-test-automation"
        );

        let stale_id = SkillMarketPackageRequest {
            id: "skillhub_packages:invalid slug".into(),
            ..valid.clone()
        };
        assert_eq!(
            resolve_skillhub_package_slug(&stale_id).unwrap(),
            "tech-test-automation"
        );

        let conflicting = SkillMarketPackageRequest {
            url: "https://skillhub.cn/skillspackage/another-package".into(),
            ..valid
        };
        assert!(resolve_skillhub_package_slug(&conflicting).is_err());
    }

    #[test]
    fn package_detail_parser_requires_matching_slug_and_content() {
        let body = serde_json::json!({
            "slug": "tech-test-automation",
            "displayName": "Test Automation",
            "summary": "End-to-end automated testing workflow.",
            "skillSlugs": ["superpowers-tdd"],
            "content": "# Test Automation\nUse this package."
        })
        .to_string();

        let package = parse_skillhub_package_detail(&body, "tech-test-automation").unwrap();
        assert_eq!(package.name, "Test Automation");
        assert_eq!(package.skill_slugs, vec!["superpowers-tdd"]);
        assert!(parse_skillhub_package_detail(&body, "another-package").is_err());

        let missing_content = serde_json::json!({ "slug": "tech-test-automation" }).to_string();
        assert!(parse_skillhub_package_detail(&missing_content, "tech-test-automation").is_err());
    }

    #[test]
    fn package_detail_failures_are_classified_for_legacy_clients() {
        let missing = resolve_package_failure_response(
            "removed-package",
            AppError::NotFound("SkillHub package not found".into()),
        )
        .unwrap();
        assert_eq!(missing.failure_class.as_deref(), Some("deterministic"));
        assert_eq!(missing.failure_code.as_deref(), Some("PACKAGE_NOT_FOUND"));
        assert_eq!(missing.errors[0].http_status, Some(404));

        let network = resolve_package_failure_response(
            "slow-package",
            AppError::Timeout("skill market fetch timed out".into()),
        )
        .unwrap();
        assert_eq!(network.failure_class.as_deref(), Some("transient"));
        assert_eq!(network.failure_code.as_deref(), Some("PACKAGE_DETAIL_NETWORK"));

        let invalid = resolve_package_failure_response(
            "invalid-package",
            AppError::BadGateway("SkillHub package JSON parse failed".into()),
        )
        .unwrap();
        assert_eq!(invalid.failure_class.as_deref(), Some("deterministic"));
        assert_eq!(invalid.failure_code.as_deref(), Some("PACKAGE_DETAIL_INVALID"));
    }

    #[test]
    fn package_child_slug_preserves_the_full_backend_limit() {
        let max_slug = format!("a{}z", "b".repeat(94));
        let over_limit = format!("a{}z", "b".repeat(95));
        let package = serde_json::json!({ "skillSlugs": [max_slug, over_limit.clone()] });

        let slugs = package_skill_slugs(&package, "# Package");

        assert_eq!(
            slugs,
            vec![
                format!("a{}z", "b".repeat(94)),
                over_limit.clone(),
            ]
        );

        let invalid_for_reporting = normalize_package_skill_install_slugs(vec![over_limit.clone()]);
        assert_eq!(invalid_for_reporting, vec![over_limit]);
        assert!(!is_market_slug(&invalid_for_reporting[0]));
    }

    /// Manual smoke test for the exact lightweight endpoint used by Add.
    /// Ignored in normal test runs because SkillHub is outside NomiFun's
    /// availability control.
    #[tokio::test]
    #[ignore = "requires public SkillHub access"]
    async fn live_skillhub_package_detail_contract() {
        let package = resolve_market_package(SkillMarketPackageRequest {
            source: SKILLHUB_PACKAGES_SOURCE.into(),
            id: "skillhub_packages:tech-test-automation".into(),
            url: "https://skillhub.cn/skillspackage/tech-test-automation".into(),
            preset_id: None,
        })
        .await
        .unwrap();

        assert!(!package.name.is_empty());
        assert!(!package.instructions.is_empty());
        assert!(!package.skill_slugs.is_empty());
    }

    /// Manual smoke test for the official API -> Tencent COS redirect used by
    /// child-skill archives. The redirect target remains exact-allowlisted.
    #[tokio::test]
    #[ignore = "requires public SkillHub and Tencent COS access"]
    async fn live_skillhub_child_archive_redirect_contract() {
        let client = build_market_client().unwrap();
        let archive = request_skillhub_skill_zip(&client, "superpowers-tdd")
            .await
            .unwrap();

        assert!(archive.starts_with(b"PK"));
    }

    #[test]
    fn markdown_frontmatter_requires_closed_leading_block() {
        let doc = "---\nname: x\norchestration:\n  children:\n    - a\n---\nbody";
        assert_eq!(
            markdown_frontmatter(doc),
            Some("name: x\norchestration:\n  children:\n    - a")
        );
        // CRLF and `...` terminator variants.
        assert_eq!(markdown_frontmatter("---\r\nname: y\r\n---\r\nbody"), Some("name: y"));
        assert_eq!(markdown_frontmatter("---\nname: z\n...\n"), Some("name: z"));
        // No frontmatter / unterminated block.
        assert_eq!(markdown_frontmatter("# heading"), None);
        assert_eq!(markdown_frontmatter("---\nname: never closed"), None);
    }

    #[test]
    fn skillhub_skill_download_url_rejects_unsafe_slug() {
        assert!(skillhub_skill_download_url("superpowers-tdd").is_ok());
        assert!(skillhub_skill_download_url("../superpowers-tdd").is_err());
        assert!(skillhub_skill_download_url("owner/skill").is_err());
    }

    #[test]
    fn select_skillhub_search_slug_requires_exact_safe_slug() {
        let root = serde_json::json!({
            "results": [
                { "slug": "superpowers-tdd-extra", "displayName": "Superpowers TDD Extra" },
                { "skill": { "slug": "superpowers-tdd" }, "displayName": "Superpowers TDD" },
                { "slug": "../bad", "displayName": "Bad" }
            ]
        });

        assert_eq!(
            select_skillhub_search_slug(&root, "superpowers-tdd"),
            Some("superpowers-tdd".into())
        );
        assert_eq!(select_skillhub_search_slug(&root, "missing"), None);
    }

    #[tokio::test]
    async fn install_skillhub_package_skills_uses_existing_available_skill() {
        let paths = make_paths();
        let skill_dir = paths.user_skills_dir.join("superpowers-tdd");
        tokio::fs::create_dir_all(&skill_dir).await.unwrap();
        tokio::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: superpowers-tdd\ndescription: TDD workflow\n---\n# Superpowers TDD",
        )
        .await
        .unwrap();

        let installed = install_skillhub_package_skills(&paths, &["superpowers-tdd".into()])
            .await
            .unwrap();

        assert_eq!(installed.installed_skill_names, vec!["superpowers-tdd"]);
        assert!(installed.errors.is_empty());
    }

    #[tokio::test]
    async fn install_skillhub_package_skills_rejects_invalid_declaration_before_staging() {
        let paths = make_paths();
        let skill_dir = paths.user_skills_dir.join("superpowers-tdd");
        tokio::fs::create_dir_all(&skill_dir).await.unwrap();
        tokio::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: superpowers-tdd\ndescription: TDD workflow\n---\n# Superpowers TDD",
        )
        .await
        .unwrap();

        let installed = install_skillhub_package_skills(
            &paths,
            &["../missing-child".into(), "superpowers-tdd".into()],
        )
        .await
        .unwrap();

        assert!(installed.installed_skill_names.is_empty());
        assert_eq!(installed.errors.len(), 1);
        assert_eq!(installed.errors[0].skill_slug, "../missing-child");
        assert!(installed.errors[0].error.contains("invalid SkillHub skill slug"));
        assert!(!paths.user_skills_dir.join(".market-import").exists());
    }

    #[tokio::test]
    async fn install_skillhub_package_skills_rejects_duplicate_declaration() {
        let paths = make_paths();
        let installed = install_skillhub_package_skills(
            &paths,
            &["superpowers-tdd".into(), "superpowers-tdd".into()],
        )
        .await
        .unwrap();

        assert!(installed.installed_skill_names.is_empty());
        assert_eq!(installed.errors.len(), 1);
        assert!(installed.errors[0].error.contains("duplicate"));
        assert!(!paths.user_skills_dir.join(".market-import").exists());
    }

    #[tokio::test]
    async fn install_skillhub_package_skills_does_not_reuse_invalid_existing_skill() {
        let paths = make_paths();
        let skill_dir = paths.user_skills_dir.join("superpowers-tdd");
        tokio::fs::create_dir_all(&skill_dir).await.unwrap();
        tokio::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: another-skill\ndescription: Wrong identity\n---\n# Wrong",
        )
        .await
        .unwrap();

        let installed = install_skillhub_package_skills(&paths, &["superpowers-tdd".into()])
            .await
            .unwrap();

        assert!(installed.installed_skill_names.is_empty());
        assert_eq!(installed.errors.len(), 1);
        assert!(installed.errors[0].error.contains("existing Skill"));
        assert!(skill_dir.join("SKILL.md").exists());
    }
}
