use std::collections::HashMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use include_dir::{Dir, include_dir};
use nomifun_api_types::{SkillCatalogSource, SkillId};
use nomifun_common::dir_config::write_atomic_replace;
use sha2::{Digest, Sha256};
use tracing::{debug, warn};

use crate::constants::{
    PRESET_RULES_DIR_NAME, PRESET_SKILLS_DIR_NAME, BUILTIN_AUTO_SKILLS_SUBDIR,
    BUILTIN_RULES_DIR_NAME, COMMON_SKILL_DIRS, CRON_SKILLS_DIR_NAME, SKILL_MANIFEST_FILE,
    SKILLS_DIR_NAME,
};
use crate::error::ExtensionError;

/// Built-in skill corpus embedded into the binary at compile time.
///
/// Mirrors the strategy used by `nomifun-preset::builtin`: the corpus is
/// authoritative at build time; an optional on-disk override
/// (`NOMIFUN_BUILTIN_SKILLS_PATH`) is consulted at runtime for rapid
/// iteration and E2E fixtures.
static BUILTIN_SKILLS: Dir<'static> =
    include_dir!("$CARGO_MANIFEST_DIR/../nomifun-app/assets/builtin-skills");

/// Name of the environment variable that, when set, overrides the embedded
/// corpus with an on-disk directory. Consumed by
/// [`resolve_skill_paths`] when building [`SkillPaths`].
pub const BUILTIN_SKILLS_ENV_VAR: &str = "NOMIFUN_BUILTIN_SKILLS_PATH";

const AGENT_SKILLS_DIR: &str = ".agents/skills";
const GLOBAL_AGENT_SKILLS_SOURCE_KEY: &str = "agents";
const PROJECT_AGENT_SKILLS_SOURCE_KEY: &str = "workspace";

/// One process-local fence for every operation that mutates the user Skill
/// tree. The app's data-dir `server.lock` serializes separate backend
/// processes; this mutex closes the remaining same-process multi-window race.
static SKILL_MUTATION_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

pub(crate) fn skill_mutation_lock() -> &'static tokio::sync::Mutex<()> {
    SKILL_MUTATION_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

#[derive(Default)]
struct StagingCleanupState {
    extraction_in_flight: bool,
    cleanup_requested: bool,
}

/// Coordinates staging cleanup with the synchronous ZIP worker. Dropping an
/// async `JoinHandle` does not stop `spawn_blocking`, so cleanup must wait for
/// the worker instead of racing it on request cancellation.
#[derive(Clone)]
pub(crate) struct StagingCleanupHandle {
    root: PathBuf,
    parent: PathBuf,
    state: Arc<Mutex<StagingCleanupState>>,
}

impl StagingCleanupHandle {
    pub(crate) fn new(root: PathBuf, parent: PathBuf) -> Self {
        Self {
            root,
            parent,
            state: Arc::new(Mutex::new(StagingCleanupState::default())),
        }
    }

    fn begin_extraction(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.cleanup_requested {
            return false;
        }
        state.extraction_in_flight = true;
        true
    }

    fn finish_extraction(&self) {
        let should_cleanup = {
            let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            state.extraction_in_flight = false;
            state.cleanup_requested
        };
        if should_cleanup {
            remove_staging_directory_sync(&self.root);
            remove_empty_staging_parent_sync(&self.parent);
        }
    }

    pub(crate) fn request_cleanup(&self) {
        let should_cleanup = {
            let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            state.cleanup_requested = true;
            !state.extraction_in_flight
        };
        if should_cleanup {
            remove_staging_directory_sync(&self.root);
            remove_empty_staging_parent_sync(&self.parent);
        }
    }
}

struct ExtractionWorkerGuard {
    cleanup: StagingCleanupHandle,
}

impl Drop for ExtractionWorkerGuard {
    fn drop(&mut self) {
        self.cleanup.finish_extraction();
    }
}

/// Acquire the process-wide Skill mutation lock for callers that need to
/// coordinate additional bookkeeping with a projection pass.
pub async fn acquire_skill_mutation_lock() -> tokio::sync::MutexGuard<'static, ()> {
    skill_mutation_lock().lock().await
}

/// Expose the embedded builtin skills corpus for startup
/// materialization. Consumers outside this crate should not depend on
/// `include_dir` directly.
pub fn builtin_skills_corpus() -> &'static Dir<'static> {
    &BUILTIN_SKILLS
}

/// Build the on-disk materialization version for the embedded builtin skill
/// corpus. This deliberately includes a content fingerprint, not just the app
/// version, so asset-only changes refresh `{data_dir}/builtin-skills` even
/// when the crate version stays unchanged during development.
pub fn builtin_skills_materialize_version(app_version: &str) -> String {
    let fingerprint = builtin_skills_corpus_fingerprint();
    format!("{app_version}+skills.{}", &fingerprint[..12])
}

/// Deterministic SHA-256 fingerprint for the embedded builtin skill corpus.
pub fn builtin_skills_corpus_fingerprint() -> String {
    let mut files = Vec::new();
    collect_corpus_files(&BUILTIN_SKILLS, &mut files);
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = Sha256::new();
    for (path, contents) in files {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(contents);
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}

fn collect_corpus_files(dir: &'static Dir<'static>, out: &mut Vec<(String, &'static [u8])>) {
    for file in dir.files() {
        out.push((file.path().to_string_lossy().into_owned(), file.contents()));
    }
    for subdir in dir.dirs() {
        collect_corpus_files(subdir, out);
    }
}

// ---------------------------------------------------------------------------
// Skill paths resolution
// ---------------------------------------------------------------------------

/// Resolved base directories for skill and rule management.
///
/// `builtin_skills_dir` always points at a real on-disk directory.
/// In production it resolves to `{data_dir}/builtin-skills/`, populated
/// at startup by [`crate::startup_materialize::materialize_if_needed`].
/// In dev/test it can be redirected via [`BUILTIN_SKILLS_ENV_VAR`].
#[derive(Debug, Clone)]
pub struct SkillPaths {
    /// Root data directory (~/.nomifun/).
    pub data_dir: PathBuf,
    /// User-created skills directory (~/.nomifun/skills/).
    pub user_skills_dir: PathBuf,
    /// Per-job cron skills directory (~/.nomifun/cron/skills/).
    pub cron_skills_dir: PathBuf,
    /// Built-in skills directory on disk. Always set.
    /// Points to `{data_dir}/builtin-skills/` in production (populated at
    /// startup by `startup_materialize::materialize_if_needed`) or
    /// wherever [`BUILTIN_SKILLS_ENV_VAR`] points in dev mode.
    pub builtin_skills_dir: PathBuf,
    /// Built-in rules directory (app bundle resource).
    pub builtin_rules_dir: PathBuf,
    /// Preset-level rules directory (~/.nomifun/preset-rules/).
    pub preset_rules_dir: PathBuf,
    /// Preset-level skills directory (~/.nomifun/preset-skills/).
    pub preset_skills_dir: PathBuf,
    /// Stable roots for Skills discovered outside application data.
    pub catalog_roots: CatalogSkillRoots,
}

/// Resolve standard skill paths.
///
/// `app_resource_dir` is the application's bundled resource directory
/// (e.g. the binary's parent or a configured resource path); only
/// `builtin_rules_dir` is still derived from it — built-in skills live
/// under `data_dir` (materialized at startup from the embedded corpus)
/// unless redirected via [`BUILTIN_SKILLS_ENV_VAR`].
///
/// `data_dir` is the user-level data root (e.g. `~/.nomifun/`) and
/// determines where user skills, preset resources, and the built-in
/// skills tree (`{data_dir}/builtin-skills/`) live. Per-conversation
/// agent skills are no longer materialized on disk — see
/// [`materialize_skills_for_agent`] for the symlink contract.
pub fn resolve_skill_paths(app_resource_dir: &Path, data_dir: &Path) -> SkillPaths {
    let builtin_skills_dir = std::env::var(BUILTIN_SKILLS_ENV_VAR)
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| data_dir.join(crate::constants::BUILTIN_SKILLS_DIR_NAME));

    SkillPaths {
        data_dir: data_dir.to_path_buf(),
        user_skills_dir: data_dir.join(SKILLS_DIR_NAME),
        cron_skills_dir: data_dir.join(CRON_SKILLS_DIR_NAME),
        builtin_skills_dir,
        builtin_rules_dir: app_resource_dir.join(BUILTIN_RULES_DIR_NAME),
        preset_rules_dir: data_dir.join(PRESET_RULES_DIR_NAME),
        preset_skills_dir: data_dir.join(PRESET_SKILLS_DIR_NAME),
        catalog_roots: CatalogSkillRoots::discover(),
    }
}

// ---------------------------------------------------------------------------
// A'. 伙伴自进化技能：分层范围 + 路径助手 + 写原语
//
// 专属技能落 {user_skills_dir}/companion/{companion_id}/{name}/，共享技能落
// {user_skills_dir}/shared/{name}/，草稿落 {user_skills_dir}/_drafts/{companion_id}/{name}/。
// 正文以磁盘 SKILL.md 为事实源（companion store 的 companion_skills 表只存元数据）。
// ---------------------------------------------------------------------------

/// 技能归属范围：`Shared`（全员可用）或 `Companion(id)`（伙伴专属）。
#[derive(Debug, Clone)]
pub enum SkillScope {
    Shared,
    Companion(String),
}

/// `{user_skills_dir}/companion`
pub fn companion_skills_root(paths: &SkillPaths) -> PathBuf {
    paths.user_skills_dir.join("companion")
}

/// `{user_skills_dir}/shared`
pub fn shared_skills_root(paths: &SkillPaths) -> PathBuf {
    paths.user_skills_dir.join("shared")
}

/// `{user_skills_dir}/_drafts`
pub fn drafts_root(paths: &SkillPaths) -> PathBuf {
    paths.user_skills_dir.join("_drafts")
}

/// Resolve the on-disk directory for a scoped skill. `draft=true` routes to the
/// review staging area. Both `name` and any `companion_id` are validated against
/// path traversal via [`validate_filename`].
pub fn skill_dir_for(
    paths: &SkillPaths,
    scope: &SkillScope,
    name: &str,
    draft: bool,
) -> Result<PathBuf, ExtensionError> {
    validate_filename(name)?;
    let base = match (scope, draft) {
        (SkillScope::Companion(cid), false) => {
            validate_filename(cid)?;
            companion_skills_root(paths).join(cid)
        }
        (SkillScope::Companion(cid), true) => {
            validate_filename(cid)?;
            drafts_root(paths).join(cid)
        }
        (SkillScope::Shared, _) => shared_skills_root(paths),
    };
    Ok(base.join(name))
}

/// 起草一份技能所需的字段。`name`/`description` 必填，其余可选。
#[derive(Debug, Clone)]
pub struct SkillDraftInput {
    pub name: String,
    pub description: String,
    pub when_to_use: Option<String>,
    pub allowed_tools: Option<String>,
    pub paths: Option<String>,
    pub body: String,
}

/// 拼一份合法 SKILL.md：YAML frontmatter（name/description 必填，其余可选）+ 正文。
pub fn build_skill_md(input: &SkillDraftInput) -> String {
    let mut fm = String::from("---\n");
    fm.push_str(&format!("name: {}\n", input.name));
    fm.push_str(&format!("description: {}\n", input.description));
    if let Some(w) = &input.when_to_use {
        if !w.is_empty() {
            fm.push_str(&format!("when-to-use: {w}\n"));
        }
    }
    if let Some(t) = &input.allowed_tools {
        if !t.is_empty() {
            fm.push_str(&format!("allowed-tools: {t}\n"));
        }
    }
    if let Some(p) = &input.paths {
        if !p.is_empty() {
            fm.push_str(&format!("paths: {p}\n"));
        }
    }
    fm.push_str("---\n\n");
    fm.push_str(&input.body);
    if !input.body.ends_with('\n') {
        fm.push('\n');
    }
    fm
}

/// 把 agent 起草的字段物化成磁盘上的 SKILL.md（整条自进化链路唯一缺失的底层原语）。
/// 空 description 直接拒（frontmatter 双侧契约）；`draft=true` 落到审阅暂存区。
pub async fn create_skill(
    paths: &SkillPaths,
    scope: &SkillScope,
    draft: bool,
    input: &SkillDraftInput,
) -> Result<PathBuf, ExtensionError> {
    validate_filename(&input.name)?;
    if input.description.trim().is_empty() {
        return Err(ExtensionError::InvalidSkillPath(format!(
            "skill '{}' has empty description",
            input.name
        )));
    }
    let dir = skill_dir_for(paths, scope, &input.name, draft)?;
    tokio::fs::create_dir_all(&dir).await?;
    let content = build_skill_md(input);
    tokio::fs::write(dir.join(SKILL_MANIFEST_FILE), content).await?;
    // Optimization 6: create support file subdirectories so the skill can
    // reference external files (reference docs, code templates, helper scripts).
    // These are created empty — the skill body or downstream tools populate them.
    for sub in &["references", "templates", "scripts"] {
        tokio::fs::create_dir_all(dir.join(sub)).await?;
    }
    debug!(skill = %input.name, dir = %dir.display(), draft, "companion skill created");
    Ok(dir)
}

/// 应用内编辑：整文覆写一份已存在技能的 SKILL.md，但写前校验 frontmatter 合法且 description 非空。
pub async fn write_skill(
    paths: &SkillPaths,
    scope: &SkillScope,
    draft: bool,
    name: &str,
    full_markdown: &str,
) -> Result<(), ExtensionError> {
    let (_n, desc) = parse_frontmatter_fields(full_markdown).ok_or_else(|| {
        ExtensionError::InvalidSkillPath(format!("invalid frontmatter for skill '{name}'"))
    })?;
    if desc.trim().is_empty() {
        return Err(ExtensionError::InvalidSkillPath(format!(
            "skill '{name}' has empty description"
        )));
    }
    let dir = skill_dir_for(paths, scope, name, draft)?;
    tokio::fs::create_dir_all(&dir).await?;
    tokio::fs::write(dir.join(SKILL_MANIFEST_FILE), full_markdown).await?;
    Ok(())
}

/// Copy a skill's SKILL.md from one scope to another (skill transfer / 互教). Reads the active
/// source SKILL.md and writes it into the target's active dir (validated by `write_skill`).
pub async fn copy_skill(
    paths: &SkillPaths,
    from: &SkillScope,
    to: &SkillScope,
    name: &str,
) -> Result<(), ExtensionError> {
    let src = skill_dir_for(paths, from, name, false)?;
    let content = tokio::fs::read_to_string(src.join(SKILL_MANIFEST_FILE)).await?;
    write_skill(paths, to, false, name, &content).await
}

// ---------------------------------------------------------------------------
// A. Built-in resource reading
// ---------------------------------------------------------------------------

/// Read a built-in rule file by name.
///
/// Returns the file content as a string. Returns an empty string if the
/// file does not exist (graceful degradation per API spec).
pub async fn read_builtin_rule(
    paths: &SkillPaths,
    file_name: &str,
) -> Result<String, ExtensionError> {
    validate_filename(file_name)?;
    let file_path = paths.builtin_rules_dir.join(file_name);
    read_file_or_empty(&file_path).await
}

/// Read a built-in skill file by name.
///
/// `file_name` is a relative path inside the built-in skills corpus
/// (e.g. `"auto-inject/cron/SKILL.md"` or
/// `"planning-with-files/SKILL.md"`). Returns
/// the file content as a string, or an empty string if the file does not
/// exist (preserves the legacy graceful-degradation contract consumed by
/// the renderer).
///
/// Reads from `paths.builtin_skills_dir`, which is always populated at
/// startup by [`crate::startup_materialize::materialize_if_needed`].
/// Rejects `..`-style traversal.
pub async fn read_builtin_skill(
    paths: &SkillPaths,
    file_name: &str,
) -> Result<String, ExtensionError> {
    validate_builtin_skill_path(file_name)?;
    let file_path = paths.builtin_skills_dir.join(file_name);
    read_file_or_empty(&file_path).await
}

// ---------------------------------------------------------------------------
// C. Skill listing & info
// ---------------------------------------------------------------------------

/// Origin of a listed skill.
///
/// Matches the renderer contract in
/// `src/common/adapter/ipcBridge.ts::listAvailableSkills`, which filters the
/// Skills Hub UI by this value. `Extension` is reserved for
/// extension-contributed skills once `ExtensionRegistry` is wired into the
/// Rust backend; the pilot only emits `Builtin` / `Custom`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillSource {
    Builtin,
    Custom,
    Extension,
}

/// A discovered skill item for listing.
///
/// For `source=Builtin`, `location` is the absolute path of the on-disk
/// SKILL.md under `paths.builtin_skills_dir` (populated at startup by
/// [`crate::startup_materialize::materialize_if_needed`]). The
/// `relative_location` carries the relative path suitable for
/// `POST /api/skills/builtin-skill` (e.g. `"auto-inject/cron/SKILL.md"`
/// or `"planning-with-files/SKILL.md"`). Other sources leave
/// `relative_location` `None`.
#[derive(Debug, Clone, PartialEq)]
pub struct SkillListItem {
    pub name: String,
    pub description: String,
    pub location: String,
    pub relative_location: Option<String>,
    pub is_custom: bool,
    pub source: SkillSource,
}

/// Lightweight item used by the user-facing Skill catalog.
///
/// Unlike [`SkillListItem`], this representation preserves two Skills with
/// the same display name when they originate from different sources. The
/// legacy list still follows its existing user-overrides-builtin behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillCatalogItem {
    pub name: String,
    pub description: String,
    pub source: SkillCatalogSource,
    /// Source-owner key, used by multi-owner sources such as extensions and
    /// MCP servers. `None` means the source has a single catalog owner.
    pub source_key: Option<String>,
    /// Stable key inside this source. It is intentionally separate from the
    /// frontmatter display name because two directories may declare the same
    /// name.
    pub local_key: String,
}

/// Filesystem roots that contribute Skills to the user-facing catalog.
///
/// The global root follows the Agent Skills convention (`~/.agents/skills`).
/// The project root is captured from the backend process's current working
/// directory, allowing a checked-out project's `.agents/skills` directory to
/// travel with the project without copying it into application data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogSkillRoots {
    pub global_agent_skills_dir: Option<PathBuf>,
    pub project_agent_skills_dir: Option<PathBuf>,
}

impl CatalogSkillRoots {
    fn discover() -> Self {
        Self {
            global_agent_skills_dir: dirs::home_dir().map(|home| home.join(AGENT_SKILLS_DIR)),
            project_agent_skills_dir: std::env::current_dir()
                .ok()
                .map(|project_dir| project_dir.join(AGENT_SKILLS_DIR)),
        }
    }
}

/// Immutable source content resolved for one explicit `LoadSkill` request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedCatalogSkill {
    pub skill_id: String,
    pub name: String,
    pub description: String,
    pub source: String,
    pub version_hash: String,
    pub content: String,
}

/// List all available skills (built-in + user custom), deduplicated.
///
/// User custom skills override built-in skills with the same name.
///
/// For built-in entries, the caller sees an absolute `location` pointing
/// at `paths.builtin_skills_dir/.../SKILL.md` — the tree is populated
/// at startup by
/// [`crate::startup_materialize::materialize_if_needed`] so downstream
/// consumers (e.g. the SkillsHubSettings export-symlink flow) can
/// resolve the path on disk. `relative_location` is populated for
/// built-ins only.
pub async fn list_available_skills(
    paths: &SkillPaths,
) -> Result<Vec<SkillListItem>, ExtensionError> {
    let mut builtin_skills = std::collections::HashMap::new();

    // 1. Built-in skills (lower priority)
    for item in list_builtin_skills(paths).await {
        builtin_skills.insert(item.name.clone(), item);
    }

    // 2. User custom skills (higher priority, overrides builtin)
    let mut custom_skills = Vec::new();
    if let Ok(entries) = scan_skill_dirs(&paths.user_skills_dir).await {
        for item in entries {
            builtin_skills.remove(&item.name);
            custom_skills.push(SkillListItem {
                name: item.name,
                description: item.description,
                location: item.path,
                relative_location: None,
                is_custom: true,
                source: SkillSource::Custom,
            });
        }
    }

    custom_skills.sort_by(|a, b| {
        skill_modified_time(&b.location)
            .cmp(&skill_modified_time(&a.location))
            .then_with(|| a.name.cmp(&b.name))
    });

    let mut builtin_items: Vec<SkillListItem> = builtin_skills.into_values().collect();
    builtin_items.sort_by(|a, b| a.name.cmp(&b.name));

    let mut result = custom_skills;
    result.extend(builtin_items);
    Ok(result)
}

/// List Skills that a user or Agent may discover through the new catalog.
///
/// This is intentionally distinct from [`list_available_skills`]: source
/// collisions are retained rather than resolved by the legacy execution
/// precedence, and built-in auto-injected system Skills are not exposed as
/// user-selectable entries.
pub async fn list_catalog_skills(paths: &SkillPaths) -> Result<Vec<SkillCatalogItem>, ExtensionError> {
    list_catalog_skills_with_roots(paths, &paths.catalog_roots).await
}

/// List the catalog from the roots captured during application startup.
async fn list_catalog_skills_with_roots(
    paths: &SkillPaths,
    roots: &CatalogSkillRoots,
) -> Result<Vec<SkillCatalogItem>, ExtensionError> {
    let mut catalog: Vec<SkillCatalogItem> = catalog_skill_files(paths, roots, true)
        .await?
        .into_iter()
        .map(|candidate| SkillCatalogItem {
            name: candidate.name,
            description: candidate.description,
            source: candidate.source,
            source_key: candidate.source_key,
            local_key: candidate.local_key,
        })
        .collect();

    catalog.sort_by(|left, right| {
        catalog_source_sort_key(left.source)
            .cmp(catalog_source_sort_key(right.source))
            .then_with(|| left.source_key.cmp(&right.source_key))
            .then_with(|| left.local_key.cmp(&right.local_key))
    });
    Ok(catalog)
}

/// Resolve source-qualified catalog IDs into immutable `SKILL.md` snapshots.
///
/// This is deliberately all-or-nothing: callers may persist or inject the
/// result only after every requested Skill has been resolved and read.
pub async fn load_catalog_skills(
    paths: &SkillPaths,
    skill_ids: &[String],
) -> Result<Vec<LoadedCatalogSkill>, ExtensionError> {
    load_catalog_skills_with_roots(paths, &paths.catalog_roots, skill_ids).await
}

