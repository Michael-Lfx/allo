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
    /// Aggregate Flowy video-task credits for this session (persisted).
    #[serde(default, skip_serializing_if = "is_zero_i64")]
    pub credits_consumed: i64,
    /// Absolute session working directory (for UI / debugging).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_dir_abs: Option<String>,
    /// RFC3339 timestamp of the last status / progress update.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<ProgressEvent>,
}

fn is_zero_i64(v: &i64) -> bool {
    *v == 0
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
        self.push_event(stage, message, metadata);
    }

    /// Append a terminal event (`cancelled` / `interrupted` / `failed`) without
    /// overwriting the pipeline stage — resume needs the last working stage.
    pub fn emit_terminal(&mut self, stage: &str, message: &str) {
        self.message = message.to_string();
        self.push_event(stage, message, None);
    }

    fn push_event(&mut self, stage: &str, message: &str, metadata: Option<Value>) {
        self.touch();
        // Collapse consecutive identical stages (e.g. parallel plan_scene fan-out,
        // video_poll heartbeats) so the activity log stays readable.
        if let Some(last) = self.events.last_mut() {
            if last.stage == stage {
                last.message = message.to_string();
                last.metadata = metadata;
                return;
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emit_collapses_consecutive_same_stage() {
        let mut status = RenderStatus::default();
        status.emit("plan_scene", "one", None);
        status.emit("plan_scene", "two", Some(serde_json::json!({ "progress": 10 })));
        status.emit("plan_scene", "three", None);
        status.emit("planned", "done", None);

        assert_eq!(status.events.len(), 2);
        assert_eq!(status.events[0].stage, "plan_scene");
        assert_eq!(status.events[0].message, "three");
        assert_eq!(status.events[1].stage, "planned");
    }

    #[test]
    fn emit_terminal_preserves_pipeline_stage() {
        let mut status = RenderStatus::default();
        status.emit("video_poll", "waiting", None);
        status.emit_terminal("cancelled", "cancelled");

        assert_eq!(status.stage, "video_poll");
        assert_eq!(status.events.len(), 2);
        assert_eq!(status.events[1].stage, "cancelled");
    }
}
