use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type ProgressCallback = Arc<dyn Fn(&str, &str, Option<Value>) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Researching,
    Scripting,
    Aligning,
    Composing,
    Succeeded,
    Failed,
    Hold,
    Cancelled,
    Interrupted,
    #[default]
    #[serde(other)]
    Idle,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Researching => "researching",
            Self::Scripting => "scripting",
            Self::Aligning => "aligning",
            Self::Composing => "composing",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Hold => "hold",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }

    pub fn is_active(self) -> bool {
        matches!(
            self,
            Self::Researching | Self::Scripting | Self::Aligning | Self::Composing
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProgressEvent {
    pub stage: String,
    pub message: String,
    #[serde(default)]
    pub metadata: Option<Value>,
    #[serde(default)]
    pub at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RunSnapshot {
    pub status: RunStatus,
    #[serde(default)]
    pub stage: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub events: Vec<ProgressEvent>,
    #[serde(default)]
    pub final_video: Option<String>,
    #[serde(default)]
    pub updated_at: String,
}

impl RunSnapshot {
    pub fn emit(&mut self, stage: &str, message: &str) {
        self.stage = stage.to_string();
        self.message = message.to_string();
        self.updated_at = chrono::Local::now().to_rfc3339();
        self.events.push(ProgressEvent {
            stage: stage.to_string(),
            message: message.to_string(),
            metadata: None,
            at: self.updated_at.clone(),
        });
        if self.events.len() > 200 {
            let drain = self.events.len() - 200;
            self.events.drain(0..drain);
        }
    }
}

#[derive(Debug, Clone)]
pub struct BriefingTerminalTelemetry {
    pub briefing_id: String,
    pub status: RunStatus,
    pub research_depth: String,
    pub beat_count: i64,
    pub citation_count: i64,
    pub credits_consumed: i64,
    pub duration_ms: i64,
    pub error_code: Option<String>,
    pub occurred_at: String,
}

/// Briefing terminal events. Never returns `film_*`.
pub fn briefing_event_name(status: RunStatus) -> Option<&'static str> {
    match status {
        RunStatus::Succeeded => Some("briefing_succeeded"),
        RunStatus::Failed | RunStatus::Hold => Some("briefing_failed"),
        RunStatus::Cancelled | RunStatus::Interrupted => Some("briefing_cancelled"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_names_are_briefing_not_film() {
        assert_eq!(briefing_event_name(RunStatus::Succeeded), Some("briefing_succeeded"));
        assert_eq!(briefing_event_name(RunStatus::Hold), Some("briefing_failed"));
        assert_ne!(briefing_event_name(RunStatus::Succeeded), Some("film_succeeded"));
    }
}