/// Load selected catalog Skills from the roots captured during application startup.
async fn load_catalog_skills_with_roots(
    paths: &SkillPaths,
    roots: &CatalogSkillRoots,
    skill_ids: &[String],
) -> Result<Vec<LoadedCatalogSkill>, ExtensionError> {
    let candidates = catalog_skill_files(paths, roots, false).await?;
    let mut loaded = Vec::with_capacity(skill_ids.len());

    for skill_id in skill_ids {
        let candidate = candidates
            .iter()
            .find(|candidate| candidate.skill_id == *skill_id)
            .ok_or_else(|| ExtensionError::SkillNotFound(skill_id.clone()))?;
        let content = match &candidate.location {
            CatalogSkillLocation::Builtin(relative_location) => read_builtin_skill(paths, relative_location).await?,
            CatalogSkillLocation::File(path) => tokio::fs::read_to_string(path).await?,
        };
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        loaded.push(LoadedCatalogSkill {
            skill_id: candidate.skill_id.clone(),
            name: candidate.name.clone(),
            description: candidate.description.clone(),
            source: candidate.source.as_str().to_owned(),
            version_hash: format!("{:x}", hasher.finalize()),
            content,
        });
    }

    Ok(loaded)
}

#[derive(Debug, Clone)]
struct CatalogSkillFile {
    skill_id: String,
    name: String,
    description: String,
    source: SkillCatalogSource,
    source_key: Option<String>,
    local_key: String,
    location: CatalogSkillLocation,
}

#[derive(Debug, Clone)]
enum CatalogSkillLocation {
    Builtin(String),
    File(PathBuf),
}

async fn catalog_skill_files(
    paths: &SkillPaths,
    roots: &CatalogSkillRoots,
    tolerate_user_root_error: bool,
) -> Result<Vec<CatalogSkillFile>, ExtensionError> {
    let mut candidates = Vec::new();
    for item in list_builtin_skills(paths).await {
        if is_system_owned_builtin(&item) {
            continue;
        }
        let Some(relative_location) = item.relative_location.clone() else {
            continue;
        };
        let local_key = builtin_catalog_local_key(&item);
        let name = item.name;
        let source = SkillCatalogSource::Builtin;
        candidates.push(CatalogSkillFile {
            skill_id: SkillId::new(source, None, &local_key).as_str().to_owned(),
            name,
            description: item.description,
            source,
            source_key: None,
            local_key,
            location: CatalogSkillLocation::Builtin(relative_location),
        });
    }

    let user_root_result = extend_catalog_with_directory(
        &mut candidates,
        &paths.user_skills_dir,
        SkillCatalogSource::User,
        None,
    )
    .await;
    if let Err(error) = user_root_result {
        if tolerate_user_root_error {
            warn!(path = %paths.user_skills_dir.display(), error = %error, "skipping unavailable user Skill root");
        } else {
            return Err(error);
        }
    }

    if let Some(global_agent_skills_dir) = roots.global_agent_skills_dir.as_deref() {
        extend_catalog_with_optional_directory(
            &mut candidates,
            global_agent_skills_dir,
            SkillCatalogSource::User,
            Some(GLOBAL_AGENT_SKILLS_SOURCE_KEY),
        )
        .await?;
    }

    if let Some(project_agent_skills_dir) = roots.project_agent_skills_dir.as_deref() {
        extend_catalog_with_optional_directory(
            &mut candidates,
            project_agent_skills_dir,
            SkillCatalogSource::Project,
            Some(PROJECT_AGENT_SKILLS_SOURCE_KEY),
        )
        .await?;
    }

    Ok(candidates)
}

async fn extend_catalog_with_optional_directory(
    candidates: &mut Vec<CatalogSkillFile>,
    root: &Path,
    source: SkillCatalogSource,
    source_key: Option<&str>,
) -> Result<(), ExtensionError> {
    match extend_catalog_with_directory(candidates, root, source, source_key).await {
        Ok(()) => Ok(()),
        Err(error) => {
            warn!(path = %root.display(), error = %error, "skipping unavailable optional Skill root");
            Ok(())
        }
    }
}

async fn extend_catalog_with_directory(
    candidates: &mut Vec<CatalogSkillFile>,
    root: &Path,
    source: SkillCatalogSource,
    source_key: Option<&str>,
) -> Result<(), ExtensionError> {
    for item in scan_skill_dirs(root).await? {
        let local_key = catalog_local_key(root, &item.path, &item.name);
        candidates.push(CatalogSkillFile {
            skill_id: SkillId::new(source, source_key, &local_key).as_str().to_owned(),
            name: item.name,
            description: item.description,
            source,
            source_key: source_key.map(str::to_owned),
            local_key,
            location: CatalogSkillLocation::File(PathBuf::from(item.path).join(SKILL_MANIFEST_FILE)),
        });
    }
    Ok(())
}

fn builtin_catalog_local_key(item: &SkillListItem) -> String {
    item.relative_location
        .as_deref()
        .and_then(|location| location.strip_suffix(&format!("/{SKILL_MANIFEST_FILE}")))
        .filter(|key| !key.is_empty())
        .unwrap_or(&item.name)
        .to_owned()
}

fn catalog_local_key(root: &Path, location: &str, fallback: &str) -> String {
    Path::new(location)
        .strip_prefix(root)
        .ok()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .filter(|key| !key.is_empty())
        .unwrap_or_else(|| fallback.to_owned())
}

fn is_system_owned_builtin(item: &SkillListItem) -> bool {
    item.source == SkillSource::Builtin
        && item
            .relative_location
            .as_deref()
            .is_some_and(|path| path.starts_with(&format!("{BUILTIN_AUTO_SKILLS_SUBDIR}/")))
}

fn catalog_source_sort_key(source: SkillCatalogSource) -> &'static str {
    match source {
        SkillCatalogSource::Builtin => "builtin",
        SkillCatalogSource::User => "user",
        SkillCatalogSource::Project => "project",
        SkillCatalogSource::Extension => "extension",
        SkillCatalogSource::Mcp => "mcp",
        SkillCatalogSource::Legacy => "legacy",
    }
}

/// Emit a [`SkillListItem`] for every built-in skill (both auto-inject
/// and opt-in). All paths resolve directly against
/// `paths.builtin_skills_dir`.
async fn list_builtin_skills(paths: &SkillPaths) -> Vec<SkillListItem> {
    list_builtin_skills_from_disk(&paths.builtin_skills_dir).await
}

async fn list_builtin_skills_from_disk(dir: &Path) -> Vec<SkillListItem> {
    let mut items = Vec::new();

    // Top-level opt-in skills (siblings of auto-inject/).
    if let Ok(top) = scan_skill_dirs(dir).await {
        for s in top {
            if s.name == BUILTIN_AUTO_SKILLS_SUBDIR {
                continue;
            }
            // Use the on-disk directory name (basename of scanned path)
            // rather than the frontmatter name, so the path we emit
            // matches the real filesystem layout when the two disagree.
            let dir_name = Path::new(&s.path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&s.name)
                .to_string();
            let rel = format!("{dir_name}/{SKILL_MANIFEST_FILE}");
            let location = dir
                .join(&dir_name)
                .join(SKILL_MANIFEST_FILE)
                .to_string_lossy()
                .into_owned();
            items.push(SkillListItem {
                name: s.name,
                description: s.description,
                location,
                relative_location: Some(rel),
                is_custom: false,
                source: SkillSource::Builtin,
            });
        }
    }

    // auto-inject children.
    let auto_dir = dir.join(BUILTIN_AUTO_SKILLS_SUBDIR);
    if let Ok(auto) = scan_skill_dirs(&auto_dir).await {
        for s in auto {
            let dir_name = Path::new(&s.path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&s.name)
                .to_string();
            let rel = format!("{BUILTIN_AUTO_SKILLS_SUBDIR}/{dir_name}/{SKILL_MANIFEST_FILE}");
            let location = auto_dir
                .join(&dir_name)
                .join(SKILL_MANIFEST_FILE)
                .to_string_lossy()
                .into_owned();
            items.push(SkillListItem {
                name: s.name,
                description: s.description,
                location,
                relative_location: Some(rel),
                is_custom: false,
                source: SkillSource::Builtin,
            });
        }
    }

    items
}

/// A skill discovered during directory scanning.
#[derive(Debug, Clone, PartialEq)]
pub struct ScannedSkill {
    pub name: String,
    pub description: String,
    pub path: String,
}

/// An auto-injected built-in skill.
///
/// Returned by `GET /api/skills/builtin-auto`. `location` is the
/// relative path the frontend passes back into
/// `POST /api/skills/builtin-skill`, e.g. `"auto-inject/cron/SKILL.md"`.
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltinAutoSkillItem {
    pub name: String,
    pub description: String,
    pub location: String,
}

/// List built-in skills that are auto-injected into every preset.
///
/// Reads from `{paths.builtin_skills_dir}/auto-inject/`. A missing
/// `auto-inject/` directory yields an empty list, matching the
/// graceful-degradation semantics used elsewhere in this module.
pub async fn list_builtin_auto_skills(
    paths: &SkillPaths,
) -> Result<Vec<BuiltinAutoSkillItem>, ExtensionError> {
    let auto_dir = paths.builtin_skills_dir.join(BUILTIN_AUTO_SKILLS_SUBDIR);
    let mut items = list_auto_skills_from_disk(&auto_dir).await;
    items.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(items)
}

/// Built-in skill → (audience_tags, scenario_tags) seed map, loaded from the
/// embedded `skill-tags.json`. Graceful: missing/malformed → empty map.
pub fn load_builtin_skill_tags() -> HashMap<String, (Vec<String>, Vec<String>)> {
    #[derive(serde::Deserialize)]
    struct Entry {
        name: String,
        #[serde(default)]
        audience_tags: Vec<String>,
        #[serde(default)]
        scenario_tags: Vec<String>,
    }
    #[derive(serde::Deserialize, Default)]
    struct Manifest {
        #[serde(default)]
        skills: Vec<Entry>,
    }
    let bytes = match BUILTIN_SKILLS.get_file("skill-tags.json") {
        Some(f) => f.contents(),
        None => return HashMap::new(),
    };
    let manifest: Manifest = serde_json::from_slice(bytes).unwrap_or_default();
    manifest
        .skills
        .into_iter()
        .map(|e| (e.name, (e.audience_tags, e.scenario_tags)))
        .collect()
}

/// Built-in skill display metadata for localized UI surfaces. This lives beside
/// the tag seed so the Skills Hub can localize descriptions without changing the
/// executable `SKILL.md` files that agents read.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BuiltinSkillDisplayMetadata {
    pub name_i18n: HashMap<String, String>,
    pub description_i18n: HashMap<String, String>,
}

pub fn load_builtin_skill_display_metadata() -> HashMap<String, BuiltinSkillDisplayMetadata> {
    #[derive(serde::Deserialize)]
    struct Entry {
        name: String,
        #[serde(default)]
        name_i18n: HashMap<String, String>,
        #[serde(default)]
        description_i18n: HashMap<String, String>,
    }
    #[derive(serde::Deserialize, Default)]
    struct Manifest {
        #[serde(default)]
        skills: Vec<Entry>,
    }
    let bytes = match BUILTIN_SKILLS.get_file("skill-tags.json") {
        Some(f) => f.contents(),
        None => return HashMap::new(),
    };
    let manifest: Manifest = serde_json::from_slice(bytes).unwrap_or_default();
    manifest
        .skills
        .into_iter()
        .filter_map(|e| {
            if e.name_i18n.is_empty() && e.description_i18n.is_empty() {
                return None;
            }
            Some((
                e.name,
                BuiltinSkillDisplayMetadata {
                    name_i18n: e.name_i18n,
                    description_i18n: e.description_i18n,
                },
            ))
        })
        .collect()
}

async fn list_auto_skills_from_disk(auto_dir: &Path) -> Vec<BuiltinAutoSkillItem> {
    let entries = match scan_skill_dirs(auto_dir).await {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    entries
        .into_iter()
        .map(|s| {
            let name = s.name.clone();
            BuiltinAutoSkillItem {
                name,
                description: s.description,
                location: format!(
                    "{BUILTIN_AUTO_SKILLS_SUBDIR}/{}/{SKILL_MANIFEST_FILE}",
                    s.name
                ),
            }
        })
        .collect()
}

/// Read skill info from a SKILL.md file without importing.
///
/// Returns `(name, description)` extracted from frontmatter.
pub async fn read_skill_info(skill_path: &Path) -> Result<(String, String), ExtensionError> {
    let skill_file = if skill_path.is_dir() {
        skill_path.join(SKILL_MANIFEST_FILE)
    } else {
        skill_path.to_path_buf()
    };

    let content = tokio::fs::read_to_string(&skill_file)
        .await
        .map_err(|_| ExtensionError::SkillNotFound(skill_path.display().to_string()))?;

    let (name, description) = parse_frontmatter_fields(&content).ok_or_else(|| {
        ExtensionError::InvalidSkillPath(format!(
            "No valid frontmatter in {}",
            skill_file.display()
        ))
    })?;

    // Fallback: use directory name if name is empty
    let final_name = if name.is_empty() {
        skill_path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        name
    };

    Ok((final_name, description))
}

/// Maximum directory depth used by the market package staging validator.
/// Kept separate from the public best-effort importer so a malformed archive
/// can never make the atomic package path discover an unbounded tree.
pub(crate) const MARKET_IMPORT_SCAN_DEPTH: usize = 6;

/// Extract one downloaded archive into a caller-owned staging directory. This
/// deliberately does not import anything into the user's Skill root.
pub(crate) async fn extract_skill_archive_to_staging(
    archive_path: &Path,
    destination: &Path,
    cleanup: StagingCleanupHandle,
) -> Result<(), ExtensionError> {
    ensure_regular_skill_directory(destination).await?;
    let archive = archive_path.to_path_buf();
    let destination = destination.to_path_buf();
    if !cleanup.begin_extraction() {
        return Err(ExtensionError::InvalidSkillPath(
            "Skill archive extraction was cancelled before it started".into(),
        ));
    }
    let extraction_cleanup = cleanup.clone();
    tokio::task::spawn_blocking(move || {
        let _worker_guard = ExtractionWorkerGuard {
            cleanup: extraction_cleanup,
        };
        #[cfg(test)]
        test_overrides::wait_for_archive_extraction(&destination);
        crate::zip_safe::extract_zip_archive(&archive, &destination)
    })
        .await
        .map_err(|error| ExtensionError::InvalidSkillPath(format!("Zip extraction task failed: {error}")))??;
    Ok(())
}

/// Create a staging directory without following an attacker-controlled link
/// in any ancestor. Importers keep their roots inside the user Skill tree, so
/// `create_dir_all` would otherwise silently redirect untrusted archive writes
/// outside that tree.
pub(crate) async fn ensure_regular_skill_directory(path: &Path) -> Result<(), ExtensionError> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => {
            if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
                Err(ExtensionError::InvalidSkillPath(format!(
                    "staging path is not a regular directory: {}",
                    path.display()
                )))
            } else {
                Ok(())
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent()
                && parent != path
                && !parent.as_os_str().is_empty()
            {
                Box::pin(ensure_regular_skill_directory(parent)).await?;
            }
            match tokio::fs::create_dir(path).await {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    let metadata = tokio::fs::symlink_metadata(path).await?;
                    if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
                        return Err(ExtensionError::InvalidSkillPath(format!(
                            "staging path is not a regular directory: {}",
                            path.display()
                        )));
                    }
                    Ok(())
                }
                Err(error) => Err(ExtensionError::Io(error)),
            }
        }
        Err(error) => Err(ExtensionError::Io(error)),
    }
}

/// Strictly validate one staged market Skill. Unlike [`read_skill_info`], this
/// does not fall back to the directory name: the downloaded Skill must declare
/// the exact slug that the package advertised and must have a description.
pub(crate) async fn validate_market_skill_directory(
    skill_dir: &Path,
    expected_name: &str,
) -> Result<String, ExtensionError> {
    validate_filename(expected_name)?;
    let metadata = tokio::fs::symlink_metadata(skill_dir).await?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(ExtensionError::InvalidSkillPath(format!(
            "market Skill '{expected_name}' is not a regular directory"
        )));
    }
    let skill_file = skill_dir.join(SKILL_MANIFEST_FILE);
    let content = tokio::fs::read_to_string(&skill_file).await.map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ExtensionError::SkillNotFound(skill_file.display().to_string())
        } else {
            ExtensionError::Io(error)
        }
    })?;
    let (declared_name, description) = parse_frontmatter_fields(&content).ok_or_else(|| {
        ExtensionError::InvalidSkillPath(format!(
            "market Skill '{expected_name}' has invalid SKILL.md frontmatter"
        ))
    })?;
    if declared_name.trim().is_empty() || description.trim().is_empty() {
        return Err(ExtensionError::InvalidSkillPath(format!(
            "market Skill '{expected_name}' must declare a non-empty name and description"
        )));
    }
    if declared_name != expected_name {
        return Err(ExtensionError::InvalidSkillPath(format!(
            "market Skill name mismatch: expected '{expected_name}', got '{declared_name}'"
        )));
    }
    Ok(declared_name)
}

/// Validate the manifest of one staged market Skill and return its declared
/// name. Unlike [`validate_market_skill_directory`], the declared name is NOT
/// required to equal the URL slug the market advertised: upstream SkillHub
/// packages routinely declare a manifest name that differs from their public
/// slug (e.g. slug `baozheng` ships `name: baozheng-skills`). Callers that
/// need the slug-equality contract (the expert-package flow) keep using
/// [`validate_market_skill_directory`].
pub(crate) async fn validate_market_skill_manifest(skill_dir: &Path) -> Result<String, ExtensionError> {
    let metadata = tokio::fs::symlink_metadata(skill_dir).await?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(ExtensionError::InvalidSkillPath(format!(
            "market Skill path '{}' is not a regular directory",
            skill_dir.display()
        )));
    }
    let skill_file = skill_dir.join(SKILL_MANIFEST_FILE);
    let content = tokio::fs::read_to_string(&skill_file).await.map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ExtensionError::SkillNotFound(skill_file.display().to_string())
        } else {
            ExtensionError::Io(error)
        }
    })?;
    let (declared_name, description) = parse_frontmatter_fields(&content).ok_or_else(|| {
        ExtensionError::InvalidSkillPath("market Skill has invalid SKILL.md frontmatter".into())
    })?;
    if declared_name.trim().is_empty() || description.trim().is_empty() {
        return Err(ExtensionError::InvalidSkillPath(
            "market Skill must declare a non-empty name and description".into(),
        ));
    }
    Ok(declared_name)
}

/// Result of committing one staged market Skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MarketSkillCommit {
    Created,
    Reused,
}

/// Atomically move one validated staged Skill into the user Skill root.
/// Existing valid user Skills are reused; any other existing entry is left
/// untouched and reported as an error.
pub(crate) async fn commit_market_skill_directory(
    paths: &SkillPaths,
    staged_dir: &Path,
    expected_name: &str,
) -> Result<MarketSkillCommit, ExtensionError> {
    validate_filename(expected_name)?;
    validate_market_skill_directory(staged_dir, expected_name).await?;
    tokio::fs::create_dir_all(&paths.user_skills_dir).await?;
    let target = paths.user_skills_dir.join(expected_name);

    match tokio::fs::symlink_metadata(&target).await {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(ExtensionError::InvalidSkillPath(format!(
                    "user Skill target '{expected_name}' already exists and is not a valid directory"
                )));
            }
            validate_market_skill_directory(&target, expected_name).await?;
            tokio::fs::remove_dir_all(staged_dir).await?;
            Ok(MarketSkillCommit::Reused)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match tokio::fs::rename(staged_dir, &target).await {
                Ok(()) => Ok(MarketSkillCommit::Created),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    // Another window may have committed the same Skill between
                    // the metadata check and rename. Reuse it only if it is
                    // valid; never overwrite the winner.
                    let metadata = tokio::fs::symlink_metadata(&target).await?;
                    if metadata.file_type().is_symlink() || !metadata.is_dir() {
                        return Err(ExtensionError::InvalidSkillPath(format!(
                            "user Skill target '{expected_name}' was concurrently created with an invalid shape"
                        )));
                    }
                    validate_market_skill_directory(&target, expected_name).await?;
                    Ok(MarketSkillCommit::Reused)
                }
                Err(error) => Err(ExtensionError::Io(error)),
            }
        }
        Err(error) => Err(ExtensionError::Io(error)),
    }
}

