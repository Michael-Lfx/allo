//! Business logic for POI topic list, pin, status, and settings.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use nomi_config::{
    GatewayConfig, InterestConfig, config_yaml_path, load_user_config_file, save_config_yaml,
};
use nomi_poi::{InterestStore, TopicStatus};
use nomifun_api_types::{
    PoiSettingsResponse, PoiStatusResponse, PoiTopicListResponse, PoiTopicResponse,
    UpdatePoiSettingsRequest,
};
use nomifun_common::AppError;
use tracing::info;

#[derive(Clone)]
pub struct PoiService {
    data_dir: PathBuf,
    gateway: Arc<Mutex<GatewayConfig>>,
    store: Arc<Mutex<InterestStore>>,
}

impl PoiService {
    /// Open POI service loading interest settings from `{data_dir}/../config.yaml`.
    pub fn new(data_dir: PathBuf) -> Result<Self, AppError> {
        let gateway = load_gateway_for_data_dir(&data_dir)?;
        Self::from_gateway(data_dir, gateway)
    }

    /// Open with an explicit interest config (tests / legacy callers).
    pub fn open(data_dir: impl AsRef<Path>, config: InterestConfig) -> Result<Self, AppError> {
        let data_dir = data_dir.as_ref().to_path_buf();
        let mut gateway = load_gateway_for_data_dir(&data_dir).unwrap_or_default();
        gateway.interest = config;
        Self::from_gateway(data_dir, gateway)
    }

    fn from_gateway(data_dir: PathBuf, gateway: GatewayConfig) -> Result<Self, AppError> {
        std::fs::create_dir_all(&data_dir)
            .map_err(|e| AppError::Internal(format!("create data dir: {e}")))?;
        let db_path = data_dir.join("interest.db");
        let store = InterestStore::open(&db_path, gateway.interest.clone())
            .map_err(|e| AppError::Internal(format!("open interest store: {e}")))?;
        Ok(Self {
            data_dir,
            gateway: Arc::new(Mutex::new(gateway)),
            store: Arc::new(Mutex::new(store)),
        })
    }

