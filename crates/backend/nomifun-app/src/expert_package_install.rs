//! Install a Flowy catalog expert by downloading its package and writing a
//! user preset. The package is a WorkBuddy plugin tree; the preset is the
//! same record a person would save from the preset editor.

use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::{Path as AxumPath, State};
use axum::extract::rejection::JsonRejection;
use axum::routing::{get, post};
use nomifun_api_types::{
    CreatePresetRequest, ExpertDetail, ExpertInstallIndex, ExpertInstallResponse, PresetTarget,
    SetPresetStateRequest, SkillBinding, SkillCatalogSource, SkillId,
};
use nomifun_cloud::{CloudService, prepare_expert_slug};
use nomifun_common::AppError;
use nomifun_extension::{SkillPaths, delete_skill, import_skill, read_skill_info};
use nomifun_importer::frontmatter::parse_agent;
use nomifun_importer::manifest::{LocalizedText, PluginManifest, parse_plugin_manifest};
use nomifun_preset::PresetService;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use url::Url;

const MAX_ZIP_BYTES: u64 = 128 * 1024 * 1024;
const MAX_AVATAR_BYTES: u64 = 2 * 1024 * 1024;
const MAX_INSTRUCTIONS_CHARS: usize = 200_000;
const MAX_EXAMPLE_CHARS: usize = 500;
const MAX_EXAMPLES: usize = 12;
const REGISTRY_FILE: &str = "expert-installs.json";
/// Current catalog packages use `.flowy-plugin`. Older WorkBuddy packages use
/// `.codebuddy-plugin`. The first file that exists wins.
const PLUGIN_MANIFEST_RELATIVES: &[&str] = &[
    ".flowy-plugin/plugin.json",
    ".codebuddy-plugin/plugin.json",
];

#[derive(Clone)]
pub struct ExpertInstallState {
    cloud: Arc<CloudService>,
    presets: Arc<PresetService>,
    skill_paths: SkillPaths,
    data_dir: PathBuf,
    http: reqwest::Client,
    gate: Arc<Mutex<()>>,
}

impl ExpertInstallState {
    pub fn new(
        cloud: Arc<CloudService>,
        presets: Arc<PresetService>,
        skill_paths: SkillPaths,
        data_dir: PathBuf,
    ) -> Self {
        let http = reqwest::Client::builder()
            // ModelScope's CDN answers the default reqwest user agent with
            // HTTP 403 (`denied by UA ACL = blacklist`). Name this client so
            // the package redirect can be fetched.
            .user_agent(format!("nomifun/{}", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 4 {
                    return attempt.error(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "too many redirects",
                    ));
                }
                if download_url_allowed(attempt.url()).is_err() {
                    return attempt.error(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "expert package url is not allowed",
                    ));
                }
                attempt.follow()
            }))
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            cloud,
            presets,
            skill_paths,
            data_dir,
            http,
            gate: Arc::new(Mutex::new(())),
        }
    }
}

pub fn expert_install_routes(state: ExpertInstallState) -> Router {
    Router::new()
        .route("/api/cloud/expert-installs", get(list_installed))
        .route("/api/cloud/experts/{slug}/install", post(install_expert))
        .with_state(state)
}

