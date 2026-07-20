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
    #[default]
    Idle,
    Planning,
    Rendering,
    Succeeded,
    Failed,
    Cancelled,
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
        }
    }
}

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
    pub fn emit(&mut self, stage: &str, message: &str, metadata: Option<Value>) {
        self.stage = stage.to_string();
        self.message = message.to_string();
        self.events.push(ProgressEvent {
            stage: stage.to_string(),
            message: message.to_string(),
            metadata,
            at: chrono::Local::now().to_rfc3339(),
        });
        // Cap event log so status payloads stay bounded.
        if self.events.len() > 200 {
            let drain = self.events.len() - 200;
            self.events.drain(0..drain);
        }
    }
}
