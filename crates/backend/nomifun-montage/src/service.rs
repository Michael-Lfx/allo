//! Thin wrapper around [`nomi_montage::MontageService`] with GatewayConfig media reload.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use nomi_config::{GatewayConfig, config_yaml_path, load_user_config_file};
use nomi_media_backends::FlowyMediaServices;
use nomi_montage::orchestrator::{ApprovalRequest, ProviderMenu};
use nomi_montage::pipeline::{PipelineManifest, PipelineSummary};
use nomi_montage::project::{BoardState, CreateProjectRequest, ProjectRecord};
use nomi_montage::service::{ProjectDetail, RunStatus};
use nomi_montage::{MontageError, MontageService, creative::CreativeFilm, events::EventRecord};
use nomifun_common::AppError;

pub struct MontageApiService {
    data_dir: PathBuf,
    inner: Arc<MontageService>,
}

impl MontageApiService {
    pub fn new(data_dir: PathBuf) -> Result<Self, AppError> {
        let cfg: GatewayConfig = load_user_config_file(&config_yaml_path(Some(&data_dir)))
            .map_err(|e| AppError::Internal(format!("failed to load config: {e}")))?;
        let inner = MontageService::try_new(&cfg, &data_dir).ok_or_else(|| {
            AppError::Internal("montage runtime unavailable (failed to load embedded assets)".into())
        })?;
        Ok(Self {
            data_dir,
            inner: Arc::new(inner),
        })
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn inner(&self) -> Arc<MontageService> {
        Arc::clone(&self.inner)
    }

    /// Rebind Flowy media after sign-in / config reload (mirrors former vimax refresh).
    pub fn set_media_from_config(&self) {
        let media = load_flowy(&self.data_dir);
        self.inner.set_media(media);
    }

    pub fn refresh_media(&self) {
        self.set_media_from_config();
    }

    pub fn list_pipelines(&self) -> Vec<PipelineSummary> {
        self.inner.list_pipelines()
    }

    pub fn get_pipeline(&self, name: &str) -> Result<PipelineManifest, AppError> {
        self.inner.get_pipeline(name).map_err(map_montage_err)
    }

    pub fn provider_menu(&self) -> ProviderMenu {
        self.refresh_media();
        self.inner.provider_menu()
    }

    pub async fn list_projects(&self) -> Result<Vec<ProjectRecord>, AppError> {
        self.inner.list_projects().await.map_err(map_montage_err)
    }

    pub async fn create_project(
        &self,
        req: CreateProjectRequest,
    ) -> Result<ProjectRecord, AppError> {
        self.inner
            .create_project(req)
            .await
            .map_err(map_montage_err)
    }

    pub async fn get_project(&self, id: &str) -> Result<ProjectDetail, AppError> {
        self.inner.get_project(id).await.map_err(map_montage_err)
    }

    pub async fn delete_project(&self, id: &str) -> Result<(), AppError> {
        self.inner.delete_project(id).await.map_err(map_montage_err)
    }

    pub async fn import_project(&self, archive_path: &Path) -> Result<ProjectRecord, AppError> {
        self.inner
            .import_project(archive_path)
            .await
            .map_err(map_montage_err)
    }

    pub async fn start(&self, id: &str) -> Result<(), AppError> {
        self.refresh_media();
        self.inner.start(id).await.map_err(map_montage_err)
    }

    pub async fn cancel(&self, id: &str) -> Result<(), AppError> {
        self.inner.cancel(id).await.map_err(map_montage_err)
    }

    pub async fn status(&self, id: &str) -> Result<RunStatus, AppError> {
        self.inner.status(id).await.map_err(map_montage_err)
    }

    pub async fn board_state(&self, id: &str) -> Result<BoardState, AppError> {
        self.inner.board_state(id).await.map_err(map_montage_err)
    }

    pub fn recent_events(&self, id: &str, limit: usize) -> Result<Vec<EventRecord>, AppError> {
        self.inner
            .recent_events(id, limit)
            .map_err(map_montage_err)
    }

    pub fn list_artifacts(&self, id: &str) -> Result<Vec<String>, AppError> {
        self.inner.list_artifacts(id).map_err(map_montage_err)
    }

    pub async fn get_artifact(
        &self,
        id: &str,
        name: &str,
    ) -> Result<serde_json::Value, AppError> {
        self.inner
            .get_artifact(id, name)
            .await
            .map_err(map_montage_err)
    }

    pub async fn put_artifact(
        &self,
        id: &str,
        name: &str,
        value: serde_json::Value,
    ) -> Result<(), AppError> {
        self.inner
            .put_artifact(id, name, value)
            .await
            .map_err(map_montage_err)
    }

    pub async fn approve(
        &self,
        id: &str,
        request: ApprovalRequest,
    ) -> Result<BoardState, AppError> {
        self.refresh_media();
        self.inner
            .approve(id, request)
            .await
            .map_err(map_montage_err)
    }

    pub async fn export_zip(&self, id: &str, dest_path: &Path) -> Result<PathBuf, AppError> {
        self.inner
            .export_zip(id, dest_path)
            .await
            .map_err(map_montage_err)
    }

    pub async fn scan_creative_film(&self, id: &str) -> Result<CreativeFilm, AppError> {
        self.inner
            .scan_creative_film(id)
            .await
            .map_err(map_montage_err)
    }

    pub fn project_root(&self, id: &str) -> Result<PathBuf, AppError> {
        Ok(self
            .inner
            .project_paths(id)
            .map_err(map_montage_err)?
            .root)
    }

    /// Resolve a relative path under the project root with canonicalize containment.
    /// Rejects absolute paths, empty segments, and `..` escapes.
    pub fn resolve_project_file(&self, id: &str, rel_path: &str) -> Result<PathBuf, AppError> {
        let rel = rel_path.trim().trim_start_matches('/').trim_start_matches('\\');
        if rel.is_empty() {
            return Err(AppError::BadRequest("path is required".into()));
        }
        let rel_pathbuf = PathBuf::from(rel);
        if rel_pathbuf.is_absolute() {
            return Err(AppError::BadRequest("absolute paths are not allowed".into()));
        }
        for component in rel_pathbuf.components() {
            match component {
                std::path::Component::Normal(_) => {}
                std::path::Component::CurDir => {}
                _ => {
                    return Err(AppError::BadRequest(
                        "path must stay under the project root".into(),
                    ));
                }
            }
        }

        let root = self.project_root(id)?;
        let root_canon = root.canonicalize().map_err(|e| {
            AppError::NotFound(format!("project root missing for {id}: {e}"))
        })?;
        let candidate = root.join(&rel_pathbuf);
        let file_canon = candidate.canonicalize().map_err(|_| {
            AppError::NotFound(format!("file not found: {rel}"))
        })?;
        if !file_canon.starts_with(&root_canon) {
            return Err(AppError::BadRequest(
                "path escapes project root".into(),
            ));
        }
        if !file_canon.is_file() {
            return Err(AppError::NotFound(format!("not a file: {rel}")));
        }
        Ok(file_canon)
    }
}

fn load_flowy(data_dir: &Path) -> Option<FlowyMediaServices> {
    let cfg: GatewayConfig = load_user_config_file(&config_yaml_path(Some(data_dir))).ok()?;
    FlowyMediaServices::try_new(&cfg, data_dir)
}

pub(crate) fn map_montage_err(e: MontageError) -> AppError {
    match e {
        MontageError::ProjectNotFound(id) => AppError::NotFound(format!("project {id}")),
        MontageError::PipelineNotFound(name) => AppError::NotFound(format!("pipeline {name}")),
        MontageError::InvalidParams(m) | MontageError::Message(m) => AppError::BadRequest(m),
        MontageError::ArtifactInvalid(name, msg) => {
            AppError::BadRequest(format!("artifact '{name}' invalid: {msg}"))
        }
        MontageError::NotAuthenticated => AppError::Unauthorized(e.to_string()),
        MontageError::Busy(id) => AppError::BadRequest(format!("project busy: {id}")),
        MontageError::GovernanceBlocked(m) => AppError::BadRequest(m),
        other => AppError::Internal(other.to_string()),
    }
}