async fn list_installed(
    State(state): State<ExpertInstallState>,
) -> Result<Json<nomifun_api_types::ApiResponse<ExpertInstallIndex>>, AppError> {
    let _guard = state.gate.lock().await;
    let slugs = prune_registry(&state).await?;
    Ok(Json(nomifun_api_types::ApiResponse::ok(ExpertInstallIndex { slugs })))
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpertInstallBody {
    #[serde(default)]
    id: String,
}

async fn install_expert(
    State(state): State<ExpertInstallState>,
    AxumPath(slug): AxumPath<String>,
    body: Result<Json<ExpertInstallBody>, JsonRejection>,
) -> Result<Json<nomifun_api_types::ApiResponse<ExpertInstallResponse>>, AppError> {
    let slug = prepare_expert_slug(&slug).map_err(|message| AppError::BadRequest(message.into()))?;
    let listed_id = body
        .map(|Json(payload)| payload.id)
        .unwrap_or_default();
    let _guard = state.gate.lock().await;
    // Every summon records one download, including a repeat of an expert that
    // is already a local preset. The id is the catalog list row id.
    let mut counted = false;
    if !listed_id.trim().is_empty() {
        state.cloud.record_expert_download(listed_id.trim()).await?;
        counted = true;
    }
    if let Some(existing) = existing_install(&state, &slug).await? {
        if !counted {
            let detail = state.cloud.get_expert(&slug).await?;
            state.cloud.record_expert_download(&detail.summary.id).await?;
        }
        return Ok(Json(nomifun_api_types::ApiResponse::ok(existing)));
    }

    let detail = state.cloud.get_expert(&slug).await?;
    if !counted {
        state.cloud.record_expert_download(&detail.summary.id).await?;
    }
    let bytes = download_package(&state.http, &detail).await?;
    let staging = tempfile::tempdir().map_err(|error| AppError::Internal(error.to_string()))?;
    let archive_path = staging.path().join("package.zip");
    std::fs::write(&archive_path, &bytes).map_err(|error| AppError::Internal(error.to_string()))?;
    let extracted = staging.path().join("tree");
    std::fs::create_dir_all(&extracted).map_err(|error| AppError::Internal(error.to_string()))?;
    crate::market_source::extract_zip_market(&archive_path, &extracted)
        .map_err(|error| AppError::BadRequest(format!("expert package: {error}")))?;

    let root = locate_plugin_root(&extracted, &slug)?;
    let mut projected = project_plugin(&root)?;
    apply_catalog_fallback(&mut projected, &detail);
    if projected.name.trim().is_empty() {
        projected.name = slug.clone();
    }

    let (skill_ids, created_skills, mut warnings) =
        import_projected_skills(&state.skill_paths, &projected.skill_dirs).await;
    warnings.extend(std::mem::take(&mut projected.warnings));

    let preset_id = match write_preset(&state, &projected, skill_ids.clone()).await {
        Ok(id) => id,
        Err(error) => {
            for name in &created_skills {
                let _ = delete_skill(&state.skill_paths, name).await;
            }
            return Err(error);
        }
    };
    if let Some(avatar) = projected.avatar_file.as_ref() {
        if let Err(error) = install_avatar(&state, &preset_id, avatar).await {
            warnings.push(format!("avatar was not saved: {error}"));
        }
    }
    remember_install(&state, &slug, &preset_id)?;

    Ok(Json(nomifun_api_types::ApiResponse::ok(ExpertInstallResponse {
        preset_id,
        name: projected.name,
        already_installed: false,
        installed_skills: skill_ids
            .into_iter()
            .map(|id| {
                id.rsplit(':')
                    .next()
                    .unwrap_or(id.as_str())
                    .to_owned()
            })
            .collect(),
        warnings,
    })))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct InstallRegistry {
    installs: BTreeMap<String, String>,
}

#[derive(Debug)]
struct ProjectedExpert {
    name: String,
    name_i18n: HashMap<String, String>,
    description: Option<String>,
    description_i18n: HashMap<String, String>,
    routing_description: Option<String>,
    instructions: String,
    examples: Vec<String>,
    examples_i18n: HashMap<String, Vec<String>>,
    skill_dirs: Vec<PathBuf>,
    avatar_file: Option<PathBuf>,
    warnings: Vec<String>,
}

fn registry_path(data_dir: &Path) -> PathBuf {
    data_dir.join(REGISTRY_FILE)
}

fn read_registry(data_dir: &Path) -> Result<InstallRegistry, AppError> {
    let path = registry_path(data_dir);
    if !path.is_file() {
        return Ok(InstallRegistry::default());
    }
    let text = std::fs::read_to_string(&path).map_err(|error| AppError::Internal(error.to_string()))?;
    serde_json::from_str(&text).map_err(|error| AppError::Internal(format!("expert install index: {error}")))
}

fn write_registry(data_dir: &Path, registry: &InstallRegistry) -> Result<(), AppError> {
    let path = registry_path(data_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| AppError::Internal(error.to_string()))?;
    }
    let text = serde_json::to_string_pretty(registry)
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|error| AppError::Internal(error.to_string()))?;
    std::fs::rename(&tmp, &path).map_err(|error| AppError::Internal(error.to_string()))?;
    Ok(())
}

