//! Threshold-driven POI / insights extraction for active conversations.
//!
//! Replaces reliance on explicit session teardown (delete/reset/clear) by
//! flushing when turn/char thresholds are met or when a session goes idle.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use nomi_config::{InsightsContributionConfig, InterestConfig};
use nomi_insights_core::spawn_session_end_pipeline;
use nomi_poi::SessionPoiBuffer;
use nomifun_insights::InsightsService;
use nomifun_poi::PoiService;
use serde_json::Value;
use tracing::info;

use crate::auxiliary_provider::{try_build_auxiliary_client, AuxiliaryClientFactory};

pub type MessageLoader = Arc<
    dyn Fn(String) -> Pin<Box<dyn Future<Output = Option<Vec<Value>>> + Send>> + Send + Sync,
>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractionTrigger {
    TurnThreshold,
    IdleTimeout,
    SessionEnd,
}

#[derive(Debug, Default)]
struct ActiveSessionState {
    poi_buffer: SessionPoiBuffer,
    user_turns_since_flush: u32,
    user_chars_since_flush: usize,
    last_activity_ms: i64,
    message_count_at_last_flush: usize,
    flush_in_progress: bool,
}

/// Tracks per-conversation extraction progress and spawns pipelines.
pub struct ProactiveSessionExtractor {
    poi_service: Arc<PoiService>,
    insights_service: Arc<InsightsService>,
    auxiliary_factory: Option<Arc<AuxiliaryClientFactory>>,
    message_loader: Option<MessageLoader>,
    sessions: RwLock<HashMap<String, ActiveSessionState>>,
}

