//! Canvas project + media + generation task service (file-backed).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use nomifun_common::{AppError, generate_id, now_ms};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::dto::{
    CanvasMediaMeta, CanvasProjectMeta, GenerationTaskStatus, GenerationTaskView,
};
use crate::fsio::{ensure_dir, read_json_file, write_atomic, write_json_file};
use crate::{CANVAS_REL_DIR, DEFAULT_DOC, MAX_DOC_BYTES, MAX_MEDIA_BYTES};

#[derive(Debug, Clone)]
pub struct ProjectWithDoc {
    pub meta: CanvasProjectMeta,
    pub doc: Value,
}

/// `HEAD /api/video-canvas/media/{id}` response metadata.
#[derive(Debug, Clone)]
pub struct MediaServeHead {
    pub mime: String,
    pub bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternalTask {
    pub task_id: String,
    pub status: GenerationTaskStatus,
    pub mode: String,
    pub prompt: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub aspect_ratio: Option<String>,
    #[serde(default)]
    pub resolution: Option<String>,
    #[serde(default)]
    pub duration_secs: Option<u32>,
    #[serde(default)]
    pub reference_media_ids: Vec<String>,
    #[serde(default)]
    pub first_frame_media_id: Option<String>,
    #[serde(default)]
    pub last_frame_media_id: Option<String>,
    #[serde(default)]
    pub progress: f32,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub result_media_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct TaskIndex {
    items: Vec<InternalTask>,
}

impl InternalTask {
    pub fn to_view(&self) -> GenerationTaskView {
        GenerationTaskView {
            task_id: self.task_id.clone(),
            status: self.status.clone(),
            mode: self.mode.clone(),
            prompt: self.prompt.clone(),
            model: self.model.clone(),
            progress: self.progress,
            error: self.error.clone(),
            result_media_id: self.result_media_id.clone(),
            aspect_ratio: self.aspect_ratio.clone(),
            resolution: self.resolution.clone(),
            duration_secs: self.duration_secs,
            reference_media_ids: self.reference_media_ids.clone(),
            first_frame_media_id: self.first_frame_media_id.clone(),
            last_frame_media_id: self.last_frame_media_id.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct MediaIndex {
    pub(crate) items: Vec<MediaIndexEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MediaIndexEntry {
    pub(crate) media_id: String,
    pub(crate) kind: String,
    pub(crate) title: String,
    pub(crate) mime: String,
    pub(crate) ext: String,
    pub(crate) bytes: u64,
    pub(crate) width: Option<u32>,
    pub(crate) height: Option<u32>,
    pub(crate) duration_ms: Option<u64>,
    pub(crate) created_at: i64,
}

/// session_id → canvas project_id for idempotent Agent→Canvas opens.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct VimaxSessionLinks {
    #[serde(default)]
    sessions: HashMap<String, String>,
}

pub struct NewGenerationRequest {
    pub mode: String,
    pub prompt: String,
    pub model: Option<String>,
    pub aspect_ratio: Option<String>,
    pub resolution: Option<String>,
    pub duration_secs: Option<u32>,
    pub reference_media_ids: Vec<String>,
    pub first_frame_media_id: Option<String>,
    pub last_frame_media_id: Option<String>,
}

pub struct CanvasService {
    data_dir: PathBuf,
    tasks: RwLock<HashMap<String, InternalTask>>,
    /// Set after `tasks.json` has been loaded (or confirmed missing).
    tasks_hydrated: AtomicBool,
    /// Hard-cancel tokens for in-flight Flowy video (and cooperative image) tasks.
    cancels: RwLock<HashMap<String, CancellationToken>>,
}

impl CanvasService {
    pub fn new(data_dir: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            data_dir,
            tasks: RwLock::new(HashMap::new()),
            tasks_hydrated: AtomicBool::new(false),
            cancels: RwLock::new(HashMap::new()),
        })
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub(crate) fn root(&self) -> PathBuf {
        self.data_dir.join(CANVAS_REL_DIR)
    }

    fn projects_dir(&self) -> PathBuf {
        self.root().join("projects")
    }

    pub(crate) fn project_dir(&self, id: &str) -> PathBuf {
        self.projects_dir().join(id)
    }

    pub(crate) fn media_dir(&self) -> PathBuf {
        self.root().join("media")
    }

    fn media_index_path(&self) -> PathBuf {
        self.media_dir().join("index.json")
    }

    fn vimax_links_path(&self) -> PathBuf {
        self.root().join("vimax_session_links.json")
    }

    fn task_index_path(&self) -> PathBuf {
        self.root().join("tasks.json")
    }

    pub async fn ensure_dirs(&self) -> Result<(), AppError> {
        ensure_dir(&self.projects_dir()).await?;
        ensure_dir(&self.media_dir()).await?;
        ensure_dir(&self.root().join("scratch")).await?;
        if !self.media_index_path().exists() {
            write_json_file(&self.media_index_path(), &MediaIndex::default()).await?;
        }
        Ok(())
    }

    // ── projects ────────────────────────────────────────────────────────────

    pub async fn list_projects(&self) -> Result<Vec<CanvasProjectMeta>, AppError> {
        self.ensure_dirs().await?;
        let mut out = Vec::new();
        let mut rd = tokio::fs::read_dir(self.projects_dir())
            .await
            .map_err(|e| AppError::Internal(format!("list projects: {e}")))?;
        while let Some(entry) = rd
            .next_entry()
            .await
            .map_err(|e| AppError::Internal(format!("read projects dir: {e}")))?
        {
            if !entry
                .file_type()
                .await
                .map(|t| t.is_dir())
                .unwrap_or(false)
            {
                continue;
            }
            let meta_path = entry.path().join("meta.json");
            if !meta_path.exists() {
                continue;
            }
            match read_json_file::<CanvasProjectMeta>(&meta_path).await {
                Ok(meta) => out.push(meta),
                Err(e) => tracing::warn!(path = %meta_path.display(), error = %e, "skip bad meta"),
            }
        }
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(out)
    }

    pub async fn create_project(&self, title: Option<String>) -> Result<CanvasProjectMeta, AppError> {
        self.ensure_dirs().await?;
        let id = generate_id();
        let now = now_ms();
        let title = title
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "未命名画布".into());
        let meta = CanvasProjectMeta {
            project_id: id.clone(),
            title,
            node_count: 0,
            created_at: now,
            updated_at: now,
            source_vimax_session_id: None,
        };
        let dir = self.project_dir(&id);
        ensure_dir(&dir).await?;
        write_json_file(&dir.join("meta.json"), &meta).await?;
        write_atomic(&dir.join("doc.json"), DEFAULT_DOC.as_bytes()).await?;
        info!(project_id = %id, "video-canvas project created");
        Ok(meta)
    }

    /// Create a canvas project bound to a ViMax session (idempotent materialize target).
    pub async fn create_project_for_vimax_session(
        &self,
        session_id: &str,
        title: Option<String>,
    ) -> Result<CanvasProjectMeta, AppError> {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return Err(AppError::BadRequest("session_id required".into()));
        }
        let mut meta = self.create_project(title).await?;
        meta.source_vimax_session_id = Some(session_id.to_string());
        meta.updated_at = now_ms();
        write_json_file(
            &self.project_dir(&meta.project_id).join("meta.json"),
            &meta,
        )
        .await?;
        self.bind_vimax_session(session_id, &meta.project_id)
            .await?;
        Ok(meta)
    }

    /// Resolve the canvas project for a ViMax session, if one still exists.
    ///
    /// Lookup order: link index → meta.source_vimax_session_id → doc.alloCreative.sessionId.
    /// When found via a fallback path, the link index is repaired.
    pub async fn find_project_for_vimax_session(
        &self,
        session_id: &str,
    ) -> Result<Option<CanvasProjectMeta>, AppError> {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return Ok(None);
        }
        self.ensure_dirs().await?;

        if let Some(project_id) = self.load_vimax_link(session_id).await? {
            if let Ok(project) = self.get_project(&project_id).await {
                return Ok(Some(project.meta));
            }
            // Stale link — drop and continue discovery.
            self.unbind_vimax_session(session_id).await?;
        }

        let mut best: Option<CanvasProjectMeta> = None;
        for meta in self.list_projects().await? {
            let linked = meta
                .source_vimax_session_id
                .as_deref()
                .is_some_and(|s| s == session_id);
            let doc_linked = if linked {
                true
            } else {
                self.project_doc_links_session(&meta.project_id, session_id)
                    .await
                    .unwrap_or(false)
            };
            if !doc_linked {
                continue;
            }
            let take = best
                .as_ref()
                .map(|b| meta.updated_at > b.updated_at)
                .unwrap_or(true);
            if take {
                best = Some(meta);
            }
        }

        if let Some(meta) = &best {
            // Repair meta + link for next time.
            if meta.source_vimax_session_id.as_deref() != Some(session_id) {
                let _ = self
                    .set_vimax_session_on_project(&meta.project_id, session_id)
                    .await;
            }
            let _ = self
                .bind_vimax_session(session_id, &meta.project_id)
                .await;
        }
        Ok(best)
    }

    async fn project_doc_links_session(
        &self,
        project_id: &str,
        session_id: &str,
    ) -> Result<bool, AppError> {
        let doc: Value = read_json_file(&self.project_dir(project_id).join("doc.json")).await?;
        let linked = doc
            .pointer("/alloCreative/sessionId")
            .and_then(|v| v.as_str())
            == Some(session_id)
            || doc
                .pointer("/alloCreative/writeBack/sessionId")
                .and_then(|v| v.as_str())
                == Some(session_id);
        Ok(linked)
    }

    pub async fn set_vimax_session_on_project(
        &self,
        project_id: &str,
        session_id: &str,
    ) -> Result<CanvasProjectMeta, AppError> {
        validate_project_id(project_id)?;
        let dir = self.project_dir(project_id);
        let mut meta: CanvasProjectMeta = read_json_file(&dir.join("meta.json")).await?;
        meta.source_vimax_session_id = Some(session_id.trim().to_string());
        meta.updated_at = now_ms();
        write_json_file(&dir.join("meta.json"), &meta).await?;
        self.bind_vimax_session(session_id, project_id).await?;
        Ok(meta)
    }

    async fn load_vimax_links(&self) -> Result<VimaxSessionLinks, AppError> {
        self.ensure_dirs().await?;
        match read_json_file::<VimaxSessionLinks>(&self.vimax_links_path()).await {
            Ok(v) => Ok(v),
            Err(_) => Ok(VimaxSessionLinks::default()),
        }
    }

    async fn save_vimax_links(&self, links: &VimaxSessionLinks) -> Result<(), AppError> {
        write_json_file(&self.vimax_links_path(), links).await
    }

    async fn load_vimax_link(&self, session_id: &str) -> Result<Option<String>, AppError> {
        let links = self.load_vimax_links().await?;
        Ok(links.sessions.get(session_id).cloned())
    }

    pub async fn bind_vimax_session(
        &self,
        session_id: &str,
        project_id: &str,
    ) -> Result<(), AppError> {
        let mut links = self.load_vimax_links().await?;
        links
            .sessions
            .insert(session_id.trim().to_string(), project_id.to_string());
        self.save_vimax_links(&links).await
    }

    pub async fn unbind_vimax_session(&self, session_id: &str) -> Result<(), AppError> {
        let mut links = self.load_vimax_links().await?;
        links.sessions.remove(session_id.trim());
        self.save_vimax_links(&links).await
    }

    pub async fn unbind_project_from_vimax_links(&self, project_id: &str) -> Result<(), AppError> {
        let mut links = self.load_vimax_links().await?;
        let before = links.sessions.len();
        links.sessions.retain(|_, pid| pid != project_id);
        if links.sessions.len() != before {
            self.save_vimax_links(&links).await?;
        }
        Ok(())
    }

    pub async fn get_project(&self, id: &str) -> Result<ProjectWithDoc, AppError> {
        self.ensure_dirs().await?;
        validate_project_id(id)?;
        let dir = self.project_dir(id);
        let meta: CanvasProjectMeta = read_json_file(&dir.join("meta.json")).await?;
        let doc: Value = read_json_file(&dir.join("doc.json")).await?;
        Ok(ProjectWithDoc { meta, doc })
    }

    pub async fn patch_project_title(
        &self,
        id: &str,
        title: String,
    ) -> Result<CanvasProjectMeta, AppError> {
        validate_project_id(id)?;
        let title = title.trim().to_string();
        if title.is_empty() {
            return Err(AppError::BadRequest("title required".into()));
        }
        let dir = self.project_dir(id);
        let mut meta: CanvasProjectMeta = read_json_file(&dir.join("meta.json")).await?;
        meta.title = title;
        meta.updated_at = now_ms();
        write_json_file(&dir.join("meta.json"), &meta).await?;
        Ok(meta)
    }

    pub async fn put_doc(&self, id: &str, doc: Value) -> Result<CanvasProjectMeta, AppError> {
        validate_project_id(id)?;
        let bytes = serde_json::to_vec(&doc)
            .map_err(|e| AppError::BadRequest(format!("invalid doc json: {e}")))?;
        if bytes.len() > MAX_DOC_BYTES {
            return Err(AppError::BadRequest(format!(
                "doc exceeds {MAX_DOC_BYTES} bytes"
            )));
        }
        let dir = self.project_dir(id);
        let mut meta: CanvasProjectMeta = read_json_file(&dir.join("meta.json")).await?;
        let node_count = doc
            .get("nodes")
            .and_then(|n| n.as_array())
            .map(|a| a.len() as u32)
            .unwrap_or(0);
        write_atomic(&dir.join("doc.json"), &bytes).await?;
        meta.node_count = node_count;
        meta.updated_at = now_ms();
        // Mirror title from doc when present.
        if let Some(t) = doc
            .get("title")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            meta.title = t.to_string();
        }
        write_json_file(&dir.join("meta.json"), &meta).await?;
        Ok(meta)
    }