async fn prune_registry(state: &ExpertInstallState) -> Result<Vec<String>, AppError> {
    let mut registry = read_registry(&state.data_dir)?;
    let mut slugs = Vec::new();
    let known: Vec<(String, String)> = registry.installs.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    for (slug, preset_id) in known {
        match state.presets.get(&preset_id).await {
            Ok(_) => slugs.push(slug),
            Err(AppError::NotFound(_)) => {
                registry.installs.remove(&slug);
            }
            Err(error) => return Err(error),
        }
    }
    slugs.sort();
    write_registry(&state.data_dir, &registry)?;
    Ok(slugs)
}

async fn existing_install(
    state: &ExpertInstallState,
    slug: &str,
) -> Result<Option<ExpertInstallResponse>, AppError> {
    let registry = read_registry(&state.data_dir)?;
    let Some(preset_id) = registry.installs.get(slug).cloned() else {
        return Ok(None);
    };
    match state.presets.get(&preset_id).await {
        Ok(preset) => Ok(Some(ExpertInstallResponse {
            preset_id,
            name: preset.name,
            already_installed: true,
            installed_skills: preset
                .included_skills
                .iter()
                .map(|skill| skill.skill_id.clone())
                .collect(),
            warnings: Vec::new(),
        })),
        Err(AppError::NotFound(_)) => {
            let mut registry = registry;
            registry.installs.remove(slug);
            write_registry(&state.data_dir, &registry)?;
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn remember_install(state: &ExpertInstallState, slug: &str, preset_id: &str) -> Result<(), AppError> {
    let mut registry = read_registry(&state.data_dir)?;
    registry.installs.insert(slug.to_owned(), preset_id.to_owned());
    write_registry(&state.data_dir, &registry)
}

fn download_url_allowed(url: &Url) -> Result<(), AppError> {
    if url.scheme() != "https" {
        return Err(AppError::BadRequest("expert package url must be https".into()));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AppError::BadRequest("expert package url must not carry credentials".into()));
    }
    let host = url
        .host_str()
        .ok_or_else(|| AppError::BadRequest("expert package url has no host".into()))?;
    if host_name_blocked(host) {
        return Err(AppError::BadRequest("expert package url host is not allowed".into()));
    }
    if let Some(ip) = url_host_ip(url) {
        if ip_blocked(ip) {
            return Err(AppError::BadRequest("expert package url host is not allowed".into()));
        }
    }
    Ok(())
}

fn host_name_blocked(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    host == "localhost"
        || host == "localhost.localdomain"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host == "metadata.google.internal"
}

fn url_host_ip(url: &Url) -> Option<IpAddr> {
    match url.host()? {
        url::Host::Ipv4(ip) => Some(IpAddr::V4(ip)),
        url::Host::Ipv6(ip) => Some(IpAddr::V6(ip)),
        url::Host::Domain(_) => None,
    }
}

fn ip_blocked(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.is_documentation()
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_unicast_link_local()
                || (ip.segments()[0] & 0xfe00) == 0xfc00
        }
    }
}

async fn ensure_public_host(url: &Url) -> Result<(), AppError> {
    download_url_allowed(url)?;
    if url_host_ip(url).is_some() {
        return Ok(());
    }
    let host = url
        .host_str()
        .ok_or_else(|| AppError::BadRequest("expert package url has no host".into()))?;
    let port = url.port_or_known_default().unwrap_or(443);
    let addrs = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| AppError::BadGateway("expert package host could not be resolved".into()))?;
    let mut saw = false;
    for addr in addrs {
        saw = true;
        if ip_blocked(addr.ip()) {
            return Err(AppError::BadRequest("expert package url host is not allowed".into()));
        }
    }
    if !saw {
        return Err(AppError::BadGateway("expert package host could not be resolved".into()));
    }
    Ok(())
}