    fn config_path(&self) -> PathBuf {
        config_root_for_data_dir(&self.data_dir).join("config.yaml")
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn store(&self) -> Arc<Mutex<InterestStore>> {
        Arc::clone(&self.store)
    }

    pub fn interest_config(&self) -> InterestConfig {
        self.gateway
            .lock()
            .map(|c| c.interest.clone())
            .unwrap_or_else(|_| InterestConfig::default())
    }

    fn db_path(&self) -> PathBuf {
        self.data_dir.join("interest.db")
    }

    fn with_store<F, T>(&self, f: F) -> Result<T, AppError>
    where
        F: FnOnce(&InterestStore) -> Result<T, String>,
    {
        let guard = self
            .store
            .lock()
            .map_err(|e| AppError::Internal(format!("interest store lock: {e}")))?;
        f(&guard).map_err(|e| AppError::Internal(e))
    }

    fn topic_to_dto(topic: nomi_poi::InterestTopic) -> PoiTopicResponse {
        PoiTopicResponse {
            id: topic.id,
            label: topic.label,
            summary: topic.summary,
            weight: topic.weight,
            status: topic.status.as_str().to_string(),
            source: topic.source.as_str().to_string(),
            confidence: topic.confidence,
            evidence_count: topic.evidence_count,
            tags: topic.tags,
            pinned: topic.pinned,
            last_seen_at: topic.last_seen_at.to_rfc3339(),
        }
    }

    pub fn list_topics(&self) -> Result<PoiTopicListResponse, AppError> {
        let db_path = self.db_path().display().to_string();
        let topics = self.with_store(|store| store.list_for_cli(true))?;
        Ok(PoiTopicListResponse {
            topics: topics.into_iter().map(Self::topic_to_dto).collect(),
            database_path: db_path,
        })
    }

    pub fn status(&self) -> Result<PoiStatusResponse, AppError> {
        let config = self.interest_config();
        let topic_count = self
            .with_store(|store| store.list_for_cli(true).map(|rows| rows.len() as u32))?;
        Ok(PoiStatusResponse {
            enabled: config.enabled,
            extract_mode: config.extract_mode.clone(),
            per_turn_buffer: config.per_turn_buffer,
            per_turn_persist: config.per_turn_persist,
            session_end_llm: config.session_end_llm_enabled(),
            topic_count,
            database_path: self.db_path().display().to_string(),
        })
    }

    pub fn pin_topic(&self, topic_id: &str, pinned: bool) -> Result<bool, AppError> {
        if pinned {
            return self.with_store(|store| store.pin_topic(topic_id));
        }
        self.with_store(|store| store.unpin_topic(topic_id))
    }

    pub fn set_topic_status(&self, topic_id: &str, status: &str) -> Result<bool, AppError> {
        let parsed = match status.trim().to_ascii_lowercase().as_str() {
            "candidate" => TopicStatus::Candidate,
            "rejected" => TopicStatus::Rejected,
            "active" => TopicStatus::Active,
            other => {
                return Err(AppError::BadRequest(format!(
                    "invalid POI status '{other}' (expected active, candidate, or rejected)"
                )));
            }
        };
        self.with_store(|store| store.set_topic_status(topic_id, parsed))
    }

    pub fn get_settings(&self) -> Result<PoiSettingsResponse, AppError> {
        Ok(settings_from_config(&self.interest_config()))
    }

    pub fn update_settings(&self, req: UpdatePoiSettingsRequest) -> Result<PoiSettingsResponse, AppError> {
        if req.is_empty() {
            return Err(AppError::BadRequest("no settings fields provided".into()));
        }

        let path = self.config_path();
        let mut gateway = load_user_config_file(&path).map_err(|e| AppError::Internal(e))?;
        apply_settings_patch(&mut gateway.interest, &req);
        save_config_yaml(&path, &gateway).map_err(|e| AppError::Internal(e))?;
        info!(
            path = %path.display(),
            enabled = gateway.interest.enabled,
            extract_mode = %gateway.interest.extract_mode,
            auto_extract_min_turns = gateway.interest.auto_extract_min_turns,
            "poi: interest settings persisted"
        );

        let interest = gateway.interest.clone();
        {
            let mut cached = self
                .gateway
                .lock()
                .map_err(|e| AppError::Internal(format!("gateway config lock: {e}")))?;
            *cached = gateway;
        }

        let store = InterestStore::open(&self.db_path(), interest.clone())
            .map_err(|e| AppError::Internal(format!("reopen interest store: {e}")))?;
        *self
            .store
            .lock()
            .map_err(|e| AppError::Internal(format!("interest store lock: {e}")))? = store;

        Ok(settings_from_config(&interest))
    }

    pub fn clear_topics(&self) -> Result<(), AppError> {
        let path = self.db_path();
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| AppError::Internal(format!("clear interest store: {e}")))?;
        }
        let config = self.interest_config();
        let store = InterestStore::open(&path, config)
            .map_err(|e| AppError::Internal(format!("reopen interest store: {e}")))?;
        *self
            .store
            .lock()
            .map_err(|e| AppError::Internal(format!("interest store lock: {e}")))? = store;
        Ok(())
    }
}

fn config_root_for_data_dir(data_dir: &Path) -> PathBuf {
    data_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| data_dir.to_path_buf())
}

fn load_gateway_for_data_dir(data_dir: &Path) -> Result<GatewayConfig, AppError> {
    let path = config_yaml_path(Some(&config_root_for_data_dir(data_dir)));
    load_user_config_file(&path).map_err(|e| AppError::Internal(e))
}

fn settings_from_config(config: &InterestConfig) -> PoiSettingsResponse {
    PoiSettingsResponse {
        enabled: config.enabled,
        max_topics: config.max_topics,
        snapshot_top_k: config.snapshot_top_k,
        prefetch_top_k: config.prefetch_top_k,
        char_budget_snapshot: config.char_budget_snapshot,
        char_budget_prefetch: config.char_budget_prefetch,
        extract_mode: config.extract_mode.clone(),
        decay_half_life_days: config.decay_half_life_days,
        llm_on_session_end: config.llm_on_session_end,
        per_turn_buffer: config.per_turn_buffer,
        per_turn_persist: config.per_turn_persist,
        promote_min_evidence: config.promote_min_evidence,
        promote_min_confidence: config.promote_min_confidence,
        min_turn_chars: config.min_turn_chars,
        auto_extract_enabled: config.auto_extract_enabled,
        auto_extract_min_turns: config.auto_extract_min_turns,
        auto_extract_min_user_chars: config.auto_extract_min_user_chars,
        auto_extract_idle_secs: config.auto_extract_idle_secs,
        llm_model: config.llm_model.clone(),
    }
}

