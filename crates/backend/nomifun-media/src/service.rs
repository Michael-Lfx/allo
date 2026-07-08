use std::path::{Path, PathBuf};
use std::sync::Mutex;

use nomi_config::{
    GatewayConfig, config_yaml_path, flowy_media_exposed, load_user_config_file, save_config_yaml,
};
use nomi_media::workflows::store::{WorkflowRunRecord, WorkflowRunStore};
use nomifun_api_types::{
    MediaCreditsResponse, MediaSettingsResponse, MediaWorkflowHistoryItem,
    MediaWorkflowHistoryResponse, UpdateMediaSettingsRequest,
};
use nomifun_cloud::{FlowyApiClient, MODEL_CATEGORY_IMAGE, MODEL_CATEGORY_VIDEO};
use nomifun_common::AppError;

pub struct MediaApiService {
    data_dir: PathBuf,
    config: Mutex<GatewayConfig>,
    workflow_store: WorkflowRunStore,
}

impl MediaApiService {
    pub fn new(data_dir: PathBuf) -> Result<Self, AppError> {
        let config = load_user_config_file(&config_yaml_path(Some(&data_dir)))
            .map_err(|e| AppError::Internal(e))?;
        let workflow_root = data_dir.join("media").join("workflows");
        Ok(Self {
            data_dir,
            config: Mutex::new(config),
            workflow_store: WorkflowRunStore::with_root(workflow_root),
        })
    }

    fn gateway_config(&self) -> GatewayConfig {
        self.config.lock().expect("media config lock").clone()
    }

    fn config_path(&self) -> PathBuf {
        config_yaml_path(Some(&self.data_dir))
    }

    pub fn settings(&self) -> MediaSettingsResponse {
        let mut cfg = self.gateway_config();
        if !cfg.media.provider.eq_ignore_ascii_case("flowy") {
            cfg.media.provider = "flowy".to_string();
        }
        let media = &cfg.media;
        MediaSettingsResponse {
            provider: media.provider.clone(),
            image_model: media.image.model.clone(),
            video_model: media.video.model.clone(),
            image_save_locally: media.image.save_locally,
            video_save_locally: media.video.save_locally,
            video_default_duration: media.video.default_duration,
            video_default_aspect_ratio: media.video.default_aspect_ratio.clone(),
            video_default_resolution: media.video.default_resolution.clone(),
            workflows_enabled: media.workflows.enabled,
            workflows_max_retries: media.workflows.max_retries,
            workflows_async_execution: media.workflows.async_execution,
            workflows_llm_prompt_refine: media.workflows.llm_prompt_refine,
            workflows_check_credits: media.workflows.check_credits,
            flowy_media_exposed: flowy_media_exposed(&cfg),
        }
    }

    pub fn update_settings(
        &self,
        req: UpdateMediaSettingsRequest,
    ) -> Result<MediaSettingsResponse, AppError> {
        {
            let mut cfg = self.config.lock().expect("media config lock");
            cfg.media.provider = "flowy".to_string();
            if let Some(model) = req.image_model {
                cfg.media.image.model = model;
            }
            if let Some(model) = req.video_model {
                cfg.media.video.model = model;
            }
            if let Some(save) = req.image_save_locally {
                cfg.media.image.save_locally = save;
            }
            if let Some(save) = req.video_save_locally {
                cfg.media.video.save_locally = save;
            }
            if let Some(duration) = req.video_default_duration {
                cfg.media.video.default_duration = duration;
            }
            if let Some(enabled) = req.workflows_enabled {
                cfg.media.workflows.enabled = enabled;
            }
            if let Some(retries) = req.workflows_max_retries {
                cfg.media.workflows.max_retries = retries;
            }
            save_config_yaml(&self.config_path(), &cfg).map_err(|e| AppError::Internal(e))?;
        }
        Ok(self.settings())
    }

    pub async fn credits(&self) -> Result<MediaCreditsResponse, AppError> {
        let cfg = self.gateway_config();
        if !flowy_media_exposed(&cfg) {
            return Ok(MediaCreditsResponse {
                balance: 0,
                authenticated: false,
            });
        }
        let api = FlowyApiClient::new(&cfg.server).map_err(|e| AppError::Internal(e.to_string()))?;
        let session = nomifun_cloud::ServerSession::from_config(&cfg.server, &self.data_dir);
        let token = session
            .access_token()
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;
        let Some(token) = token.filter(|t| !t.trim().is_empty()) else {
            return Ok(MediaCreditsResponse {
                balance: 0,
                authenticated: false,
            });
        };
        let _ = token;
        let balance = api
            .get_credits_balance(&session)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;
        Ok(MediaCreditsResponse {
            balance: balance.balance,
            authenticated: true,
        })
    }

    pub fn workflow_history(&self, limit: usize) -> MediaWorkflowHistoryResponse {
        let runs = self
            .workflow_store
            .list_records_newest_first()
            .into_iter()
            .take(limit)
            .map(record_to_item)
            .collect();
        MediaWorkflowHistoryResponse { runs }
    }

    pub async fn list_models(&self) -> Result<(Vec<String>, Vec<String>), AppError> {
        let cfg = self.gateway_config();
        if !flowy_media_exposed(&cfg) {
            return Ok((Vec::new(), Vec::new()));
        }
        let api = FlowyApiClient::new(&cfg.server).map_err(|e| AppError::Internal(e.to_string()))?;
        let session = nomifun_cloud::ServerSession::from_config(&cfg.server, &self.data_dir);
        let image = api
            .get_available_models_claw(&session, Some(MODEL_CATEGORY_IMAGE))
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;
        let video = api
            .get_available_models_claw(&session, Some(MODEL_CATEGORY_VIDEO))
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;
        Ok((
            image.cloud.into_iter().map(|m| m.id).collect(),
            video.cloud.into_iter().map(|m| m.id).collect(),
        ))
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }
}

fn record_to_item(record: WorkflowRunRecord) -> MediaWorkflowHistoryItem {
    MediaWorkflowHistoryItem {
        run_id: record.run_id,
        workflow_id: record.workflow_id,
        status: format!("{:?}", record.status).to_ascii_lowercase(),
        current_step: record.current_step,
        error: record.error,
        artifacts: record.artifacts,
    }
}