async fn download_package(client: &reqwest::Client, detail: &ExpertDetail) -> Result<Vec<u8>, AppError> {
    let raw = detail.summary.download_url.trim();
    if raw.is_empty() {
        return Err(AppError::BadRequest("expert package has no download url".into()));
    }
    let url = Url::parse(raw).map_err(|_| AppError::BadRequest("expert package url is invalid".into()))?;
    ensure_public_host(&url).await?;
    let response = client.get(url).send().await.map_err(|error| {
        AppError::BadGateway(format!("expert package download failed: {error}"))
    })?;
    if let Err(error) = download_url_allowed(response.url()) {
        return Err(error);
    }
    if !response.status().is_success() {
        return Err(AppError::BadGateway(format!(
            "expert package download returned {}",
            response.status()
        )));
    }
    if response.content_length().is_some_and(|len| len > MAX_ZIP_BYTES) {
        return Err(AppError::BadRequest("expert package is too large".into()));
    }
    let mut bytes = Vec::new();
    let mut stream = response;
    while let Some(chunk) = stream
        .chunk()
        .await
        .map_err(|error| AppError::BadGateway(format!("expert package download failed: {error}")))?
    {
        if bytes.len() as u64 + chunk.len() as u64 > MAX_ZIP_BYTES {
            return Err(AppError::BadRequest("expert package is too large".into()));
        }
        bytes.extend_from_slice(&chunk);
    }
    let expected = detail.summary.sha256.trim();
    if !expected.is_empty() {
        let actual = hex::encode(Sha256::digest(&bytes));
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(AppError::BadRequest("expert package checksum does not match".into()));
        }
    }
    if !bytes.starts_with(b"PK") {
        return Err(AppError::BadRequest("expert package is not a zip archive".into()));
    }
    Ok(bytes)
}

fn locate_plugin_root(extracted: &Path, slug: &str) -> Result<PathBuf, AppError> {
    let mut found = Vec::new();
    collect_plugin_roots(extracted, 0, &mut found);
    if found.is_empty() {
        return Err(AppError::BadRequest(
            "expert package has no plugin.json".into(),
        ));
    }
    if let Some(matched) = found.iter().find(|path| {
        path.file_name().and_then(|name| name.to_str()) == Some(slug)
    }) {
        return Ok(matched.clone());
    }
    found.sort_by_key(|path| path.components().count());
    Ok(found.remove(0))
}

fn collect_plugin_roots(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    if depth > 4 || !dir.is_dir() {
        return;
    }
    if plugin_manifest_file(dir).is_some() {
        found.push(dir.to_path_buf());
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.path().is_dir() {
            collect_plugin_roots(&entry.path(), depth + 1, found);
        }
    }
}

fn plugin_manifest_file(root: &Path) -> Option<PathBuf> {
    PLUGIN_MANIFEST_RELATIVES
        .iter()
        .map(|relative| root.join(relative))
        .find(|path| path.is_file())
}

fn project_plugin(root: &Path) -> Result<ProjectedExpert, AppError> {
    let manifest_path = plugin_manifest_file(root)
        .ok_or_else(|| AppError::BadRequest("expert package has no plugin.json".into()))?;
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|error| AppError::BadRequest(format!("plugin.json: {error}")))?;
    let manifest = parse_plugin_manifest(&text)
        .map_err(|error| AppError::BadRequest(format!("plugin.json: {error}")))?;
    let agent_files = agent_files(root, &manifest);
    let primary = choose_primary(&manifest, &agent_files);
    let agent = primary.as_ref().and_then(|path| read_agent(path));
    let display_name = manifest.display_name.clone().filter(|text| !text.is_empty());
    let agent_name = agent.as_ref().and_then(|doc| {
        if doc.display_name.is_empty() {
            None
        } else {
            Some(doc.display_name.clone())
        }
    });
    let name_source = display_name.or(agent_name);
    let name = name_source
        .as_ref()
        .and_then(|text| text.primary())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .unwrap_or(manifest.name.trim())
        .chars()
        .take(120)
        .collect::<String>();
    let description_source = manifest.localized_display_description();
    let description = description_source
        .as_ref()
        .and_then(|text| text.primary())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            manifest
                .description
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        })
        .or_else(|| {
            agent
                .as_ref()
                .and_then(|doc| doc.description.as_deref())
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        });
    let routing = manifest
        .description
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
        .or_else(|| description.clone());
    let instructions = agent
        .as_ref()
        .map(|doc| clip_chars(&doc.body, MAX_INSTRUCTIONS_CHARS))
        .unwrap_or_default();
    let prompts = prompt_list(&manifest);
    Ok(ProjectedExpert {
        name_i18n: locale_map(name_source.as_ref()),
        description_i18n: locale_map(description_source.as_ref()),
        name,
        description,
        routing_description: routing,
        instructions,
        examples: examples_for(&prompts, true),
        examples_i18n: examples_i18n(&prompts),
        skill_dirs: skill_dirs(root, &manifest),
        avatar_file: avatar_file(root, &manifest),
        warnings: Vec::new(),
    })
}

