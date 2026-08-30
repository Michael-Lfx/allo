use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use nomi_config::{
    GatewayConfig, config_yaml_path, flowy_media_exposed, load_user_config_file, save_config_yaml,
};
use nomi_media::workflows::store::{WorkflowRunRecord, WorkflowRunStore};
use nomifun_api_types::{
    MediaCreditsCheckinResponse, MediaCreditsResponse, MediaModelListResponse,
    MediaSettingsResponse, MediaTurnCreditUsage, MediaTurnCreditUsageCall,
    MediaWorkflowHistoryItem, MediaWorkflowHistoryResponse, UpdateMediaSettingsRequest,
};
use nomifun_cloud::{
    FlowyApiClient, MODEL_CATEGORY_IMAGE, MODEL_CATEGORY_TTS, MODEL_CATEGORY_VIDEO,
    ensure_gateway_defaults,
};
use nomifun_common::AppError;

/// Short-lived catalog cache so first-enter UI / Canvas sync / preferences
/// popovers do not each pay three Flowy catalog round-trips (image / video / TTS).
const MODELS_CACHE_TTL: Duration = Duration::from_secs(120);

struct ModelsCatalogCache {
    fetched_at: Instant,
    image: Vec<nomifun_api_types::MediaModelOption>,
    video: Vec<nomifun_api_types::MediaModelOption>,
    audio: Vec<nomifun_api_types::MediaModelOption>,
}

pub struct MediaApiService {
    data_dir: PathBuf,
    config: Mutex<GatewayConfig>,
    workflow_store: WorkflowRunStore,
    models_cache: Mutex<Option<ModelsCatalogCache>>,
}

impl MediaApiService {
    pub fn new(data_dir: PathBuf) -> Result<Self, AppError> {
        let path = config_yaml_path(Some(&data_dir));
        let mut config = load_user_config_file(&path).map_err(|e| AppError::Internal(e))?;
        // Match CloudService: empty `server.base_url` must get production defaults,
        // otherwise `flowy_media_exposed` stays false and model lists return empty.
        ensure_gateway_defaults(&mut config);
        let workflow_root = data_dir.join("media").join("workflows");
        Ok(Self {
            data_dir,
            config: Mutex::new(config),
            workflow_store: WorkflowRunStore::with_root(workflow_root),
            models_cache: Mutex::new(None),
        })
    }

    fn config_path(&self) -> PathBuf {
        config_yaml_path(Some(&self.data_dir))
    }

    /// Load config from disk (so CloudService / settings writes are visible), then
    /// apply gateway defaults. Never rely solely on the process-start snapshot.
    fn load_effective_config(&self) -> Result<GatewayConfig, AppError> {
        let mut cfg = load_user_config_file(&self.config_path()).map_err(|e| AppError::Internal(e))?;
        ensure_gateway_defaults(&mut cfg);
        // Keep in-memory copy aligned for subsequent update_settings merges.
        if let Ok(mut guard) = self.config.lock() {
            *guard = cfg.clone();
        }
        Ok(cfg)
    }

