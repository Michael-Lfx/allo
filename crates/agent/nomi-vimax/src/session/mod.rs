//! Session index — `{data_dir}/vimax/.vimax/sessions.json` + `.working_dir/<id>/`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::domain::WorkflowKind;
use crate::error::{VimaxError, VimaxResult};
use crate::progress::{RenderStatus, RunStatus};

pub mod action_assets;
pub mod archive;
pub mod cameo;
pub mod path_remap;
pub use action_assets::ActionAssetsInfo;
pub use archive::{ARCHIVE_EXTENSION, ArchiveManifest};
pub use cameo::{CameoManifest, CameoPhotoEntry, CameoUpdate};
pub use path_remap::{remap_imported_working_paths, resolve_stored_asset_path};

const STALE_KEYS: &[&str] = &[
    "story",
    "characters",
    "script",
    "storyboard",
    "shot_descriptions",
    "camera_tree",
    "frames",
    "clips",
    "final_video",
    "cover",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    #[serde(rename = "id", alias = "session_id")]
    pub session_id: String,
    pub working_dir: String,
    #[serde(default)]
    pub title: String,
    pub workflow: WorkflowKind,
    #[serde(default)]
    pub idea: String,
    #[serde(default)]
    pub script: String,
    #[serde(default)]
    pub novel_text: String,
    #[serde(default)]
    pub user_requirement: String,
    #[serde(default)]
    pub style: String,
    /// Source-qualified vertical skill ids attached to this session (`builtin:…`, `user:…`).
    #[serde(default)]
    pub vertical_skill_ids: Vec<String>,
    /// Flowy chat / LLM model id (e.g. `AIPC-glm-4.7`). Empty → server default.
    #[serde(default)]
    pub llm_model: String,
    /// Flowy image model id. Empty → media settings / catalog first.
    #[serde(default)]
    pub image_model: String,
    /// Flowy video model id. Empty → media settings / catalog first.
    #[serde(default)]
    pub video_model: String,
    /// User target for finished video length (seconds). `0` / missing → default in planning.
    #[serde(default)]
    pub target_duration_secs: u32,
    /// Seedance video + poster aspect ratio (`16:9`, `9:16`, …). Empty → media default.
    #[serde(default)]
    pub aspect_ratio: String,
    /// Seedance output resolution (`480p` / `720p` / `1080p`). Empty → media default.
    #[serde(default)]
    pub resolution: String,
    /// Output frame rate. Seedance is fixed at 24; stored for UI + future models.
    #[serde(default)]
    pub fps: u32,
    #[serde(default = "default_stage")]
    pub stage: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub status: RunStatus,
    #[serde(default)]
    pub stale: BTreeMap<String, bool>,
    #[serde(default)]
    pub final_video: Option<String>,
    /// Relative path to film poster (`…/cover.png`). Display-only; not muxed into the film.
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

/// Compact list-row projection of a [`SessionRecord`].
///
/// The home page and sidebar need lifecycle and preview metadata, not the
/// script and planning payload kept in the full session endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct SessionSummary {
    #[serde(rename = "id")]
    pub session_id: String,
    pub title: String,
    pub workflow: WorkflowKind,
    pub stage: String,
    pub status: RunStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_video: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<SessionRecord> for SessionSummary {
    fn from(record: SessionRecord) -> Self {
        Self {
            session_id: record.session_id,
            title: record.title,
            workflow: record.workflow,
            stage: record.stage,
            status: record.status,
            final_video: record.final_video,
            cover: record.cover,
            created_at: record.created_at,
            updated_at: record.updated_at,
        }
    }
}

fn default_stage() -> String {
    "created".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SessionsFile {
    #[serde(default)]
    active_session_id: String,
    #[serde(default)]
    sessions: BTreeMap<String, SessionRecord>,
}

/// On-disk session registry under `{data_dir}/vimax/`.
#[derive(Clone)]
pub struct SessionIndex {
    workspace_root: PathBuf,
    lock: Arc<Mutex<()>>,
    summaries: Arc<Mutex<Option<Vec<SessionSummary>>>>,
}

impl SessionIndex {
    pub fn open(data_dir: &Path) -> VimaxResult<Self> {
        let workspace_root = data_dir.join("vimax");
        let vimax_dir = workspace_root.join(".vimax");
        let working_root = workspace_root.join(".working_dir");
        std::fs::create_dir_all(&vimax_dir)?;
        std::fs::create_dir_all(&working_root)?;
        let sessions_path = vimax_dir.join("sessions.json");
        if !sessions_path.exists() {
            let empty = SessionsFile::default();
            atomic_write_json(&sessions_path, &empty)?;
        }
        let memory = vimax_dir.join("memory.md");
        if !memory.exists() {
            std::fs::write(&memory, "# User Preferences\n")?;
        }
        Ok(Self {
            workspace_root,
            lock: Arc::new(Mutex::new(())),
            summaries: Arc::new(Mutex::new(None)),
        })
    }

    /// Mark planning/rendering sessions as interrupted after a process restart.
    /// Preserves `stage` so the UI can resume plan vs render from checkpoint.
    pub fn reconcile_orphaned_active_runs(&self) -> VimaxResult<usize> {
        use crate::progress::{INTERRUPTED_SUMMARY, RunStatus};
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut data = self.load()?;
        let mut n = 0usize;
        let now = chrono::Local::now().to_rfc3339();
        for record in data.sessions.values_mut() {
            if !record.status.is_active() {
                continue;
            }
            record.status = RunStatus::Interrupted;
            record.summary = INTERRUPTED_SUMMARY.to_string();
            record.updated_at = now.clone();
            n += 1;
        }
        if n > 0 {
            self.save(&data)?;
        }
        Ok(n)
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    fn sessions_path(&self) -> PathBuf {
        self.workspace_root.join(".vimax").join("sessions.json")
    }

    fn load(&self) -> VimaxResult<SessionsFile> {
        let path = self.sessions_path();
        let raw = std::fs::read_to_string(&path)?;
        match serde_json::from_str(&raw) {
            Ok(v) => Ok(v),
            Err(e) => {
                tracing::error!(
                    error = %e,
                    path = %path.display(),
                    "failed to parse vimax sessions.json; attempting per-record salvage"
                );
                match salvage_sessions_file(&raw) {
                    Ok(file) if !file.sessions.is_empty() => {
                        tracing::warn!(
                            recovered = file.sessions.len(),
                            "salvaged vimax session index after parse failure"
                        );
                        Ok(file)
                    }
                    Ok(_) | Err(_) => {
                        let backup = path.with_extension(format!(
                            "json.corrupt-{}",
                            chrono::Local::now().format("%Y%m%d-%H%M%S")
                        ));
                        match std::fs::copy(&path, &backup) {
                            Ok(_) => tracing::error!(
                                backup = %backup.display(),
                                "left original sessions.json in place; not resetting the index"
                            ),
                            Err(copy_err) => tracing::error!(
                                error = %copy_err,
                                "failed to backup unreadable sessions.json"
                            ),
                        }
                        Err(e.into())
                    }
                }
            }
        }
    }

    fn save(&self, data: &SessionsFile) -> VimaxResult<()> {
        atomic_write_json(&self.sessions_path(), data)?;
        *self.summaries.lock().unwrap_or_else(|e| e.into_inner()) = None;
        Ok(())
    }

    pub fn list(&self) -> VimaxResult<Vec<SessionRecord>> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let data = self.load()?;
        let mut sessions: Vec<_> = data.sessions.into_values().collect();
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(sessions)
    }

    pub fn list_summaries(&self) -> VimaxResult<Vec<SessionSummary>> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(summaries) = self
            .summaries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return Ok(summaries);
        }
        let data = self.load()?;
        let mut sessions: Vec<_> = data
            .sessions
            .into_values()
            .map(SessionSummary::from)
            .collect();
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        *self.summaries.lock().unwrap_or_else(|e| e.into_inner()) = Some(sessions.clone());
        Ok(sessions)
    }

    pub fn get(&self, session_id: &str) -> VimaxResult<SessionRecord> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let data = self.load()?;
        data.sessions
            .get(session_id)
            .cloned()
            .ok_or_else(|| VimaxError::SessionNotFound(session_id.to_string()))
    }

    pub fn create(
        &self,
        workflow: WorkflowKind,
        title: Option<String>,
    ) -> VimaxResult<SessionRecord> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut data = self.load()?;
        let session_id = Uuid::new_v4().to_string();
        let now = chrono::Local::now().to_rfc3339();
        let working_rel = format!(".working_dir/{session_id}");
        let working_abs = self.workspace_root.join(&working_rel);
        std::fs::create_dir_all(working_abs.join(workflow.artifact_root()))?;

        let record = SessionRecord {
            session_id: session_id.clone(),
            working_dir: working_rel,
            title: title.unwrap_or_else(|| format!("{} session", workflow.as_str())),
            workflow,
            idea: String::new(),
            script: String::new(),
            novel_text: String::new(),
            user_requirement: String::new(),
            style: String::new(),
            vertical_skill_ids: Vec::new(),
            llm_model: String::new(),
            image_model: String::new(),
            video_model: String::new(),
            target_duration_secs: 0,
            aspect_ratio: String::new(),
            resolution: String::new(),
            fps: 0,
            stage: "created".into(),
            summary: String::new(),
            status: RunStatus::Idle,
            stale: STALE_KEYS.iter().map(|k| ((*k).to_string(), false)).collect(),
            final_video: None,
            cover: None,
            created_at: now.clone(),
            updated_at: now,
        };
        data.sessions.insert(session_id.clone(), record.clone());
        data.active_session_id = session_id;
        self.save(&data)?;
        Ok(record)
    }

    pub fn update_stage(&self, session_id: &str, stage: &str, summary: &str) -> VimaxResult<()> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut data = self.load()?;
        let record = data
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| VimaxError::SessionNotFound(session_id.to_string()))?;
        record.stage = stage.to_string();
        if !summary.is_empty() {
            record.summary = summary.to_string();
        }
        record.updated_at = chrono::Local::now().to_rfc3339();
        self.save(&data)
    }

    pub fn update_fields<F>(&self, session_id: &str, mutator: F) -> VimaxResult<SessionRecord>
    where
        F: FnOnce(&mut SessionRecord),
    {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut data = self.load()?;
        let record = data
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| VimaxError::SessionNotFound(session_id.to_string()))?;
        mutator(record);
        record.updated_at = chrono::Local::now().to_rfc3339();
        let out = record.clone();
        self.save(&data)?;
        Ok(out)
    }

    /// Remove session metadata and delete its working directory (idempotent if already gone).
    pub fn delete(&self, session_id: &str) -> VimaxResult<()> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut data = self.load()?;
        let Some(record) = data.sessions.remove(session_id) else {
            return Err(VimaxError::SessionNotFound(session_id.to_string()));
        };
        if data.active_session_id == session_id {
            data.active_session_id = data
                .sessions
                .keys()
                .next()
                .cloned()
                .unwrap_or_default();
        }
        self.save(&data)?;

        let path = self.workspace_root.join(&record.working_dir);
        let working_root = self.workspace_root.join(".working_dir");
        if path != working_root && path.starts_with(&working_root) && path.exists() {
            if let Err(e) = std::fs::remove_dir_all(&path) {
                tracing::warn!(
                    session_id,
                    path = %path.display(),
                    error = %e,
                    "failed to remove vimax session working_dir (index entry already deleted)"
                );
            }
        }
        Ok(())
    }

    pub fn working_dir(&self, session_id: &str) -> VimaxResult<PathBuf> {
        let record = self.get(session_id)?;
        let path = self.workspace_root.join(&record.working_dir);
        let working_root = self.workspace_root.join(".working_dir");
        if path != working_root && !path.starts_with(&working_root) {
            return Err(VimaxError::msg(format!(
                "session working_dir escapes .working_dir: {}",
                record.working_dir
            )));
        }
        std::fs::create_dir_all(&path)?;
        Ok(path)
    }

    /// Artifact presence checklist (ViMax `SessionIndex.artifact_checklist`).
    pub fn artifact_checklist(&self, session_id: &str) -> VimaxResult<BTreeMap<String, bool>> {
        let root = self.working_dir(session_id)?;
        let idea_dir = root.join("idea2video");
        let script_dir = root.join("script2video");
        let novel_dir = root.join("novel2video");

        let idea_scene_dirs: Vec<_> = if idea_dir.exists() {
            walkdir::WalkDir::new(&idea_dir)
                .min_depth(1)
                .max_depth(1)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_type().is_dir()
                        && e.file_name()
                            .to_string_lossy()
                            .starts_with("scene_")
                })
                .map(|e| e.path().to_path_buf())
                .collect()
        } else {
            vec![]
        };

        let mut map = BTreeMap::new();
        map.insert(
            "idea2video/story.txt".into(),
            idea_dir.join("story.txt").exists(),
        );
        map.insert(
            "idea2video/characters.json".into(),
            idea_dir.join("characters.json").exists(),
        );
        map.insert(
            "idea2video/script.json".into(),
            idea_dir.join("script.json").exists(),
        );
        map.insert(
            "idea2video/scene_*/storyboard.json".into(),
            !idea_scene_dirs.is_empty()
                && idea_scene_dirs
                    .iter()
                    .all(|p| p.join("storyboard.json").exists()),
        );
        map.insert(
            "idea2video/final_video.mp4".into(),
            idea_dir.join("final_video.mp4").exists(),
        );
        map.insert(
            "script2video/script.txt".into(),
            script_dir.join("script.txt").exists(),
        );
        map.insert(
            "script2video/characters.json".into(),
            script_dir.join("characters.json").exists(),
        );
        map.insert(
            "script2video/storyboard.json".into(),
            script_dir.join("storyboard.json").exists(),
        );
        map.insert(
            "script2video/camera_tree.json".into(),
            script_dir.join("camera_tree.json").exists(),
        );
        map.insert(
            "script2video/final_video.mp4".into(),
            script_dir.join("final_video.mp4").exists(),
        );
        map.insert(
            "novel2video/novel/novel.txt".into(),
            novel_dir.join("novel").join("novel.txt").exists(),
        );
        map.insert(
            "novel2video/novel/novel_compressed.txt".into(),
            novel_dir
                .join("novel")
                .join("novel_compressed.txt")
                .exists(),
        );
        map.insert(
            "action2video/character".into(),
            action_assets::character_abs(&root.join("action2video")).is_some(),
        );
        map.insert(
            "action2video/reference".into(),
            action_assets::reference_abs(&root.join("action2video")).is_some(),
        );
        map.insert(
            "action2video/final_video.mp4".into(),
            root.join("action2video")
                .join(action_assets::FINAL_VIDEO_FILENAME)
                .exists(),
        );
        map.insert(
            "references/manifest.json".into(),
            root.join(cameo::CAMEO_MANIFEST_REL).exists()
                || root.join("cameo/manifest.json").exists(),
        );
        Ok(map)
    }

    /// Build a recursive artifact tree for the UI.
    pub fn list_artifacts(&self, session_id: &str) -> VimaxResult<Vec<ArtifactNode>> {
        let root = self.working_dir(session_id)?;
        Ok(walk_tree(&root, &root)?)
    }

    pub fn artifact_abs_path(&self, session_id: &str, rel: &str) -> VimaxResult<PathBuf> {
        let root = self.working_dir(session_id)?;
        let cleaned = rel.replace('\\', "/");
        if cleaned.contains("..") {
            return Err(VimaxError::InvalidParams("path traversal".into()));
        }
        let path = root.join(&cleaned);
        if !path.starts_with(&root) {
            return Err(VimaxError::InvalidParams("path escapes working dir".into()));
        }
        Ok(path)
    }

    /// Export a session (metadata + full working tree) to a `.nomivimax` archive.
    pub fn export_to_path(&self, session_id: &str, dest_path: &Path) -> VimaxResult<PathBuf> {
        let record = self.get(session_id)?;
        let working = self.working_dir(session_id)?;
        archive::export_session_to_path(&record, &working, dest_path)
    }

    /// Import a `.nomivimax` archive as a **new** session (new UUID, new working_dir).
    ///
    /// Extracts into a temporary directory under `.working_dir`, validates, then
    /// atomically renames into place and inserts the index entry. Failures clean
    /// up staging / half-imported dirs and never touch existing sessions.
    pub fn import_from_path(&self, archive_path: &Path) -> VimaxResult<SessionRecord> {
        let working_root = self.workspace_root.join(".working_dir");
        std::fs::create_dir_all(&working_root)?;

        let staging_name = format!(".import_tmp_{}", Uuid::new_v4());
        let staging = working_root.join(&staging_name);
        // Ensure staging stays inside working_root.
        if !staging.starts_with(&working_root) || staging == working_root {
            return Err(VimaxError::msg("invalid import staging path"));
        }

        let cleanup_staging = |path: &Path| {
            if path.exists() {
                let _ = std::fs::remove_dir_all(path);
            }
        };

        let imported = match archive::import_session_from_path(archive_path, &staging) {
            Ok(s) => s,
            Err(e) => {
                cleanup_staging(&staging);
                return Err(e);
            }
        };

        let new_id = Uuid::new_v4().to_string();
        let final_rel = format!(".working_dir/{new_id}");
        let final_abs = self.workspace_root.join(&final_rel);
        if final_abs.exists() {
            cleanup_staging(&staging);
            return Err(VimaxError::msg(format!(
                "import target already exists: {new_id}"
            )));
        }

        if let Err(e) = std::fs::rename(&staging, &final_abs) {
            cleanup_staging(&staging);
            return Err(VimaxError::Io(e));
        }

        // Exporter machines write absolute paths into asset registries. Remap them
        // onto this session's working tree before the project is usable.
        match path_remap::remap_imported_working_paths(&final_abs) {
            Ok(n) if n > 0 => {
                tracing::info!(
                    session_id = %new_id,
                    rewritten = n,
                    "remapped absolute asset paths after import"
                );
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(
                    session_id = %new_id,
                    error = %e,
                    "path remap after import failed; render may hit missing-path IO errors"
                );
            }
        }

        let now = chrono::Local::now().to_rfc3339();
        let mut record = imported;
        record.session_id = new_id.clone();
        record.working_dir = final_rel;
        record.status = RunStatus::Idle;
        // Keep stage / final_video so UI can resume without regenerating.
        if record.stage.is_empty() {
            record.stage = if record.final_video.is_some() {
                "succeeded".into()
            } else {
                "imported".into()
            };
        }
        record.created_at = now.clone();
        record.updated_at = now;
        if record.summary.is_empty() {
            record.summary = "imported project".into();
        }

        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut data = match self.load() {
            Ok(d) => d,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&final_abs);
                return Err(e);
            }
        };
        data.sessions.insert(new_id.clone(), record.clone());
        data.active_session_id = new_id;
        if let Err(e) = self.save(&data) {
            let _ = std::fs::remove_dir_all(&final_abs);
            return Err(e);
        }
        // Drop orphan cameo pointers after import (files may have been missing in archive).
        let _ = cameo::scrub_manifest(&final_abs);
        Ok(record)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<ArtifactNode>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