fn apply_catalog_fallback(projected: &mut ProjectedExpert, detail: &ExpertDetail) {
    if projected.name_i18n.is_empty() {
        if !detail.summary.name.trim().is_empty() {
            projected.name = clip_chars(detail.summary.name.trim(), 120);
            projected
                .name_i18n
                .insert("zh-CN".into(), projected.name.clone());
        }
        if !detail.summary.name_en.trim().is_empty() {
            projected
                .name_i18n
                .insert("en-US".into(), detail.summary.name_en.trim().to_owned());
            if projected.name.trim().is_empty() {
                projected.name = clip_chars(detail.summary.name_en.trim(), 120);
            }
        }
    }
    if projected.description.as_deref().unwrap_or("").trim().is_empty() {
        let summary = detail.summary.summary.trim();
        let summary_en = detail.summary.summary_en.trim();
        let description = detail.description.trim();
        let chosen = if !summary.is_empty() {
            summary
        } else if !description.is_empty() {
            description
        } else {
            summary_en
        };
        if !chosen.is_empty() {
            projected.description = Some(chosen.to_owned());
        }
        if !summary.is_empty() {
            projected.description_i18n.insert("zh-CN".into(), summary.to_owned());
        }
        if !summary_en.is_empty() {
            projected.description_i18n.insert("en-US".into(), summary_en.to_owned());
        }
    }
    if projected.instructions.trim().is_empty() && !detail.detail_markdown.trim().is_empty() {
        projected.instructions = clip_chars(detail.detail_markdown.trim(), MAX_INSTRUCTIONS_CHARS);
    }
}

fn agent_files(root: &Path, manifest: &PluginManifest) -> Vec<PathBuf> {
    let declared = if manifest.agents.is_empty() {
        vec!["./agents".to_owned()]
    } else {
        manifest.agents.clone()
    };
    let mut files = Vec::new();
    for raw in declared {
        let Some(relative) = safe_rel(&raw) else {
            continue;
        };
        let path = root.join(relative);
        if path.is_file() && is_markdown(&path) {
            files.push(path);
        } else if path.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&path) {
                let mut children: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
                children.sort();
                for child in children {
                    if child.is_file() && is_markdown(&child) {
                        files.push(child);
                    }
                }
            }
        }
    }
    files.sort();
    files.dedup();
    files
}

fn choose_primary(manifest: &PluginManifest, files: &[PathBuf]) -> Option<PathBuf> {
    let wanted = manifest
        .team_info
        .as_ref()
        .map(|team| team.lead_agent.trim().to_owned())
        .filter(|name| !name.is_empty())
        .or_else(|| {
            manifest
                .agent_name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
        });
    if let Some(wanted) = wanted.as_deref() {
        if let Some(found) = files.iter().find(|path| agent_matches(path, wanted)) {
            return Some(found.clone());
        }
    }
    files.first().cloned()
}

fn agent_matches(path: &Path, wanted: &str) -> bool {
    if path.file_stem().and_then(|stem| stem.to_str()) == Some(wanted) {
        return true;
    }
    read_agent(path).is_some_and(|doc| doc.name == wanted)
}

fn read_agent(path: &Path) -> Option<nomifun_importer::frontmatter::AgentDoc> {
    let text = std::fs::read_to_string(path).ok()?;
    parse_agent(&text, path.to_string_lossy().as_ref()).ok().or_else(|| {
        let body = markdown_body(&text);
        if body.trim().is_empty() {
            return None;
        }
        Some(nomifun_importer::frontmatter::AgentDoc {
            name: path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("agent").to_owned(),
            description: None,
            model: None,
            effort: None,
            max_turns: None,
            tools: Vec::new(),
            disallowed_tools: Vec::new(),
            skills: Vec::new(),
            memory: None,
            background: None,
            isolation: None,
            permission_mode: None,
            display_name: LocalizedText::default(),
            profession: LocalizedText::default(),
            body,
        })
    })
}

