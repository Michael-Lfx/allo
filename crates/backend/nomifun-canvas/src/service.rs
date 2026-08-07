//! Canvas project + media + generation task service (file-backed).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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

#[derive(Debug, Clone)]
pub struct InternalTask {
    pub task_id: String,
    pub status: GenerationTaskStatus,
    pub mode: String,
    pub prompt: String,
    pub model: Option<String>,
    pub aspect_ratio: Option<String>,
    pub resolution: Option<String>,
    pub duration_secs: Option<u32>,
    pub reference_media_ids: Vec<String>,
    pub first_frame_media_id: Option<String>,
    pub last_frame_media_id: Option<String>,
    pub progress: f32,
    pub error: Option<String>,
    pub result_media_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
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
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct MediaIndex {
    items: Vec<MediaIndexEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MediaIndexEntry {
    media_id: String,
    kind: String,
    title: String,
    mime: String,
    ext: String,
    bytes: u64,
    width: Option<u32>,
    height: Option<u32>,
    duration_ms: Option<u64>,
    created_at: i64,
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
    /// Hard-cancel tokens for in-flight Flowy video (and cooperative image) tasks.
    cancels: RwLock<HashMap<String, CancellationToken>>,
}

impl CanvasService {
    pub fn new(data_dir: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            data_dir,
            tasks: RwLock::new(HashMap::new()),
            cancels: RwLock::new(HashMap::new()),
        })
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    fn root(&self) -> PathBuf {
        self.data_dir.join(CANVAS_REL_DIR)
    }

    fn projects_dir(&self) -> PathBuf {
        self.root().join("projects")
    }

    fn project_dir(&self, id: &str) -> PathBuf {
        self.projects_dir().join(id)
    }

    fn media_dir(&self) -> PathBuf {
        self.root().join("media")
    }

    fn media_index_path(&self) -> PathBuf {
        self.media_dir().join("index.json")
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
        };
        let dir = self.project_dir(&id);
        ensure_dir(&dir).await?;
        write_json_file(&dir.join("meta.json"), &meta).await?;
        write_atomic(&dir.join("doc.json"), DEFAULT_DOC.as_bytes()).await?;
        info!(project_id = %id, "video-canvas project created");
        Ok(meta)
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
        Ok(())
    }

    // ── media ───────────────────────────────────────────────────────────────

    async fn load_media_index(&self) -> Result<MediaIndex, AppError> {
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

    pub async fn serve_media(&self, media_id: &str) -> Result<(String, Vec<u8>), AppError> {
        validate_project_id(media_id)?;
        let idx = self.load_media_index().await?;
        let entry = idx
            .items
            .iter()
            .find(|e| e.media_id == media_id)
            .ok_or_else(|| AppError::NotFound(format!("media {media_id}")))?
            .clone();
        let path = self.media_dir().join(format!("{}.{}", entry.media_id, entry.ext));
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|e| AppError::NotFound(format!("media file: {e}")))?;
        Ok((entry.mime, bytes))
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
        self.cancels.write().await.insert(task_id.clone(), cancel);
        self.tasks.write().await.insert(task_id.clone(), task);
        let svc = Arc::clone(self);
        tokio::spawn(async move {
            crate::generate::run_generation_task(svc, task_id).await;
        });
        Ok(view)
    }

    pub async fn task_snapshot(&self, task_id: &str) -> Option<InternalTask> {
        self.tasks.read().await.get(task_id).cloned()
    }

    pub async fn task_cancel_token(&self, task_id: &str) -> Option<CancellationToken> {
        self.cancels.read().await.get(task_id).cloned()
    }

    pub async fn get_task(&self, task_id: &str) -> Result<GenerationTaskView, AppError> {
        self.tasks
            .read()
            .await
            .get(task_id)
            .map(InternalTask::to_view)
            .ok_or_else(|| AppError::NotFound(format!("task {task_id}")))
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
    }

    pub async fn cancel_task(&self, task_id: &str) -> Result<GenerationTaskView, AppError> {
        if let Some(token) = self.cancels.read().await.get(task_id).cloned() {
            token.cancel();
        }
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
        Ok(task.to_view())
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
        let refs: Vec<&Path> = paths.iter().map(|p| p.as_path()).collect();
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

fn validate_project_id(id: &str) -> Result<(), AppError> {
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