    pub async fn delete_project(&self, id: &str) -> Result<(), AppError> {
        validate_project_id(id)?;
        let dir = self.project_dir(id);
        if !dir.exists() {
            return Err(AppError::NotFound(format!("project {id}")));
        }
        tokio::fs::remove_dir_all(&dir)
            .await
            .map_err(|e| AppError::Internal(format!("delete project: {e}")))?;
        let _ = self.unbind_project_from_vimax_links(id).await;
        Ok(())
    }

    // ── media ───────────────────────────────────────────────────────────────

    pub(crate) async fn load_media_index(&self) -> Result<MediaIndex, AppError> {
        self.ensure_dirs().await?;
        match read_json_file::<MediaIndex>(&self.media_index_path()).await {
            Ok(idx) => Ok(idx),
            Err(_) => Ok(MediaIndex::default()),
        }
    }

    async fn save_media_index(&self, index: &MediaIndex) -> Result<(), AppError> {
        write_json_file(&self.media_index_path(), index).await
    }

    fn entry_to_meta(entry: &MediaIndexEntry) -> CanvasMediaMeta {
        CanvasMediaMeta {
            media_id: entry.media_id.clone(),
            kind: entry.kind.clone(),
            title: entry.title.clone(),
            mime: entry.mime.clone(),
            bytes: entry.bytes,
            width: entry.width,
            height: entry.height,
            duration_ms: entry.duration_ms,
            url: format!("/api/video-canvas/media/{}", entry.media_id),
            created_at: entry.created_at,
        }
    }