fn markdown_body(text: &str) -> String {
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    let Some(rest) = trimmed.strip_prefix("---") else {
        return trimmed.trim().to_owned();
    };
    let rest = rest.trim_start_matches('\r');
    let Some(rest) = rest.strip_prefix('\n') else {
        return trimmed.trim().to_owned();
    };
    if let Some(end) = rest.find("\n---") {
        return rest[end + 4..].trim().to_owned();
    }
    trimmed.trim().to_owned()
}

fn skill_dirs(root: &Path, manifest: &PluginManifest) -> Vec<PathBuf> {
    let declared = if manifest.skills.is_empty() {
        vec!["./skills".to_owned()]
    } else {
        manifest.skills.clone()
    };
    let mut dirs = Vec::new();
    for raw in declared {
        let Some(relative) = safe_rel(&raw) else {
            continue;
        };
        let path = root.join(relative);
        if path.join("SKILL.md").is_file() {
            dirs.push(path);
            continue;
        }
        if !path.is_dir() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(&path) {
            let mut children: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
            children.sort();
            for child in children {
                if child.join("SKILL.md").is_file() {
                    dirs.push(child);
                }
            }
        }
    }
    dirs.sort();
    dirs.dedup();
    dirs
}

fn avatar_file(root: &Path, manifest: &PluginManifest) -> Option<PathBuf> {
    let relative = safe_rel(manifest.avatar.as_deref()?)?;
    let path = root.join(relative);
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif") {
        return None;
    }
    let len = std::fs::metadata(&path).ok()?.len();
    if !path.is_file() || len == 0 || len > MAX_AVATAR_BYTES {
        return None;
    }
    Some(path)
}

fn prompt_list(manifest: &PluginManifest) -> Vec<LocalizedText> {
    let mut prompts = Vec::new();
    if let Some(init) = manifest.default_init_prompt.clone() {
        if !init.is_empty() {
            prompts.push(init);
        }
    }
    prompts.extend(manifest.quick_prompts.clone());
    prompts
}

fn examples_for(prompts: &[LocalizedText], prefer_zh: bool) -> Vec<String> {
    let mut out = Vec::new();
    for prompt in prompts {
        let text = if prefer_zh {
            prompt.primary()
        } else {
            prompt.en.as_deref().or(prompt.zh.as_deref())
        };
        let Some(text) = text.map(str::trim).filter(|text| !text.is_empty()) else {
            continue;
        };
        let text = clip_chars(text, MAX_EXAMPLE_CHARS);
        if !out.iter().any(|existing: &String| existing == &text) {
            out.push(text);
        }
        if out.len() == MAX_EXAMPLES {
            break;
        }
    }
    out
}

fn examples_i18n(prompts: &[LocalizedText]) -> HashMap<String, Vec<String>> {
    let mut map = HashMap::new();
    let zh = localized_examples(prompts, true);
    let en = localized_examples(prompts, false);
    if !zh.is_empty() {
        map.insert("zh-CN".into(), zh);
    }
    if !en.is_empty() {
        map.insert("en-US".into(), en);
    }
    map
}

fn localized_examples(prompts: &[LocalizedText], zh: bool) -> Vec<String> {
    let mut out = Vec::new();
    for prompt in prompts {
        let text = if zh { prompt.zh.as_deref() } else { prompt.en.as_deref() };
        let Some(text) = text.map(str::trim).filter(|text| !text.is_empty()) else {
            continue;
        };
        let text = clip_chars(text, MAX_EXAMPLE_CHARS);
        if !out.iter().any(|existing: &String| existing == &text) {
            out.push(text);
        }
        if out.len() == MAX_EXAMPLES {
            break;
        }
    }
    out
}

fn locale_map(text: Option<&LocalizedText>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Some(text) = text else {
        return map;
    };
    if let Some(zh) = text.zh.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        map.insert("zh-CN".into(), zh.to_owned());
    }
    if let Some(en) = text.en.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        map.insert("en-US".into(), en.to_owned());
    }
    map
}