fn walk_tree(root: &Path, dir: &Path) -> VimaxResult<Vec<ArtifactNode>> {
    let mut entries = Vec::new();
    let mut read = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .collect::<Vec<_>>();
    read.sort_by_key(|e| e.file_name());
    for entry in read {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if path.is_dir() {
            entries.push(ArtifactNode {
                name,
                path: rel,
                is_dir: true,
                children: Some(walk_tree(root, &path)?),
                mime: None,
                size: None,
            });
        } else {
            let meta = entry.metadata().ok();
            entries.push(ArtifactNode {
                name,
                path: rel,
                is_dir: false,
                children: None,
                mime: guess_mime(&path),
                size: meta.map(|m| m.len()),
            });
        }
    }
    Ok(entries)
}

fn guess_mime(path: &Path) -> Option<String> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => Some("image/png".into()),
        "jpg" | "jpeg" => Some("image/jpeg".into()),
        "webp" => Some("image/webp".into()),
        "mp4" => Some("video/mp4".into()),
        "webm" => Some("video/webm".into()),
        "json" => Some("application/json".into()),
        "txt" | "md" => Some("text/plain".into()),
        _ => None,
    }
}

/// Recover whatever session records still deserialize after a full-file parse failure.
fn salvage_sessions_file(raw: &str) -> Result<SessionsFile, serde_json::Error> {
    let value: Value = serde_json::from_str(raw)?;
    let mut file = SessionsFile::default();
    if let Some(id) = value.get("active_session_id").and_then(|v| v.as_str()) {
        file.active_session_id = id.to_string();
    }
    let Some(map) = value.get("sessions").and_then(|v| v.as_object()) else {
        return Ok(file);
    };
    for (id, rec) in map {
        match serde_json::from_value::<SessionRecord>(rec.clone()) {
            Ok(mut record) => {
                if record.session_id.is_empty() {
                    record.session_id = id.clone();
                }
                file.sessions.insert(id.clone(), record);
            }
            Err(err) => {
                tracing::warn!(
                    session_id = %id,
                    error = %err,
                    "skipping unreadable vimax session record"
                );
            }
        }
    }
    Ok(file)
}

fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> VimaxResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let raw = serde_json::to_string_pretty(value)?;
    std::fs::write(&tmp, raw)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Persist JSON artifact helper used by pipelines.
pub async fn write_json_artifact<T: Serialize>(path: &Path, value: &T) -> VimaxResult<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let raw = serde_json::to_string_pretty(value)?;
    tokio::fs::write(path, raw).await?;
    Ok(())
}

pub async fn read_json_artifact<T: for<'de> Deserialize<'de>>(path: &Path) -> VimaxResult<T> {
    let raw = tokio::fs::read_to_string(path).await?;
    Ok(serde_json::from_str(&raw)?)
}

pub async fn write_text_artifact(path: &Path, text: &str) -> VimaxResult<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, text).await?;
    Ok(())
}

/// Sync a live [`RenderStatus`] into the session record.
pub fn apply_status_to_record(record: &mut SessionRecord, status: &RenderStatus) {
    record.status = status.status;
    if !status.stage.is_empty() {
        record.stage = status.stage.clone();
    }
    if !status.message.is_empty() {
        record.summary = status.message.clone();
    }
    if let Some(v) = &status.final_video {
        record.final_video = Some(v.clone());
    }
    if let Some(v) = &status.cover {
        record.cover = Some(v.clone());
    }
}