    pub async fn list_media(&self) -> Result<Vec<CanvasMediaMeta>, AppError> {
        let idx = self.load_media_index().await?;
        Ok(idx.items.iter().map(Self::entry_to_meta).collect())
    }

    pub async fn ingest_generated_bytes(
        &self,
        bytes: Vec<u8>,
        kind: &str,
        mime: &str,
        ext: &str,
        title: String,
    ) -> Result<String, AppError> {
        self.store_media_bytes(bytes, kind, mime, ext, title).await
    }

    /// Copy a local file into the canvas media store (avoids double-buffering large videos).
    pub async fn ingest_local_file(
        &self,
        path: &Path,
        kind: &str,
        mime: &str,
        ext: &str,
        title: String,
    ) -> Result<CanvasMediaMeta, AppError> {
        self.ensure_dirs().await?;
        let meta = tokio::fs::metadata(path)
            .await
            .map_err(|e| AppError::BadRequest(format!("media file: {e}")))?;
        if !meta.is_file() {
            return Err(AppError::BadRequest(format!(
                "not a file: {}",
                path.display()
            )));
        }
        if meta.len() as usize > MAX_MEDIA_BYTES {
            return Err(AppError::BadRequest(format!(
                "media exceeds {MAX_MEDIA_BYTES} bytes"
            )));
        }
        let media_id = generate_id();
        let dest = self.media_dir().join(format!("{media_id}.{ext}"));
        tokio::fs::copy(path, &dest)
            .await
            .map_err(|e| AppError::Internal(format!("copy media: {e}")))?;
        let entry = MediaIndexEntry {
            media_id: media_id.clone(),
            kind: kind.to_string(),
            title,
            mime: mime.to_string(),
            ext: ext.to_string(),
            bytes: meta.len(),
            width: None,
            height: None,
            duration_ms: None,
            created_at: now_ms(),
        };
        let mut idx = self.load_media_index().await?;
        idx.items.insert(0, entry.clone());
        self.save_media_index(&idx).await?;
        Ok(Self::entry_to_meta(&entry))
    }