fn safe_rel(raw: &str) -> Option<PathBuf> {
    let text = raw.trim().trim_start_matches("./");
    if text.is_empty() {
        return Some(PathBuf::new());
    }
    if text.contains('\\') || text.contains(':') || text.starts_with('/') {
        return None;
    }
    let mut out = PathBuf::new();
    for part in text.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.contains('\0') {
            return None;
        }
        out.push(part);
    }
    if out.as_os_str().is_empty() {
        None
    } else {
        Some(out)
    }
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
}

fn clip_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

async fn import_projected_skills(
    paths: &SkillPaths,
    dirs: &[PathBuf],
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut ids = Vec::new();
    let mut created = Vec::new();
    let mut warnings = Vec::new();
    for dir in dirs {
        let name = match read_skill_info(dir).await {
            Ok((name, _)) => name,
            Err(error) => {
                warnings.push(format!("{}: {error}", dir.display()));
                continue;
            }
        };
        let target = paths.user_skills_dir.join(&name);
        if target.join("SKILL.md").is_file() {
            ids.push(SkillId::new(SkillCatalogSource::User, None, &name).as_str().to_owned());
            continue;
        }
        match import_skill(paths, dir).await {
            Ok(imported) => {
                created.push(imported.clone());
                ids.push(SkillId::new(SkillCatalogSource::User, None, &imported).as_str().to_owned());
            }
            Err(error) => warnings.push(format!("{name}: {error}")),
        }
    }
    (ids, created, warnings)
}

async fn write_preset(
    state: &ExpertInstallState,
    projected: &ProjectedExpert,
    skill_ids: Vec<String>,
) -> Result<String, AppError> {
    let included_skills = skill_ids
        .into_iter()
        .map(|skill_id| SkillBinding { skill_id, required: true })
        .collect();
    let created = state
        .presets
        .create(CreatePresetRequest {
            preset_id: None,
            name: projected.name.clone(),
            description: projected.description.clone(),
            routing_description: projected.routing_description.clone(),
            instructions: projected.instructions.clone(),
            avatar: None,
            fallback_allowed: false,
            targets: vec![PresetTarget::Conversation, PresetTarget::ExecutionStep],
            agent_preferences: Vec::new(),
            model_preferences: Vec::new(),
            included_skills,
            excluded_auto_skills: Vec::new(),
            knowledge_policy: Default::default(),
            knowledge_bases: Vec::new(),
            mcp_server_ids: Vec::new(),
            examples: projected.examples.clone(),
            examples_i18n: projected.examples_i18n.clone(),
            audience_tag_ids: Vec::new(),
            scenario_tag_ids: Vec::new(),
            name_i18n: projected.name_i18n.clone(),
            description_i18n: projected.description_i18n.clone(),
            instructions_i18n: HashMap::new(),
        })
        .await?;
    state
        .presets
        .set_state(
            &created.preset_id,
            SetPresetStateRequest {
                enabled: Some(true),
                auto_selectable: Some(true),
                ..Default::default()
            },
        )
        .await?;
    Ok(created.preset_id)
}

