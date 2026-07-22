//! Thin wrapper around `nomi_vimax::VimaxService` with GatewayConfig reload.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use nomi_config::{GatewayConfig, config_yaml_path, load_user_config_file};
use nomi_vimax::{
    ArtifactNode, FlowyVimaxServices, RenderStatus, SessionRecord, VimaxService, WorkflowKind,
};
use nomifun_common::AppError;

pub struct VimaxApiService {
    data_dir: PathBuf,
    inner: Arc<VimaxService>,
}

impl VimaxApiService {
    pub fn new(data_dir: PathBuf) -> Result<Self, AppError> {
        let flowy = load_flowy(&data_dir);
        let inner = VimaxService::start(&data_dir, flowy)
            .map_err(|e| AppError::Internal(e.to_string()))?;
        Ok(Self { data_dir, inner })
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    async fn refresh_backends(&self) {
        let flowy = load_flowy(&self.data_dir);
        self.inner.set_flowy(flowy).await;
    }

    pub fn list_sessions(&self) -> Result<Vec<SessionRecord>, AppError> {
        self.inner
            .list_sessions()
            .map_err(|e| AppError::Internal(e.to_string()))
    }

    pub fn create_session(
        &self,
        workflow: &str,
        title: Option<String>,
    ) -> Result<SessionRecord, AppError> {
        let kind = WorkflowKind::parse(workflow)
            .ok_or_else(|| AppError::BadRequest(format!("unknown workflow: {workflow}")))?;
        self.inner
            .create_session(kind, title)
            .map_err(|e| AppError::Internal(e.to_string()))
    }

    pub fn get_session(&self, id: &str) -> Result<SessionRecord, AppError> {
        self.inner.get_session(id).map_err(map_vimax_err)
    }

    pub async fn plan(
        &self,
        id: &str,
        idea: Option<String>,
        script: Option<String>,
        novel_text: Option<String>,
        user_requirement: Option<String>,
        style: Option<String>,
        llm_model: Option<String>,
        image_model: Option<String>,
        video_model: Option<String>,
        target_duration_secs: Option<u32>,
    ) -> Result<(), AppError> {
        self.refresh_backends().await;
        self.inner
            .plan(
                id,
                idea,
                script,
                novel_text,
                user_requirement,
                style,
                llm_model,
                image_model,
                video_model,
                target_duration_secs,
            )
            .await
            .map_err(map_vimax_err)
    }

    pub async fn revise(
        &self,
        id: &str,
        revision_target: String,
        revision_instruction: String,
    ) -> Result<(), AppError> {
        self.refresh_backends().await;
        self.inner
            .revise(id, revision_target, revision_instruction)
            .await
            .map_err(map_vimax_err)
    }

    pub async fn render(
        &self,
        id: &str,
        llm_model: Option<String>,
        image_model: Option<String>,
        video_model: Option<String>,
    ) -> Result<(), AppError> {
        self.refresh_backends().await;
        self.inner
            .render(id, llm_model, image_model, video_model)
            .await
            .map_err(map_vimax_err)
    }

    pub async fn status(&self, id: &str) -> Result<RenderStatus, AppError> {
        self.inner.status(id).await.map_err(map_vimax_err)
    }

    pub async fn cancel(&self, id: &str) -> Result<(), AppError> {
        self.inner.cancel(id).await.map_err(map_vimax_err)
    }

    pub async fn delete_session(&self, id: &str) -> Result<(), AppError> {
        self.inner.delete_session(id).await.map_err(map_vimax_err)
    }

    pub fn list_artifacts(&self, id: &str) -> Result<Vec<ArtifactNode>, AppError> {
        self.inner.list_artifacts(id).map_err(map_vimax_err)
    }

    pub fn artifact_path(&self, id: &str, rel: &str) -> Result<PathBuf, AppError> {
        self.inner.artifact_path(id, rel).map_err(map_vimax_err)
    }
}

fn load_flowy(data_dir: &Path) -> Option<FlowyVimaxServices> {
    let cfg: GatewayConfig = load_user_config_file(&config_yaml_path(Some(data_dir))).ok()?;
    FlowyVimaxServices::try_new(&cfg, data_dir)
}

fn map_vimax_err(e: nomi_vimax::VimaxError) -> AppError {
    match e {
        nomi_vimax::VimaxError::SessionNotFound(id) => AppError::NotFound(format!("session {id}")),
        nomi_vimax::VimaxError::InvalidParams(m) => AppError::BadRequest(m),
        nomi_vimax::VimaxError::NotAuthenticated => AppError::Unauthorized(e.to_string()),
        other => AppError::Internal(other.to_string()),
    }
}