    pub async fn upload_media(
        &self,
        file_name: String,
        content_type: Option<String>,
        bytes: Vec<u8>,
        title: Option<String>,
    ) -> Result<CanvasMediaMeta, AppError> {
        if bytes.len() > MAX_MEDIA_BYTES {
            return Err(AppError::BadRequest(format!(
                "media exceeds {MAX_MEDIA_BYTES} bytes"
            )));
        }
        let ext = Path::new(&file_name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("bin")
            .to_ascii_lowercase();
        let mime = content_type
            .filter(|s| !s.trim().is_empty())
            .or_else(|| mime_guess::from_path(&file_name).first().map(|m| m.to_string()))
            .unwrap_or_else(|| "application/octet-stream".into());
        let kind = if mime.starts_with("video/") {
            "video"
        } else if mime.starts_with("audio/") {
            "audio"
        } else if mime.starts_with("image/") {
            "image"
        } else {
            "file"
        };
        let title = title
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| file_name.clone());
        let media_id = self
            .store_media_bytes(bytes, kind, &mime, &ext, title)
            .await?;
        let idx = self.load_media_index().await?;
        let entry = idx
            .items
            .iter()
            .find(|e| e.media_id == media_id)
            .ok_or_else(|| AppError::Internal("media index missing after store".into()))?;
        Ok(Self::entry_to_meta(entry))
    }

