//! Progress callbacks and render status for ViMax pipelines / UI polling.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Pipeline progress hook: `(stage, message, optional metadata)`.
pub type ProgressCallback = Arc<dyn Fn(&str, &str, Option<Value>) + Send + Sync>;

/// Run status mirrored by `GET /api/vimax/sessions/:id/status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Planning,
    Rendering,
    Succeeded,
    Failed,
    Cancelled,
    /// App quit / crash while a job was active — not running; user can resume.
    Interrupted,
    /// Also absorbs unknown persisted variants so one new status cannot fail the whole index.
    #[default]
    #[serde(other)]
    Idle,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Planning => "planning",
            Self::Rendering => "rendering",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Planning | Self::Rendering)
    }
}

/// Persisted summary when a run is paused because the process exited.
pub const INTERRUPTED_SUMMARY: &str = "应用已退出，任务已暂停。可从断点继续。";


#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RenderStatus {
    pub status: RunStatus,
    #[serde(default)]
    pub stage: String,
    #[serde(default)]
    pub message: String,
    /// 0.0–100.0 progress percentage when known.
    #[serde(default)]
    pub progress: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_video: Option<String>,
    /// Relative path to film poster image (not part of the muxed video).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    /// Absolute session working directory (for UI / debugging).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_dir_abs: Option<String>,
    /// RFC3339 timestamp of the last status / progress update.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<ProgressEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressEvent {
    pub stage: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    #[serde(default)]
    pub at: String,
}

impl RenderStatus {
    pub fn touch(&mut self) {
        self.updated_at = chrono::Local::now().to_rfc3339();
    }

    pub fn emit(&mut self, stage: &str, message: &str, metadata: Option<Value>) {
        self.stage = stage.to_string();
        self.message = message.to_string();
        self.touch();
        self.events.push(ProgressEvent {
            stage: stage.to_string(),
            message: message.to_string(),
            metadata,
            at: self.updated_at.clone(),
        });
        // Cap event log so status payloads stay bounded.
        if self.events.len() > 200 {
            let drain = self.events.len() - 200;
            self.events.drain(0..drain);
        }
    }
}
