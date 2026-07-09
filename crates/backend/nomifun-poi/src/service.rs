//! Business logic for POI topic list, pin, status, and settings.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use nomi_config::InterestConfig;
use nomi_poi::{InterestStore, TopicStatus};
use nomifun_api_types::{
    PoiSettingsResponse, PoiStatusResponse, PoiTopicListResponse, PoiTopicResponse,
    UpdatePoiSettingsRequest,
};
use nomifun_common::AppError;

#[derive(Clone)]
pub struct PoiService {
    data_dir: PathBuf,
    config: Arc<RwLock<InterestConfig>>,
    store: Arc<Mutex<InterestStore>>,
}

impl PoiService {
    pub fn open(data_dir: impl AsRef<Path>, config: InterestConfig) -> Result<Self, AppError> {
        let data_dir = data_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&data_dir)
            .map_err(|e| AppError::Internal(format!("create data dir: {e}")))?;
        let db_path = data_dir.join("interest.db");
        let store = InterestStore::open(&db_path, config.clone())
            .map_err(|e| AppError::Internal(format!("open interest store: {e}")))?;
        Ok(Self {
            data_dir,
            config: Arc::new(RwLock::new(config)),
            store: Arc::new(Mutex::new(store)),
        })
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn store(&self) -> Arc<Mutex<InterestStore>> {
        Arc::clone(&self.store)
    }

    pub fn interest_config(&self) -> InterestConfig {
        self.config
            .read()
            .map(|c| c.clone())
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
        let config = self
            .config
            .read()
            .map_err(|e| AppError::Internal(format!("interest config lock: {e}")))?;
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
        let config = self
            .config
            .read()
            .map_err(|e| AppError::Internal(format!("interest config lock: {e}")))?;
        Ok(settings_from_config(&config))
    }

    pub fn update_settings(&self, req: UpdatePoiSettingsRequest) -> Result<PoiSettingsResponse, AppError> {
        if req.is_empty() {
            return Err(AppError::BadRequest("no settings fields provided".into()));
        }
        let mut config = self
            .config
            .write()
            .map_err(|e| AppError::Internal(format!("interest config lock: {e}")))?;
        apply_settings_patch(&mut config, &req);
        let store = InterestStore::open(&self.db_path(), config.clone())
            .map_err(|e| AppError::Internal(format!("reopen interest store: {e}")))?;
        *self
            .store
            .lock()
            .map_err(|e| AppError::Internal(format!("interest store lock: {e}")))? = store;
        Ok(settings_from_config(&config))
    }

    pub fn clear_topics(&self) -> Result<(), AppError> {
        let path = self.db_path();
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| AppError::Internal(format!("clear interest store: {e}")))?;
        }
        let config = self
            .config
            .read()
            .map_err(|e| AppError::Internal(format!("interest config lock: {e}")))?
            .clone();
        let store = InterestStore::open(&path, config)
            .map_err(|e| AppError::Internal(format!("reopen interest store: {e}")))?;
        *self
            .store
            .lock()
            .map_err(|e| AppError::Internal(format!("interest store lock: {e}")))? = store;
        Ok(())
    }
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
        config.extract_mode = v.clone();
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
        config.auto_extract_min_turns = v;
    }
    if let Some(v) = req.auto_extract_min_user_chars {
        config.auto_extract_min_user_chars = v;
    }
    if let Some(v) = req.auto_extract_idle_secs {
        config.auto_extract_idle_secs = v;
    }
}