async fn install_avatar(state: &ExpertInstallState, preset_id: &str, source: &Path) -> Result<(), AppError> {
    let ext = source
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();
    let dir = state.data_dir.join("preset-avatars");
    std::fs::create_dir_all(&dir).map_err(|error| AppError::Internal(error.to_string()))?;
    let dest = dir.join(format!("{preset_id}.{ext}"));
    std::fs::copy(source, &dest).map_err(|error| AppError::Internal(error.to_string()))?;
    state
        .presets
        .update(
            preset_id,
            nomifun_api_types::UpdatePresetRequest {
                avatar: Some(format!("/api/presets/{preset_id}/avatar")),
                ..Default::default()
            },
        )
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_url_rejects_local_and_plain_http() {
        assert!(download_url_allowed(&Url::parse("http://cdn.example/a.zip").unwrap()).is_err());
        assert!(download_url_allowed(&Url::parse("https://127.0.0.1/a.zip").unwrap()).is_err());
        assert!(download_url_allowed(&Url::parse("https://localhost/a.zip").unwrap()).is_err());
        assert!(download_url_allowed(&Url::parse("https://10.1.2.3/a.zip").unwrap()).is_err());
        assert!(download_url_allowed(&Url::parse("https://user:pass@cdn.example/a.zip").unwrap()).is_err());
        assert!(download_url_allowed(&Url::parse("https://cdn.example/experts/demo.zip").unwrap()).is_ok());
    }

    #[test]
    fn projects_a_plugin_into_preset_fields() {
        let root = tempfile::tempdir().unwrap();
        let plugin = root.path().join("plugins/demo");
        std::fs::create_dir_all(plugin.join(".codebuddy-plugin")).unwrap();
        std::fs::create_dir_all(plugin.join("agents")).unwrap();
        std::fs::create_dir_all(plugin.join("skills/briefing")).unwrap();
        std::fs::write(
            plugin.join(".codebuddy-plugin/plugin.json"),
            r#"{
                "name": "demo",
                "description": "English capability line",
                "agents": ["./agents"],
                "skills": ["./skills/briefing"],
                "displayName": { "zh": "演示专家", "en": "Demo Expert" },
                "displayDescription": { "zh": "中文说明", "en": "English summary" },
                "defaultInitPrompt": { "zh": "今天有什么新动态？", "en": "What is new?" },
                "quickPrompts": [{ "zh": "看一下日报", "en": "Show the daily" }],
                "teamInfo": { "leadAgent": "lead", "memberAgents": ["demo"] }
            }"#,
        )
        .unwrap();
        std::fs::write(
            plugin.join("agents/demo.md"),
            "---\nname: demo\ndescription: trigger\n---\n\n# 人设\n\n按技能回答。\n",
        )
        .unwrap();
        std::fs::write(
            plugin.join("agents/lead.md"),
            "---\nname: lead\n---\n\n# 队长\n",
        )
        .unwrap();
        std::fs::write(
            plugin.join("skills/briefing/SKILL.md"),
            "---\nname: briefing\ndescription: daily\n---\n\n# Skill\n",
        )
        .unwrap();

        let found = locate_plugin_root(root.path(), "demo").unwrap();
        let projected = project_plugin(&found).unwrap();
        assert_eq!(projected.name, "演示专家");
        assert_eq!(projected.name_i18n.get("zh-CN").map(String::as_str), Some("演示专家"));
        assert_eq!(projected.name_i18n.get("en-US").map(String::as_str), Some("Demo Expert"));
        assert_eq!(projected.description.as_deref(), Some("中文说明"));
        assert_eq!(projected.routing_description.as_deref(), Some("English capability line"));
        assert!(projected.instructions.contains("队长"));
        assert_eq!(projected.examples, vec!["今天有什么新动态？".to_owned(), "看一下日报".to_owned()]);
        assert_eq!(projected.skill_dirs.len(), 1);
        assert!(projected.skill_dirs[0].ends_with("briefing"));
    }

    #[test]
    fn flowy_plugin_manifest_is_a_plugin_root() {
        let root = tempfile::tempdir().unwrap();
        let plugin = root.path().join("academic-journal-selector");
        std::fs::create_dir_all(plugin.join(".flowy-plugin")).unwrap();
        std::fs::create_dir_all(plugin.join("agents")).unwrap();
        std::fs::write(
            plugin.join(".flowy-plugin/plugin.json"),
            r#"{
                "name": "academic-journal-selector",
                "displayName": { "zh": "万方数据", "en": "Academic Journal Advisor" },
                "agents": ["./agents/lead.md"]
            }"#,
        )
        .unwrap();
        std::fs::write(plugin.join("agents/lead.md"), "# 队长\n").unwrap();

        let found = locate_plugin_root(root.path(), "academic-journal-selector").unwrap();
        assert_eq!(found.file_name().and_then(|name| name.to_str()), Some("academic-journal-selector"));
        let projected = project_plugin(&found).unwrap();
        assert_eq!(projected.name, "万方数据");
        assert!(projected.instructions.contains("队长"));
    }

    #[test]
    fn safe_rel_rejects_traversal() {
        assert!(safe_rel("../secret").is_none());
        assert!(safe_rel("/etc/passwd").is_none());
        assert!(safe_rel("C:/windows").is_none());
        assert_eq!(safe_rel("./agents/demo.md").unwrap(), PathBuf::from("agents/demo.md"));
    }
}