    async fn store_media_bytes(
        &self,
        bytes: Vec<u8>,
        kind: &str,
        mime: &str,
        ext: &str,
        title: String,
    ) -> Result<String, AppError> {
        self.ensure_dirs().await?;
        let media_id = generate_id();
        let path = self.media_dir().join(format!("{media_id}.{ext}"));
        write_atomic(&path, &bytes).await?;
        let entry = MediaIndexEntry {
            media_id: media_id.clone(),
            kind: kind.to_string(),
            title,
            mime: mime.to_string(),
            ext: ext.to_string(),
            bytes: bytes.len() as u64,
            width: None,
            height: None,
            duration_ms: None,
            created_at: now_ms(),
        };
        let mut idx = self.load_media_index().await?;
        idx.items.insert(0, entry);
        self.save_media_index(&idx).await?;
        Ok(media_id)
    }

    pub async fn media_file_path(&self, media_id: &str) -> Result<PathBuf, AppError> {
        validate_project_id(media_id)?;
        let idx = self.load_media_index().await?;
        let entry = idx
            .items
            .iter()
            .find(|e| e.media_id == media_id)
            .ok_or_else(|| AppError::NotFound(format!("media {media_id}")))?;
        let path = self.media_dir().join(format!("{}.{}", entry.media_id, entry.ext));
        if !path.exists() {
            return Err(AppError::NotFound(format!("media file {media_id}")));
        }
        Ok(path)
    }

    /// Lookup metadata for `HEAD /media/{id}`: index-only, never opens the
    /// media file. Falls back to `metadata()` when a legacy index entry has no
    /// recorded byte size.
    pub async fn head_media(&self, media_id: &str) -> Result<MediaServeHead, AppError> {
        let (mime, path, bytes) = self.media_serve_target(media_id).await?;
        let bytes = match bytes {
            Some(bytes) => bytes,
            None => tokio::fs::metadata(&path)
                .await
                .map_err(|e| AppError::NotFound(format!("media file: {e}")))?
                .len(),
        };
        Ok(MediaServeHead { mime, bytes })
    }

    /// Open the stored media file for streaming `GET /media/{id}`; returns the
    /// handle instead of reading contents so large videos never buffer in RAM.
    pub async fn open_media(
        &self,
        media_id: &str,
    ) -> Result<(String, u64, tokio::fs::File), AppError> {
        let (mime, path, bytes) = self.media_serve_target(media_id).await?;
        let file = tokio::fs::File::open(&path)
            .await
            .map_err(|e| AppError::NotFound(format!("media file: {e}")))?;
        let bytes = match bytes {
            Some(bytes) => bytes,
            None => file.metadata().await.map_err(|e| AppError::NotFound(format!("media file: {e}")))?.len(),
        };
        Ok((mime, bytes, file))
    }

    /// Resolve `{mime, path, indexed_bytes}` from the media index.
    async fn media_serve_target(
        &self,
        media_id: &str,
    ) -> Result<(String, std::path::PathBuf, Option<u64>), AppError> {
        validate_project_id(media_id)?;
        let idx = self.load_media_index().await?;
        let entry = idx
            .items
            .iter()
            .find(|e| e.media_id == media_id)
            .ok_or_else(|| AppError::NotFound(format!("media {media_id}")))?;
        let path = self.media_dir().join(format!("{}.{}", entry.media_id, entry.ext));
        // 0 means legacy entry without a recorded size — callers fall back to
        // `metadata()` so Content-Length stays correct.
        let bytes = (entry.bytes > 0).then_some(entry.bytes);
        Ok((entry.mime.clone(), path, bytes))
    }

    pub async fn delete_media(&self, media_id: &str) -> Result<(), AppError> {
        validate_project_id(media_id)?;
        let mut idx = self.load_media_index().await?;
        let Some(pos) = idx.items.iter().position(|e| e.media_id == media_id) else {
            return Err(AppError::NotFound(format!("media {media_id}")));
        };
        let entry = idx.items.remove(pos);
        let path = self.media_dir().join(format!("{}.{}", entry.media_id, entry.ext));
        let _ = tokio::fs::remove_file(&path).await;
        self.save_media_index(&idx).await?;
        Ok(())
    }

    // ── generation tasks ────────────────────────────────────────────────────

    async fn write_task_index(&self, tasks: &HashMap<String, InternalTask>) -> Result<(), AppError> {
        let index = TaskIndex {
            items: tasks.values().cloned().collect(),
        };
        write_json_file(&self.task_index_path(), &index).await
    }

    async fn persist_tasks(&self) {
        let snapshot = self.tasks.read().await.clone();
        if let Err(error) = self.write_task_index(&snapshot).await {
            tracing::warn!(error = %error, "failed to persist generation tasks");
        }
    }

