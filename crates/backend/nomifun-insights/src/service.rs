//! Insights contribution service — wraps [`nomi_insights_core::ContributionService`].

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use nomi_config::{
    GatewayConfig, InsightsContributionConfig, config_yaml_path, load_user_config_file,
    save_config_yaml,
};
use nomi_insights_core::{
    ContributionService, INSIGHTS_CONSENT_VERSION, load_or_create_installation_id,
};
use nomifun_api_types::{
    InsightsContributionStatusResponse, InsightsFlushResponse, InsightsResetOutboxRequest,
    InsightsResetOutboxResponse, UpdateInsightsContributionRequest,
};
use nomifun_cloud::effective_insights_contribution_config;
use nomifun_common::AppError;

pub struct InsightsService {
    data_dir: PathBuf,
    config: Mutex<GatewayConfig>,
}

impl InsightsService {
    pub fn new(data_dir: PathBuf) -> Result<Self, AppError> {
        let root = data_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| data_dir.clone());
        let config = load_user_config_file(&config_yaml_path(Some(&root)))
            .map_err(|e| AppError::Internal(e))?;
        Ok(Self {
            data_dir,
            config: Mutex::new(config),
        })
    }

    fn config_path(&self) -> PathBuf {
        let root = self
            .data_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.data_dir.clone());
        config_yaml_path(Some(&root))
    }

    fn auth_data_dir(&self) -> PathBuf {
        self.data_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.data_dir.clone())
    }

    pub async fn contribution_config(&self) -> InsightsContributionConfig {
        let cfg = self.config.lock().expect("insights config lock").clone();
        effective_insights_contribution_config(
            cfg.insights.contribution.clone(),
            &cfg.server,
            &self.auth_data_dir(),
        )
        .await
    }

    async fn open_service(&self) -> Result<ContributionService, AppError> {
        let effective = self.contribution_config().await;
        ContributionService::open(self.data_dir.clone(), effective)
            .map_err(|e| AppError::Internal(e))
    }

    pub async fn status(&self) -> Result<InsightsContributionStatusResponse, AppError> {
        let cfg = self.contribution_config().await;
        let svc = self.open_service().await?;
        let counts = svc.outbox_counts().map_err(|e| AppError::Internal(e))?;
        let installation_id = load_or_create_installation_id(&self.data_dir)
            .unwrap_or_else(|_| "(unknown)".to_string());
        Ok(InsightsContributionStatusResponse {
            enabled: cfg.enabled,
            on_session_end: cfg.on_session_end,
            auto_extract_enabled: cfg.auto_extract_enabled,
            auto_extract_idle_secs: cfg.auto_extract_idle_secs,
            skill_mining_enabled: cfg.skill_mining_enabled,
            min_evidence_tier: cfg.min_evidence_tier.clone(),
            require_skill_binding: cfg.require_skill_binding,
            min_work_turns: cfg.min_work_turns,
            redacted_body: cfg.redacted_body,
            endpoint: cfg.endpoint.clone(),
            auth_configured: cfg.effective_token().is_some(),
            upload_ready: cfg.upload_ready(),
            outbox_pending: counts.pending,
            outbox_failed: counts.failed,
            outbox_sent: counts.sent,
            installation_id,
            consent_version: INSIGHTS_CONSENT_VERSION,
        })
    }

    pub async fn update_contribution(
        &self,
        req: UpdateInsightsContributionRequest,
    ) -> Result<InsightsContributionStatusResponse, AppError> {
        {
            let mut cfg = self.config.lock().expect("insights config lock");
            let contribution = &mut cfg.insights.contribution;
            if let Some(enabled) = req.enabled {
                contribution.enabled = enabled;
            }
            if let Some(on_session_end) = req.on_session_end {
                contribution.on_session_end = on_session_end;
            }
            if let Some(auto_extract_enabled) = req.auto_extract_enabled {
                contribution.auto_extract_enabled = auto_extract_enabled;
            }
            if let Some(auto_extract_idle_secs) = req.auto_extract_idle_secs {
                contribution.auto_extract_idle_secs = auto_extract_idle_secs;
            }
            if let Some(skill_mining_enabled) = req.skill_mining_enabled {
                contribution.skill_mining_enabled = skill_mining_enabled;
            }
            if let Some(redacted_body) = req.redacted_body {
                contribution.redacted_body = redacted_body;
            }
            save_config_yaml(&self.config_path(), &cfg).map_err(|e| AppError::Internal(e))?;
        }
        self.status().await
    }

    pub async fn flush(&self) -> Result<InsightsFlushResponse, AppError> {
        let svc = self.open_service().await?;
        let result = svc.flush().await.map_err(|e| AppError::Internal(e))?;
        Ok(InsightsFlushResponse {
            uploaded: result.uploaded,
            duplicates: result.duplicates,
            rejected: result.rejected,
            skipped_no_endpoint: result.skipped_no_endpoint,
        })
    }

    pub fn reset_outbox(
        &self,
        req: InsightsResetOutboxRequest,
    ) -> Result<InsightsResetOutboxResponse, AppError> {
        let cfg = self
            .config
            .lock()
            .expect("insights config lock")
            .insights
            .contribution
            .clone();
        let svc = ContributionService::open(self.data_dir.clone(), cfg)
            .map_err(|e| AppError::Internal(e))?;
        let affected = svc
            .reset_outbox(req.clear_all)
            .map_err(|e| AppError::Internal(e))?;
        let counts = svc.outbox_counts().map_err(|e| AppError::Internal(e))?;
        Ok(InsightsResetOutboxResponse {
            affected,
            outbox_pending: counts.pending,
            outbox_failed: counts.failed,
            outbox_sent: counts.sent,
        })
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }
}