/// Remove a newly committed Skill during expert-package rollback. Single
/// Skill installation commits provenance before its directory, so it does not
/// use this best-effort package rollback path.
pub(crate) async fn rollback_market_skill(paths: &SkillPaths, name: &str) -> Result<(), ExtensionError> {
    validate_filename(name)?;
    let _ = remove_path_entry(&paths.user_skills_dir.join(name)).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// D. Skill import / export / delete
// ---------------------------------------------------------------------------

/// Import a skill by copying its directory to the user skills directory.
///
/// Returns the skill name.
pub async fn import_skill(paths: &SkillPaths, skill_path: &Path) -> Result<String, ExtensionError> {
    let _mutation_guard = skill_mutation_lock().lock().await;
    let (name, _) = read_skill_info(skill_path).await?;
    validate_filename(&name)?;

    let target_dir = paths.user_skills_dir.join(&name);
    tokio::fs::create_dir_all(&paths.user_skills_dir).await?;

    // Remove stale market provenance before changing the user Skill. If the
    // provenance store is unavailable, leave the existing Skill untouched
    // rather than importing new content under an old market identity.
    clear_market_installation_record(paths, &name).await?;
    // A same-name entry may be an imported directory link. Remove the entry
    // itself before copying so a ZIP refresh cannot write through a Windows
    // junction (or a Unix symlink) into the external source directory.
    remove_path_entry(&target_dir).await?;
    if let Err(error) = copy_dir_recursive(skill_path, &target_dir).await {
        // The target was absent before this operation. Do not expose a
        // partially copied Skill to catalog readers after a failed import.
        let _ = remove_path_entry(&target_dir).await;
        return Err(error);
    }

    debug!(skill = %name, target = %target_dir.display(), "skill imported (copy)");
    Ok(name)
}

/// Import a skill by creating a symlink in the user skills directory.
///
/// Returns the skill name.
pub async fn import_skill_with_symlink(
    paths: &SkillPaths,
    skill_path: &Path,
) -> Result<String, ExtensionError> {
    let _mutation_guard = skill_mutation_lock().lock().await;
    let (name, _) = read_skill_info(skill_path).await?;
    validate_filename(&name)?;

    let target_link = paths.user_skills_dir.join(&name);
    tokio::fs::create_dir_all(&paths.user_skills_dir).await?;

    // See the copy importer: provenance must not survive a local import.
    clear_market_installation_record(paths, &name).await?;
    remove_path_entry(&target_link).await?;

    // Materialize the link, degrading to a recursive copy when the platform
    // symlink/junction primitive fails (non-NTFS removable media, UNC/network
    // source, path-too-long, locked target, or Windows symlink privilege).
    // Mirrors the resilience the per-agent path already has via
    // `link_workspace_skills`; without it these failures surfaced as an opaque
    // 500 "导入技能出错".
    link_skill_or_fallback_copy(skill_path, &target_link).await?;

    debug!(skill = %name, link = %target_link.display(), "skill imported (symlink)");
    Ok(name)
}

/// Maximum directory depth descended when scanning a user-selected folder (or
/// extracted zip) for skills. Bounds the walk so picking a huge tree (e.g. a
/// drive root) cannot trigger a full-disk scan. Descent stops early at any
/// directory containing `SKILL.md`, so this only caps `SKILL.md`-less nesting.
const MAX_IMPORT_SCAN_DEPTH: usize = 6;

/// Import one skill, a parent directory containing skills, or a zip archive.
///
/// Directory inputs preserve the existing symlink behavior. Zip inputs are
/// extracted into an internal temporary directory, then copied into the user
/// skills directory so imported skills do not point at disposable files.
///
/// A directory that is not itself a skill is scanned **recursively** (bounded
/// by [`MAX_IMPORT_SCAN_DEPTH`]) so the user can pick a parent/grandparent or a
/// nested skill bundle — not just a folder whose immediate children are skills.
/// Multi-skill imports are **best-effort**: one malformed skill is skipped
/// (with a warning) rather than aborting the whole import and leaving the
/// already-linked skills orphaned. An error is only returned when *no* skill
/// could be imported, preserving the precise reason for the single-skill case.
pub async fn import_skills_with_symlink(
    paths: &SkillPaths,
    source_path: &Path,
) -> Result<Vec<String>, ExtensionError> {
    if is_zip_path(source_path) {
        return import_skills_from_zip(paths, source_path).await;
    }

    let source_path = normalize_import_source_path(source_path)?;

    if source_path.is_dir() {
        if source_path.join(SKILL_MANIFEST_FILE).exists() {
            return Ok(vec![import_skill_with_symlink(paths, &source_path).await?]);
        }

        let mut skill_dirs = Vec::new();
        collect_skill_dirs_recursive(&source_path, &mut skill_dirs, MAX_IMPORT_SCAN_DEPTH).await?;
        if skill_dirs.is_empty() {
            return Err(ExtensionError::InvalidSkillPath(format!(
                "No skill directories found in {}",
                source_path.display()
            )));
        }

        let mut imported = Vec::new();
        let mut last_err: Option<ExtensionError> = None;
        for skill_dir in &skill_dirs {
            match import_skill_with_symlink(paths, skill_dir).await {
                Ok(name) => imported.push(name),
                Err(e) => {
                    warn!(skill_dir = %skill_dir.display(), error = %e, "skipping skill that failed to import");
                    last_err = Some(e);
                }
            }
        }
        if imported.is_empty() {
            return Err(last_err.unwrap_or_else(|| {
                ExtensionError::InvalidSkillPath(format!(
                    "No importable skills found in {}",
                    source_path.display()
                ))
            }));
        }
        imported.sort();
        imported.dedup();
        return Ok(imported);
    }

    Err(ExtensionError::InvalidSkillPath(format!(
        "Expected a skill directory, parent directory, SKILL.md, or zip archive: {}",
        source_path.display()
    )))
}

async fn import_skills_from_zip(
    paths: &SkillPaths,
    archive_path: &Path,
) -> Result<Vec<String>, ExtensionError> {
    let staging = SkillImportStaging::create(paths).await?;
    let extract_dir = staging.root.clone();

    let extraction = extract_skill_archive_to_staging(
        archive_path,
        &extract_dir,
        staging.cleanup_handle.clone(),
    )
    .await;

    if let Err(err) = extraction {
        staging.cleanup().await;
        return Err(err);
    }

    let result = async {
        let mut skill_dirs = Vec::new();
        collect_skill_dirs_recursive(&extract_dir, &mut skill_dirs, MAX_IMPORT_SCAN_DEPTH).await?;
        if skill_dirs.is_empty() {
            return Err(ExtensionError::InvalidSkillPath(format!(
                "No skill directories found in {}",
                archive_path.display()
            )));
        }

        let mut imported = Vec::new();
        for skill_dir in skill_dirs {
            imported.push(import_skill(paths, &skill_dir).await?);
        }
        imported.sort();
        imported.dedup();
        Ok(imported)
    }
    .await;

    staging.cleanup().await;
    result
}

/// Cancellation/unwind guard for the existing ZIP import path. Market
/// installation has its own guard because it uses a different archive layout,
/// but both guards share the same conservative staging-directory contract.
struct SkillImportStaging {
    root: PathBuf,
    parent: PathBuf,
    cleanup_handle: StagingCleanupHandle,
    armed: bool,
}

impl SkillImportStaging {
    async fn create(paths: &SkillPaths) -> Result<Self, ExtensionError> {
        let parent = paths.user_skills_dir.join(".import-tmp");
        ensure_regular_skill_directory(&parent).await?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = parent.join(format!("skills-{}-{nonce}", std::process::id()));
        if let Err(error) = tokio::fs::create_dir(&root).await {
            let _ = tokio::fs::remove_dir(&parent).await;
            return Err(ExtensionError::Io(error));
        }
        Ok(Self {
            root: root.clone(),
            parent: parent.clone(),
            cleanup_handle: StagingCleanupHandle::new(root, parent),
            armed: true,
        })
    }

    async fn cleanup(mut self) {
        let root_removed = match tokio::fs::remove_dir_all(&self.root).await {
            Ok(()) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
            Err(_) => false,
        };
        let parent_removed = match tokio::fs::remove_dir(&self.parent).await {
            Ok(()) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
            Err(_) => false,
        };
        if root_removed && parent_removed {
            self.armed = false;
        }
    }
}

impl Drop for SkillImportStaging {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        self.cleanup_handle.request_cleanup();
    }
}

/// Remove an operation-owned staging directory without following a symlink or
/// Windows reparse point. This is shared by normal ZIP imports and market
/// installers because their Drop guards run after the async future is gone.
pub(crate) fn remove_staging_directory_sync(path: &Path) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return;
    }
    let _ = fs::remove_dir_all(path);
}

pub(crate) fn remove_empty_staging_parent_sync(path: &Path) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return;
    }
    let _ = fs::remove_dir(path);
}

fn is_zip_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
}

fn skill_modified_time(path: &str) -> SystemTime {
    std::fs::symlink_metadata(path)
        .and_then(|metadata| metadata.modified())
        .unwrap_or(UNIX_EPOCH)
}

fn normalize_import_source_path(source_path: &Path) -> Result<PathBuf, ExtensionError> {
    if source_path.is_file() {
        let file_name = source_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if file_name == SKILL_MANIFEST_FILE {
            return source_path.parent().map(Path::to_path_buf).ok_or_else(|| {
                ExtensionError::InvalidSkillPath(source_path.display().to_string())
            });
        }
    }
    Ok(source_path.to_path_buf())
}

/// Export a skill by creating a symlink in the target directory.
pub async fn export_skill_with_symlink(
    skill_path: &Path,
    target_dir: &Path,
) -> Result<(), ExtensionError> {
    let skill_name = skill_path
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .ok_or_else(|| ExtensionError::InvalidSkillPath(skill_path.display().to_string()))?;

    let target_link = target_dir.join(&skill_name);
    tokio::fs::create_dir_all(target_dir).await?;

    remove_path_entry(&target_link).await?;

    create_symlink(skill_path, &target_link).await?;

    debug!(
        skill = %skill_name,
        link = %target_link.display(),
        "skill exported (symlink)"
    );
    Ok(())
}

/// Delete a user-custom skill by name.
///
/// Returns an error if the skill is built-in or does not exist.
pub async fn delete_skill(paths: &SkillPaths, skill_name: &str) -> Result<(), ExtensionError> {
    // Safety: reject path traversal. Shares [`validate_filename`] rather than
    // re-deriving the rules — this join feeds a *recursive delete*, so an
    // escape here is the costliest in the module, and the inline check this
    // replaced also let `""` and `"."` through, both of which resolve back to
    // `user_skills_dir` itself.
    validate_filename(skill_name)?;
    let _mutation_guard = skill_mutation_lock().lock().await;

    let user_path = paths.user_skills_dir.join(skill_name);

    let target_exists = match tokio::fs::symlink_metadata(&user_path).await {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(ExtensionError::Io(error)),
    };
    if !target_exists {
        // Check if it exists as a built-in (disk override → filesystem,
        // otherwise embedded corpus).
        if builtin_skill_exists(paths, skill_name) {
            return Err(ExtensionError::BuiltinSkillDeletion(skill_name.to_string()));
        }
        // A previous delete may have removed the directory but failed before
        // clearing provenance. Repair that stale sidecar even though the
        // visible Skill is already gone.
        clear_market_installation_record(paths, skill_name).await?;
        return Err(ExtensionError::SkillNotFound(skill_name.to_string()));
    }

    // Move the entry aside before clearing its optional provenance. The
    // rename is on the same filesystem, so a failed provenance update or
    // final deletion can restore the exact directory/link without exposing a
    // half-deleted Skill to another mutation operation.
    let record_snapshot = snapshot_market_installation_record(paths, skill_name).await?;
    let parent = user_path
        .parent()
        .ok_or_else(|| ExtensionError::InvalidSkillPath("skill target has no parent".into()))?;
    let backup = parent.join(format!(".nomifun-delete-backup-{}", projection_nonce()));
    if tokio::fs::symlink_metadata(&backup).await.is_ok() {
        return Err(ExtensionError::Io(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "temporary Skill delete target already exists",
        )));
    }
    tokio::fs::rename(&user_path, &backup).await?;

    if let Err(error) = clear_market_installation_record(paths, skill_name).await {
        if let Err(restore_error) = tokio::fs::rename(&backup, &user_path).await {
            tracing::error!(
                skill = %skill_name,
                error = %restore_error,
                "failed to restore Skill after provenance cleanup failure"
            );
        }
        return Err(error);
    }

    if let Err(error) = remove_path_entry(&backup).await {
        if let Err(restore_error) = tokio::fs::rename(&backup, &user_path).await {
            tracing::error!(
                skill = %skill_name,
                error = %restore_error,
                "failed to restore Skill after delete failure"
            );
        }
        if let Some((record_path, bytes)) = record_snapshot {
            restore_market_installation_record(&record_path, &bytes).await;
        }
        return Err(error);
    }

    debug!(skill = %skill_name, "skill deleted");
    Ok(())
}

const MAX_MARKET_RECORD_SNAPSHOT_BYTES: u64 = 64 * 1024;

/// Snapshot a regular market provenance file before a delete transaction.
/// Invalid or link-like sidecars fail closed; deleting a Skill must not turn
/// an unsafe record path into an untracked visible directory.
async fn snapshot_market_installation_record(
    paths: &SkillPaths,
    skill_name: &str,
) -> Result<Option<(PathBuf, Vec<u8>)>, ExtensionError> {
    validate_filename(skill_name)?;
    let market_root = paths.data_dir.join("skill-market");
    let installations = market_root.join("installations");
    let market_metadata = match tokio::fs::symlink_metadata(&market_root).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(ExtensionError::Io(error)),
    };
    if metadata_is_link_or_reparse(&market_metadata) || !market_metadata.is_dir() {
        return Err(ExtensionError::InvalidSkillPath(
            "market directory is not a regular directory".into(),
        ));
    }
    let installation_metadata = match tokio::fs::symlink_metadata(&installations).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(ExtensionError::Io(error)),
    };
    if metadata_is_link_or_reparse(&installation_metadata) || !installation_metadata.is_dir() {
        return Err(ExtensionError::InvalidSkillPath(
            "market installation records directory is not a regular directory".into(),
        ));
    }
    let path = installations.join(format!("{skill_name}.json"));
    let metadata = match tokio::fs::symlink_metadata(&path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(ExtensionError::Io(error)),
    };
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err(ExtensionError::InvalidSkillPath(
            "market installation record is not a regular file".into(),
        ));
    }
    if metadata.len() > MAX_MARKET_RECORD_SNAPSHOT_BYTES {
        return Err(ExtensionError::InvalidSkillPath(
            "market installation record is too large".into(),
        ));
    }
    let bytes = tokio::fs::read(&path).await?;
    Ok(Some((path, bytes)))
}

async fn restore_market_installation_record(path: &Path, bytes: &[u8]) {
    let path = path.to_path_buf();
    let path_for_log = path.display().to_string();
    let bytes = bytes.to_vec();
    if let Err(error) = tokio::task::spawn_blocking(move || write_atomic_replace(&path, &bytes))
        .await
        .unwrap_or_else(|error| Err(io::Error::other(error.to_string())))
    {
        tracing::error!(path = %path_for_log, %error, "failed to restore market installation record");
    }
}

/// Remove the optional managed-market provenance for a Skill that was replaced
/// by a local import or deleted by the user. A missing record is intentionally
/// a no-op: ordinary user Skills must never become dependent on this sidecar.
pub(crate) async fn clear_market_installation_record(
    paths: &SkillPaths,
    skill_name: &str,
) -> Result<(), ExtensionError> {
    validate_filename(skill_name)?;
    let market_root = paths.data_dir.join("skill-market");
    let installations = market_root.join("installations");
    let market_metadata = match tokio::fs::symlink_metadata(&market_root).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(ExtensionError::Io(error)),
    };
    if metadata_is_link_or_reparse(&market_metadata) || !market_metadata.is_dir() {
        return Err(ExtensionError::InvalidSkillPath(
            "market directory is not a regular directory".into(),
        ));
    }
    let metadata = match tokio::fs::symlink_metadata(&installations).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(ExtensionError::Io(error)),
    };
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err(ExtensionError::InvalidSkillPath(
            "market installation records directory is not a regular directory".into(),
        ));
    }
    match tokio::fs::remove_file(installations.join(format!("{skill_name}.json"))).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ExtensionError::Io(error)),
    }
}

/// Remove an entry without following a link outside the managed skills tree.
///
/// Windows directory junctions are reported as symlinks by
/// [`std::fs::symlink_metadata`], but Windows rejects `DeleteFile` for them
/// with `ERROR_ACCESS_DENIED` (os error 5). Try `remove_dir` for every link
/// first: it removes directory links and junctions themselves, never their
/// targets. A file link reports `NotADirectory`, so it is removed with
/// `remove_file` instead. `symlink_metadata` also lets callers remove dangling
/// links that `Path::exists` would otherwise hide.
///
/// Returns `false` only when no directory entry exists at `path`.
async fn remove_path_entry(path: &Path) -> Result<bool, ExtensionError> {
    #[cfg(test)]
    if test_overrides::should_fail_projection_backup_delete(path) {
        return Err(ExtensionError::Io(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "forced projection backup cleanup failure (test)",
        )));
    }
    let metadata = match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(ExtensionError::Io(error)),
    };

    if metadata.file_type().is_symlink() {
        match tokio::fs::remove_dir(path).await {
            Ok(()) => return Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotADirectory => {
                tokio::fs::remove_file(path).await?;
                return Ok(true);
            }
            Err(error) => return Err(ExtensionError::Io(error)),
        }
    }

    if metadata.is_dir() {
        // ZIP imports may retain Windows read-only attributes, which makes
        // remove_dir_all fail with Access Denied (os error 5).
        clear_readonly_for_deletion(path)?;
        tokio::fs::remove_dir_all(path).await?;
    } else {
        clear_readonly_for_deletion(path)?;
        tokio::fs::remove_file(path).await?;
    }

    Ok(true)
}

/// Clear read-only attributes without following symlinks outside the skill.
///
/// Windows exposes a read-only file attribute that blocks recursive deletion.
/// On Unix, `Permissions::set_readonly(false)` would broaden the file mode to
/// world-writable, so no attribute rewrite is necessary or safe there.
#[cfg(windows)]
fn clear_readonly_for_deletion(path: &Path) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_dir() {
        for entry in std::fs::read_dir(path)? {
            clear_readonly_for_deletion(&entry?.path())?;
        }
    }
    let mut permissions = metadata.permissions();
    if permissions.readonly() {
        permissions.set_readonly(false);
        std::fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn clear_readonly_for_deletion(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Check whether a skill name exists in the built-in corpus — either as
/// a top-level opt-in skill or under `auto-inject/`. Consults the
/// on-disk tree at `paths.builtin_skills_dir`.
fn builtin_skill_exists(paths: &SkillPaths, skill_name: &str) -> bool {
    paths.builtin_skills_dir.join(skill_name).is_dir()
        || paths
            .builtin_skills_dir
            .join(BUILTIN_AUTO_SKILLS_SUBDIR)
            .join(skill_name)
            .is_dir()
}

// ---------------------------------------------------------------------------
// D2. Per-agent skill resolution
// ---------------------------------------------------------------------------

/// A resolved skill reference returned by [`materialize_skills_for_agent`].
///
/// `name` is the skill's requested name; `source_path` is the absolute
/// on-disk directory containing its `SKILL.md`. The caller is expected
/// to symlink that directory into the agent CLI's native skills dir
/// rather than copy it — backend no longer owns per-conversation files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAgentSkill {
    pub name: String,
    pub source_path: PathBuf,
}

/// Summary of one workspace projection pass. The result is intentionally
/// structured so conversation admission can fail closed on a protected user
/// directory instead of treating a partial best-effort link pass as success.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorkspaceSkillProjectionReport {
    pub created: usize,
    pub reused: usize,
    pub repaired: usize,
    pub migrated: usize,
}

const MAX_NATIVE_SKILLS_REL_DIR_LENGTH: usize = 512;

/// Validate a native agent Skill directory before it is joined to a
/// workspace. The value is configuration supplied by a custom ACP agent, so
/// the check is deliberately portable: both slash styles are treated as path
/// separators even when Flowy is running on Unix.
pub fn validate_native_skills_relative_dir(rel: &str) -> Result<(), ExtensionError> {
    let normalized = rel.replace('\\', "/");
    let components: Vec<&str> = normalized.split('/').collect();
    let path = Path::new(rel);
    let invalid = rel.is_empty()
        || rel.len() > MAX_NATIVE_SKILLS_REL_DIR_LENGTH
        || rel.trim().is_empty()
        || rel.chars().any(char::is_control)
        || rel.contains(':')
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::Prefix(_) | Component::RootDir | Component::CurDir | Component::ParentDir
            )
        })
        || normalized.starts_with('/')
        || components.iter().any(|component| component.is_empty() || *component == "." || *component == "..");
    if invalid {
        return Err(ExtensionError::InvalidSkillPath(format!(
            "native Skill directory must be a relative path without traversal: {rel}"
        )));
    }
    Ok(())
}

const MAX_SKILL_PROJECTION_MARKER_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
enum SkillProjectionMode {
    Symlink,
    Junction,
    Copy,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillProjectionMarker {
    schema_version: u32,
    skill_name: String,
    source_path: String,
    content_sha256: String,
    mode: SkillProjectionMode,
}

/// Resolve each requested skill name to its on-disk source directory.
///
/// Search order per name (first match wins):
/// 1. `{user_skills_dir}/{name}/` — user-created custom skill.
/// 2. `{builtin_skills_dir}/{name}/` — top-level opt-in builtin.
/// 3. `{builtin_skills_dir}/auto-inject/{name}/` — auto-inject builtin.
/// 4. `{cron_skills_dir}/{name}/` — per-job cron skill.
///
/// No files are copied and no per-conversation directory is created —
/// the backend just hands the absolute source paths back to the caller,
/// which is responsible for symlinking them where the CLI expects. This
/// replaces the older "copy into `{data_dir}/agent-skills/{conv_id}/`"
/// behavior once the frontend moved to a symlink-only contract.
///
/// Unknown names are silently skipped (a warning is emitted). Names
/// containing path separators or `..` are rejected with a warn and
/// skipped, matching the legacy behavior. Empty names are ignored.
///
/// The returned list is sorted by `name` for determinism. The
/// `conversation_id` is still validated (rejects path-traversal values)
/// so downstream callers can safely use it in log lines or paths even
/// though this function no longer touches disk per-conversation.
pub async fn materialize_skills_for_agent(
    paths: &SkillPaths,
    conversation_id: &str,
    skills: &[String],
) -> Result<Vec<ResolvedAgentSkill>, ExtensionError> {
    validate_filename(conversation_id)?;

    let mut resolved = Vec::with_capacity(skills.len());
    for name in skills {
        if name.is_empty() {
            continue;
        }
        if validate_filename(name).is_err() {
            warn!(skill = %name, "skipping skill with invalid name");
            continue;
        }
        match resolve_skill_source_path(paths, name) {
            Some(source_path) => resolved.push(ResolvedAgentSkill {
                name: name.clone(),
                source_path,
            }),
            None => warn!(skill = %name, "skill not found in any source"),
        }
    }

    resolved.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(resolved)
}

/// Create symlinks from a set of resolved skills into the agent CLI's
/// native skills directories inside `workspace`.
///
/// For each relative `skills_rel_dir` (e.g. `.claude/skills`):
/// 1. Ensure `{workspace}/{skills_rel_dir}/` exists.
/// 2. For each `{ name, source_path }` in `skills`, create a symlink
///    `{workspace}/{skills_rel_dir}/{name} -> {source_path}`.
///
/// Existing projections are reconciled only when a Flowy marker proves that
/// the old entry is ours. Unmarked ordinary directories and untrusted links
/// are protected and return [`ExtensionError::SkillProjectionConflict`].
/// Every filesystem mutation in this function is serialized by the shared
/// Skill mutation lock used by imports, deletes, and market installation.
pub async fn link_workspace_skills(
    paths: &SkillPaths,
    workspace: &Path,
    skills_rel_dirs: &[&str],
    skills: &[ResolvedAgentSkill],
) -> Result<WorkspaceSkillProjectionReport, ExtensionError> {
    let _mutation_guard = acquire_skill_mutation_lock().await;
    link_workspace_skills_with_held_lock(paths, workspace, skills_rel_dirs, skills).await
}

/// Reconcile workspace Skill projections while the caller owns the shared
/// Skill mutation lock. This is used by Companion, which must make its
/// manifest bookkeeping part of the same mutation boundary.
pub async fn link_workspace_skills_with_held_lock(
    paths: &SkillPaths,
    workspace: &Path,
    skills_rel_dirs: &[&str],
    skills: &[ResolvedAgentSkill],
) -> Result<WorkspaceSkillProjectionReport, ExtensionError> {
    if skills_rel_dirs.is_empty() || skills.is_empty() {
        return Ok(WorkspaceSkillProjectionReport::default());
    }

    for rel in skills_rel_dirs {
        validate_native_skills_relative_dir(rel)
            .map_err(|_| projection_conflict("<workspace>", "native Skill directory is not a safe relative path"))?;
    }

    ensure_projection_directory(workspace).await?;
    let mut report = WorkspaceSkillProjectionReport::default();
    for rel in skills_rel_dirs {
        let target_skills_dir = workspace.join(rel);
        ensure_projection_directory(&target_skills_dir).await?;

        for skill in skills {
            match reconcile_workspace_skill_projection(paths, &target_skills_dir, skill).await? {
                ProjectionOutcome::Created => report.created += 1,
                ProjectionOutcome::Reused => report.reused += 1,
                ProjectionOutcome::Repaired => report.repaired += 1,
                ProjectionOutcome::Migrated => report.migrated += 1,
            }
        }
    }
    Ok(report)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectionOutcome {
    Created,
    Reused,
    Repaired,
    Migrated,
}

async fn ensure_projection_directory(path: &Path) -> Result<(), ExtensionError> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => {
            if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
                return Err(ExtensionError::SkillProjectionConflict(format!(
                    "workspace Skill directory is not a regular directory: {}",
                    path.display()
                )));
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path.parent().ok_or_else(|| {
                ExtensionError::SkillProjectionConflict(format!(
                    "workspace Skill directory has no regular parent: {}",
                    path.display()
                ))
            })?;
            if parent != path && !parent.as_os_str().is_empty() {
                Box::pin(ensure_projection_directory(parent)).await?;
            }
            match tokio::fs::create_dir(path).await {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let metadata = tokio::fs::symlink_metadata(path).await?;
                    if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
                        return Err(ExtensionError::SkillProjectionConflict(format!(
                            "workspace Skill directory is not a regular directory: {}",
                            path.display()
                        )));
                    }
                    Ok(())
                }
                Err(error) => Err(ExtensionError::Io(error)),
            }
        }
        Err(error) => Err(ExtensionError::Io(error)),
    }
}

