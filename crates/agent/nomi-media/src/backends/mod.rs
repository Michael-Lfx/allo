//! Shared Flowy media service handle.

use std::path::PathBuf;
use std::sync::Arc;

use nomi_config::{GatewayConfig, MediaGenConfig, ServerConfig};
use nomifun_cloud::{
    ClawModelEntry, FlowyApiClient, MODEL_CATEGORY_IMAGE, MODEL_CATEGORY_VIDEO, ServerSession,
    resolve_model_in_catalog,
};

use crate::flowy_params::is_flowy_model_id;

/// Runtime handle for Flowy image/video APIs (login token + config).
#[derive(Clone)]
pub struct FlowyMediaServices {
    pub api: Arc<FlowyApiClient>,
    pub session: ServerSession,
    pub media: MediaGenConfig,
    pub server: ServerConfig,
    pub data_dir: PathBuf,
}

impl FlowyMediaServices {
    pub fn try_new(config: &GatewayConfig, data_dir: &std::path::Path) -> Option<Self> {
        if !config.media.uses_flowy() || !config.server.api_ready() {
            return None;
        }
        let api = FlowyApiClient::new(&config.server).ok()?;
        let session = ServerSession::from_config(&config.server, data_dir);
        Some(Self {
            api: Arc::new(api),
            session,
            media: config.media.clone(),
            server: config.server.clone(),
            data_dir: data_dir.to_path_buf(),
        })
    }

    pub async fn is_authenticated(&self) -> bool {
        self.session
            .access_token()
            .await
            .ok()
            .flatten()
            .is_some_and(|t| !t.trim().is_empty())
    }

    pub async fn credit_balance(&self) -> Result<i64, nomi_types::ToolError> {
        self.require_token().await?;
        let balance = self
            .api
            .get_credits_balance(&self.session)
            .await
            .map_err(map_server_err)?;
        Ok(balance.balance)
    }

    /// Ensure sufficient credits for image generation.
    pub async fn ensure_image_credits(&self) -> Result<(), nomi_types::ToolError> {
        if !self.media.workflows.check_credits {
            return Ok(());
        }
        let min = self.media.workflows.image_min_credits;
        self.ensure_min_credits(min, "image generation").await
    }

    /// Ensure sufficient credits for a video of `duration_secs` seconds.
    pub async fn ensure_video_credits(
        &self,
        duration_secs: u32,
    ) -> Result<(), nomi_types::ToolError> {
        if !self.media.workflows.check_credits {
            return Ok(());
        }
        let per_sec = self.media.workflows.video_credits_per_second;
        let required = u64::from(duration_secs.max(1)).saturating_mul(per_sec);
        self.ensure_min_credits(required, "video generation").await
    }

    async fn ensure_min_credits(
        &self,
        required: u64,
        context: &str,
    ) -> Result<(), nomi_types::ToolError> {
        let balance = self.credit_balance().await?;
        if balance < required as i64 {
            return Err(nomi_types::ToolError::ExecutionFailed(format!(
                "insufficient credits for {context}: need {required}, balance {balance}"
            )));
        }
        Ok(())
    }

    pub async fn fetch_image_models(&self) -> Result<Vec<ClawModelEntry>, nomi_types::ToolError> {
        self.require_token().await?;
        let models = self
            .api
            .get_available_models_claw(&self.session, Some(MODEL_CATEGORY_IMAGE))
            .await
            .map_err(map_server_err)?;
        Ok(models.cloud)
    }

    pub async fn fetch_video_models(&self) -> Result<Vec<ClawModelEntry>, nomi_types::ToolError> {
        self.require_token().await?;
        let models = self
            .api
            .get_available_models_claw(&self.session, Some(MODEL_CATEGORY_VIDEO))
            .await
            .map_err(map_server_err)?;
        Ok(models.cloud)
    }

    pub async fn resolve_image_model(
        &self,
        agent_model: Option<&str>,
    ) -> Result<String, nomi_types::ToolError> {
        let catalog = self.fetch_image_models().await?;
        self.resolve_model_in_catalog(
            agent_model,
            self.media.image.model.as_str(),
            &catalog,
            "image",
        )
    }

    pub async fn resolve_video_model(
        &self,
        agent_model: Option<&str>,
    ) -> Result<String, nomi_types::ToolError> {
        let catalog = self.fetch_video_models().await?;
        self.resolve_model_in_catalog(
            agent_model,
            self.media.video.model.as_str(),
            &catalog,
            "video",
        )
    }

    fn resolve_model_in_catalog(
        &self,
        agent_model: Option<&str>,
        configured: &str,
        catalog: &[ClawModelEntry],
        kind: &str,
    ) -> Result<String, nomi_types::ToolError> {
        if let Some(m) = agent_model.map(str::trim).filter(|s| !s.is_empty()) {
            if is_flowy_model_id(m) || resolve_model_in_catalog(m, catalog).is_some() {
                if let Some(resolved) = resolve_model_in_catalog(m, catalog) {
                    return Ok(resolved);
                }
            } else {
                tracing::warn!(
                    agent_model = m,
                    "ignoring non-Flowy model id from tool call; using configured default"
                );
            }
        }

        let configured = configured.trim();
        if !configured.is_empty() {
            if let Some(resolved) = resolve_model_in_catalog(configured, catalog) {
                return Ok(resolved);
            }
            return Err(nomi_types::ToolError::ExecutionFailed(format!(
                "configured {kind} model '{configured}' not found in server catalog — check Flowy media model settings"
            )));
        }

        catalog.first().map(|m| m.api_model_id()).ok_or_else(|| {
            nomi_types::ToolError::ExecutionFailed(format!(
                "no {kind} models available — check login and credits"
            ))
        })
    }

    pub async fn default_image_model(&self) -> Result<String, nomi_types::ToolError> {
        self.resolve_image_model(None).await
    }

    pub async fn default_video_model(&self) -> Result<String, nomi_types::ToolError> {
        self.resolve_video_model(None).await
    }

    pub async fn require_token(&self) -> Result<String, nomi_types::ToolError> {
        self.session
            .access_token()
            .await
            .map_err(|e| nomi_types::ToolError::ExecutionFailed(e.to_string()))?
            .filter(|t| !t.trim().is_empty())
            .ok_or_else(|| {
                nomi_types::ToolError::ExecutionFailed(
                    "not logged in — sign in via Settings → Cloud Account first".into(),
                )
            })
    }
}

pub fn map_server_err(err: nomifun_cloud::ServerClientError) -> nomi_types::ToolError {
    nomi_types::ToolError::ExecutionFailed(err.to_string())
}

pub mod flowy_image;
pub mod flowy_video;
pub mod flowy_video_router;
pub mod traits;