    /// Load `{data_dir}/video-canvas/tasks.json` once. In-flight jobs cannot
    /// resume after process exit, so queued/running rows become failed history.
    async fn hydrate_tasks(&self) {
        if self.tasks_hydrated.load(Ordering::Acquire) {
            return;
        }
        if let Err(error) = self.ensure_dirs().await {
            tracing::warn!(error = %error, "ensure canvas dirs before hydrating tasks");
        }
        let mut guard = self.tasks.write().await;
        if self.tasks_hydrated.load(Ordering::Acquire) {
            return;
        }
        let path = self.task_index_path();
        let mut rewritten = false;
        if path.exists() {
            match read_json_file::<TaskIndex>(&path).await {
                Ok(index) => {
                    let now = now_ms();
                    for mut task in index.items {
                        if matches!(
                            task.status,
                            GenerationTaskStatus::Queued | GenerationTaskStatus::Running
                        ) {
                            task.status = GenerationTaskStatus::Failed;
                            task.progress = 1.0;
                            task.error = Some(
                                "Interrupted when the app exited. The history entry was kept."
                                    .into(),
                            );
                            task.updated_at = now;
                            rewritten = true;
                        }
                        guard.insert(task.task_id.clone(), task);
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %error,
                        "failed to load generation task index"
                    );
                    self.tasks_hydrated.store(true, Ordering::Release);
                    return;
                }
            }
        }
        let snapshot = rewritten.then(|| guard.clone());
        self.tasks_hydrated.store(true, Ordering::Release);
        drop(guard);
        if let Some(snapshot) = snapshot {
            if let Err(error) = self.write_task_index(&snapshot).await {
                tracing::warn!(error = %error, "failed to persist interrupted generation tasks");
            }
        }
    }

    pub async fn create_generation_task(
        self: &Arc<Self>,
        req: NewGenerationRequest,
    ) -> Result<GenerationTaskView, AppError> {
        let prompt = req.prompt.trim().to_string();
        if prompt.is_empty() {
            return Err(AppError::BadRequest("prompt required".into()));
        }
        let mode = req.mode.trim().to_ascii_lowercase();
        if !matches!(
            mode.as_str(),
            "image" | "video" | "t2i" | "i2i" | "t2v" | "i2v"
        ) {
            return Err(AppError::BadRequest(format!(
                "unsupported mode '{mode}' (expected image|video|t2i|i2i|t2v|i2v)"
            )));
        }
        for id in req
            .reference_media_ids
            .iter()
            .chain(req.first_frame_media_id.iter())
            .chain(req.last_frame_media_id.iter())
        {
            let _ = self.media_file_path(id).await?;
        }
        let now = now_ms();
        let task_id = generate_id();
        let task = InternalTask {
            task_id: task_id.clone(),
            status: GenerationTaskStatus::Queued,
            mode,
            prompt,
            model: req.model.filter(|s| !s.trim().is_empty()),
            aspect_ratio: req.aspect_ratio.filter(|s| !s.trim().is_empty()),
            resolution: req.resolution.filter(|s| !s.trim().is_empty()),
            duration_secs: req.duration_secs,
            reference_media_ids: req.reference_media_ids,
            first_frame_media_id: req.first_frame_media_id,
            last_frame_media_id: req.last_frame_media_id,
            progress: 0.0,
            error: None,
            result_media_id: None,
            created_at: now,
            updated_at: now,
        };
        let view = task.to_view();
        let cancel = CancellationToken::new();
        self.hydrate_tasks().await;
        self.cancels.write().await.insert(task_id.clone(), cancel);
        self.tasks.write().await.insert(task_id.clone(), task);
        self.persist_tasks().await;
        let svc = Arc::clone(self);
        tokio::spawn(async move {
            crate::generate::run_generation_task(svc, task_id).await;
        });
        Ok(view)
    }

    pub async fn task_snapshot(&self, task_id: &str) -> Option<InternalTask> {
        self.hydrate_tasks().await;
        self.tasks.read().await.get(task_id).cloned()
    }

    pub async fn task_cancel_token(&self, task_id: &str) -> Option<CancellationToken> {
        self.cancels.read().await.get(task_id).cloned()
    }

    pub async fn get_task(&self, task_id: &str) -> Result<GenerationTaskView, AppError> {
        self.hydrate_tasks().await;
        self.tasks
            .read()
            .await
            .get(task_id)
            .map(InternalTask::to_view)
            .ok_or_else(|| AppError::NotFound(format!("task {task_id}")))
    }

    /// List generation tasks ordered by most-recently-updated first.
    ///
    /// `limit` and `offset` paginate a stable, descending sort so callers can
    /// render the first page immediately and stream older items on demand.
    pub async fn list_tasks(
        &self,
        limit: usize,
        offset: usize,
    ) -> Vec<GenerationTaskView> {
        self.hydrate_tasks().await;
        let guard = self.tasks.read().await;
        let mut tasks: Vec<&InternalTask> = guard.values().collect();
        tasks.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        tasks
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(InternalTask::to_view)
            .collect()
    }