async fn reconcile_workspace_skill_projection(
    paths: &SkillPaths,
    target_skills_dir: &Path,
    skill: &ResolvedAgentSkill,
) -> Result<ProjectionOutcome, ExtensionError> {
    validate_filename(&skill.name)
        .map_err(|_| ExtensionError::SkillProjectionConflict(format!("invalid Skill name: {}", skill.name)))?;

    let source_metadata = tokio::fs::metadata(&skill.source_path).await?;
    if !source_metadata.is_dir() {
        return Err(projection_conflict(&skill.name, "source is not a regular Skill directory"));
    }
    let source_canonical = tokio::fs::canonicalize(&skill.source_path)
        .await
        .map_err(|_| projection_conflict(&skill.name, "source Skill cannot be canonicalized"))?;
    let source_canonical_metadata = tokio::fs::metadata(&source_canonical).await?;
    if !source_canonical_metadata.is_dir() {
        return Err(projection_conflict(&skill.name, "source is not a regular Skill directory"));
    }
    // Imported user Skills may intentionally be represented by a symlink or
    // junction in the canonical user Skill root. Hash the resolved regular
    // directory, but retain the logical Flowy path in the marker so the
    // provenance check can recognize that import on the next materialize pass.
    let source_hash = hash_skill_directory_for_projection(&source_canonical).await?;
    let target = target_skills_dir.join(&skill.name);
    let marker_path = target_skills_dir
        .join(".nomifun-managed")
        .join(format!("{}.json", skill.name));
    let marker = read_projection_marker(&marker_path, &skill.name).await?;
    let target_metadata = tokio::fs::symlink_metadata(&target).await;

    match target_metadata {
        Ok(metadata) if metadata_is_link_or_reparse(&metadata) => {
            let Some(marker) = marker else {
                let target_canonical = tokio::fs::canonicalize(&target)
                    .await
                    .map_err(|_| projection_conflict(&skill.name, "unmarked Skill link is dangling"))?;
                if is_known_flowy_projection_link(paths, &target, &target_canonical, &skill.name).await {
                    write_projection_marker(
                        &marker_path,
                        &SkillProjectionMarker {
                            schema_version: 1,
                            skill_name: skill.name.clone(),
                            source_path: skill.source_path.to_string_lossy().into_owned(),
                            content_sha256: source_hash,
                            mode: existing_link_projection_mode(),
                        },
                    )
                    .await?;
                    return Ok(ProjectionOutcome::Migrated);
                }
                return Err(projection_conflict(&skill.name, "unmarked link is not Flowy-owned"));
            };
            if marker.mode == SkillProjectionMode::Copy {
                return Err(projection_conflict(&skill.name, "copy marker points at a link"));
            }
            let marker_source = canonicalize_marker_source(paths, &marker, &skill.name).await?;
            let target_canonical = match tokio::fs::canonicalize(&target).await {
                Ok(target_canonical) => target_canonical,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    // The marker proves that this link was previously created
                    // by Flowy. A deleted external source or a user-owned
                    // unmarked link is handled above as a conflict; a marked
                    // dangling projection can therefore be safely rebuilt.
                    replace_workspace_skill_projection(
                        &target,
                        &marker_path,
                        Some(marker),
                        &skill.source_path,
                        &skill.name,
                        &skill.source_path,
                        &source_hash,
                    )
                    .await?;
                    return Ok(ProjectionOutcome::Repaired);
                }
                Err(_) => return Err(projection_conflict(&skill.name, "managed Skill link cannot be inspected")),
            };
            if !same_projection_path(&marker_source, &target_canonical) {
                return Err(projection_conflict(&skill.name, "managed Skill link was redirected"));
            }
            if same_projection_path(&source_canonical, &marker_source) && marker.content_sha256 == source_hash {
                return Ok(ProjectionOutcome::Reused);
            }
            replace_workspace_skill_projection(
                &target,
                &marker_path,
                Some(marker),
                &skill.source_path,
                &skill.name,
                &skill.source_path,
                &source_hash,
            )
            .await?;
            Ok(ProjectionOutcome::Repaired)
        }
        Ok(metadata) if metadata.is_dir() => {
            let Some(marker) = marker else {
                return Err(projection_conflict(&skill.name, "unmarked ordinary directory is protected"));
            };
            if marker.mode != SkillProjectionMode::Copy {
                return Err(projection_conflict(&skill.name, "link marker points at an ordinary directory"));
            }
            let marker_source = canonicalize_marker_source(paths, &marker, &skill.name).await?;
            let target_hash = hash_skill_directory_for_projection(&target).await?;
            if target_hash != marker.content_sha256 {
                return Err(projection_conflict(&skill.name, "managed copy was modified"));
            }
            if same_projection_path(&source_canonical, &marker_source) && marker.content_sha256 == source_hash {
                return Ok(ProjectionOutcome::Reused);
            }
            replace_workspace_skill_projection(
                &target,
                &marker_path,
                Some(marker),
                &skill.source_path,
                &skill.name,
                &skill.source_path,
                &source_hash,
            )
            .await?;
            Ok(ProjectionOutcome::Repaired)
        }
        Ok(_) => Err(projection_conflict(&skill.name, "Skill projection target is not a directory")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            replace_workspace_skill_projection(
                &target,
                &marker_path,
                marker,
                &skill.source_path,
                &skill.name,
                &skill.source_path,
                &source_hash,
            )
            .await?;
            Ok(ProjectionOutcome::Created)
        }
        Err(error) => Err(ExtensionError::Io(error)),
    }
}

fn projection_conflict(skill_name: &str, reason: &str) -> ExtensionError {
    ExtensionError::SkillProjectionConflict(format!("Skill '{skill_name}' projection conflict: {reason}"))
}

async fn canonicalize_marker_source(
    paths: &SkillPaths,
    marker: &SkillProjectionMarker,
    skill_name: &str,
) -> Result<PathBuf, ExtensionError> {
    if marker.source_path.trim().is_empty() {
        return Err(projection_conflict(skill_name, "marker has no source path"));
    }
    let source = PathBuf::from(&marker.source_path);
    let metadata = tokio::fs::metadata(&source).await.map_err(|_| {
        projection_conflict(skill_name, "marker source no longer exists")
    })?;
    if !metadata.is_dir() {
        return Err(projection_conflict(skill_name, "marker source is not a regular directory"));
    }
    let canonical = tokio::fs::canonicalize(&source)
        .await
        .map_err(|_| projection_conflict(skill_name, "marker source cannot be canonicalized"))?;
    if !is_known_flowy_skill_source_path(paths, &source, &canonical, skill_name).await {
        return Err(projection_conflict(skill_name, "marker source is outside Flowy Skill roots"));
    }
    Ok(canonical)
}

async fn read_projection_marker(
    path: &Path,
    skill_name: &str,
) -> Result<Option<SkillProjectionMarker>, ExtensionError> {
    let Some(parent) = path.parent() else {
        return Err(projection_conflict(skill_name, "marker path has no parent"));
    };
    match tokio::fs::symlink_metadata(parent).await {
        Ok(metadata) if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() => {
            return Err(projection_conflict(skill_name, "marker directory is not regular"));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(ExtensionError::Io(error)),
        Ok(_) => {}
    }
    let metadata = match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(ExtensionError::Io(error)),
    };
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err(projection_conflict(skill_name, "marker is not a regular file"));
    }
    if metadata.len() > MAX_SKILL_PROJECTION_MARKER_BYTES as u64 {
        return Err(projection_conflict(skill_name, "marker is too large"));
    }
    let bytes = tokio::fs::read(path).await?;
    let marker = serde_json::from_slice::<SkillProjectionMarker>(&bytes)
        .map_err(|_| projection_conflict(skill_name, "marker is invalid"))?;
    if marker.schema_version != 1
        || marker.skill_name != skill_name
        || marker.source_path.trim().is_empty()
        || !is_sha256(&marker.content_sha256)
    {
        return Err(projection_conflict(skill_name, "marker fields are invalid"));
    }
    Ok(Some(marker))
}

async fn replace_workspace_skill_projection(
    target: &Path,
    marker_path: &Path,
    previous_marker: Option<SkillProjectionMarker>,
    source: &Path,
    skill_name: &str,
    source_marker_path: &Path,
    source_hash: &str,
) -> Result<(), ExtensionError> {
    let backup = match tokio::fs::symlink_metadata(target).await {
        Ok(_) => {
            let parent = target
                .parent()
                .ok_or_else(|| projection_conflict(skill_name, "projection target has no parent"))?;
            let backup = parent.join(format!(".{}.projection-backup-{}", skill_name, projection_nonce()));
            tokio::fs::rename(target, &backup).await?;
            Some(backup)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(ExtensionError::Io(error)),
    };

    let mode = match link_skill_or_fallback_copy_with_mode(source, target).await {
        Ok(mode) => mode,
        Err(error) => {
            if let Err(rollback_error) = restore_workspace_projection(
                target,
                backup.as_deref(),
                marker_path,
                skill_name,
                previous_marker.as_ref(),
            )
            .await
            {
                return Err(projection_conflict(
                    skill_name,
                    &format!("projection creation failed and rollback failed: {error}; {rollback_error}"),
                ));
            }
            return Err(error);
        }
    };
    let marker = SkillProjectionMarker {
        schema_version: 1,
        skill_name: skill_name.to_owned(),
        source_path: source_marker_path.to_string_lossy().into_owned(),
        content_sha256: source_hash.to_owned(),
        mode,
    };
    if let Err(error) = write_projection_marker(marker_path, &marker).await {
        if let Err(rollback_error) = restore_workspace_projection(
            target,
            backup.as_deref(),
            marker_path,
            skill_name,
            previous_marker.as_ref(),
        )
        .await
        {
            return Err(projection_conflict(
                skill_name,
                &format!("projection marker write failed and rollback failed: {error}; {rollback_error}"),
            ));
        }
        return Err(error);
    }
    if let Some(backup) = backup {
        // The projection and marker are already committed. A backup cleanup
        // failure must not report an error that leaves callers believing the
        // new projection was rolled back; retain this private recovery entry
        // for a later cleanup pass instead.
        if let Err(error) = remove_path_entry(&backup).await {
            warn!(
                backup = %backup.display(),
                error = %error,
                "committed Skill projection backup could not be removed"
            );
        }
    }
    Ok(())
}

async fn restore_workspace_projection(
    target: &Path,
    backup: Option<&Path>,
    marker_path: &Path,
    skill_name: &str,
    previous_marker: Option<&SkillProjectionMarker>,
) -> Result<(), ExtensionError> {
    let mut failures = Vec::new();
    let target_removed = match remove_path_entry(target).await {
        Ok(_) => true,
        Err(error) => {
            failures.push(format!("remove new target: {error}"));
            false
        }
    };
    if target_removed
        && let Some(backup) = backup
        && let Err(error) = tokio::fs::rename(backup, target).await
    {
        failures.push(format!("restore previous target: {error}"));
    }
    match previous_marker {
        Some(marker) => {
            if let Err(error) = write_projection_marker(marker_path, marker).await {
                failures.push(format!("restore previous marker: {error}"));
            }
        }
        None => {
            if let Err(error) = remove_projection_marker(marker_path).await {
                failures.push(format!("remove new marker: {error}"));
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(projection_conflict(
            skill_name,
            &format!("rollback incomplete: {}", failures.join("; ")),
        ))
    }
}

async fn write_projection_marker(path: &Path, marker: &SkillProjectionMarker) -> Result<(), ExtensionError> {
    #[cfg(test)]
    if test_overrides::should_fail_next_projection_marker_write() {
        return Err(ExtensionError::Io(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "forced projection marker write failure (test)",
        )));
    }
    let parent = path
        .parent()
        .ok_or_else(|| ExtensionError::SkillProjectionConflict("marker path has no parent".into()))?;
    ensure_projection_directory(parent).await?;
    let bytes = serde_json::to_vec_pretty(marker)
        .map_err(|error| ExtensionError::Io(io::Error::other(error.to_string())))?;
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || write_atomic_replace(&path, &bytes))
        .await
        .map_err(|error| ExtensionError::Io(io::Error::other(error.to_string())))?
        .map_err(ExtensionError::Io)
}

async fn remove_projection_marker(path: &Path) -> Result<(), ExtensionError> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) if metadata_is_link_or_reparse(&metadata) || !metadata.is_file() => {
            Err(ExtensionError::SkillProjectionConflict("projection marker is not a regular file".into()))
        }
        Ok(_) => tokio::fs::remove_file(path).await.map_err(ExtensionError::Io),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ExtensionError::Io(error)),
    }
}

fn projection_nonce() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

async fn hash_skill_directory_for_projection(path: &Path) -> Result<String, ExtensionError> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || hash_skill_directory_content_sync(&path))
        .await
        .map_err(|error| ExtensionError::Io(io::Error::other(error.to_string())))?
        .map_err(ExtensionError::Io)
}

const MAX_SKILL_CONTENT_HASH_DEPTH: usize = 32;
const MAX_SKILL_CONTENT_HASH_ENTRIES: usize = 8_192;
const MAX_SKILL_CONTENT_HASH_BYTES: u64 = 256 * 1024 * 1024;

/// Hash a regular Skill directory deterministically without following links
/// and with bounded traversal. Market provenance and workspace markers use
/// this same implementation so their ownership evidence cannot disagree.
pub(crate) fn hash_skill_directory_content_sync(path: &Path) -> io::Result<String> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err(io::Error::other("Skill directory is not a regular directory"));
    }
    let mut files = Vec::new();
    let mut entries_seen = 0;
    let mut bytes_seen = 0;
    collect_projection_files(path, path, &mut files, 0, &mut entries_seen, &mut bytes_seen)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));

    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut hashed_bytes = 0_u64;
    for (relative, file_path, length) in files {
        let path_bytes = relative.as_bytes();
        hasher.update((path_bytes.len() as u64).to_le_bytes());
        hasher.update(path_bytes);
        hasher.update(length.to_le_bytes());
        let mut file = std::fs::File::open(file_path)?;
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hashed_bytes = hashed_bytes.saturating_add(read as u64);
            if hashed_bytes > MAX_SKILL_CONTENT_HASH_BYTES {
                return Err(io::Error::other("Skill directory exceeds the supported size"));
            }
            hasher.update(&buffer[..read]);
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn collect_projection_files(
    root: &Path,
    current: &Path,
    files: &mut Vec<(String, PathBuf, u64)>,
    depth: usize,
    entries_seen: &mut usize,
    bytes_seen: &mut u64,
) -> io::Result<()> {
    if depth > MAX_SKILL_CONTENT_HASH_DEPTH {
        return Err(io::Error::other("Skill directory exceeds the supported depth"));
    }
    let mut entries = std::fs::read_dir(current)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        *entries_seen = entries_seen.saturating_add(1);
        if *entries_seen > MAX_SKILL_CONTENT_HASH_ENTRIES {
            return Err(io::Error::other("Skill directory contains too many entries"));
        }
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata_is_link_or_reparse(&metadata) {
            return Err(io::Error::other("Skill directory contains a link or reparse point"));
        }
        if metadata.is_dir() {
            collect_projection_files(root, &path, files, depth + 1, entries_seen, bytes_seen)?;
        } else if metadata.is_file() {
            *bytes_seen = bytes_seen.saturating_add(metadata.len());
            if *bytes_seen > MAX_SKILL_CONTENT_HASH_BYTES {
                return Err(io::Error::other("Skill directory exceeds the supported size"));
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|_| io::Error::other("Skill path escaped its root"))?
                .to_string_lossy()
                .replace('\\', "/");
            files.push((relative, path, metadata.len()));
        } else {
            return Err(io::Error::other("Skill directory contains an unsupported entry"));
        }
    }
    Ok(())
}

async fn is_known_flowy_skill_source(paths: &SkillPaths, candidate: &Path, skill_name: &str) -> bool {
    if candidate.file_name().and_then(|name| name.to_str()) != Some(skill_name) || !candidate.is_dir() {
        return false;
    }
    let roots = [
        paths.user_skills_dir.clone(),
        paths.builtin_skills_dir.clone(),
        paths.builtin_skills_dir.join(BUILTIN_AUTO_SKILLS_SUBDIR),
        paths.cron_skills_dir.clone(),
    ];
    for root in roots {
        let Ok(root) = tokio::fs::canonicalize(root).await else {
            continue;
        };
        // Historical links are migratable only when their resolved target is
        // the direct Skill child that Flowy itself would resolve. A nested
        // descendant under a known root may be user data and must not gain a
        // managed marker merely because it shares the root prefix.
        if candidate
            .parent()
            .is_some_and(|parent| same_projection_path(parent, &root))
        {
            return true;
        }
    }
    false
}

async fn is_known_flowy_skill_source_path(
    paths: &SkillPaths,
    logical: &Path,
    canonical: &Path,
    skill_name: &str,
) -> bool {
    if logical.file_name().and_then(|name| name.to_str()) != Some(skill_name) {
        return false;
    }
    let roots = [
        paths.user_skills_dir.clone(),
        paths.builtin_skills_dir.clone(),
        paths.builtin_skills_dir.join(BUILTIN_AUTO_SKILLS_SUBDIR),
        paths.cron_skills_dir.clone(),
    ];
    // A user import can be a link whose canonical target is intentionally
    // outside Flowy's roots. Accept only the exact direct-child path that the
    // resolver itself can return; parent-prefix checks would allow `..` shapes.
    if roots.iter().any(|root| {
        logical
            .parent()
            .is_some_and(|parent| same_projection_path(parent, root))
    }) {
        return tokio::fs::canonicalize(logical)
            .await
            .is_ok_and(|resolved| same_projection_path(&resolved, canonical));
    }
    is_known_flowy_skill_source(paths, canonical, skill_name).await
}

async fn is_known_flowy_projection_link(
    paths: &SkillPaths,
    target: &Path,
    target_canonical: &Path,
    skill_name: &str,
) -> bool {
    let Some(parent) = target.parent() else {
        return false;
    };
    let Some(logical_target) = projection_link_logical_target(target, parent).await else {
        return is_known_flowy_skill_source(paths, target_canonical, skill_name).await;
    };
    is_known_flowy_skill_source_path(paths, &logical_target, target_canonical, skill_name).await
}

async fn projection_link_logical_target(target: &Path, parent: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let target_path = target.to_path_buf();
        if let Ok(Ok(link_target)) =
            tokio::task::spawn_blocking(move || junction::get_target(target_path)).await
        {
            return Some(link_target);
        }
    }

    let link_target = tokio::fs::read_link(target).await.ok()?;
    Some(if link_target.is_absolute() {
        link_target
    } else {
        parent.join(link_target)
    })
}

fn same_projection_path(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        left.to_string_lossy().eq_ignore_ascii_case(&right.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn existing_link_projection_mode() -> SkillProjectionMode {
    #[cfg(windows)]
    {
        SkillProjectionMode::Junction
    }
    #[cfg(not(windows))]
    {
        SkillProjectionMode::Symlink
    }
}

/// Resolve a skill name to its on-disk source directory using the same
/// search order as [`materialize_skills_for_agent`]. Returns `None` if
/// no matching directory exists in any known source.
fn resolve_skill_source_path(paths: &SkillPaths, name: &str) -> Option<PathBuf> {
    // Keep execution precedence aligned with `list_available_skills`: a
    // user-authored skill intentionally overrides a same-name builtin.
    let user = paths.user_skills_dir.join(name);
    if user.is_dir() {
        return Some(user);
    }
    let top = paths.builtin_skills_dir.join(name);
    if top.is_dir() {
        return Some(top);
    }
    let auto = paths
        .builtin_skills_dir
        .join(BUILTIN_AUTO_SKILLS_SUBDIR)
        .join(name);
    if auto.is_dir() {
        return Some(auto);
    }
    let cron = paths.cron_skills_dir.join(name);
    if cron.is_dir() {
        return Some(cron);
    }
    None
}

// ---------------------------------------------------------------------------
// E. Scanning & discovery
// ---------------------------------------------------------------------------

/// Scan a directory for subdirectories containing SKILL.md.
pub async fn scan_for_skills(folder_path: &Path) -> Result<Vec<ScannedSkill>, ExtensionError> {
    scan_skill_dirs(folder_path).await
}

/// Named filesystem path.
#[derive(Debug, Clone, PartialEq)]
pub struct NamedPath {
    pub name: String,
    pub path: String,
}

/// Detect common skill paths relative to the user's home directory.
///
/// Returns paths that actually exist on the filesystem.
pub async fn detect_common_skill_paths() -> Vec<NamedPath> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };

    let mut result = Vec::new();
    for (name, rel_path, _slug) in COMMON_SKILL_DIRS {
        let full_path = home.join(rel_path);
        if full_path.exists() {
            result.push(NamedPath {
                name: (*name).to_string(),
                path: full_path.to_string_lossy().into_owned(),
            });
        }
    }

    result
}

