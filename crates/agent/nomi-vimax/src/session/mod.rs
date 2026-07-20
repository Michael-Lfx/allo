//! Session index — `{data_dir}/vimax/.vimax/sessions.json` + `.working_dir/<id>/`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::domain::WorkflowKind;
use crate::error::{VimaxError, VimaxResult};
use crate::progress::{RenderStatus, RunStatus};

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
    /// Flowy chat / LLM model id (e.g. `AIPC-glm-4.7`). Empty → server default.
    #[serde(default)]
    pub llm_model: String,
    /// Flowy image model id. Empty → media settings / catalog first.
    #[serde(default)]
    pub image_model: String,
    /// Flowy video model id. Empty → media settings / catalog first.
    #[serde(default)]
    pub video_model: String,
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
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
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
pub struct SessionIndex {
    workspace_root: PathBuf,
    lock: Mutex<()>,
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
            lock: Mutex::new(()),
        })
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
            Err(_) => {
                let backup = path.with_extension(format!(
                    "json.corrupt-{}",
                    chrono::Local::now().format("%Y%m%d-%H%M%S")
                ));
                let _ = std::fs::rename(&path, &backup);
                Ok(SessionsFile::default())
            }
        }
    }

    fn save(&self, data: &SessionsFile) -> VimaxResult<()> {
        atomic_write_json(&self.sessions_path(), data)
    }

    pub fn list(&self) -> VimaxResult<Vec<SessionRecord>> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let data = self.load()?;
        let mut sessions: Vec<_> = data.sessions.into_values().collect();
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
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
            llm_model: String::new(),
            image_model: String::new(),
            video_model: String::new(),
            stage: "created".into(),
            summary: String::new(),
            status: RunStatus::Idle,
            stale: STALE_KEYS.iter().map(|k| ((*k).to_string(), false)).collect(),
            final_video: None,
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
}

/// Convenience: empty metadata object for progress events.
pub fn meta_json(map: impl IntoIterator<Item = (&'static str, Value)>) -> Option<Value> {
    let mut obj = serde_json::Map::new();
    for (k, v) in map {
        obj.insert(k.into(), v);
    }
    Some(Value::Object(obj))
}