    /// Total count of persisted generation tasks. Used by the UI to
    /// decide whether more pages can be loaded.
    pub async fn task_count(&self) -> usize {
        self.hydrate_tasks().await;
        self.tasks.read().await.len()
    }

    /// Drop a finished task from the persisted index. The result media file
    /// (if any) is intentionally kept on disk so the URL stays resolvable for
    /// callers that already cached it.
    pub async fn delete_task(&self, task_id: &str) -> Result<(), AppError> {
        self.hydrate_tasks().await;
        let snapshot = {
            let mut guard = self.tasks.write().await;
            guard
                .remove(task_id)
                .ok_or_else(|| AppError::NotFound(format!("task {task_id}")))?;
            guard.clone()
        };
        self.cancels.write().await.remove(task_id);
        self.write_task_index(&snapshot).await?;
        Ok(())
    }

    pub async fn set_task_status(
        &self,
        task_id: &str,
        status: GenerationTaskStatus,
        progress: f32,
        error: Option<String>,
        result_media_id: Option<String>,
    ) {
        let terminal = matches!(
            status,
            GenerationTaskStatus::Succeeded
                | GenerationTaskStatus::Failed
                | GenerationTaskStatus::Canceled
        );
        {
            let mut guard = self.tasks.write().await;
            if let Some(task) = guard.get_mut(task_id) {
                task.status = status;
                task.progress = progress;
                task.error = error;
                if result_media_id.is_some() {
                    task.result_media_id = result_media_id;
                }
                task.updated_at = now_ms();
            }
        }
        if terminal {
            self.cancels.write().await.remove(task_id);
        }
        self.persist_tasks().await;
    }

    pub async fn cancel_task(&self, task_id: &str) -> Result<GenerationTaskView, AppError> {
        self.hydrate_tasks().await;
        if let Some(token) = self.cancels.read().await.get(task_id).cloned() {
            token.cancel();
        }
        let (view, snapshot) = {
            let mut guard = self.tasks.write().await;
            let task = guard
                .get_mut(task_id)
                .ok_or_else(|| AppError::NotFound(format!("task {task_id}")))?;
            match task.status {
                GenerationTaskStatus::Succeeded
                | GenerationTaskStatus::Failed
                | GenerationTaskStatus::Canceled => {}
                _ => {
                    task.status = GenerationTaskStatus::Canceled;
                    task.updated_at = now_ms();
                }
            }
            (task.to_view(), guard.clone())
        };
        self.write_task_index(&snapshot).await?;
        Ok(view)
    }

    /// Concatenate video media clips (order preserved) via local ffmpeg.
    pub async fn concat_media(
        &self,
        media_ids: Vec<String>,
        title: Option<String>,
    ) -> Result<CanvasMediaMeta, AppError> {
        if media_ids.len() < 2 {
            return Err(AppError::BadRequest(
                "concat requires at least 2 video media_ids".into(),
            ));
        }
        let idx = self.load_media_index().await?;
        let mut paths = Vec::with_capacity(media_ids.len());
        for id in &media_ids {
            let entry = idx
                .items
                .iter()
                .find(|e| e.media_id == *id)
                .ok_or_else(|| AppError::NotFound(format!("media {id}")))?;
            if entry.kind != "video" {
                return Err(AppError::BadRequest(format!(
                    "media {id} is not a video (kind={})",
                    entry.kind
                )));
            }
            paths.push(self.media_file_path(id).await?);
        }
        let out_path = self
            .root()
            .join("scratch")
            .join(format!("concat-{}.mp4", generate_id()));
        crate::fsio::ensure_dir(
            out_path
                .parent()
                .ok_or_else(|| AppError::Internal("scratch parent missing".into()))?,
        )
        .await?;
        // User-picked clips are unrelated takes, so every join is a real cut.
        let refs: Vec<nomi_vimax::media_local::ConcatClip<'_>> = paths
            .iter()
            .map(|p| nomi_vimax::media_local::ConcatClip::cut(p.as_path()))
            .collect();
        nomi_vimax::media_local::concat_videos(&refs, &out_path)
            .await
            .map_err(|e| AppError::Internal(format!("ffmpeg concat: {e}")))?;
        let bytes = tokio::fs::read(&out_path)
            .await
            .map_err(|e| AppError::Internal(format!("read concat output: {e}")))?;
        let _ = tokio::fs::remove_file(&out_path).await;
        let title = title
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "concat".into());
        let media_id = self
            .store_media_bytes(bytes, "video", "video/mp4", "mp4", title)
            .await?;
        let idx = self.load_media_index().await?;
        let entry = idx
            .items
            .iter()
            .find(|e| e.media_id == media_id)
            .ok_or_else(|| AppError::Internal("media index missing after concat".into()))?;
        Ok(Self::entry_to_meta(entry))
    }
}