impl ProactiveSessionExtractor {
    pub fn new(
        poi_service: Arc<PoiService>,
        insights_service: Arc<InsightsService>,
        auxiliary_factory: Option<Arc<AuxiliaryClientFactory>>,
    ) -> Self {
        Self {
            poi_service,
            insights_service,
            auxiliary_factory,
            message_loader: None,
            sessions: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_message_loader(mut self, loader: MessageLoader) -> Self {
        self.message_loader = Some(loader);
        self
    }

    /// Record a user message and flush when proactive thresholds are met.
    pub async fn on_user_message(
        &self,
        session_id: &str,
        user_text: &str,
        message_count: usize,
    ) {
        if !self.proactive_enabled().await {
            return;
        }

        let now = now_ms();
        let user_chars = user_text.trim().chars().count();
        let should_flush = {
            let interest_cfg = self.poi_service.interest_config();
            let insights_cfg = self.insights_service.contribution_config().await;
            let mut sessions = self.sessions.write().expect("session extraction lock");
            let state = sessions.entry(session_id.to_owned()).or_default();
            state.last_activity_ms = now;

            if interest_cfg.proactive_extraction_enabled() {
                state.poi_buffer.absorb_turn(user_text, &interest_cfg);
            }

            state.user_turns_since_flush = state.user_turns_since_flush.saturating_add(1);
            state.user_chars_since_flush = state.user_chars_since_flush.saturating_add(user_chars);

            let should_flush = !state.flush_in_progress
                && turn_threshold_met(
                    &interest_cfg,
                    &insights_cfg,
                    state,
                    message_count,
                );
            should_flush
        };

        if should_flush {
            self.spawn_flush(session_id, message_count, ExtractionTrigger::TurnThreshold)
                .await;
        }
    }

    /// Flush a session that has been idle beyond configured thresholds.
    pub async fn flush_idle_session(&self, session_id: &str, message_count: usize) {
        if !self.proactive_enabled().await {
            return;
        }

        let now = now_ms();
        let should_flush = {
            let interest_cfg = self.poi_service.interest_config();
            let insights_cfg = self.insights_service.contribution_config().await;
            let mut sessions = self.sessions.write().expect("session extraction lock");
            let Some(state) = sessions.get_mut(session_id) else {
                return;
            };
            if state.flush_in_progress {
                return;
            }
            let idle_ms = effective_idle_secs(&interest_cfg, &insights_cfg).saturating_mul(1000);
            if now.saturating_sub(state.last_activity_ms) < idle_ms as i64 {
                return;
            }
            if state.user_turns_since_flush == 0 && state.poi_buffer.len() == 0 {
                return;
            }
            idle_threshold_met(state, message_count, &interest_cfg, &insights_cfg)
        };

        if should_flush {
            self.spawn_flush(session_id, message_count, ExtractionTrigger::IdleTimeout)
                .await;
        }
    }

    /// Final flush on explicit session teardown; removes tracked state afterward.
    pub async fn flush_on_session_end(&self, session_id: &str, messages: Vec<Value>) {
        self.flush_with_messages(session_id, messages, ExtractionTrigger::SessionEnd)
            .await;
        let mut sessions = self.sessions.write().expect("session extraction lock");
        sessions.remove(session_id);
    }

    pub fn remove_session(&self, session_id: &str) {
        let mut sessions = self.sessions.write().expect("session extraction lock");
        sessions.remove(session_id);
    }

    pub fn tracked_session_ids(&self) -> Vec<String> {
        self.sessions
            .read()
            .expect("session extraction lock")
            .keys()
            .cloned()
            .collect()
    }

    async fn proactive_enabled(&self) -> bool {
        let interest_cfg = self.poi_service.interest_config();
        let insights_cfg = self.insights_service.contribution_config().await;
        interest_cfg.proactive_extraction_enabled() || insights_cfg.proactive_extraction_enabled()
    }

    async fn spawn_flush(
        &self,
        session_id: &str,
        message_count: usize,
        trigger: ExtractionTrigger,
    ) {
        if message_count == 0 {
            return;
        }
        {
            let mut sessions = self.sessions.write().expect("session extraction lock");
            let Some(state) = sessions.get_mut(session_id) else {
                return;
            };
            if state.flush_in_progress {
                return;
            }
            state.flush_in_progress = true;
        }

        let loader = match &self.message_loader {
            Some(loader) => loader.clone(),
            None => {
                self.clear_flush_in_progress(session_id);
                return;
            }
        };

        let messages = match loader(session_id.to_owned()).await {
            Some(msgs) if !msgs.is_empty() => msgs,
            _ => {
                self.clear_flush_in_progress(session_id);
                return;
            }
        };

        self.flush_with_messages(session_id, messages, trigger).await;
    }

    async fn flush_with_messages(
        &self,
        session_id: &str,
        messages: Vec<Value>,
        trigger: ExtractionTrigger,
    ) {
        let interest_cfg = self.poi_service.interest_config();
        let insights_cfg = self.insights_service.contribution_config().await;
        if !interest_cfg.enabled && !insights_cfg.enabled {
            self.clear_flush_in_progress(session_id);
            return;
        }

        let buffered = {
            let mut sessions = self.sessions.write().expect("session extraction lock");
            sessions
                .get_mut(session_id)
                .map(|s| s.poi_buffer.drain())
                .unwrap_or_default()
        };

        let auxiliary = match &self.auxiliary_factory {
            Some(factory) => try_build_auxiliary_client(factory).await,
            None => None,
        };

        let message_count = messages.len();
        info!(
            session_id = %session_id,
            trigger = ?trigger,
            message_count,
            buffered_signals = buffered.len(),
            interest_enabled = interest_cfg.enabled,
            insights_enabled = insights_cfg.enabled,
            auxiliary = auxiliary.is_some(),
            "session_extraction: proactive flush"
        );

        if interest_cfg.enabled {
            let mut insights_off = insights_cfg.clone();
            insights_off.enabled = false;
            spawn_session_end_pipeline(
                self.poi_service.data_dir().to_path_buf(),
                interest_cfg,
                insights_off,
                session_id.to_owned(),
                messages.clone(),
                buffered,
                auxiliary.clone(),
            );
        }

        if insights_cfg.enabled {
            let mut interest_off = InterestConfig::default();
            interest_off.enabled = false;
            spawn_session_end_pipeline(
                self.insights_service.data_dir().to_path_buf(),
                interest_off,
                insights_cfg,
                session_id.to_owned(),
                messages,
                Vec::new(),
                auxiliary,
            );
        }

        {
            let mut sessions = self.sessions.write().expect("session extraction lock");
            if let Some(state) = sessions.get_mut(session_id) {
                state.user_turns_since_flush = 0;
                state.user_chars_since_flush = 0;
                state.message_count_at_last_flush = message_count;
                state.flush_in_progress = false;
            }
        }
    }

    fn clear_flush_in_progress(&self, session_id: &str) {
        let mut sessions = self.sessions.write().expect("session extraction lock");
        if let Some(state) = sessions.get_mut(session_id) {
            state.flush_in_progress = false;
        }
    }
}

fn turn_threshold_met(
    interest_cfg: &InterestConfig,
    insights_cfg: &InsightsContributionConfig,
    state: &ActiveSessionState,
    message_count: usize,
) -> bool {
    if message_count <= state.message_count_at_last_flush {
        return false;
    }

    let turn_threshold = effective_turn_threshold(interest_cfg, insights_cfg);
    let char_threshold = if interest_cfg.proactive_extraction_enabled() {
        interest_cfg.auto_extract_min_user_chars
    } else {
        usize::MAX
    };

    state.user_turns_since_flush >= turn_threshold
        || state.user_chars_since_flush >= char_threshold
        || (state.poi_buffer.len() > 0 && state.user_turns_since_flush >= turn_threshold)
}

fn idle_threshold_met(
    state: &ActiveSessionState,
    message_count: usize,
    interest_cfg: &InterestConfig,
    insights_cfg: &InsightsContributionConfig,
) -> bool {
    if message_count <= state.message_count_at_last_flush {
        return false;
    }
    let min_turns = effective_turn_threshold(interest_cfg, insights_cfg);
    state.user_turns_since_flush >= min_turns.saturating_sub(1).max(1)
        || state.poi_buffer.len() > 0
}

fn effective_turn_threshold(
    interest_cfg: &InterestConfig,
    insights_cfg: &InsightsContributionConfig,
) -> u32 {
    let mut threshold = u32::MAX;
    if interest_cfg.proactive_extraction_enabled() {
        threshold = threshold.min(interest_cfg.auto_extract_min_turns.max(1));
    }
    if insights_cfg.proactive_extraction_enabled() {
        threshold = threshold.min(insights_cfg.min_work_turns.max(1));
    }
    if threshold == u32::MAX {
        4
    } else {
        threshold
    }
}

fn effective_idle_secs(interest_cfg: &InterestConfig, insights_cfg: &InsightsContributionConfig) -> u64 {
    let mut idle = u64::MAX;
    if interest_cfg.proactive_extraction_enabled() {
        idle = idle.min(interest_cfg.auto_extract_idle_secs.max(30));
    }
    if insights_cfg.proactive_extraction_enabled() {
        idle = idle.min(insights_cfg.auto_extract_idle_secs.max(30));
    }
    if idle == u64::MAX {
        300
    } else {
        idle
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_threshold_respects_poi_and_insights_minimums() {
        let interest = InterestConfig {
            auto_extract_min_turns: 4,
            ..InterestConfig::default()
        };
        let insights = InsightsContributionConfig {
            enabled: true,
            auto_extract_enabled: true,
            min_work_turns: 2,
            ..InsightsContributionConfig::default()
        };
        assert_eq!(effective_turn_threshold(&interest, &insights), 2);
    }

    #[test]
    fn turn_threshold_met_after_enough_turns() {
        let interest = InterestConfig::default();
        let insights = InsightsContributionConfig::default();
        let state = ActiveSessionState {
            user_turns_since_flush: 4,
            user_chars_since_flush: 10,
            message_count_at_last_flush: 0,
            ..Default::default()
        };
        assert!(turn_threshold_met(&interest, &insights, &state, 5));
    }
}