/// Convenience: empty metadata object for progress events.
pub fn meta_json(map: impl IntoIterator<Item = (&'static str, Value)>) -> Option<Value> {
    let mut obj = serde_json::Map::new();
    for (k, v) in map {
        obj.insert(k.into(), v);
    }
    Some(Value::Object(obj))
}

#[cfg(test)]
mod import_export_tests {
    use super::*;
    use crate::domain::WorkflowKind;
    use crate::progress::{INTERRUPTED_SUMMARY, RunStatus};
    use tempfile::tempdir;

    #[test]
    fn reconcile_orphaned_active_runs_preserves_stage() {
        let dir = tempdir().unwrap();
        let index = SessionIndex::open(dir.path()).unwrap();
        let record = index.create(WorkflowKind::Script2Video, Some("t".into())).unwrap();
        index
            .update_fields(&record.session_id, |r| {
                r.status = RunStatus::Rendering;
                r.stage = "video_clips_start".into();
                r.summary = "generating clip 3".into();
            })
            .unwrap();

        let n = index.reconcile_orphaned_active_runs().unwrap();
        assert_eq!(n, 1);
        let healed = index.get(&record.session_id).unwrap();
        assert_eq!(healed.status, RunStatus::Interrupted);
        assert_eq!(healed.stage, "video_clips_start");
        assert_eq!(healed.summary, INTERRUPTED_SUMMARY);
        assert_eq!(index.reconcile_orphaned_active_runs().unwrap(), 0);
    }

    #[test]
    fn unknown_run_status_does_not_fail_deserialize() {
        let status: RunStatus = serde_json::from_str("\"paused\"").unwrap();
        assert_eq!(status, RunStatus::Idle);
        let interrupted: RunStatus = serde_json::from_str("\"interrupted\"").unwrap();
        assert_eq!(interrupted, RunStatus::Interrupted);
    }

    #[test]
    fn load_salvages_readable_records_instead_of_resetting_index() {
        let dir = tempdir().unwrap();
        let index = SessionIndex::open(dir.path()).unwrap();
        let keep = index
            .create(WorkflowKind::Idea2Video, Some("Keep me".into()))
            .unwrap();
        let raw = serde_json::json!({
            "active_session_id": keep.session_id,
            "sessions": {
                keep.session_id.clone(): serde_json::to_value(index.get(&keep.session_id).unwrap()).unwrap(),
                "broken": {
                    "id": "broken",
                    "working_dir": ".working_dir/broken",
                    "workflow": "not-a-real-workflow",
                    "status": "succeeded"
                }
            }
        });
        std::fs::write(index.sessions_path(), serde_json::to_vec_pretty(&raw).unwrap()).unwrap();

        let listed = index.list().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].session_id, keep.session_id);
        assert_eq!(listed[0].title, "Keep me");
    }

    #[test]
    fn load_does_not_wipe_sessions_file_when_json_is_invalid() {
        let dir = tempdir().unwrap();
        let index = SessionIndex::open(dir.path()).unwrap();
        let path = index.sessions_path();
        std::fs::write(&path, "not-json{{{").unwrap();

        let err = index.load().unwrap_err();
        assert!(matches!(err, VimaxError::Json(_)));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "not-json{{{");
        let backups: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains("corrupt-"))
            .collect();
        assert_eq!(backups.len(), 1);
    }

    #[test]
    fn list_summaries_omits_editing_payload_and_preserves_recent_order() {
        let dir = tempdir().unwrap();
        let index = SessionIndex::open(dir.path()).unwrap();
        let older = index
            .create(WorkflowKind::Idea2Video, Some("Older".into()))
            .unwrap();
        let newer = index
            .create(WorkflowKind::Script2Video, Some("Newer".into()))
            .unwrap();
        let mut sessions = index.load().unwrap();
        let older_record = sessions.sessions.get_mut(&older.session_id).unwrap();
        older_record.idea = "private planning payload".into();
        older_record.updated_at = "2026-01-01T00:00:00Z".into();
        let newer_record = sessions.sessions.get_mut(&newer.session_id).unwrap();
        newer_record.script = "private script payload".into();
        newer_record.stage = "rendering".into();
        newer_record.status = RunStatus::Rendering;
        newer_record.updated_at = "2026-01-02T00:00:00Z".into();
        index.save(&sessions).unwrap();

        let summaries = index.list_summaries().unwrap();
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].session_id, newer.session_id);
        assert_eq!(summaries[0].title, "Newer");
        assert_eq!(summaries[0].status, RunStatus::Rendering);

        let encoded = serde_json::to_value(&summaries).unwrap();
        assert!(encoded[0].get("script").is_none());
        assert!(encoded[1].get("idea").is_none());
        assert!(encoded[0].get("working_dir").is_none());
    }

    #[test]
    fn index_export_import_creates_new_id_and_keeps_assets() {
        let dir = tempdir().unwrap();
        let index = SessionIndex::open(dir.path()).unwrap();
        let created = index
            .create(WorkflowKind::Script2Video, Some("Share Me".into()))
            .unwrap();
        let working = index.working_dir(&created.session_id).unwrap();
        let video_rel = "script2video/final_video.mp4";
        std::fs::create_dir_all(working.join("script2video/shots/0")).unwrap();
        std::fs::write(working.join("script2video/script.txt"), b"scene").unwrap();
        std::fs::write(working.join(video_rel), b"VIDEOBYTES").unwrap();
        std::fs::write(
            working.join("script2video/shots/0/first_frame.png"),
            b"PNG",
        )
        .unwrap();
        index
            .update_fields(&created.session_id, |r| {
                r.status = RunStatus::Succeeded;
                r.stage = "succeeded".into();
                r.final_video = Some(video_rel.into());
            })
            .unwrap();

        let archive = dir.path().join("out.nomivimax");
        index
            .export_to_path(&created.session_id, &archive)
            .unwrap();

        let imported = index.import_from_path(&archive).unwrap();
        assert_ne!(imported.session_id, created.session_id);
        assert_eq!(imported.title, "Share Me");
        assert_eq!(imported.status, RunStatus::Idle);
        assert_eq!(imported.final_video.as_deref(), Some(video_rel));
        assert_eq!(
            std::fs::read(index.working_dir(&imported.session_id).unwrap().join(video_rel))
                .unwrap(),
            b"VIDEOBYTES"
        );
        // Original still listed.
        let list = index.list().unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn export_import_preserves_cameo_photos() {
        use image::{ImageFormat, Rgb, RgbImage};

        let dir = tempdir().unwrap();
        let index = SessionIndex::open(dir.path()).unwrap();
        let created = index
            .create(WorkflowKind::Idea2Video, Some("Cameo Share".into()))
            .unwrap();
        let working = index.working_dir(&created.session_id).unwrap();
        let mut jpeg = Vec::new();
        RgbImage::from_pixel(20, 16, Rgb([1, 2, 3]))
            .write_to(&mut std::io::Cursor::new(&mut jpeg), ImageFormat::Jpeg)
            .unwrap();
        let entry = cameo::upload_photo(&working, &jpeg, "Hero", "me").unwrap();
        let png_bytes = std::fs::read(working.join(&entry.rel_path)).unwrap();

        let archive = dir.path().join("cameo.nomivimax");
        index
            .export_to_path(&created.session_id, &archive)
            .unwrap();
        let imported = index.import_from_path(&archive).unwrap();
        let imported_working = index.working_dir(&imported.session_id).unwrap();
        let photos = cameo::list_photos(&imported_working).unwrap();
        assert_eq!(photos.len(), 1);
        assert_eq!(photos[0].character_name, "Hero");
        assert_eq!(
            std::fs::read(imported_working.join(&photos[0].rel_path)).unwrap(),
            png_bytes
        );
    }

    #[test]
    fn import_failure_does_not_pollute_index() {
        let dir = tempdir().unwrap();
        let index = SessionIndex::open(dir.path()).unwrap();
        let before = index.list().unwrap().len();
        let bad = dir.path().join("not-an-archive.nomivimax");
        std::fs::write(&bad, b"garbage").unwrap();
        assert!(index.import_from_path(&bad).is_err());
        assert_eq!(index.list().unwrap().len(), before);
        // No leftover import_tmp dirs.
        let working_root = dir.path().join("vimax/.working_dir");
        if working_root.exists() {
            for entry in std::fs::read_dir(&working_root).unwrap() {
                let name = entry.unwrap().file_name().to_string_lossy().to_string();
                assert!(
                    !name.starts_with(".import_tmp_"),
                    "leftover staging dir: {name}"
                );
            }
        }
    }
}