/// An external skill source with discovered skills.
///
/// `source` is a stable slug identifying the origin — matches the
/// `ExternalSkillSourceResponse.source` contract consumed by the renderer.
/// Values are drawn from [`COMMON_SKILL_DIRS`] for built-in entries or
/// `format!("custom-{path}")` for user-added paths, so they stay unique
/// across the returned list.
#[derive(Debug, Clone, PartialEq)]
pub struct ExternalSkillSource {
    pub name: String,
    pub path: String,
    pub source: String,
    pub skill_count: usize,
    pub skills: Vec<ScannedSkill>,
}

/// Compute the stable `source` slug for a custom external path.
fn custom_source_slug(path: &str) -> String {
    format!("custom-{path}")
}

/// Discover external skills from common paths and custom external paths.
///
/// The returned list preserves deterministic `source` slugs — see
/// [`ExternalSkillSource::source`] for the contract.
pub async fn detect_and_count_external_skills(
    custom_paths: &[NamedPath],
) -> Vec<ExternalSkillSource> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };

    let mut sources = Vec::new();

    // 1. Common paths (iterate the constant table so we keep the per-entry slug).
    for (name, rel_path, slug) in COMMON_SKILL_DIRS {
        let full_path = home.join(rel_path);
        if !full_path.exists() {
            continue;
        }
        if let Ok(skills) = scan_skill_dirs(&full_path).await {
            sources.push(ExternalSkillSource {
                name: (*name).to_string(),
                path: full_path.to_string_lossy().into_owned(),
                source: (*slug).to_string(),
                skill_count: skills.len(),
                skills,
            });
        }
    }

    // 2. Custom external paths
    for np in custom_paths {
        let path = Path::new(&np.path);
        if let Ok(skills) = scan_skill_dirs(path).await {
            sources.push(ExternalSkillSource {
                name: np.name.clone(),
                path: np.path.clone(),
                source: custom_source_slug(&np.path),
                skill_count: skills.len(),
                skills,
            });
        }
    }

    sources
}

/// Get the user and built-in skill directory paths.
///
/// Both values are real on-disk paths. The built-in path points at the
/// tree populated at startup by
/// [`crate::startup_materialize::materialize_if_needed`], or at the
/// [`BUILTIN_SKILLS_ENV_VAR`] override when set.
pub fn get_skill_paths(paths: &SkillPaths) -> (String, String) {
    (
        paths.user_skills_dir.to_string_lossy().into_owned(),
        paths.builtin_skills_dir.to_string_lossy().into_owned(),
    )
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Read a file and return its content, or an empty string if it does not exist.
async fn read_file_or_empty(path: &Path) -> Result<String, ExtensionError> {
    match tokio::fs::read_to_string(path).await {
        Ok(content) => Ok(content),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(ExtensionError::Io(e)),
    }
}

/// Validate a filename to prevent path traversal. The name must be usable as a
/// single path segment appended to a base directory.
///
/// `':'` is rejected outright. A leading `<letter>:` makes `base.join(name)`
/// *drive-relative* on Windows: `Path::is_absolute("c:evil")` is `false`, so an
/// absolute-path guard never fires, yet `join` discards the base and resolves
/// against the current directory of that drive. A later `':'` opens an NTFS
/// alternate data stream, where the bytes land on a hidden stream of a
/// differently-named visible file and `Path::extension` reads the stream name
/// instead of the file's. Neither is a legal Windows path segment, so this
/// costs no usable name.
///
/// A lone `"."` is rejected because `base.join(".")` resolves back to `base`,
/// which would aim a per-skill delete or write at the whole skills tree.
fn validate_filename(name: &str) -> Result<(), ExtensionError> {
    if name.is_empty()
        || name == "."
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.contains(':')
    {
        return Err(ExtensionError::PathTraversal(name.to_string()));
    }
    Ok(())
}

/// Validate a relative path inside the built-in skill corpus. Allows
/// forward slashes (paths like `"auto-inject/cron/SKILL.md"` are
/// normal) but forbids empty segments, backslashes, leading slash,
/// absolute paths, and any `..` component.
/// Rejects `':'` in any segment for the reasons given on [`validate_filename`]
/// — note that `Path::new("z:x/y").is_absolute()` is `false` on Windows, so the
/// absolute check below does not cover a drive prefix.
fn validate_builtin_skill_path(rel: &str) -> Result<(), ExtensionError> {
    if rel.is_empty() || rel.contains('\\') || rel.contains("..") || rel.starts_with('/') {
        return Err(ExtensionError::PathTraversal(rel.to_string()));
    }
    if rel.contains(':') {
        return Err(ExtensionError::PathTraversal(rel.to_string()));
    }
    if rel.split('/').any(|seg| seg.is_empty() || seg == ".") {
        return Err(ExtensionError::PathTraversal(rel.to_string()));
    }
    if Path::new(rel).is_absolute() {
        return Err(ExtensionError::PathTraversal(rel.to_string()));
    }
    Ok(())
}

/// Scan a directory for subdirectories containing a SKILL.md file.
async fn scan_skill_dirs(dir: &Path) -> Result<Vec<ScannedSkill>, ExtensionError> {
    let mut result = Vec::new();

    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(result),
        Err(e) => return Err(ExtensionError::Io(e)),
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let entry_path = entry.path();
        if !entry_path.is_dir() {
            continue;
        }

        let skill_file = entry_path.join(SKILL_MANIFEST_FILE);
        if !skill_file.exists() {
            continue;
        }

        match tokio::fs::read_to_string(&skill_file).await {
            Ok(content) => {
                if let Some((name, description)) = parse_frontmatter_fields(&content) {
                    let final_name = if name.is_empty() {
                        entry_path
                            .file_name()
                            .map(|f| f.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    } else {
                        name
                    };
                    result.push(ScannedSkill {
                        name: final_name,
                        description,
                        path: entry_path.to_string_lossy().into_owned(),
                    });
                } else {
                    warn!(
                        path = %skill_file.display(),
                        "skipping skill: SKILL.md has no valid frontmatter (missing/empty description?)"
                    );
                }
            }
            Err(e) => {
                warn!(
                    path = %skill_file.display(),
                    error = %e,
                    "failed to read SKILL.md"
                );
            }
        }
    }

    result.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(result)
}

pub(crate) async fn collect_skill_dirs_recursive(
    dir: &Path,
    result: &mut Vec<PathBuf>,
    max_depth: usize,
) -> Result<(), ExtensionError> {
    if dir.join(SKILL_MANIFEST_FILE).exists() {
        result.push(dir.to_path_buf());
        return Ok(());
    }

    if max_depth == 0 {
        return Ok(());
    }

    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(ExtensionError::Io(e)),
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let entry_path = entry.path();
        if entry_path.is_dir() {
            Box::pin(collect_skill_dirs_recursive(
                &entry_path,
                result,
                max_depth - 1,
            ))
            .await?;
        }
    }

    result.sort();
    Ok(())
}

/// Parse SKILL.md frontmatter to extract name and description.
///
/// Expected format:
/// ```text
/// ---
/// name: skill-name
/// description: One line description
/// ---
/// Body content here...
/// ```
///
/// `description` may also be a YAML block scalar (`|` / `>`, with optional
/// chomping indicators) or have its value on the following indented line(s);
/// such continuations are gathered into the value rather than mis-read as the
/// literal indicator `"|"`/`">"` or dropped entirely. Surrounding quotes on a
/// single-line value are stripped. A `description` that resolves to empty (or
/// is absent) yields `None` — the description is the agent's trigger text and
/// is treated as required, consistent with the Skills spec.
fn parse_frontmatter_fields(content: &str) -> Option<(String, String)> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }

    // Closing fence must be on its own line so a `---` inside a value doesn't
    // truncate the block.
    let after_open = &trimmed[3..];
    let close_idx = after_open.find("\n---")?;
    let frontmatter = &after_open[..close_idx];

    let mut name = String::new();
    let mut description = String::new();

    let lines: Vec<&str> = frontmatter.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        i += 1;

        if line.is_empty() {
            continue;
        }

        if let Some(val) = strip_yaml_key(line, "name") {
            name = unquote(val.trim()).to_string();
        } else if let Some(val) = strip_yaml_key(line, "description") {
            let val = val.trim();
            let is_block = val.starts_with('|') || val.starts_with('>');
            if is_block || val.is_empty() {
                // Folded (`>` or plain next-line) joins with spaces; literal
                // (`|`) preserves line breaks.
                let folded = !val.starts_with('|');
                let mut collected: Vec<String> = Vec::new();
                while i < lines.len() {
                    let cont = lines[i];
                    if cont.trim().is_empty() {
                        collected.push(String::new());
                        i += 1;
                        continue;
                    }
                    // Continuation lines are indented; a column-0 line starts a
                    // new key and ends the block.
                    if cont.len() == cont.trim_start().len() {
                        break;
                    }
                    collected.push(cont.trim().to_string());
                    i += 1;
                }
                while collected.last().is_some_and(|s| s.is_empty()) {
                    collected.pop();
                }
                description = if folded {
                    collected
                        .join(" ")
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                } else {
                    collected.join("\n")
                };
            } else {
                description = unquote(val).to_string();
            }
        }
    }

    if description.trim().is_empty() {
        return None;
    }

    Some((name, description))
}

/// Strip a `key:` prefix from an already-trimmed line, returning the raw value
/// (which may be empty). Rejects look-alike keys such as `namespace:` for
/// `name`.
fn strip_yaml_key<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.strip_prefix(key)?.strip_prefix(':')
}

/// Remove a single pair of matching surrounding quotes, if present.
fn unquote(s: &str) -> &str {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 {
        let (first, last) = (bytes[0], bytes[bytes.len() - 1]);
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return &s[1..s.len() - 1];
        }
    }
    s
}

const MAX_COPY_TREE_DEPTH: usize = 32;
const MAX_COPY_TREE_ENTRIES: usize = 8_192;
const MAX_COPY_TREE_BYTES: u64 = 256 * 1024 * 1024;

/// Validate a copy source without following links or reparse points. This
/// preflight keeps a failed fallback from leaving a partially copied target
/// and bounds the amount of local data an import can traverse.
async fn validate_copy_tree(
    path: &Path,
    depth: usize,
    entries_seen: &mut usize,
    bytes_seen: &mut u64,
) -> Result<(), ExtensionError> {
    if depth > MAX_COPY_TREE_DEPTH {
        return Err(ExtensionError::InvalidSkillPath(
            "Skill copy source exceeds the supported directory depth".into(),
        ));
    }
    let metadata = tokio::fs::symlink_metadata(path).await?;
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err(ExtensionError::InvalidSkillPath(
            "Skill copy source contains a link or is not a directory".into(),
        ));
    }
    let mut dir = tokio::fs::read_dir(path).await?;
    while let Some(entry) = dir.next_entry().await? {
        *entries_seen = entries_seen.saturating_add(1);
        if *entries_seen > MAX_COPY_TREE_ENTRIES {
            return Err(ExtensionError::InvalidSkillPath(
                "Skill copy source contains too many entries".into(),
            ));
        }
        let entry_path = entry.path();
        let entry_metadata = tokio::fs::symlink_metadata(&entry_path).await?;
        if metadata_is_link_or_reparse(&entry_metadata) {
            return Err(ExtensionError::InvalidSkillPath(
                "Skill copy source contains a link or reparse point".into(),
            ));
        }
        if entry_metadata.is_dir() {
            Box::pin(validate_copy_tree(
                &entry_path,
                depth + 1,
                entries_seen,
                bytes_seen,
            ))
            .await?;
        } else if entry_metadata.is_file() {
            *bytes_seen = bytes_seen.saturating_add(entry_metadata.len());
            if *bytes_seen > MAX_COPY_TREE_BYTES {
                return Err(ExtensionError::InvalidSkillPath(
                    "Skill copy source exceeds the supported size".into(),
                ));
            }
        } else {
            return Err(ExtensionError::InvalidSkillPath(
                "Skill copy source contains an unsupported entry".into(),
            ));
        }
    }
    Ok(())
}

/// Recursively copy a directory after a no-link, bounded preflight.
async fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), ExtensionError> {
    let mut entries_seen = 0;
    let mut bytes_seen = 0;
    validate_copy_tree(src, 0, &mut entries_seen, &mut bytes_seen).await?;
    copy_dir_recursive_unchecked(src, dst).await
}

async fn copy_dir_recursive_unchecked(src: &Path, dst: &Path) -> Result<(), ExtensionError> {
    tokio::fs::create_dir_all(dst).await?;

    let mut entries = tokio::fs::read_dir(src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let entry_path = entry.path();
        let dest_path = dst.join(entry.file_name());
        let metadata = tokio::fs::symlink_metadata(&entry_path).await?;
        if metadata_is_link_or_reparse(&metadata) {
            return Err(ExtensionError::InvalidSkillPath(
                "Skill copy source changed to contain a link or reparse point".into(),
            ));
        }

        if metadata.is_dir() {
            Box::pin(copy_dir_recursive_unchecked(&entry_path, &dest_path)).await?;
        } else if metadata.is_file() {
            tokio::fs::copy(&entry_path, &dest_path).await?;
        } else {
            return Err(ExtensionError::InvalidSkillPath(
                "Skill copy source contains an unsupported entry".into(),
            ));
        }
    }

    Ok(())
}

/// Try to symlink `src` into `dst`; on failure, fall back to a recursive
/// copy of the source directory.
///
/// Motivation: on Windows machines without "Developer Mode" or admin
/// privileges, `CreateSymbolicLinkW` fails with `os error 1314`
/// (`ERROR_PRIVILEGE_NOT_HELD`). Auto-injected builtin skills under each
/// backend's `.<backend>/skills/` directory then become invisible to the
/// CLI agent — silently degrading the product. Falling back to a copy
/// keeps the skills discoverable; the trade-off is that copies do not
/// track upstream changes until the next link pass clears them. The
/// fallback applies on every platform (Linux/macOS shouldn't normally
/// hit this, but we keep behavior uniform so a future EPERM/EROFS sandbox
/// also stays healthy).
///
/// Logs a `warn!` with the OS error kind and `raw_os_error` so we can
/// keep tracking 1314 vs other failure modes in telemetry. No
/// user-identifying data is logged — only the source/target paths
/// (already considered safe to log elsewhere in this module) and the
/// error code.
async fn link_skill_or_fallback_copy(src: &Path, dst: &Path) -> Result<(), ExtensionError> {
    link_skill_or_fallback_copy_with_mode(src, dst).await.map(|_| ())
}

async fn link_skill_or_fallback_copy_with_mode(
    src: &Path,
    dst: &Path,
) -> Result<SkillProjectionMode, ExtensionError> {
    match create_symlink_for_link(src, dst).await {
        Ok(()) => Ok(existing_link_projection_mode()),
        Err(e) => {
            // Surface the raw OS error so dashboards can keep counting 1314
            // (ERROR_PRIVILEGE_NOT_HELD) separately from other failure modes.
            let raw_os_error = match &e {
                ExtensionError::Io(io_err) => io_err.raw_os_error(),
                _ => None,
            };
            warn!(
                src = %src.display(),
                dst = %dst.display(),
                error = %e,
                raw_os_error = ?raw_os_error,
                "create_symlink failed; falling back to copy_dir_recursive"
            );
            if let Err(error) = copy_dir_recursive(src, dst).await {
                // The destination is operation-owned and was absent before
                // this fallback. Do not leave a partial ordinary directory
                // that a later projection pass would mistake for user data.
                let _ = remove_path_entry(dst).await;
                return Err(error);
            }
            Ok(SkillProjectionMode::Copy)
        }
    }
}

fn metadata_is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Wrapper around [`create_symlink`] that allows tests to inject a
/// synthetic failure. In non-test builds this is a thin call-through to
/// the platform-specific [`create_symlink`] below.
async fn create_symlink_for_link(src: &Path, dst: &Path) -> Result<(), ExtensionError> {
    #[cfg(test)]
    {
        if test_overrides::should_force_symlink_failure() {
            // Use PermissionDenied to mimic the shape Windows returns
            // for ERROR_PRIVILEGE_NOT_HELD. The exact raw_os_error is
            // platform-specific so we only assert on kind in tests.
            return Err(ExtensionError::Io(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "forced symlink failure (test)",
            )));
        }
    }
    create_symlink(src, dst).await
}

/// Test-only knob to force the symlink primitive to fail, exercising
/// the [`copy_dir_recursive`] fallback branch on platforms where
/// symlinking would otherwise succeed (Linux/macOS CI).
#[cfg(test)]
pub(crate) mod test_overrides {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Barrier, Mutex};

    static FORCE_SYMLINK_FAILURE: AtomicBool = AtomicBool::new(false);
    static FAIL_NEXT_PROJECTION_MARKER_WRITE: AtomicBool = AtomicBool::new(false);
    static FAIL_NEXT_PROJECTION_BACKUP_DELETE: AtomicBool = AtomicBool::new(false);
    /// Optional (started, release, scope) triple. `scope` restricts the pause
    /// to extractions whose destination lives under that directory, so
    /// unrelated tests extracting archives in parallel are never parked on
    /// someone else's barriers (which previously deadlocked them: a stray
    /// arrival consumes one slot of a `Barrier::new(2)` and then waits on a
    /// release generation that never completes).
    static EXTRACTION_BARRIERS: Mutex<Option<(Arc<Barrier>, Arc<Barrier>, std::path::PathBuf)>> =
        Mutex::new(None);

    pub fn should_force_symlink_failure() -> bool {
        FORCE_SYMLINK_FAILURE.load(Ordering::SeqCst)
    }

    /// RAII guard that flips `FORCE_SYMLINK_FAILURE` on creation and
    /// resets it on drop. Tests using this guard must be marked
    /// `#[serial_test::serial]` if any other test in the binary also
    /// flips the flag — at present only one test uses it, so a guard
    /// is enough.
    pub struct ForceFailureGuard;

    impl ForceFailureGuard {
        pub fn new() -> Self {
            FORCE_SYMLINK_FAILURE.store(true, Ordering::SeqCst);
            Self
        }
    }

    impl Drop for ForceFailureGuard {
        fn drop(&mut self) {
            FORCE_SYMLINK_FAILURE.store(false, Ordering::SeqCst);
        }
    }

    pub fn fail_next_projection_marker_write() {
        FAIL_NEXT_PROJECTION_MARKER_WRITE.store(true, Ordering::SeqCst);
    }

    pub fn should_fail_next_projection_marker_write() -> bool {
        FAIL_NEXT_PROJECTION_MARKER_WRITE.swap(false, Ordering::SeqCst)
    }

    pub fn fail_next_projection_backup_delete() {
        FAIL_NEXT_PROJECTION_BACKUP_DELETE.store(true, Ordering::SeqCst);
    }

    pub fn should_fail_projection_backup_delete(path: &std::path::Path) -> bool {
        if !path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.contains(".projection-backup-"))
        {
            return false;
        }
        FAIL_NEXT_PROJECTION_BACKUP_DELETE.swap(false, Ordering::SeqCst)
    }

    pub struct ExtractionBarrierGuard;

    pub fn pause_archive_extraction(
        started: Arc<Barrier>,
        release: Arc<Barrier>,
        scope: std::path::PathBuf,
    ) -> ExtractionBarrierGuard {
        *EXTRACTION_BARRIERS.lock().unwrap() = Some((started, release, scope));
        ExtractionBarrierGuard
    }

    pub fn wait_for_archive_extraction(destination: &std::path::Path) {
        let barriers = EXTRACTION_BARRIERS.lock().unwrap().clone();
        if let Some((started, release, scope)) = barriers
            && destination.starts_with(&scope)
        {
            started.wait();
            release.wait();
        }
    }

    impl Drop for ExtractionBarrierGuard {
        fn drop(&mut self) {
            *EXTRACTION_BARRIERS.lock().unwrap() = None;
        }
    }
}

/// Create a symlink (platform-aware).
#[cfg(unix)]
async fn create_symlink(src: &Path, dst: &Path) -> Result<(), ExtensionError> {
    tokio::fs::symlink(src, dst)
        .await
        .map_err(ExtensionError::Io)
}

