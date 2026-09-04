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

/// Sidecar under `{working_dir}/` so Agent session history + live credits survive
/// process restart. Hidden from the artifact tree.
pub const RUN_STATUS_FILENAME: &str = "run_status.json";

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
    /// Sum of Flowy `credits_consumed` from video generation tasks in this session.
    #[serde(default)]
    pub credits_consumed: i64,
    /// Local Flowy video-task ids already folded into [`Self::credits_consumed`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub billed_video_task_ids: Vec<i64>,
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
    /// Sum of Flowy video-task credits billed for this project.
    #[serde(default, skip_serializing_if = "is_zero_i64")]
    pub credits_consumed: i64,
    pub created_at: String,
    pub updated_at: String,
}

fn is_zero_i64(v: &i64) -> bool {
    *v == 0
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
            credits_consumed: record.credits_consumed,
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
            credits_consumed: 0,
            billed_video_task_ids: Vec::new(),
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

    fn run_status_path(&self, session_id: &str) -> VimaxResult<PathBuf> {
        Ok(self.working_dir(session_id)?.join(RUN_STATUS_FILENAME))
    }

    /// Load the persisted pipeline log for a session (empty after first create).
    pub fn load_run_status(&self, session_id: &str) -> Option<RenderStatus> {
        let path = self.run_status_path(session_id).ok()?;
        let raw = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&raw).ok()
    }

    /// Atomically write the pipeline log next to session artifacts.
    pub fn save_run_status(&self, session_id: &str, status: &RenderStatus) -> VimaxResult<()> {
        let path = self.run_status_path(session_id)?;
        atomic_write_json(&path, status)
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
        if name == RUN_STATUS_FILENAME || name == "run_status.json.tmp" {
            continue;
        }
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
        "gif" => Some("image/gif".into()),
        "bmp" => Some("image/bmp".into()),
        "mp4" => Some("video/mp4".into()),
        "webm" => Some("video/webm".into()),
        "mov" => Some("video/quicktime".into()),
        "wav" => Some("audio/wav".into()),
        "mp3" => Some("audio/mpeg".into()),
        "m4a" => Some("audio/mp4".into()),
        "aac" => Some("audio/aac".into()),
        "ogg" | "oga" => Some("audio/ogg".into()),
        "flac" => Some("audio/flac".into()),
        "opus" => Some("audio/opus".into()),
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

fn unique_part_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("artifact.json");
    path.with_file_name(format!("{name}.{}.part", Uuid::new_v4().simple()))
}

/// Persist JSON via a unique temp file + rename so concurrent readers never
/// observe a truncated empty body (`EOF while parsing a value at line 1 column 0`).
pub async fn write_json_artifact<T: Serialize>(path: &Path, value: &T) -> VimaxResult<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let raw = serde_json::to_string_pretty(value)?;
    let tmp = unique_part_path(path);
    tokio::fs::write(&tmp, &raw).await?;
    // Unix `rename` replaces the dest atomically. Windows cannot replace, so only
    // then remove+rename — readers retry `NotFound` across that tiny window.
    match tokio::fs::rename(&tmp, path).await {
        Ok(()) => Ok(()),
        Err(_) if path.exists() => {
            let _ = tokio::fs::remove_file(path).await;
            if let Err(e) = tokio::fs::rename(&tmp, path).await {
                let _ = tokio::fs::remove_file(&tmp).await;
                return Err(e.into());
            }
            Ok(())
        }
        Err(e) => {
            let _ = tokio::fs::remove_file(&tmp).await;
            Err(e.into())
        }
    }
}