fn apply_settings_patch(config: &mut InterestConfig, req: &UpdatePoiSettingsRequest) {
    if let Some(v) = req.enabled {
        config.enabled = v;
    }
    if let Some(v) = req.max_topics {
        config.max_topics = v;
    }
    if let Some(v) = req.snapshot_top_k {
        config.snapshot_top_k = v;
    }
    if let Some(v) = req.prefetch_top_k {
        config.prefetch_top_k = v;
    }
    if let Some(v) = req.char_budget_snapshot {
        config.char_budget_snapshot = v;
    }
    if let Some(v) = req.char_budget_prefetch {
        config.char_budget_prefetch = v;
    }
    if let Some(v) = &req.extract_mode {
        config.extract_mode = normalize_extract_mode(v);
    }
    if let Some(v) = req.decay_half_life_days {
        config.decay_half_life_days = v;
    }
    if let Some(v) = req.llm_on_session_end {
        config.llm_on_session_end = v;
    }
    if let Some(v) = req.per_turn_buffer {
        config.per_turn_buffer = v;
    }
    if let Some(v) = req.per_turn_persist {
        config.per_turn_persist = v;
    }
    if let Some(v) = req.promote_min_evidence {
        config.promote_min_evidence = v;
    }
    if let Some(v) = req.promote_min_confidence {
        config.promote_min_confidence = v;
    }
    if let Some(v) = req.min_turn_chars {
        config.min_turn_chars = v;
    }
    if let Some(v) = req.auto_extract_enabled {
        config.auto_extract_enabled = v;
    }
    if let Some(v) = req.auto_extract_min_turns {
        config.auto_extract_min_turns = v.max(1);
    }
    if let Some(v) = req.auto_extract_min_user_chars {
        config.auto_extract_min_user_chars = v.max(1);
    }
    if let Some(v) = req.auto_extract_idle_secs {
        config.auto_extract_idle_secs = v.max(30);
    }
    if let Some(v) = &req.llm_model {
        let trimmed = v.trim();
        config.llm_model = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        };
    }
}

/// UI uses `keywords`; backend rule path accepts `keywords` and `rules`.
fn normalize_extract_mode(mode: &str) -> String {
    match mode.trim().to_ascii_lowercase().as_str() {
        "keywords" => "keywords".to_string(),
        "rules" => "rules".to_string(),
        "llm" => "llm".to_string(),
        "hybrid" => "hybrid".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomi_config::save_config_yaml;

    #[test]
    fn update_settings_persists_to_config_yaml_and_reload() {
        let root = tempfile::tempdir().unwrap();
        let poi_dir = root.path().join("poi");
        let config_path = root.path().join("config.yaml");

        let mut gateway = GatewayConfig::default();
        gateway.interest.auto_extract_min_turns = 4;
        save_config_yaml(&config_path, &gateway).unwrap();

        let service = PoiService::new(poi_dir.clone()).unwrap();
        assert_eq!(service.interest_config().auto_extract_min_turns, 4);

        let updated = service
            .update_settings(UpdatePoiSettingsRequest {
                auto_extract_min_turns: Some(2),
                auto_extract_enabled: Some(true),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(updated.auto_extract_min_turns, 2);

        let reloaded = load_user_config_file(&config_path).unwrap();
        assert_eq!(reloaded.interest.auto_extract_min_turns, 2);

        let service2 = PoiService::new(poi_dir).unwrap();
        assert_eq!(service2.interest_config().auto_extract_min_turns, 2);
        assert!(service2.interest_config().proactive_extraction_enabled());
    }

    #[test]
    fn update_settings_reload_merges_without_clobbering_other_sections() {
        let root = tempfile::tempdir().unwrap();
        let poi_dir = root.path().join("poi");
        let config_path = root.path().join("config.yaml");

        let mut gateway = GatewayConfig::default();
        gateway.insights.contribution.enabled = true;
        gateway.insights.contribution.min_work_turns = 7;
        save_config_yaml(&config_path, &gateway).unwrap();

        let service = PoiService::new(poi_dir).unwrap();
        service
            .update_settings(UpdatePoiSettingsRequest {
                enabled: Some(false),
                ..Default::default()
            })
            .unwrap();

        let reloaded = load_user_config_file(&config_path).unwrap();
        assert!(!reloaded.interest.enabled);
        assert!(reloaded.insights.contribution.enabled);
        assert_eq!(reloaded.insights.contribution.min_work_turns, 7);
    }
}
