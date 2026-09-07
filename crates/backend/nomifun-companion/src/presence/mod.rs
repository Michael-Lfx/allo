//! Presence runtime: needs tick, mood derive, ABC interaction, activity compose.
//!
//! Pure functions plus a small persisted record. The Tauri overlay and `/nomi`
//! panel never compute decay themselves — they refetch `GET …/status`.

mod interaction;
mod pack;
mod stats;

pub use interaction::{apply_pointer, expire_stale_interaction};
pub use pack::{builtin_pack_for_character, load_pack_dir, validate_pack_dir};
pub use stats::{derive_mood, nudge_from_learn_mood, stats_matching_mood, tick_stats};

use nomifun_api_types::{
    clip_for_phase, compose_activity, CompanionActivity, CompanionHitArea, CompanionMood,
    CompanionPackRuntime, HitIntent, InteractionPhase, PresenceAgentSnapshot, PresenceState,
    PresenceStats,
};
use serde::{Deserialize, Serialize};

/// `companion_runtime_state` key for the JSON [`PresenceRecord`].
pub const PRESENCE_KEY: &str = "presence";

/// Background tick cadence. GET status also catch-up ticks, so WS loss is recoverable.
pub const TICK_INTERVAL_SECS: u64 = 20;

/// Cap catch-up decay so a week offline does not dump every stat to zero in one read.
pub const MAX_TICK_DT_SECS: f64 = 6.0 * 3600.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresenceRecord {
    pub stats: PresenceStats,
    pub activity: CompanionActivity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_clip: Option<String>,
    pub interaction: InteractionPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interaction_intent: Option<HitIntent>,
    pub last_tick_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_interact_ms: Option<i64>,
}

impl Default for PresenceRecord {
    fn default() -> Self {
        Self {
            stats: PresenceStats::default(),
            activity: CompanionActivity::Idle,
            active_clip: Some(CompanionActivity::Idle.as_str().to_owned()),
            interaction: InteractionPhase::Idle,
            interaction_intent: None,
            last_tick_ms: 0,
            last_interact_ms: None,
        }
    }
}

impl PresenceRecord {
    pub fn parse(raw: Option<&str>) -> Self {
        let Some(raw) = raw.filter(|value| !value.trim().is_empty()) else {
            return Self::default();
        };
        serde_json::from_str(raw).unwrap_or_default()
    }

    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string(self).map_err(|error| error.to_string())
    }

    pub fn mood(&self) -> CompanionMood {
        derive_mood(&self.stats, self.last_interact_ms, self.last_tick_ms)
    }

    pub fn into_state(self) -> PresenceState {
        let mood = self.mood();
        PresenceState {
            stats: self.stats,
            mood,
            activity: self.activity,
            active_clip: self.active_clip,
            interaction: self.interaction,
            interaction_intent: self.interaction_intent,
            last_tick_ms: self.last_tick_ms,
            last_interact_ms: self.last_interact_ms,
        }
    }
}

/// Catch up decay, expire a stuck ABC, then bind agent activity.
pub fn refresh_record(
    mut record: PresenceRecord,
    now_ms: i64,
    quiet: bool,
    snapshot: &PresenceAgentSnapshot,
) -> PresenceRecord {
    record = catch_up_tick(record, now_ms, quiet);
    record = expire_stale_interaction(record, now_ms);
    record.activity = compose_activity(record.interaction, snapshot);
    record.active_clip = match record.interaction_intent {
        Some(intent) => clip_for_phase(intent, record.interaction)
            .or_else(|| Some(record.activity.as_str().to_owned())),
        None => Some(record.activity.as_str().to_owned()),
    };
    record
}

pub fn catch_up_tick(mut record: PresenceRecord, now_ms: i64, quiet: bool) -> PresenceRecord {
    if record.last_tick_ms <= 0 {
        record.last_tick_ms = now_ms;
        return record;
    }
    if now_ms <= record.last_tick_ms {
        return record;
    }
    let dt_secs = ((now_ms - record.last_tick_ms) as f64 / 1000.0).min(MAX_TICK_DT_SECS);
    if dt_secs < TICK_INTERVAL_SECS as f64 {
        return record;
    }
    record.stats = tick_stats(
        record.stats,
        dt_secs,
        quiet,
        now_ms,
        record.last_interact_ms,
    );
    record.last_tick_ms = now_ms;
    record
}

pub fn hit_areas_for(pack: &CompanionPackRuntime) -> Vec<CompanionHitArea> {
    if pack.hit_areas.is_empty() {
        nomifun_api_types::default_hit_areas()
    } else {
        pack.hit_areas.clone()
    }
}

pub async fn apply_learn_mood_hint(
    store: &crate::store::CompanionStore,
    companion_id: &str,
    mood_word: &str,
) -> Result<CompanionMood, nomifun_common::AppError> {
    let raw = store.get_companion_state(companion_id, PRESENCE_KEY).await?;
    let mut record = PresenceRecord::parse(raw.as_deref());
    record.stats = nudge_from_learn_mood(record.stats, mood_word);
    let mood = record.mood();
    store
        .set_companion_state(companion_id, PRESENCE_KEY, &record.to_json().map_err(nomifun_common::AppError::Internal)?)
        .await?;
    Ok(mood)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomifun_api_types::PresencePointerEvent;

    #[test]
    fn catch_up_does_not_decay_first_read() {
        let record = catch_up_tick(PresenceRecord::default(), 1_000, false);
        assert_eq!(record.stats, PresenceStats::default());
        assert_eq!(record.last_tick_ms, 1_000);
    }

    #[test]
    fn refresh_binds_agent_over_idle() {
        let record = refresh_record(
            PresenceRecord::default(),
            1_000,
            false,
            &PresenceAgentSnapshot {
                conversation_processing: true,
                ..PresenceAgentSnapshot::default()
            },
        );
        assert_eq!(record.activity, CompanionActivity::Busy);
        assert_eq!(record.active_clip.as_deref(), Some("busy"));
    }

    #[test]
    fn pointer_down_wins_over_agent() {
        let after_down = apply_pointer(
            PresenceRecord::default(),
            HitIntent::Head,
            PresencePointerEvent::Down,
            50_000,
        );
        let refreshed = refresh_record(
            after_down,
            50_000,
            false,
            &PresenceAgentSnapshot {
                execution_status: Some("running".into()),
                ..PresenceAgentSnapshot::default()
            },
        );
        assert_eq!(refreshed.activity, CompanionActivity::Interacting);
        assert_eq!(refreshed.active_clip.as_deref(), Some("touch_head_a"));
    }
}