pub(crate) fn validate_project_id(id: &str) -> Result<(), AppError> {
    if id.is_empty()
        || id.contains("..")
        || id.contains('/')
        || id.contains('\\')
        || id.contains('\0')
    {
        return Err(AppError::BadRequest("invalid id".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use tokio::io::AsyncReadExt as _;

    use super::*;

    async fn service_with_media(
        bytes: &[u8],
        kind: &str,
        mime: &str,
        ext: &str,
    ) -> (Arc<CanvasService>, String) {
        let dir = tempfile::tempdir().expect("temp dir");
        let service = CanvasService::new(dir.path().to_path_buf());
        let media_id = service
            .ingest_generated_bytes(bytes.to_vec(), kind, mime, ext, "test".into())
            .await
            .expect("ingest");
        std::mem::forget(dir);
        (service, media_id)
    }

    #[tokio::test]
    async fn head_media_reads_only_the_index() {
        let (service, media_id) = service_with_media(b"0123456789", "video", "video/mp4", "mp4").await;
        // Simulate an unreadable/absent content file: HEAD must still succeed
        // because it serves the recorded index metadata without touching disk.
        let file = service.media_dir().join(format!("{media_id}.mp4"));
        std::fs::remove_file(&file).expect("remove media file");

        let head = service.head_media(&media_id).await.expect("head from index");
        assert_eq!(head.mime, "video/mp4");
        assert_eq!(head.bytes, 10);
        assert!(service.head_media("missing-id").await.is_err());
    }

    #[tokio::test]
    async fn head_media_falls_back_to_metadata_for_legacy_zero_bytes() {
        let (service, media_id) = service_with_media(b"abc", "image", "image/png", "png").await;
        // Legacy index entries recorded 0 bytes; the file still exists, so the
        // size must come from `metadata()` without reading file contents.
        let idx_path = service.media_index_path();
        let mut idx = service.load_media_index().await.expect("index");
        for entry in &mut idx.items {
            if entry.media_id == media_id {
                entry.bytes = 0;
            }
        }
        service.save_media_index(&idx).await.expect("save index");
        let head = service.head_media(&media_id).await.expect("head");
        assert_eq!(head.mime, "image/png");
        assert_eq!(head.bytes, 3);
        let _ = idx_path;
    }

    #[tokio::test]
    async fn open_media_streams_identical_bytes() {
        let payload = b"stream-me-please-1234567890";
        let (service, media_id) =
            service_with_media(payload, "video", "video/mp4", "mp4").await;

        let (mime, bytes, mut file) = service.open_media(&media_id).await.expect("open");
        assert_eq!(mime, "video/mp4");
        assert_eq!(bytes, payload.len() as u64);

        let mut got = Vec::new();
        file.read_to_end(&mut got).await.expect("read stream");
        assert_eq!(got, payload);
    }

    fn sample_task(id: &str, status: GenerationTaskStatus) -> InternalTask {
        InternalTask {
            task_id: id.into(),
            status,
            mode: "video".into(),
            prompt: "a clip".into(),
            model: None,
            aspect_ratio: Some("16:9".into()),
            resolution: Some("720p".into()),
            duration_secs: Some(5),
            reference_media_ids: vec![],
            first_frame_media_id: None,
            last_frame_media_id: None,
            progress: 1.0,
            error: None,
            result_media_id: Some("media-1".into()),
            created_at: 1,
            updated_at: 2,
        }
    }

    #[tokio::test]
    async fn generation_tasks_reload_from_disk() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().to_path_buf();
        let first = CanvasService::new(path.clone());
        first
            .tasks
            .write()
            .await
            .insert("t1".into(), sample_task("t1", GenerationTaskStatus::Succeeded));
        first.persist_tasks().await;

        let second = CanvasService::new(path);
        let listed = second.list_tasks(10, 0).await;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].task_id, "t1");
        assert_eq!(listed[0].status, GenerationTaskStatus::Succeeded);
        assert_eq!(listed[0].result_media_id.as_deref(), Some("media-1"));
        assert_eq!(listed[0].duration_secs, Some(5));
    }

    #[tokio::test]
    async fn in_flight_generation_tasks_fail_on_reload() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().to_path_buf();
        let first = CanvasService::new(path.clone());
        let mut running = sample_task("t2", GenerationTaskStatus::Running);
        running.progress = 0.4;
        running.result_media_id = None;
        first.tasks.write().await.insert("t2".into(), running);
        first.persist_tasks().await;

        let second = CanvasService::new(path);
        let listed = second.list_tasks(10, 0).await;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].status, GenerationTaskStatus::Failed);
        assert!(listed[0].error.as_ref().is_some());
    }
}