pub async fn read_json_artifact<T: for<'de> Deserialize<'de>>(path: &Path) -> VimaxResult<T> {
    const ATTEMPTS: u32 = 6;
    let mut last_err: Option<String> = None;
    for attempt in 1..=ATTEMPTS {
        let raw = match tokio::fs::read_to_string(path).await {
            Ok(raw) => raw,
            Err(e)
                if attempt < ATTEMPTS
                    && (e.kind() == std::io::ErrorKind::NotFound
                        || e.kind() == std::io::ErrorKind::Interrupted) =>
            {
                last_err = Some(format!("JSON artifact missing at {}: {e}", path.display()));
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            last_err = Some(format!(
                "empty JSON artifact at {} (interrupted write or concurrent planner)",
                path.display()
            ));
            if attempt < ATTEMPTS {
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                continue;
            }
            break;
        }
        match serde_json::from_str(trimmed) {
            Ok(v) => return Ok(v),
            Err(e) if attempt < ATTEMPTS && e.is_eof() => {
                last_err = Some(format!("JSON error at {}: {e}", path.display()));
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
            Err(e) => {
                return Err(VimaxError::msg(format!(
                    "JSON error at {}: {e}",
                    path.display()
                )));
            }
        }
    }
    Err(VimaxError::msg(last_err.unwrap_or_else(|| {
        format!("unreadable JSON artifact at {}", path.display())
    })))
}

/// Copy JSON only when the source parses. Destination is written atomically so
/// concurrent readers never see a truncated empty file.
pub async fn copy_json_artifact_if_readable(src: &Path, dest: &Path) -> VimaxResult<bool> {
    if !src.is_file() {
        return Ok(false);
    }
    if tokio::fs::metadata(src)
        .await
        .map(|m| m.len() == 0)
        .unwrap_or(false)
    {
        return Ok(false);
    }
    match read_json_artifact::<Value>(src).await {
        Ok(value) => {
            write_json_artifact(dest, &value).await?;
            Ok(true)
        }
        Err(e) => {
            tracing::warn!(
                src = %src.display(),
                dest = %dest.display(),
                error = %e,
                "skipping copy of unreadable JSON artifact"
            );
            Ok(false)
        }
    }
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
    if status.credits_consumed > record.credits_consumed {
        record.credits_consumed = status.credits_consumed;
    }
}

/// Fold a Flowy video-task credit into the session ledger (idempotent per task id).
///
/// Returns `true` when the session total changed.
pub fn apply_video_task_credits(
    record: &mut SessionRecord,
    task_id: i64,
    credits: i64,
) -> bool {
    if task_id <= 0 || credits <= 0 {
        return false;
    }
    if record.billed_video_task_ids.contains(&task_id) {
        return false;
    }
    record.billed_video_task_ids.push(task_id);
    record.credits_consumed = record.credits_consumed.saturating_add(credits);
    true
}

/// Credits on `video_poll` heartbeats are in-flight snapshots and must not hit
/// the ledger — only the terminal `video_credits` event is authoritative.
pub fn video_task_credit_delta(
    stage: &str,
    meta: Option<&Value>,
) -> Option<(i64, i64)> {
    if stage != "video_credits" {
        return None;
    }
    let m = meta?;
    let credits = m.get("credits_consumed")?.as_i64().filter(|c| *c > 0)?;
    let task_id = m.get("task_id")?.as_i64().filter(|t| *t > 0)?;
    Some((task_id, credits))
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

    #[test]
    fn apply_video_task_credits_is_idempotent_per_task() {
        let mut record = SessionRecord {
            session_id: "s1".into(),
            working_dir: ".working_dir/s1".into(),
            title: "t".into(),
            workflow: WorkflowKind::Idea2Video,
            idea: String::new(),
            script: String::new(),
            novel_text: String::new(),
            user_requirement: String::new(),
            style: String::new(),
            vertical_skill_ids: vec![],
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
            stale: Default::default(),
            final_video: None,
            cover: None,
            credits_consumed: 0,
            billed_video_task_ids: vec![],
            created_at: String::new(),
            updated_at: String::new(),
        };
        assert!(apply_video_task_credits(&mut record, 10, 5200));
        assert_eq!(record.credits_consumed, 5200);
        assert!(!apply_video_task_credits(&mut record, 10, 5200));
        assert_eq!(record.credits_consumed, 5200);
        assert!(apply_video_task_credits(&mut record, 11, 800));
        assert_eq!(record.credits_consumed, 6000);
        assert!(!apply_video_task_credits(&mut record, 12, 0));
        assert_eq!(record.credits_consumed, 6000);
    }

    #[test]
    fn video_poll_heartbeats_do_not_enter_the_credit_ledger() {
        let poll = serde_json::json!({"task_id": 10, "credits_consumed": 5200});
        assert!(video_task_credit_delta("video_poll", Some(&poll)).is_none());
        assert_eq!(
            video_task_credit_delta("video_credits", Some(&poll)),
            Some((10, 5200))
        );
    }

    #[test]
    fn run_status_sidecar_roundtrips_events_and_credits() {
        let dir = tempdir().unwrap();
        let index = SessionIndex::open(dir.path()).unwrap();
        let record = index
            .create(WorkflowKind::Script2Video, Some("t".into()))
            .unwrap();
        let mut status = RenderStatus::default();
        status.status = RunStatus::Succeeded;
        status.credits_consumed = 5200;
        status.emit("extract_characters", "ok", None);
        status.emit("character_portraits_done", "ok", None);
        status.emit("planned", "ok", None);
        index.save_run_status(&record.session_id, &status).unwrap();
        let loaded = index.load_run_status(&record.session_id).unwrap();
        assert_eq!(loaded.credits_consumed, 5200);
        assert_eq!(loaded.events.len(), 3);
        assert_eq!(loaded.events[0].stage, "extract_characters");
        let tree = index.list_artifacts(&record.session_id).unwrap();
        let names: Vec<_> = tree.iter().map(|n| n.name.as_str()).collect();
        assert!(!names.contains(&RUN_STATUS_FILENAME));
    }

    #[tokio::test]
    async fn write_json_artifact_is_never_empty_on_disk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("characters.json");
        write_json_artifact(&path, &serde_json::json!({"ok": true}))
            .await
            .unwrap();
        let raw = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(!raw.trim().is_empty());
        let value: serde_json::Value = read_json_artifact(&path).await.unwrap();
        assert_eq!(value["ok"], true);
    }

    #[tokio::test]
    async fn read_json_artifact_rejects_empty_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("empty.json");
        tokio::fs::write(&path, "").await.unwrap();
        let err = read_json_artifact::<serde_json::Value>(&path)
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("empty JSON artifact"), "{msg}");
        assert!(
            !msg.contains("EOF while parsing a value at line 1 column 0"),
            "{msg}"
        );
    }

    #[tokio::test]
    async fn copy_json_artifact_skips_empty_source() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("empty.json");
        let dest = dir.path().join("dest.json");
        tokio::fs::write(&src, "").await.unwrap();
        assert!(!copy_json_artifact_if_readable(&src, &dest).await.unwrap());
        assert!(!dest.exists());
    }

    #[tokio::test]
    async fn json_artifact_readers_never_see_empty_during_overwrite() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let dir = tempdir().unwrap();
        let path = dir.path().join("shared.json");
        write_json_artifact(&path, &serde_json::json!({"n": 0}))
            .await
            .unwrap();
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let path_w = path.clone();
        let writer = tokio::spawn(async move {
            for i in 1..=30 {
                write_json_artifact(&path_w, &serde_json::json!({ "n": i }))
                    .await
                    .unwrap();
            }
        });
        let mut readers = Vec::new();
        for _ in 0..4 {
            let path = path.clone();
            let stop = std::sync::Arc::clone(&stop);
            readers.push(tokio::spawn(async move {
                while !stop.load(Ordering::Relaxed) {
                    let value: serde_json::Value = read_json_artifact(&path).await.unwrap();
                    assert!(value.get("n").is_some(), "{value}");
                    tokio::task::yield_now().await;
                }
            }));
        }
        writer.await.unwrap();
        stop.store(true, Ordering::Relaxed);
        for reader in readers {
            reader.await.unwrap();
        }
    }
}