#[cfg(windows)]
async fn create_symlink(src: &Path, dst: &Path) -> Result<(), ExtensionError> {
    // On Windows, directory symlinks require `SeCreateSymbolicLink`
    // (Developer Mode or Admin), which most users don't have — this is
    // the source of the Sentry I1 family of `os error 1314` failures.
    //
    // NTFS junctions are an unprivileged alternative for *directory*
    // targets: the kernel exposes them via `FSCTL_SET_REPARSE_POINT`
    // which does not require the symlink privilege. Use them whenever
    // possible. File targets cannot be junctioned, so they fall back to
    // `tokio::fs::symlink_file`; in the rare cases that fails the
    // outer `link_skill_or_fallback_copy` wrapper still rescues us via
    // `copy_dir_recursive`.
    if src.is_dir() {
        let src = src.to_path_buf();
        let dst = dst.to_path_buf();
        tokio::task::spawn_blocking(move || junction::create(&src, &dst))
            .await
            .map_err(|e| {
                ExtensionError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("junction::create join error: {e}"),
                ))
            })?
            .map_err(ExtensionError::Io)
    } else {
        tokio::fs::symlink_file(src, dst)
            .await
            .map_err(ExtensionError::Io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn builtin_skills_materialize_version_includes_corpus_fingerprint() {
        let fingerprint = builtin_skills_corpus_fingerprint();
        assert_eq!(fingerprint.len(), 64);
        assert!(fingerprint.chars().all(|ch| ch.is_ascii_hexdigit()));

        let version = builtin_skills_materialize_version("1.2.3");
        assert_eq!(version, format!("1.2.3+skills.{}", &fingerprint[..12]));
    }

    /// Build a `SkillPaths` rooted at a temp dir for self-evolution path/write tests.
    fn test_paths(tmp: &TempDir) -> SkillPaths {
        SkillPaths {
            data_dir: tmp.path().to_path_buf(),
            user_skills_dir: tmp.path().join(SKILLS_DIR_NAME),
            cron_skills_dir: tmp.path().join(CRON_SKILLS_DIR_NAME),
            builtin_skills_dir: tmp.path().join("builtin-skills"),
            builtin_rules_dir: tmp.path().join("rules"),
            preset_rules_dir: tmp.path().join("preset-rules"),
            preset_skills_dir: tmp.path().join("preset-skills"),
            catalog_roots: Default::default(),
        }
    }

    #[test]
    fn skill_dir_for_scopes_and_rejects_traversal() {
        let tmp = TempDir::new().unwrap();
        let paths = test_paths(&tmp);
        let active =
            skill_dir_for(&paths, &SkillScope::Companion("c1".into()), "weekly", false).unwrap();
        assert!(
            active.ends_with("skills/companion/c1/weekly"),
            "{}",
            active.display()
        );
        let draft =
            skill_dir_for(&paths, &SkillScope::Companion("c1".into()), "weekly", true).unwrap();
        assert!(
            draft.ends_with("skills/_drafts/c1/weekly"),
            "{}",
            draft.display()
        );
        let shared = skill_dir_for(&paths, &SkillScope::Shared, "fmt", false).unwrap();
        assert!(
            shared.ends_with("skills/shared/fmt"),
            "{}",
            shared.display()
        );
        assert!(
            skill_dir_for(
                &paths,
                &SkillScope::Companion("../x".into()),
                "weekly",
                false
            )
            .is_err()
        );
        assert!(
            skill_dir_for(
                &paths,
                &SkillScope::Companion("c1".into()),
                "../escape",
                false
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn create_skill_writes_valid_manifest_and_rejects_empty_desc() {
        let tmp = TempDir::new().unwrap();
        let paths = test_paths(&tmp);
        let input = SkillDraftInput {
            name: "weekly-report".into(),
            description: "把本周工作汇总成周报".into(),
            when_to_use: Some("当用户说‘出周报’或周五收尾时".into()),
            allowed_tools: None,
            paths: None,
            body: "## 步骤\n1. 收集本周已完成任务\n2. 按项目归类\n3. 生成 markdown 周报".into(),
        };
        let dir = create_skill(&paths, &SkillScope::Companion("c1".into()), true, &input)
            .await
            .unwrap();
        let manifest = dir.join(SKILL_MANIFEST_FILE);
        assert!(manifest.exists());
        // 能被既有 read_skill_info 正确回读（frontmatter 合法）
        let (name, desc) = read_skill_info(&dir).await.unwrap();
        assert_eq!(name, "weekly-report");
        assert_eq!(desc, "把本周工作汇总成周报");
        // 空 description 必须拒
        let bad = SkillDraftInput {
            description: "".into(),
            ..input.clone()
        };
        assert!(
            create_skill(&paths, &SkillScope::Companion("c1".into()), true, &bad)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn write_skill_overwrites_and_validates() {
        let tmp = TempDir::new().unwrap();
        let paths = test_paths(&tmp);
        let md = "---\nname: fmt\ndescription: 统一代码风格\n---\n\n步骤略\n";
        write_skill(&paths, &SkillScope::Shared, false, "fmt", md)
            .await
            .unwrap();
        let dir = skill_dir_for(&paths, &SkillScope::Shared, "fmt", false).unwrap();
        let (name, desc) = read_skill_info(&dir).await.unwrap();
        assert_eq!(name, "fmt");
        assert_eq!(desc, "统一代码风格");
        // 缺 frontmatter / 空 description → 拒
        assert!(
            write_skill(
                &paths,
                &SkillScope::Shared,
                false,
                "fmt",
                "no frontmatter here"
            )
            .await
            .is_err()
        );
        assert!(
            write_skill(
                &paths,
                &SkillScope::Shared,
                false,
                "fmt",
                "---\nname: fmt\ndescription:\n---\nx"
            )
            .await
            .is_err()
        );
    }

    // -----------------------------------------------------------------------
    // Built-in skill tag seed
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn load_builtin_skill_tags_has_known_entries() {
        let m = super::load_builtin_skill_tags();
        assert!(!m.is_empty());
        let planning = m
            .get("planning-with-files")
            .expect("planning-with-files seeded");
        assert!(planning.1.iter().any(|s| s == "planning"));
    }

    #[tokio::test]
    async fn load_builtin_skill_display_metadata_has_known_entries() {
        let m = super::load_builtin_skill_display_metadata();
        let planning = m
            .get("planning-with-files")
            .expect("planning-with-files display metadata seeded");
        assert_eq!(
            planning.name_i18n.get("zh-CN").map(String::as_str),
            Some("文件化规划")
        );
        assert!(
            planning
                .description_i18n
                .get("zh-CN")
                .is_some_and(|desc| desc.contains("计划"))
        );
    }

    // -----------------------------------------------------------------------
    // Frontmatter parsing
    // -----------------------------------------------------------------------

    #[test]
    fn parse_frontmatter_valid() {
        let content = "---\nname: my-skill\ndescription: A useful skill\n---\nBody content here.";
        let (name, desc) = parse_frontmatter_fields(content).unwrap();
        assert_eq!(name, "my-skill");
        assert_eq!(desc, "A useful skill");
    }

    #[test]
    fn parse_frontmatter_empty_name() {
        let content = "---\nname: \ndescription: Has description\n---\nBody";
        let (name, desc) = parse_frontmatter_fields(content).unwrap();
        assert!(name.is_empty());
        assert_eq!(desc, "Has description");
    }

    #[test]
    fn parse_frontmatter_no_opening() {
        let content = "name: test\ndescription: desc\n---\nbody";
        assert!(parse_frontmatter_fields(content).is_none());
    }

    #[test]
    fn parse_frontmatter_no_closing() {
        let content = "---\nname: test\ndescription: desc";
        assert!(parse_frontmatter_fields(content).is_none());
    }

    #[test]
    fn parse_frontmatter_missing_description() {
        let content = "---\nname: test\n---\nbody";
        assert!(parse_frontmatter_fields(content).is_none());
    }

    #[test]
    fn parse_frontmatter_block_scalar_pipe() {
        let content =
            "---\nname: multiline\ndescription: |\n  First line of the description.\n  Second line of the description.\n---\nBody";
        let (name, desc) = parse_frontmatter_fields(content).unwrap();
        assert_eq!(name, "multiline");
        assert!(desc.contains("First line of the description."));
        assert!(desc.contains("Second line of the description."));
        // Literal block preserves the line break...
        assert!(desc.contains('\n'));
        // ...and the raw indicator must never leak as the value.
        assert_ne!(desc, "|");
    }

    #[test]
    fn parse_frontmatter_block_scalar_folded() {
        let content = "---\nname: folded\ndescription: >\n  first line\n  second line\n---\nBody";
        let (name, desc) = parse_frontmatter_fields(content).unwrap();
        assert_eq!(name, "folded");
        // Folded scalar joins continuation lines with a single space.
        assert_eq!(desc, "first line second line");
        assert_ne!(desc, ">");
    }

    #[test]
    fn parse_frontmatter_value_on_next_line() {
        let content = "---\nname: nextline\ndescription:\n  A description on the following indented line.\n---\nBody";
        let (name, desc) = parse_frontmatter_fields(content).unwrap();
        assert_eq!(name, "nextline");
        assert_eq!(desc, "A description on the following indented line.");
    }

    #[test]
    fn parse_frontmatter_strips_quotes() {
        let content = "---\nname: quoted\ndescription: \"A quoted description.\"\n---\nBody";
        let (_name, desc) = parse_frontmatter_fields(content).unwrap();
        assert_eq!(desc, "A quoted description.");
    }

    // -----------------------------------------------------------------------
    // Filename validation
    // -----------------------------------------------------------------------

    #[test]
    fn validate_filename_normal() {
        assert!(validate_filename("code-review.md").is_ok());
    }

    #[test]
    fn validate_filename_path_traversal() {
        assert!(validate_filename("../etc/passwd").is_err());
        assert!(validate_filename("foo/bar.md").is_err());
        assert!(validate_filename("foo\\bar.md").is_err());
    }

    #[test]
    fn validate_filename_empty() {
        assert!(validate_filename("").is_err());
    }

    /// Every rejected name below survives the `/`, `\`, `..`, empty check set
    /// yet still escapes — or widens — `base.join(name)` on Windows.
    #[test]
    fn validate_filename_rejects_drive_relative_ads_and_self() {
        for bad in ["c:evil", "C:evil", "z:", "payload.exe:x.md", "."] {
            assert!(validate_filename(bad).is_err(), "must reject {bad:?}");
        }
    }

    /// The concrete escape: a drive-prefixed name must never leave the base.
    /// `is_absolute()` is `false` for `c:evil`, which is why a name-level
    /// rejection is the only guard that catches it.
    #[test]
    fn drive_relative_skill_name_cannot_escape_base() {
        let base = Path::new("C:\\dest\\skills");
        let bad = "c:evil";
        assert!(!Path::new(bad).is_absolute(), "premise: reads as relative");
        assert!(
            !base.join(bad).starts_with(base),
            "premise: join escapes the base, so validation must reject it"
        );
        assert!(validate_filename(bad).is_err());
    }

    /// A lone `"."` resolves `join` back to the base, which would aim
    /// `delete_skill`'s recursive delete at the whole skills tree.
    #[test]
    fn dot_skill_name_would_resolve_to_the_base_itself() {
        let base = Path::new("C:\\dest\\skills");
        assert_eq!(base.join("."), Path::new("C:\\dest\\skills\\."));
        assert!(validate_filename(".").is_err());
    }

    #[test]
    fn native_skills_relative_dir_rejects_escape_and_accepts_portable_relative_paths() {
        for path in ["", " ", ".", "..", "../outside", r"..\outside", "/outside", r"C:\outside", "C:outside", ".claude//skills"] {
            assert!(validate_native_skills_relative_dir(path).is_err(), "{path:?}");
        }
        for path in [".claude/skills", r".codex\skills", ".agents/skills"] {
            assert!(validate_native_skills_relative_dir(path).is_ok(), "{path:?}");
        }
    }

    #[test]
    fn validate_builtin_skill_path_rejects_drive_prefix_and_dot_segment() {
        for bad in ["c:evil/SKILL.md", "skills/z:x", "skills/./SKILL.md", "."] {
            assert!(validate_builtin_skill_path(bad).is_err(), "must reject {bad:?}");
        }
        assert!(validate_builtin_skill_path("code-review/SKILL.md").is_ok());
    }

    // -----------------------------------------------------------------------
    // Built-in resource reading
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn read_builtin_rule_existing_file() {
        let tmp = TempDir::new().unwrap();
        let rules_dir = tmp.path().join(BUILTIN_RULES_DIR_NAME);
        std::fs::create_dir_all(&rules_dir).unwrap();
        std::fs::write(rules_dir.join("code-review.md"), "# Review rules").unwrap();

        let paths = SkillPaths {
            data_dir: tmp.path().to_path_buf(),
            user_skills_dir: tmp.path().join(SKILLS_DIR_NAME),
            cron_skills_dir: tmp.path().join(CRON_SKILLS_DIR_NAME),
            builtin_skills_dir: tmp.path().join(crate::constants::BUILTIN_SKILLS_DIR_NAME),
            builtin_rules_dir: rules_dir,
            preset_rules_dir: tmp.path().join(PRESET_RULES_DIR_NAME),
            preset_skills_dir: tmp.path().join(PRESET_SKILLS_DIR_NAME),
            catalog_roots: Default::default(),
        };

        let content = read_builtin_rule(&paths, "code-review.md").await.unwrap();
        assert_eq!(content, "# Review rules");
    }

    #[tokio::test]
    async fn read_builtin_rule_missing_file() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());

        let content = read_builtin_rule(&paths, "nonexistent.md").await.unwrap();
        assert!(content.is_empty());
    }

    #[tokio::test]
    async fn read_builtin_rule_path_traversal() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());

        let result = read_builtin_rule(&paths, "../secret.md").await;
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Skill listing
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn list_skills_builtin_and_custom() {
        let tmp = TempDir::new().unwrap();
        let paths = make_disk_builtin_paths(tmp.path());
        let builtin_dir = disk_builtin_dir(&paths).to_path_buf();

        // Create builtin skills
        create_skill_in_dir(&builtin_dir, "review", "Code review skill");
        create_skill_in_dir(&builtin_dir, "debug", "Debugging skill");

        // Create custom skill (overrides review)
        create_skill_in_dir(&paths.user_skills_dir, "review", "Custom review skill");
        create_skill_in_dir(&paths.user_skills_dir, "my-skill", "My custom skill");

        let skills = list_available_skills(&paths).await.unwrap();

        assert_eq!(skills.len(), 3); // debug + review (custom) + my-skill

        let review = skills.iter().find(|s| s.name == "review").unwrap();
        assert!(review.is_custom);
        assert_eq!(review.description, "Custom review skill");
        assert_eq!(review.source, SkillSource::Custom);

        let debug_skill = skills.iter().find(|s| s.name == "debug").unwrap();
        assert!(!debug_skill.is_custom);
        assert_eq!(debug_skill.source, SkillSource::Builtin);
        assert_eq!(
            debug_skill.relative_location.as_deref(),
            Some("debug/SKILL.md")
        );

        let my_skill = skills.iter().find(|s| s.name == "my-skill").unwrap();
        assert_eq!(my_skill.source, SkillSource::Custom);
        assert!(my_skill.relative_location.is_none());
    }

    #[tokio::test]
    async fn list_skills_empty_dirs() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());

        let skills = list_available_skills(&paths).await.unwrap();
        assert!(skills.is_empty());
    }

    // -----------------------------------------------------------------------
    // Built-in auto skills
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn list_builtin_auto_skills_from_disk_override() {
        let tmp = TempDir::new().unwrap();
        let paths = make_disk_builtin_paths(tmp.path());
        let builtin_dir = disk_builtin_dir(&paths).to_path_buf();
        let auto_dir = builtin_dir.join(BUILTIN_AUTO_SKILLS_SUBDIR);

        create_skill_in_dir(&auto_dir, "cron", "Schedule recurring tasks");
        create_skill_in_dir(&auto_dir, "skill-creator", "Scaffold a new skill");

        // A top-level built-in skill (NOT under auto-inject/) must be excluded.
        create_skill_in_dir(&builtin_dir, "review", "Top-level builtin");

        let autos = list_builtin_auto_skills(&paths).await.unwrap();

        assert_eq!(autos.len(), 2);
        let names: std::collections::HashSet<_> = autos.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains("cron"));
        assert!(names.contains("skill-creator"));
        assert!(!names.contains("review"));

        let cron = autos.iter().find(|s| s.name == "cron").unwrap();
        assert_eq!(cron.description, "Schedule recurring tasks");
        assert_eq!(cron.location, "auto-inject/cron/SKILL.md");
    }

    #[tokio::test]
    async fn list_builtin_auto_skills_missing_dir_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let paths = make_disk_builtin_paths(tmp.path());
        // No auto-inject/ directory created under the disk override.

        let autos = list_builtin_auto_skills(&paths).await.unwrap();
        assert!(autos.is_empty());
    }

    // -----------------------------------------------------------------------
    // Skill info
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn read_skill_info_valid() {
        let tmp = TempDir::new().unwrap();
        let skill_dir = tmp.path().join("my-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join(SKILL_MANIFEST_FILE),
            "---\nname: my-skill\ndescription: A test skill\n---\nBody",
        )
        .unwrap();

        let (name, desc) = read_skill_info(&skill_dir).await.unwrap();
        assert_eq!(name, "my-skill");
        assert_eq!(desc, "A test skill");
    }

    #[tokio::test]
    async fn read_skill_info_missing() {
        let tmp = TempDir::new().unwrap();
        let result = read_skill_info(&tmp.path().join("nonexistent")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn validate_market_skill_manifest_returns_declared_name() {
        let tmp = TempDir::new().unwrap();
        let skill_dir = tmp.path().join("baozheng");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join(SKILL_MANIFEST_FILE),
            "---\nname: baozheng-skills\ndescription: Legal assistant\n---\nBody",
        )
        .unwrap();

        // The declared manifest name is authoritative for the local Skill
        // identity; it does not have to match the directory/URL slug.
        let name = validate_market_skill_manifest(&skill_dir).await.unwrap();
        assert_eq!(name, "baozheng-skills");
    }

    #[tokio::test]
    async fn validate_market_skill_manifest_rejects_missing_frontmatter_or_description() {
        let tmp = TempDir::new().unwrap();
        let skill_dir = tmp.path().join("broken");
        std::fs::create_dir_all(&skill_dir).unwrap();

        std::fs::write(skill_dir.join(SKILL_MANIFEST_FILE), "no frontmatter at all").unwrap();
        assert!(validate_market_skill_manifest(&skill_dir).await.is_err());

        std::fs::write(skill_dir.join(SKILL_MANIFEST_FILE), "---\nname: broken\n---\nBody").unwrap();
        assert!(validate_market_skill_manifest(&skill_dir).await.is_err());

        std::fs::write(
            skill_dir.join(SKILL_MANIFEST_FILE),
            "---\ndescription: no name here\n---\nBody",
        )
        .unwrap();
        assert!(validate_market_skill_manifest(&skill_dir).await.is_err());
    }

    #[tokio::test]
    async fn validate_market_skill_manifest_rejects_missing_manifest() {
        let tmp = TempDir::new().unwrap();
        let skill_dir = tmp.path().join("empty-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        assert!(validate_market_skill_manifest(&skill_dir).await.is_err());
    }

    // -----------------------------------------------------------------------
    // Skill import / delete
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn import_skill_copies_directory() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());

        // Create source skill
        let source_dir = tmp.path().join("source-skill");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::write(
            source_dir.join(SKILL_MANIFEST_FILE),
            "---\nname: imported\ndescription: Imported skill\n---\nBody",
        )
        .unwrap();
        std::fs::write(source_dir.join("extra.txt"), "extra data").unwrap();

        let name = import_skill(&paths, &source_dir).await.unwrap();
        assert_eq!(name, "imported");

        // Verify the skill was copied
        let imported_dir = paths.user_skills_dir.join("imported");
        assert!(imported_dir.join(SKILL_MANIFEST_FILE).exists());
        assert!(imported_dir.join("extra.txt").exists());
    }

    #[tokio::test]
    async fn local_import_and_delete_clear_market_provenance() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());
        let source_dir = tmp.path().join("source-skill");

        let installations = paths.data_dir.join("skill-market/installations");
        tokio::fs::create_dir_all(&installations).await.unwrap();
        let record_path = installations.join("lifecycle.json");
        let record = serde_json::json!({
            "schema_version": 1,
            "skill_name": "lifecycle",
            "source": "skillhub",
            "source_id": "skillhub:owner/skills/lifecycle",
            "revision": "r1",
            "artifact_sha256": "a".repeat(64),
            "content_sha256": "b".repeat(64),
            "installed_at": 1
        });
        tokio::fs::write(&record_path, serde_json::to_vec(&record).unwrap())
            .await
            .unwrap();

        tokio::fs::create_dir_all(&source_dir).await.unwrap();
        tokio::fs::write(
            source_dir.join(SKILL_MANIFEST_FILE),
            "---\nname: lifecycle\ndescription: Local override\n---\nBody",
        )
        .await
        .unwrap();
        assert_eq!(import_skill(&paths, &source_dir).await.unwrap(), "lifecycle");
        assert!(!record_path.exists(), "local import must clear market provenance");

        tokio::fs::create_dir_all(&installations).await.unwrap();
        tokio::fs::write(&record_path, serde_json::to_vec(&record).unwrap())
            .await
            .unwrap();
        delete_skill(&paths, "lifecycle").await.unwrap();
        assert!(!paths.user_skills_dir.join("lifecycle").exists());
        assert!(!record_path.exists(), "deleting a Skill must clear market provenance");
    }

    #[tokio::test]
    async fn deleting_missing_skill_repairs_stale_market_provenance() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());
        let installations = paths.data_dir.join("skill-market/installations");
        tokio::fs::create_dir_all(&installations).await.unwrap();
        let record_path = installations.join("orphan.json");
        tokio::fs::write(
            &record_path,
            serde_json::json!({
                "schema_version": 1,
                "skill_name": "orphan",
                "source": "skillhub",
                "source_id": "skillhub:owner/skills/orphan",
                "artifact_sha256": "a".repeat(64),
                "content_sha256": "b".repeat(64),
                "installed_at": 1
            })
            .to_string(),
        )
        .await
        .unwrap();

        assert!(matches!(
            delete_skill(&paths, "orphan").await,
            Err(ExtensionError::SkillNotFound(name)) if name == "orphan"
        ));
        assert!(!record_path.exists());
    }

    #[tokio::test]
    async fn delete_preserves_skill_when_market_provenance_root_is_unsafe() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());
        create_skill_in_dir(&paths.user_skills_dir, "protected", "Keep me");
        tokio::fs::write(paths.data_dir.join("skill-market"), b"not a directory")
            .await
            .unwrap();

        assert!(delete_skill(&paths, "protected").await.is_err());
        assert!(paths.user_skills_dir.join("protected/SKILL.md").exists());
    }

    #[tokio::test]
    #[serial]
    async fn import_skill_with_symlink_creates_link() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());

        let source_dir = tmp.path().join("link-skill");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::write(
            source_dir.join(SKILL_MANIFEST_FILE),
            "---\nname: linked\ndescription: Linked skill\n---\nBody",
        )
        .unwrap();

        let name = import_skill_with_symlink(&paths, &source_dir)
            .await
            .unwrap();
        assert_eq!(name, "linked");

        let link_path = paths.user_skills_dir.join("linked");
        assert!(link_path.is_symlink());
    }

    #[tokio::test]
    #[serial]
    async fn import_skills_with_symlink_imports_selected_skill_manifest_parent() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());

        let source_dir = tmp.path().join("single-skill");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::write(
            source_dir.join(SKILL_MANIFEST_FILE),
            "---\nname: selected-manifest\ndescription: Selected manifest skill\n---\nBody",
        )
        .unwrap();

        let names = import_skills_with_symlink(&paths, &source_dir.join(SKILL_MANIFEST_FILE))
            .await
            .unwrap();
        assert_eq!(names, vec!["selected-manifest"]);

        let link_path = paths.user_skills_dir.join("selected-manifest");
        assert!(link_path.is_symlink());
        assert_eq!(std::fs::read_link(&link_path).unwrap(), source_dir);
        assert!(link_path.join(SKILL_MANIFEST_FILE).exists());
    }

    #[tokio::test]
    #[serial]
    async fn import_skills_with_symlink_imports_parent_directory_children() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());

        let source_dir = tmp.path().join("skill-pack");
        create_skill_in_dir(&source_dir, "alpha", "Alpha skill");
        create_skill_in_dir(&source_dir, "beta", "Beta skill");

        let names = import_skills_with_symlink(&paths, &source_dir)
            .await
            .unwrap();
        assert_eq!(names, vec!["alpha", "beta"]);
        assert!(paths.user_skills_dir.join("alpha").is_symlink());
        assert!(paths.user_skills_dir.join("beta").is_symlink());
    }

    #[tokio::test]
    async fn import_skills_with_symlink_imports_nested_bundle() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());

        // Skills nested two levels deep: pack/category/<skill>/SKILL.md.
        let nested = tmp.path().join("pack").join("category");
        create_skill_in_dir(&nested, "deep-alpha", "Deep alpha skill");
        create_skill_in_dir(&nested, "deep-beta", "Deep beta skill");

        // Picking the grandparent "pack" only works with recursive scanning —
        // neither it nor its immediate child contains a SKILL.md.
        let names = import_skills_with_symlink(&paths, &tmp.path().join("pack"))
            .await
            .unwrap();
        assert_eq!(names, vec!["deep-alpha", "deep-beta"]);
        assert!(paths.user_skills_dir.join("deep-alpha").exists());
        assert!(paths.user_skills_dir.join("deep-beta").exists());
    }

    #[tokio::test]
    async fn import_skills_with_symlink_best_effort_skips_bad_skill() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());

        let pack = tmp.path().join("mixed-pack");
        create_skill_in_dir(&pack, "good-skill", "A valid skill");
        // Malformed sibling: frontmatter present but no description -> rejected.
        let bad_dir = pack.join("bad-skill");
        std::fs::create_dir_all(&bad_dir).unwrap();
        std::fs::write(
            bad_dir.join(SKILL_MANIFEST_FILE),
            "---\nname: bad-skill\n---\nBody",
        )
        .unwrap();

        // The bad skill is skipped; the good one still imports (non-atomic,
        // best-effort) instead of aborting the whole request.
        let names = import_skills_with_symlink(&paths, &pack).await.unwrap();
        assert_eq!(names, vec!["good-skill"]);
        assert!(paths.user_skills_dir.join("good-skill").exists());
        assert!(!paths.user_skills_dir.join("bad-skill").exists());
    }

    // NOTE: the import->copy fallback (import_skill_with_symlink now routing
    // through link_skill_or_fallback_copy) is covered by
    // `link_workspace_skills_falls_back_to_copy_when_symlink_fails`; a second
    // test here would race on the global FORCE_SYMLINK_FAILURE flag under
    // `cargo test` (nextest isolates per-process), so it is intentionally omitted.

    #[tokio::test]
    async fn list_available_skills_orders_custom_skills_by_newest_import_first() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());

        let older_dir = tmp.path().join("older-source");
        let newer_dir = tmp.path().join("newer-source");
        std::fs::create_dir_all(&older_dir).unwrap();
        std::fs::create_dir_all(&newer_dir).unwrap();
        std::fs::write(
            older_dir.join(SKILL_MANIFEST_FILE),
            "---\nname: older-skill\ndescription: Older skill\n---\nBody",
        )
        .unwrap();
        std::fs::write(
            newer_dir.join(SKILL_MANIFEST_FILE),
            "---\nname: newer-skill\ndescription: Newer skill\n---\nBody",
        )
        .unwrap();

        import_skill_with_symlink(&paths, &older_dir).await.unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        import_skill_with_symlink(&paths, &newer_dir).await.unwrap();

        let skills = list_available_skills(&paths).await.unwrap();
        let names: Vec<_> = skills.into_iter().map(|skill| skill.name).collect();
        assert_eq!(names[0], "newer-skill");
        assert_eq!(names[1], "older-skill");
    }

    #[tokio::test]
    async fn import_skills_with_symlink_imports_zip_package() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());
        let zip_path = tmp.path().join("skills.zip");

        write_test_zip(
            &zip_path,
            &[
                (
                    "bundle/zip-one/SKILL.md",
                    "---\nname: zip-one\ndescription: First zipped skill\n---\nBody",
                ),
                ("bundle/zip-one/data.txt", "payload"),
                (
                    "bundle/zip-two/SKILL.md",
                    "---\nname: zip-two\ndescription: Second zipped skill\n---\nBody",
                ),
            ],
        );

        let names = import_skills_with_symlink(&paths, &zip_path).await.unwrap();
        assert_eq!(names, vec!["zip-one", "zip-two"]);
        assert!(
            paths
                .user_skills_dir
                .join("zip-one")
                .join(SKILL_MANIFEST_FILE)
                .exists()
        );
        assert!(
            paths
                .user_skills_dir
                .join("zip-one")
                .join("data.txt")
                .exists()
        );
        assert!(!paths.user_skills_dir.join("zip-one").is_symlink());
        assert!(
            !paths
                .user_skills_dir
                .join(".import-tmp")
                .join("skills.zip")
                .exists()
        );
        assert!(!paths.user_skills_dir.join(".import-tmp").exists());
    }

    #[tokio::test]
    async fn zip_import_rejects_a_non_directory_staging_parent() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());
        tokio::fs::create_dir_all(&paths.user_skills_dir).await.unwrap();
        tokio::fs::write(paths.user_skills_dir.join(".import-tmp"), b"user file")
            .await
            .unwrap();

        let error = import_skills_with_symlink(&paths, &tmp.path().join("missing.zip"))
            .await
            .unwrap_err();
        assert!(matches!(error, ExtensionError::InvalidSkillPath(_)));
        assert_eq!(
            tokio::fs::read_to_string(paths.user_skills_dir.join(".import-tmp"))
                .await
                .unwrap(),
            "user file"
        );
    }

    #[tokio::test]
    #[serial]
    async fn zip_import_replaces_existing_link_without_mutating_source() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());
        let source_dir = tmp.path().join("external-skill");
        let zip_path = tmp.path().join("replacement.zip");

        create_skill_in_dir(tmp.path(), "external-skill", "External source skill");
        std::fs::write(source_dir.join("source-only.txt"), "original").unwrap();
        import_skill_with_symlink(&paths, &source_dir).await.unwrap();

        write_test_zip(
            &zip_path,
            &[
                (
                    "bundle/external-skill/SKILL.md",
                    "---\nname: external-skill\ndescription: Replacement skill\n---\nReplacement body",
                ),
                ("bundle/external-skill/replacement-only.txt", "replacement"),
            ],
        );

        let names = import_skills_with_symlink(&paths, &zip_path).await.unwrap();

        assert_eq!(names, vec!["external-skill"]);
        let managed_skill = paths.user_skills_dir.join("external-skill");
        assert!(!std::fs::symlink_metadata(&managed_skill)
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(managed_skill.join("replacement-only.txt").exists());
        assert!(!source_dir.join("replacement-only.txt").exists());
        assert_eq!(std::fs::read_to_string(source_dir.join("source-only.txt")).unwrap(), "original");
        assert!(std::fs::read_to_string(source_dir.join(SKILL_MANIFEST_FILE))
            .unwrap()
            .contains("External source skill"));
    }

    #[tokio::test]
    async fn import_skills_with_symlink_rejects_zip_slip_entries() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());
        let zip_path = tmp.path().join("evil.zip");

        write_test_zip(&zip_path, &[("../escape.txt", "outside")]);

        let result = import_skills_with_symlink(&paths, &zip_path).await;
        assert!(matches!(result, Err(ExtensionError::PathTraversal(_))));
        assert!(!tmp.path().join("escape.txt").exists());
    }

    #[tokio::test]
    async fn import_skill_rejects_traversal_name() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());

        // Create a skill whose frontmatter name contains path traversal
        let source_dir = tmp.path().join("evil-skill");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::write(
            source_dir.join(SKILL_MANIFEST_FILE),
            "---\nname: ../../../etc/evil\ndescription: Malicious skill\n---\nBody",
        )
        .unwrap();

        let result = import_skill(&paths, &source_dir).await;
        assert!(matches!(result, Err(ExtensionError::PathTraversal(_))));
    }

    #[tokio::test]
    async fn import_skill_with_symlink_rejects_traversal_name() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());

        let source_dir = tmp.path().join("evil-skill");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::write(
            source_dir.join(SKILL_MANIFEST_FILE),
            "---\nname: ../../escape\ndescription: Malicious skill\n---\nBody",
        )
        .unwrap();

        let result = import_skill_with_symlink(&paths, &source_dir).await;
        assert!(matches!(result, Err(ExtensionError::PathTraversal(_))));
    }

    #[tokio::test]
    async fn delete_custom_skill() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());

        create_skill_in_dir(&paths.user_skills_dir, "to-delete", "Will be deleted");

        delete_skill(&paths, "to-delete").await.unwrap();
        assert!(!paths.user_skills_dir.join("to-delete").exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn delete_skill_removes_dangling_user_symlink() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());
        let source_dir = tmp.path().join("external-skill");

        create_skill_in_dir(tmp.path(), "external-skill", "External skill");
        import_skill_with_symlink(&paths, &source_dir).await.unwrap();
        std::fs::remove_dir_all(&source_dir).unwrap();

        delete_skill(&paths, "external-skill").await.unwrap();

        let link_path = paths.user_skills_dir.join("external-skill");
        assert!(matches!(
            std::fs::symlink_metadata(link_path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound
        ));
    }

    #[cfg(windows)]
    #[tokio::test]
    #[serial]
    async fn delete_skill_removes_imported_junction_without_deleting_source() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());
        let source_dir = tmp.path().join("external-skill");

        create_skill_in_dir(tmp.path(), "external-skill", "External skill");
        import_skill_with_symlink(&paths, &source_dir).await.unwrap();

        let link_path = paths.user_skills_dir.join("external-skill");
        assert!(std::fs::symlink_metadata(&link_path)
            .unwrap()
            .file_type()
            .is_symlink());

        delete_skill(&paths, "external-skill").await.unwrap();

        assert!(matches!(
            std::fs::symlink_metadata(&link_path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound
        ));
        assert!(source_dir.join(SKILL_MANIFEST_FILE).exists());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn delete_custom_skill_with_readonly_files_on_windows() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());

        create_skill_in_dir(&paths.user_skills_dir, "readonly-skill", "Will be deleted");
        let skill_file = paths
            .user_skills_dir
            .join("readonly-skill")
            .join("SKILL.md");
        let mut permissions = std::fs::metadata(&skill_file).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&skill_file, permissions).unwrap();

        delete_skill(&paths, "readonly-skill").await.unwrap();
        assert!(!paths.user_skills_dir.join("readonly-skill").exists());
    }

    #[cfg(unix)]
    #[test]
    fn clear_readonly_for_deletion_preserves_unix_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("readonly.txt");
        std::fs::write(&file, "payload").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o444)).unwrap();

        clear_readonly_for_deletion(&file).unwrap();

        assert_eq!(std::fs::metadata(file).unwrap().permissions().mode() & 0o777, 0o444);
    }

    #[tokio::test]
    async fn delete_builtin_skill_rejected() {
        let tmp = TempDir::new().unwrap();
        let paths = make_disk_builtin_paths(tmp.path());
        let builtin_dir = disk_builtin_dir(&paths).to_path_buf();

        create_skill_in_dir(&builtin_dir, "protected", "Built-in skill");

        let result = delete_skill(&paths, "protected").await;
        assert!(matches!(
            result,
            Err(ExtensionError::BuiltinSkillDeletion(_))
        ));
    }

    #[tokio::test]
    async fn delete_nonexistent_skill() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());

        let result = delete_skill(&paths, "ghost").await;
        assert!(matches!(result, Err(ExtensionError::SkillNotFound(_))));
    }

    #[tokio::test]
    async fn delete_skill_path_traversal() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());

        let result = delete_skill(&paths, "../etc").await;
        assert!(matches!(result, Err(ExtensionError::PathTraversal(_))));
    }

    // -----------------------------------------------------------------------
    // Scanning
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn scan_for_skills_finds_valid() {
        let tmp = TempDir::new().unwrap();
        create_skill_in_dir(tmp.path(), "skill-a", "First skill");
        create_skill_in_dir(tmp.path(), "skill-b", "Second skill");

        // Create a dir without SKILL.md (should be ignored)
        std::fs::create_dir_all(tmp.path().join("not-a-skill")).unwrap();

        let skills = scan_for_skills(tmp.path()).await.unwrap();
        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0].name, "skill-a");
        assert_eq!(skills[1].name, "skill-b");
    }

    #[tokio::test]
    async fn scan_for_skills_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let skills = scan_for_skills(tmp.path()).await.unwrap();
        assert!(skills.is_empty());
    }

    #[tokio::test]
    async fn scan_for_skills_nonexistent_dir() {
        let skills = scan_for_skills(Path::new("/nonexistent/path"))
            .await
            .unwrap();
        assert!(skills.is_empty());
    }

    // -----------------------------------------------------------------------
    // Export
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn export_skill_creates_symlink() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("my-skill");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::write(
            source_dir.join(SKILL_MANIFEST_FILE),
            "---\nname: my-skill\ndescription: Test\n---\nBody",
        )
        .unwrap();

        let target_dir = tmp.path().join("exports");
        export_skill_with_symlink(&source_dir, &target_dir)
            .await
            .unwrap();

        let link = target_dir.join("my-skill");
        assert!(link.is_symlink());
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn make_test_paths(base: &Path) -> SkillPaths {
        // Hand out an empty on-disk builtin-skills dir. Tests that need
        // specific fixtures seed it via `create_skill_in_dir`; tests
        // that want the full real corpus use `make_embedded_paths`.
        SkillPaths {
            data_dir: base.to_path_buf(),
            user_skills_dir: base.join(SKILLS_DIR_NAME),
            cron_skills_dir: base.join(CRON_SKILLS_DIR_NAME),
            builtin_skills_dir: base.join(crate::constants::BUILTIN_SKILLS_DIR_NAME),
            builtin_rules_dir: base.join(BUILTIN_RULES_DIR_NAME),
            preset_rules_dir: base.join(PRESET_RULES_DIR_NAME),
            preset_skills_dir: base.join(PRESET_SKILLS_DIR_NAME),
            catalog_roots: Default::default(),
        }
    }

    /// Return `SkillPaths` pre-populated with the real embedded builtin
    /// skills corpus materialized to disk. Use this for tests that
    /// previously relied on the embedded-corpus fallback.
    async fn make_embedded_paths(base: &Path) -> SkillPaths {
        crate::startup_materialize::materialize_embedded_builtin_skills(
            base,
            &BUILTIN_SKILLS,
            "test-version",
        )
        .await
        .expect("failed to materialize embedded corpus for test");
        make_test_paths(base)
    }

    /// Return a `SkillPaths` rooted at `base` with an on-disk
    /// `builtin_skills_dir`, so tests can seed fixtures in that dir.
    fn make_disk_builtin_paths(base: &Path) -> SkillPaths {
        make_test_paths(base)
    }

    fn disk_builtin_dir(paths: &SkillPaths) -> &Path {
        &paths.builtin_skills_dir
    }

    fn create_skill_in_dir(base: &Path, name: &str, description: &str) {
        let dir = base.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(SKILL_MANIFEST_FILE),
            format!("---\nname: {name}\ndescription: {description}\n---\nBody content for {name}."),
        )
        .unwrap();
    }

    fn write_test_zip(path: &Path, entries: &[(&str, &str)]) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();

        for (name, content) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        }

        zip.finish().unwrap();
    }

    // -----------------------------------------------------------------------
    // Embedded corpus
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn embedded_lists_auto_inject_from_corpus() {
        let tmp = TempDir::new().unwrap();
        let paths = make_embedded_paths(tmp.path()).await;

        let autos = list_builtin_auto_skills(&paths).await.unwrap();
        assert!(
            autos.len() >= 3,
            "expected ≥3 auto-inject entries, got {}",
            autos.len()
        );
        assert!(
            !autos.iter().any(|item| item.name == "officecli"),
            "officecli must remain an opt-in builtin skill"
        );
        for item in &autos {
            assert!(
                item.location.starts_with("auto-inject/"),
                "location must start with auto-inject/, got {}",
                item.location
            );
            assert!(item.location.ends_with("/SKILL.md"));
            assert!(!item.description.is_empty());
        }
    }

    #[tokio::test]
    async fn embedded_reads_builtin_skill_content() {
        let tmp = TempDir::new().unwrap();
        let paths = make_embedded_paths(tmp.path()).await;

        let content = read_builtin_skill(&paths, "auto-inject/cron/SKILL.md")
            .await
            .unwrap();
        assert!(!content.is_empty(), "embedded cron SKILL.md is empty");
        assert!(
            content.trim_start().starts_with("---"),
            "expected frontmatter, got: {}",
            content.chars().take(80).collect::<String>()
        );
    }

    #[tokio::test]
    async fn embedded_rejects_path_traversal() {
        let tmp = TempDir::new().unwrap();
        let paths = make_embedded_paths(tmp.path()).await;

        let result = read_builtin_skill(&paths, "../etc/passwd").await;
        assert!(matches!(result, Err(ExtensionError::PathTraversal(_))));

        let result = read_builtin_skill(&paths, "auto-inject/../../secret").await;
        assert!(matches!(result, Err(ExtensionError::PathTraversal(_))));
    }

    #[tokio::test]
    async fn embedded_handles_missing_file() {
        let tmp = TempDir::new().unwrap();
        let paths = make_embedded_paths(tmp.path()).await;

        let content = read_builtin_skill(&paths, "nonexistent/SKILL.md")
            .await
            .unwrap();
        assert!(content.is_empty());
    }

    #[tokio::test]
    async fn disk_override_reads_from_disk_not_embedded() {
        let tmp = TempDir::new().unwrap();
        let paths = make_disk_builtin_paths(tmp.path());
        let builtin_dir = disk_builtin_dir(&paths).to_path_buf();
        let auto_dir = builtin_dir.join(BUILTIN_AUTO_SKILLS_SUBDIR);
        create_skill_in_dir(&auto_dir, "fixture-only", "Fixture-only skill");

        let autos = list_builtin_auto_skills(&paths).await.unwrap();
        let names: Vec<&str> = autos.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"fixture-only"),
            "disk override should reflect seeded skill; got {names:?}"
        );
        // Embedded skills (e.g. `cron`) must NOT leak into the disk view.
        assert!(
            !names.contains(&"cron"),
            "disk override must not include embedded skills"
        );
    }

    #[tokio::test]
    async fn list_skills_builtin_has_relative_location_from_embedded() {
        let tmp = TempDir::new().unwrap();
        let paths = make_embedded_paths(tmp.path()).await;

        let skills = list_available_skills(&paths).await.unwrap();
        let builtins: Vec<_> = skills
            .iter()
            .filter(|s| s.source == SkillSource::Builtin)
            .collect();
        assert!(!builtins.is_empty(), "no builtin skills listed");
        for s in &builtins {
            let rel = s
                .relative_location
                .as_deref()
                .expect("builtin must have relative_location");
            assert!(
                rel.ends_with("/SKILL.md"),
                "relative_location must end in /SKILL.md, got {rel}"
            );
            assert!(
                s.location
                    .contains(crate::constants::BUILTIN_SKILLS_DIR_NAME),
                "builtin location must live under the view dir, got {}",
                s.location
            );
            // Lazy materialization wrote SKILL.md to disk.
            assert!(
                std::path::Path::new(&s.location).exists(),
                "materialized view missing: {}",
                s.location
            );
        }

        let officecli = builtins
            .iter()
            .find(|skill| skill.name == "officecli")
            .expect("officecli builtin skill listed");
        assert_eq!(
            officecli.relative_location.as_deref(),
            Some("officecli/SKILL.md")
        );
    }

    // -----------------------------------------------------------------------
    // Materialize (symlink contract)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn materialize_empty_list_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let paths = make_embedded_paths(tmp.path()).await;

        let list = materialize_skills_for_agent(&paths, "conv-empty", &[])
            .await
            .unwrap();
        assert!(list.is_empty());
        // No per-conversation dir should be created.
        assert!(!paths.data_dir.join("agent-skills").exists());
        assert!(!paths.data_dir.join("conversations").exists());
    }

    #[tokio::test]
    async fn materialize_resolves_auto_inject_skill_by_name() {
        // Auto-inject skills are resolved only when the caller names
        // them explicitly (see `ConversationService::create` snapshot).
        let tmp = TempDir::new().unwrap();
        let paths = make_embedded_paths(tmp.path()).await;

        let resolved = materialize_skills_for_agent(&paths, "conv-named", &["cron".to_owned()])
            .await
            .unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].name, "cron");
        // source_path points at the real on-disk auto-inject directory.
        let expected = paths
            .builtin_skills_dir
            .join(BUILTIN_AUTO_SKILLS_SUBDIR)
            .join("cron");
        assert_eq!(resolved[0].source_path, expected);
        assert!(resolved[0].source_path.is_dir());
        assert!(resolved[0].source_path.join(SKILL_MANIFEST_FILE).exists());
    }

    #[tokio::test]
    async fn materialize_resolves_opt_in_top_level_skill() {
        let tmp = TempDir::new().unwrap();
        let paths = make_embedded_paths(tmp.path()).await;

        let resolved = materialize_skills_for_agent(
            &paths,
            "conv-opt",
            &["planning-with-files".to_owned()],
        )
        .await
        .unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].name, "planning-with-files");
        let expected = paths.builtin_skills_dir.join("planning-with-files");
        assert_eq!(resolved[0].source_path, expected);
    }

    #[tokio::test]
    async fn materialize_resolves_user_skill() {
        let tmp = TempDir::new().unwrap();
        let paths = make_embedded_paths(tmp.path()).await;
        create_skill_in_dir(&paths.user_skills_dir, "my-custom", "A user skill");

        let resolved = materialize_skills_for_agent(&paths, "conv-user", &["my-custom".to_owned()])
            .await
            .unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved[0].source_path,
            paths.user_skills_dir.join("my-custom")
        );
    }

    #[tokio::test]
    async fn materialize_user_skill_overrides_same_name_builtin() {
        let tmp = TempDir::new().unwrap();
        let paths = make_embedded_paths(tmp.path()).await;
        create_skill_in_dir(
            &paths.user_skills_dir,
            "planning-with-files",
            "User override",
        );

        let resolved = materialize_skills_for_agent(
            &paths,
            "conv-user-override",
            &["planning-with-files".to_owned()],
        )
        .await
        .unwrap();

        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved[0].source_path,
            paths.user_skills_dir.join("planning-with-files")
        );
    }

    #[tokio::test]
    async fn materialize_silently_skips_unknown_skill() {
        let tmp = TempDir::new().unwrap();
        let paths = make_embedded_paths(tmp.path()).await;

        let resolved =
            materialize_skills_for_agent(&paths, "conv-missing", &["no-such-skill".to_owned()])
                .await
                .unwrap();
        assert!(resolved.is_empty());
    }

    #[tokio::test]
    async fn materialize_skips_invalid_names_but_keeps_valid_ones() {
        let tmp = TempDir::new().unwrap();
        let paths = make_embedded_paths(tmp.path()).await;

        let resolved = materialize_skills_for_agent(
            &paths,
            "conv-mixed",
            &[
                "".to_owned(),
                "../evil".to_owned(),
                "foo/bar".to_owned(),
                "cron".to_owned(),
            ],
        )
        .await
        .unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].name, "cron");
    }

    #[tokio::test]
    async fn materialize_returns_sorted_list_with_source_paths() {
        // Deterministic ordering — callers rely on it for stable symlink
        // layouts and for easier debugging / snapshot tests.
        let tmp = TempDir::new().unwrap();
        let paths = make_embedded_paths(tmp.path()).await;

        let resolved = materialize_skills_for_agent(
            &paths,
            "conv-sorted",
            &["planning-with-files".to_owned(), "cron".to_owned()],
        )
        .await
        .unwrap();
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].name, "cron");
        assert_eq!(resolved[1].name, "planning-with-files");
        for entry in &resolved {
            assert!(entry.source_path.is_absolute());
            assert!(entry.source_path.is_dir());
        }
    }

    #[tokio::test]
    async fn materialize_rejects_bad_conversation_id() {
        let tmp = TempDir::new().unwrap();
        let paths = make_embedded_paths(tmp.path()).await;

        let err = materialize_skills_for_agent(&paths, "../evil", &[])
            .await
            .unwrap_err();
        assert!(matches!(err, ExtensionError::PathTraversal(_)));
    }

    #[tokio::test]
    async fn materialize_does_not_touch_disk_beyond_reads() {
        // Guardrail: the symlink contract forbids any per-conversation
        // directory on disk. Verify the function only reads the sources
        // and never writes.
        let tmp = TempDir::new().unwrap();
        let paths = make_embedded_paths(tmp.path()).await;

        let _ = materialize_skills_for_agent(&paths, "conv-pure", &["cron".to_owned()])
            .await
            .unwrap();
        assert!(!paths.data_dir.join("agent-skills").exists());
        assert!(!paths.data_dir.join("conversations").exists());
    }

    // -----------------------------------------------------------------------
    // Windows symlink → copy_dir_recursive fallback
    // -----------------------------------------------------------------------

    /// When the platform symlink primitive fails (mirrors Windows
    /// `os error 1314 ERROR_PRIVILEGE_NOT_HELD`), `link_workspace_skills`
    /// must materialize the skill via `copy_dir_recursive` instead so the
    /// CLI agent can still discover it. Forced via `ForceFailureGuard`
    /// on Linux/macOS CI where symlinking would otherwise succeed.
    #[tokio::test]
    #[serial]
    async fn link_workspace_skills_falls_back_to_copy_when_symlink_fails() {
        let tmp = TempDir::new().unwrap();
        let workspace = tmp.path().join("workspace");
        let source_root = tmp.path().join("sources");

        // Seed a fake skill source directory with a SKILL.md and a
        // nested file so we can verify the copy is recursive.
        let skill_source = source_root.join("my-skill");
        std::fs::create_dir_all(skill_source.join("nested")).unwrap();
        std::fs::write(
            skill_source.join(SKILL_MANIFEST_FILE),
            "---\nname: my-skill\ndescription: test\n---\nbody",
        )
        .unwrap();
        std::fs::write(skill_source.join("nested").join("data.txt"), "payload").unwrap();

        let resolved = vec![ResolvedAgentSkill {
            name: "my-skill".to_owned(),
            source_path: skill_source.clone(),
        }];

        // Force the symlink primitive to fail for the duration of this
        // test, exercising the copy fallback branch.
        let _guard = test_overrides::ForceFailureGuard::new();

        let created = link_workspace_skills(&test_paths(&tmp), &workspace, &[".claude/skills"], &resolved)
            .await
            .expect("link_workspace_skills should succeed via copy fallback");
        assert_eq!(created.created, 1, "exactly one skill should be materialized");

        let target = workspace.join(".claude/skills").join("my-skill");
        assert!(target.exists(), "target directory must exist");
        // It must NOT be a symlink — fallback path uses copy_dir_recursive.
        let meta = tokio::fs::symlink_metadata(&target).await.unwrap();
        assert!(
            !meta.file_type().is_symlink(),
            "fallback must produce a real directory, not a symlink"
        );
        assert!(target.is_dir(), "target must be a directory");

        // Verify the contents were copied recursively.
        let manifest = std::fs::read_to_string(target.join(SKILL_MANIFEST_FILE)).unwrap();
        assert!(manifest.contains("name: my-skill"));
        let nested = std::fs::read_to_string(target.join("nested").join("data.txt")).unwrap();
        assert_eq!(nested, "payload");
    }

    #[cfg(unix)]
    #[tokio::test]
    #[serial]
    async fn link_workspace_skills_rejects_nested_symlink_in_copy_fallback() {
        use std::os::unix::fs::symlink;

        let tmp = TempDir::new().unwrap();
        let paths = test_paths(&tmp);
        let workspace = tmp.path().join("workspace");
        let source_root = tmp.path().join("sources");
        let outside = tmp.path().join("outside.txt");
        let skill_source = source_root.join("linked-skill");
        std::fs::create_dir_all(&skill_source).unwrap();
        std::fs::write(&outside, "must not be copied").unwrap();
        std::fs::write(
            skill_source.join(SKILL_MANIFEST_FILE),
            "---\nname: linked-skill\ndescription: test\n---\nbody",
        )
        .unwrap();
        symlink(&outside, skill_source.join("escape.txt")).unwrap();

        let resolved = vec![ResolvedAgentSkill {
            name: "linked-skill".to_owned(),
            source_path: skill_source,
        }];
        let _guard = test_overrides::ForceFailureGuard::new();
        let error = link_workspace_skills(&paths, &workspace, &[".claude/skills"], &resolved)
            .await
            .unwrap_err();

        assert!(matches!(error, ExtensionError::InvalidSkillPath(_)));
        assert!(!workspace.join(".claude/skills/linked-skill").exists());
    }

    #[tokio::test]
    async fn link_workspace_skills_records_marker_and_reuses_projection() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());
        create_skill_in_dir(&paths.user_skills_dir, "managed", "Managed source");
        let resolved = materialize_skills_for_agent(&paths, "conv-managed", &["managed".into()])
            .await
            .unwrap();
        let workspace = tmp.path().join("workspace");

        let first = link_workspace_skills(&paths, &workspace, &[".claude/skills"], &resolved)
            .await
            .unwrap();
        assert_eq!(first.created, 1);
        let marker_path = workspace.join(".claude/skills/.nomifun-managed/managed.json");
        let marker: SkillProjectionMarker =
            serde_json::from_slice(&std::fs::read(&marker_path).unwrap()).unwrap();
        assert_eq!(marker.schema_version, 1);
        assert_eq!(marker.skill_name, "managed");
        assert_eq!(marker.source_path, paths.user_skills_dir.join("managed").to_string_lossy());
        assert!(is_sha256(&marker.content_sha256));

        let second = link_workspace_skills(&paths, &workspace, &[".claude/skills"], &resolved)
            .await
            .unwrap();
        assert_eq!(second.reused, 1);
        assert_eq!(second.repaired, 0);
        assert_eq!(second.migrated, 0);
    }

    // Serial because the copy source here is itself a junction: a concurrent
    // `ForceFailureGuard` test flips the global symlink-failure flag, the link
    // step then falls back to `copy_dir_recursive`, and the junction source is
    // rejected by the no-reparse preflight. The guard tests are `#[serial]`, so
    // joining the same group keeps their windows from overlapping this test.
    #[tokio::test]
    #[serial]
    async fn link_workspace_skills_accepts_a_user_import_link_as_source() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());
        let external_root = tmp.path().join("external-root");
        create_skill_in_dir(&external_root, "linked-source", "Imported source");
        let external = external_root.join("linked-source");
        import_skill_with_symlink(&paths, &external).await.unwrap();

        let resolved = materialize_skills_for_agent(&paths, "conv-import-link", &["linked-source".into()])
            .await
            .unwrap();
        let workspace = tmp.path().join("workspace");
        let report = link_workspace_skills(&paths, &workspace, &[".claude/skills"], &resolved)
            .await
            .unwrap();

        assert_eq!(report.created, 1);
        assert!(workspace.join(".claude/skills/linked-source/SKILL.md").is_file());
        let marker_path = workspace.join(".claude/skills/.nomifun-managed/linked-source.json");
        let marker: SkillProjectionMarker =
            serde_json::from_slice(&std::fs::read(marker_path).unwrap()).unwrap();
        assert_eq!(marker.source_path, paths.user_skills_dir.join("linked-source").to_string_lossy());
    }

    #[tokio::test]
    #[serial]
    async fn modified_managed_copy_is_protected_from_replacement() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());
        create_skill_in_dir(&paths.user_skills_dir, "copied", "Copy source");
        let resolved = materialize_skills_for_agent(&paths, "conv-copied", &["copied".into()])
            .await
            .unwrap();
        let workspace = tmp.path().join("workspace");

        {
            let _guard = test_overrides::ForceFailureGuard::new();
            let report = link_workspace_skills(&paths, &workspace, &[".claude/skills"], &resolved)
                .await
                .unwrap();
            assert_eq!(report.created, 1);
        }

        let target = workspace.join(".claude/skills/copied");
        std::fs::write(target.join("user-change.txt"), "keep me").unwrap();
        let error = link_workspace_skills(&paths, &workspace, &[".claude/skills"], &resolved)
            .await
            .unwrap_err();
        assert!(matches!(error, ExtensionError::SkillProjectionConflict(_)));
        assert_eq!(std::fs::read_to_string(target.join("user-change.txt")).unwrap(), "keep me");
    }

    #[tokio::test]
    async fn managed_projection_repairs_unmodified_stale_source() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());
        create_skill_in_dir(&paths.user_skills_dir, "repairable", "Old source");
        let resolved = materialize_skills_for_agent(&paths, "conv-repair", &["repairable".into()])
            .await
            .unwrap();
        let workspace = tmp.path().join("workspace");

        link_workspace_skills(&paths, &workspace, &[".claude/skills"], &resolved)
            .await
            .unwrap();
        std::fs::write(
            paths.user_skills_dir.join("repairable/SKILL.md"),
            "---\nname: repairable\ndescription: New source\n---\nBody content.",
        )
        .unwrap();

        let report = link_workspace_skills(&paths, &workspace, &[".claude/skills"], &resolved)
            .await
            .unwrap();
        assert_eq!(report.repaired, 1);
        assert_eq!(report.reused, 0);
        assert!(std::fs::read_to_string(workspace.join(".claude/skills/repairable/SKILL.md"))
            .unwrap()
            .contains("New source"));
    }

    #[tokio::test]
    #[serial]
    async fn projection_marker_failure_restores_previous_projection_and_marker() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());
        create_skill_in_dir(&paths.user_skills_dir, "marker-failure", "Old source");
        let resolved = materialize_skills_for_agent(
            &paths,
            "conv-marker-failure",
            &["marker-failure".into()],
        )
        .await
        .unwrap();
        let workspace = tmp.path().join("workspace");

        {
            let _guard = test_overrides::ForceFailureGuard::new();
            link_workspace_skills(&paths, &workspace, &[".claude/skills"], &resolved)
                .await
                .unwrap();
        }
        let target = workspace.join(".claude/skills/marker-failure");
        let marker_path = workspace.join(".claude/skills/.nomifun-managed/marker-failure.json");
        let old_marker = std::fs::read(&marker_path).unwrap();
        std::fs::write(
            paths.user_skills_dir.join("marker-failure/SKILL.md"),
            "---\nname: marker-failure\ndescription: New source\n---\nNew body",
        )
        .unwrap();

        test_overrides::fail_next_projection_marker_write();
        let error = link_workspace_skills(&paths, &workspace, &[".claude/skills"], &resolved)
            .await
            .unwrap_err();
        assert!(matches!(error, ExtensionError::Io(_) | ExtensionError::SkillProjectionConflict(_)));
        assert!(std::fs::read_to_string(target.join(SKILL_MANIFEST_FILE))
            .unwrap()
            .contains("description: Old source"));
        assert_eq!(std::fs::read(marker_path).unwrap(), old_marker);
        assert!(!std::fs::read_dir(target.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".marker-failure.projection-backup-")
            }));
    }

    #[tokio::test]
    #[serial]
    async fn projection_backup_cleanup_failure_keeps_committed_projection() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());
        create_skill_in_dir(&paths.user_skills_dir, "backup-failure", "Old source");
        let resolved = materialize_skills_for_agent(
            &paths,
            "conv-backup-failure",
            &["backup-failure".into()],
        )
        .await
        .unwrap();
        let workspace = tmp.path().join("workspace");

        {
            let _guard = test_overrides::ForceFailureGuard::new();
            link_workspace_skills(&paths, &workspace, &[".claude/skills"], &resolved)
                .await
                .unwrap();
        }
        std::fs::write(
            paths.user_skills_dir.join("backup-failure/SKILL.md"),
            "---\nname: backup-failure\ndescription: New source\n---\nNew body",
        )
        .unwrap();

        test_overrides::fail_next_projection_backup_delete();
        let report = link_workspace_skills(&paths, &workspace, &[".claude/skills"], &resolved)
            .await
            .unwrap();
        assert_eq!(report.repaired, 1);
        assert_eq!(
            std::fs::read_to_string(workspace.join(".claude/skills/backup-failure/SKILL.md"))
                .unwrap(),
            "---\nname: backup-failure\ndescription: New source\n---\nNew body"
        );
        let backup_prefix = ".backup-failure.projection-backup-";
        assert!(std::fs::read_dir(workspace.join(".claude/skills"))
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().starts_with(backup_prefix)));
    }

    #[cfg(unix)]
    #[tokio::test]
    #[serial]
    async fn marked_dangling_projection_link_is_repaired() {
        use std::os::unix::fs::symlink;

        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());
        create_skill_in_dir(&paths.user_skills_dir, "dangling", "Current source");
        let resolved = materialize_skills_for_agent(&paths, "conv-dangling", &["dangling".into()])
            .await
            .unwrap();
        let workspace = tmp.path().join("workspace");
        link_workspace_skills(&paths, &workspace, &[".claude/skills"], &resolved)
            .await
            .unwrap();

        let target = workspace.join(".claude/skills/dangling");
        std::fs::remove_file(&target).unwrap();
        symlink(tmp.path().join("deleted-source"), &target).unwrap();

        let report = link_workspace_skills(&paths, &workspace, &[".claude/skills"], &resolved)
            .await
            .unwrap();
        assert_eq!(report.repaired, 1);
        assert!(target.join(SKILL_MANIFEST_FILE).is_file());
    }

    #[tokio::test]
    async fn unmarked_workspace_directory_is_left_untouched() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());
        create_skill_in_dir(&paths.user_skills_dir, "protected", "Managed source");
        let resolved = materialize_skills_for_agent(&paths, "conv-protected", &["protected".into()])
            .await
            .unwrap();
        let workspace = tmp.path().join("workspace");
        let target = workspace.join(".claude/skills/protected");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("user-file.txt"), "do not overwrite").unwrap();

        let error = link_workspace_skills(&paths, &workspace, &[".claude/skills"], &resolved)
            .await
            .unwrap_err();
        assert!(matches!(error, ExtensionError::SkillProjectionConflict(_)));
        assert_eq!(
            std::fs::read_to_string(target.join("user-file.txt")).unwrap(),
            "do not overwrite"
        );
        assert!(!workspace.join(".claude/skills/.nomifun-managed/protected.json").exists());
    }

    #[tokio::test]
    async fn unmarked_historical_flowy_link_is_migrated() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());
        create_skill_in_dir(&paths.user_skills_dir, "legacy", "Legacy source");
        let source = paths.user_skills_dir.join("legacy");
        let resolved = vec![ResolvedAgentSkill {
            name: "legacy".into(),
            source_path: source.clone(),
        }];
        let workspace = tmp.path().join("workspace");
        let target_dir = workspace.join(".claude/skills");
        std::fs::create_dir_all(&target_dir).unwrap();
        create_symlink(&source, &target_dir.join("legacy")).await.unwrap();

        let report = link_workspace_skills(&paths, &workspace, &[".claude/skills"], &resolved)
            .await
            .unwrap();
        assert_eq!(report.migrated, 1);
        assert!(target_dir.join(".nomifun-managed/legacy.json").is_file());

        let second = link_workspace_skills(&paths, &workspace, &[".claude/skills"], &resolved)
            .await
            .unwrap();
        assert_eq!(second.reused, 1);
    }

    #[tokio::test]
    async fn unmarked_historical_import_link_is_migrated() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());
        let external_root = tmp.path().join("external-root");
        create_skill_in_dir(&external_root, "linked-source", "Imported source");
        let external_source = external_root.join("linked-source");
        import_skill_with_symlink(&paths, &external_source).await.unwrap();

        let source = paths.user_skills_dir.join("linked-source");
        let resolved = vec![ResolvedAgentSkill {
            name: "linked-source".into(),
            source_path: source.clone(),
        }];
        let workspace = tmp.path().join("workspace");
        let target_dir = workspace.join(".claude/skills");
        std::fs::create_dir_all(&target_dir).unwrap();
        create_symlink(&source, &target_dir.join("linked-source"))
            .await
            .unwrap();

        let report = link_workspace_skills(&paths, &workspace, &[".claude/skills"], &resolved)
            .await
            .unwrap();
        assert_eq!(report.migrated, 1);
        assert!(target_dir
            .join(".nomifun-managed/linked-source.json")
            .is_file());
    }

    #[tokio::test]
    async fn unmarked_nested_flowy_root_link_is_not_migrated() {
        let tmp = TempDir::new().unwrap();
        let paths = make_test_paths(tmp.path());
        let nested_source = paths.user_skills_dir.join("nested").join("legacy");
        std::fs::create_dir_all(&nested_source).unwrap();
        std::fs::write(
            nested_source.join(SKILL_MANIFEST_FILE),
            "---\nname: legacy\ndescription: Nested source\n---\n",
        )
        .unwrap();
        let resolved = vec![ResolvedAgentSkill {
            name: "legacy".into(),
            source_path: nested_source.clone(),
        }];
        let workspace = tmp.path().join("workspace");
        let target_dir = workspace.join(".claude/skills");
        std::fs::create_dir_all(&target_dir).unwrap();
        create_symlink(&nested_source, &target_dir.join("legacy"))
            .await
            .unwrap();

        let error = link_workspace_skills(&paths, &workspace, &[".claude/skills"], &resolved)
            .await
            .unwrap_err();
        assert!(matches!(error, ExtensionError::SkillProjectionConflict(_)));
        assert!(!target_dir.join(".nomifun-managed/legacy.json").exists());
    }

    /// Windows-only: directory linking must go through an NTFS junction
    /// (created by the `junction` crate) rather than `symlink_dir`, so
    /// the link works for users without Developer Mode. We assert the
    /// resulting path is a reparse point (junction is reported as a
    /// symlink by `symlink_metadata().file_type().is_symlink()`) and
    /// that the source contents are reachable through the link.
    ///
    /// The test is skipped on non-Windows platforms.
    #[cfg(target_os = "windows")]
    #[tokio::test]
    #[serial]
    async fn link_workspace_skills_uses_junction_on_windows() {
        let tmp = TempDir::new().unwrap();
        let workspace = tmp.path().join("workspace");
        let source_root = tmp.path().join("sources");

        let skill_source = source_root.join("my-skill");
        std::fs::create_dir_all(skill_source.join("nested")).unwrap();
        std::fs::write(
            skill_source.join(SKILL_MANIFEST_FILE),
            "---\nname: my-skill\ndescription: test\n---\nbody",
        )
        .unwrap();
        std::fs::write(skill_source.join("nested").join("data.txt"), "payload").unwrap();

        let resolved = vec![ResolvedAgentSkill {
            name: "my-skill".to_owned(),
            source_path: skill_source.clone(),
        }];

        let created = link_workspace_skills(&test_paths(&tmp), &workspace, &[".claude/skills"], &resolved)
            .await
            .expect("link_workspace_skills should succeed via junction");
        assert_eq!(created.created, 1, "exactly one skill should be materialized");

        let target = workspace.join(".claude/skills").join("my-skill");
        assert!(target.exists(), "target path must exist");

        // Junctions are reparse points; `symlink_metadata` reports them
        // as symlinks on Windows. The directory copy fallback would
        // produce a real directory (is_symlink() == false).
        let meta = std::fs::symlink_metadata(&target).unwrap();
        assert!(
            meta.file_type().is_symlink(),
            "Windows directory link must be a junction (reparse point), \
             not a copied directory"
        );

        // Reading through the link must surface the source contents.
        let manifest = std::fs::read_to_string(target.join(SKILL_MANIFEST_FILE)).unwrap();
        assert!(manifest.contains("name: my-skill"));
        let nested = std::fs::read_to_string(target.join("nested").join("data.txt")).unwrap();
        assert_eq!(nested, "payload");
    }
}