    fn gateway_config(&self) -> GatewayConfig {
        self.load_effective_config().unwrap_or_else(|_| {
            let mut cfg = self.config.lock().expect("media config lock").clone();
            ensure_gateway_defaults(&mut cfg);
            cfg
        })
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
        // Reload from disk first so we never overwrite `server.base_url` with a
        // stale in-memory GatewayConfig that predated CloudService defaults.
        let mut cfg = self.load_effective_config()?;
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
        if let Some(aspect) = req.video_default_aspect_ratio {
            let t = aspect.trim();
            if !t.is_empty() {
                cfg.media.video.default_aspect_ratio = t.to_string();
            }
        }
        if let Some(enabled) = req.workflows_enabled {
            cfg.media.workflows.enabled = enabled;
        }
        if let Some(retries) = req.workflows_max_retries {
            cfg.media.workflows.max_retries = retries;
        }
        save_config_yaml(&self.config_path(), &cfg).map_err(|e| AppError::Internal(e))?;
        *self.config.lock().expect("media config lock") = cfg;
        // Default model prefs don't change the cloud catalog; keep cache warm.
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

    /// Perform the Flowy daily check-in. `time_zone` is the client's IANA
    /// timezone (e.g. "Asia/Shanghai"); the upstream server resolves the day
    /// boundary from it. The response carries the post-check-in `balance`, so
    /// callers can refresh their display without a follow-up balance fetch.
    pub async fn checkin(
        &self,
        time_zone: String,
    ) -> Result<MediaCreditsCheckinResponse, AppError> {
        let unauthenticated = || MediaCreditsCheckinResponse {
            already_checked_in: false,
            granted_points: 0,
            balance: 0,
            check_in_at: None,
            day_key: None,
            authenticated: false,
        };

        let cfg = self.gateway_config();
        if !flowy_media_exposed(&cfg) {
            return Ok(unauthenticated());
        }
        let api = FlowyApiClient::new(&cfg.server).map_err(|e| AppError::Internal(e.to_string()))?;
        let session = nomifun_cloud::ServerSession::from_config(&cfg.server, &self.data_dir);
        let token = session
            .access_token()
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;
        let Some(token) = token.filter(|t| !t.trim().is_empty()) else {
            return Ok(unauthenticated());
        };
        let _ = token;
        let resp = api
            .credits_checkin(&session, &time_zone)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;
        Ok(MediaCreditsCheckinResponse {
            already_checked_in: resp.already_checked_in,
            granted_points: resp.granted_points,
            balance: resp.balance,
            check_in_at: resp.check_in_at,
            day_key: resp.day_key,
            authenticated: true,
        })
    }

    /// Look up Flowy credit usage for one agent turn (`X-Flowy-Turn-Id`).
    pub async fn credits_usage_by_turn(
        &self,
        turn_id: String,
    ) -> Result<MediaTurnCreditUsage, AppError> {
        let turn_id = turn_id.trim().to_string();
        let unauthenticated = || MediaTurnCreditUsage {
            turn_id: turn_id.clone(),
            authenticated: false,
            ..Default::default()
        };

        if turn_id.is_empty() || turn_id.len() > 64 {
            return Err(AppError::BadRequest(
                "turnId must be 1..=64 characters after trim".into(),
            ));
        }

        let cfg = self.gateway_config();
        if !flowy_media_exposed(&cfg) {
            return Ok(unauthenticated());
        }
        let api = FlowyApiClient::new(&cfg.server).map_err(|e| AppError::Internal(e.to_string()))?;
        let session = nomifun_cloud::ServerSession::from_config(&cfg.server, &self.data_dir);
        let token = session
            .access_token()
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;
        let Some(token) = token.filter(|t| !t.trim().is_empty()) else {
            return Ok(unauthenticated());
        };
        let _ = token;

        let usage = api
            .get_credits_usage_by_turn(&session, &turn_id)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

        Ok(MediaTurnCreditUsage {
            turn_id: usage.turn_id,
            session_id: usage.session_id,
            call_count: usage.call_count,
            credits_consumed: usage.credits_consumed,
            calls: usage
                .calls
                .into_iter()
                .map(|c| MediaTurnCreditUsageCall {
                    chat_id: c.chat_id,
                    model_name: c.model_name,
                    channel_model_id: c.channel_model_id,
                    prompt_tokens: c.prompt_tokens,
                    completion_tokens: c.completion_tokens,
                    cache_tokens: c.cache_tokens,
                    credit_consumed: c.credit_consumed,
                    call_status: c.call_status,
                    created_at: c.created_at,
                })
                .collect(),
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

    /// Fetch the latest image / video / TTS catalog from the cloud.
    /// Image, video, and TTS (`category=8`) catalogs are fetched in parallel and
    /// cached briefly (`MODELS_CACHE_TTL`) to absorb concurrent first-enter callers.
    /// A TTS fetch failure is soft: image/video still return.
    pub async fn list_models(&self) -> Result<MediaModelListResponse, AppError> {
        if let Ok(guard) = self.models_cache.lock() {
            if let Some(cached) = guard.as_ref() {
                if cached.fetched_at.elapsed() < MODELS_CACHE_TTL {
                    return Ok(MediaModelListResponse {
                        image_models: cached.image.clone(),
                        video_models: cached.video.clone(),
                        audio_models: cached.audio.clone(),
                    });
                }
            }
        }

        let cfg = self.load_effective_config()?;
        if !flowy_media_exposed(&cfg) {
            tracing::warn!(
                base_url_empty = cfg.server.base_url.trim().is_empty(),
                media_provider = %cfg.media.provider,
                "media list_models: Flowy media not exposed; returning empty catalog"
            );
            return Ok(MediaModelListResponse {
                image_models: Vec::new(),
                video_models: Vec::new(),
                audio_models: Vec::new(),
            });
        }
        let api = FlowyApiClient::new(&cfg.server).map_err(|e| AppError::Internal(e.to_string()))?;
        let session = nomifun_cloud::ServerSession::from_config(&cfg.server, &self.data_dir);
        let server_base = cfg.server.base_url.trim().trim_end_matches('/').to_string();
        let (image_res, video_res, tts_res) = tokio::join!(
            api.get_available_models_claw(&session, Some(MODEL_CATEGORY_IMAGE)),
            api.get_available_models_claw(&session, Some(MODEL_CATEGORY_VIDEO)),
            api.get_available_models_claw(&session, Some(MODEL_CATEGORY_TTS)),
        );
        let image = image_res.map_err(|e| AppError::Internal(e.to_string()))?;
        let video = video_res.map_err(|e| AppError::Internal(e.to_string()))?;
        let image_models: Vec<_> = image
            .cloud
            .into_iter()
            .map(|entry| to_media_model_option(entry, &server_base))
            .collect();
        let video_models: Vec<_> = video
            .cloud
            .into_iter()
            .map(|entry| to_media_model_option(entry, &server_base))
            .collect();
        let audio_models: Vec<_> = match tts_res {
            Ok(tts) => tts
                .cloud
                .into_iter()
                .map(|entry| to_media_model_option(entry, &server_base))
                .collect(),
            Err(e) => {
                tracing::warn!(
                    "Failed to fetch Flowy TTS catalog (category=8): {e}; returning media models without TTS"
                );
                Vec::new()
            }
        };
        if let Ok(mut guard) = self.models_cache.lock() {
            *guard = Some(ModelsCatalogCache {
                fetched_at: Instant::now(),
                image: image_models.clone(),
                video: video_models.clone(),
                audio: audio_models.clone(),
            });
        }
        Ok(MediaModelListResponse {
            image_models,
            video_models,
            audio_models,
        })
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

fn rewrite_catalog_icon_url(icon: &str, server_base: &str) -> String {
    let icon = icon.trim();
    if icon.is_empty() {
        return String::new();
    }
    if icon.starts_with("https://")
        || icon.starts_with("http://")
        || icon.starts_with("data:")
        || icon.starts_with("blob:")
    {
        return icon.to_string();
    }
    if let Some(rest) = icon.strip_prefix("//") {
        return format!("https://{rest}");
    }
    let base = server_base.trim().trim_end_matches('/');
    if base.is_empty() {
        return icon.to_string();
    }
    if icon.starts_with('/') {
        format!("{base}{icon}")
    } else {
        format!("{base}/{icon}")
    }
}

fn to_media_model_option(
    entry: nomifun_cloud::ClawModelEntry,
    server_base: &str,
) -> nomifun_api_types::MediaModelOption {
    let id = entry.api_model_id();
    let name = {
        let n = entry.name.trim();
        if n.is_empty() {
            id.strip_prefix("AIPC-")
                .or_else(|| id.strip_prefix("aipc-"))
                .unwrap_or(id.as_str())
                .to_string()
        } else {
            n.to_string()
        }
    };
    nomifun_api_types::MediaModelOption {
        id,
        name,
        icon: rewrite_catalog_icon_url(&entry.icon, server_base),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomi_config::DEFAULT_WECHAT_FLOWY_SERVER_BASE;
    use nomifun_api_types::UpdateMediaSettingsRequest;

    #[test]
    fn new_applies_default_base_url_when_config_missing() {
        let dir = tempfile::tempdir().unwrap();
        let svc = MediaApiService::new(dir.path().to_path_buf()).unwrap();
        let settings = svc.settings();
        assert!(settings.flowy_media_exposed);
        let cfg = svc.gateway_config();
        assert_eq!(cfg.server.base_url, DEFAULT_WECHAT_FLOWY_SERVER_BASE);
    }

    #[tokio::test]
    async fn checkin_short_circuits_without_session() {
        // A fresh tempdir has no Flowy session, so checkin must return
        // authenticated:false without hitting the network.
        let dir = tempfile::tempdir().unwrap();
        let svc = MediaApiService::new(dir.path().to_path_buf()).unwrap();
        let resp = svc.checkin("UTC".into()).await.unwrap();
        assert!(!resp.authenticated);
        assert_eq!(resp.balance, 0);
        assert!(!resp.already_checked_in);
        assert!(resp.day_key.is_none());
    }

    #[test]
    fn new_applies_default_base_url_when_yaml_has_empty_server() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_yaml_path(Some(dir.path()));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "server:\n  enabled: false\n  base_url: \"\"\nmedia:\n  provider: flowy\n",
        )
        .unwrap();
        let svc = MediaApiService::new(dir.path().to_path_buf()).unwrap();
        assert!(svc.settings().flowy_media_exposed);
        assert_eq!(svc.gateway_config().server.base_url, DEFAULT_WECHAT_FLOWY_SERVER_BASE);
    }

    #[test]
    fn update_settings_does_not_wipe_server_base_url() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_yaml_path(Some(dir.path()));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Simulate CloudService having persisted a real base_url after Media started
        // with an empty snapshot (historical bug).
        std::fs::write(
            &path,
            format!(
                "server:\n  enabled: true\n  base_url: \"{DEFAULT_WECHAT_FLOWY_SERVER_BASE}\"\nmedia:\n  provider: flowy\n  image:\n    model: old-image\n"
            ),
        )
        .unwrap();
        let svc = MediaApiService::new(dir.path().to_path_buf()).unwrap();
        // Corrupt in-memory copy to prove update reloads from disk.
        {
            let mut guard = svc.config.lock().unwrap();
            guard.server.base_url.clear();
        }
        svc.update_settings(UpdateMediaSettingsRequest {
            image_model: Some("new-image".into()),
            ..Default::default()
        })
        .unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains(DEFAULT_WECHAT_FLOWY_SERVER_BASE));
        assert!(raw.contains("new-image"));
        assert!(svc.settings().flowy_media_exposed);
    }

    #[test]
    fn rewrite_catalog_icon_url_joins_relative_paths() {
        assert_eq!(
            rewrite_catalog_icon_url(
                "/static/seedream.png",
                "https://api.flowy.example/"
            ),
            "https://api.flowy.example/static/seedream.png"
        );
        assert_eq!(
            rewrite_catalog_icon_url("https://cdn.example/a.png", "https://api.flowy.example"),
            "https://cdn.example/a.png"
        );
        assert_eq!(
            rewrite_catalog_icon_url("//cdn.example/a.png", ""),
            "https://cdn.example/a.png"
        );
        assert!(rewrite_catalog_icon_url("  ", "https://api.flowy.example").is_empty());
    }
}
