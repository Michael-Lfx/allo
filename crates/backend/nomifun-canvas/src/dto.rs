//! Wire DTOs for `/api/video-canvas/*` (snake_case JSON).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasProjectMeta {
    pub project_id: String,
    pub title: String,
    pub node_count: u32,
    pub created_at: i64,
    pub updated_at: i64,
    /// When set, this canvas was materialized from a ViMax Agent session.
    /// Used to keep 「打开到 Canvas」 idempotent (one session → one project).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_vimax_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasMediaMeta {
    pub media_id: String,
    pub kind: String,
    pub title: String,
    pub mime: String,
    pub bytes: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u64>,
    /// Capability URL path: `/api/video-canvas/media/{id}`
    pub url: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasTranscription {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineExportClip {
    pub media_id: String,
    #[serde(default)]
    pub source_start_ms: Option<u64>,
    pub duration_ms: u64,
    #[serde(default)]
    pub gap_before_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationTaskStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationTaskView {
    pub task_id: String,
    pub status: GenerationTaskStatus,
    pub mode: String,
    pub prompt: String,
    pub model: Option<String>,
    pub progress: f32,
    pub error: Option<String>,
    pub result_media_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reference_media_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_frame_media_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_frame_media_id: Option<String>,
    /// Canvas project this job belongs to. Empty for home 「视频生成」 clips.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}
