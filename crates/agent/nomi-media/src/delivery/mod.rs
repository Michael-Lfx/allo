//! Unified tool response shape for image/video generation.

mod prompts;

use serde::Serialize;
use serde_json::{Value, json};

pub use prompts::{
    ApiPromptEntry, enrich_tool_response_with_prompts, extract_prompt_trail_from_workflow,
    format_user_prompt_block, format_workflow_user_prompt_block, workflow_prompt_json,
};

use crate::assets::MediaArtifact;

#[derive(Debug, Clone, Serialize)]
pub struct MediaAssetDelivery {
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_hint: Option<String>,
    pub mime: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<f32>,
    pub provider: String,
    pub model: String,
    pub job_id: String,
}

impl From<&MediaArtifact> for MediaAssetDelivery {
    fn from(a: &MediaArtifact) -> Self {
        let local = (!a.local_path.as_os_str().is_empty())
            .then(|| a.local_path.to_string_lossy().into_owned())
            .filter(|p| std::path::Path::new(p).is_file());
        Self {
            kind: if a.mime.starts_with("video/") {
                "video"
            } else {
                "image"
            },
            url: a.remote_url.clone(),
            local_path: local.clone(),
            media_hint: local.as_ref().map(|p| format!("MEDIA:{p}")),
            mime: a.mime.clone(),
            width: a.width,
            height: a.height,
            duration_secs: a.duration_secs,
            provider: a.provider.clone(),
            model: a.model.clone(),
            job_id: a.job_id.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct MediaProvenance {
    /// User objective before workflow refinement (when known).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_prompt: Option<String>,
    /// Exact prompt string sent to the image/video generation API.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refined_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub motion_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub negative_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
}

impl MediaProvenance {
    pub fn for_api_call(
        api_prompt: impl Into<String>,
        negative_prompt: Option<String>,
        motion_prompt: Option<String>,
        original_prompt: Option<String>,
    ) -> Self {
        let api_prompt = api_prompt.into();
        Self {
            original_prompt,
            api_prompt: Some(api_prompt.clone()),
            prompt: Some(api_prompt.clone()),
            refined_prompt: Some(api_prompt),
            motion_prompt,
            negative_prompt,
            ..Default::default()
        }
    }
}

/// Build a standard image generation JSON response.
pub fn image_generation_response(
    model: &str,
    artifacts: &[MediaArtifact],
    upstream: &Value,
    provenance: MediaProvenance,
) -> String {
    let assets: Vec<MediaAssetDelivery> = artifacts.iter().map(MediaAssetDelivery::from).collect();
    let media_hint = assets
        .iter()
        .find_map(|a| a.media_hint.clone())
        .map(Value::String)
        .unwrap_or(Value::Null);
    let images: Vec<Value> = assets
        .iter()
        .map(|a| {
            json!({
                "url": a.url,
                "local_path": a.local_path,
                "media_hint": a.media_hint,
                "mime": a.mime,
                "width": a.width,
                "height": a.height,
                "provider": a.provider,
                "model": a.model,
                "job_id": a.job_id,
            })
        })
        .collect();
    let user_prompt_block = format_user_prompt_block(&provenance);
    let prompts = json!({
        "original_prompt": provenance.original_prompt.clone(),
        "api_prompt": provenance.api_prompt.clone(),
        "refined_prompt": provenance.refined_prompt.clone(),
        "negative_prompt": provenance.negative_prompt.clone(),
        "motion_prompt": provenance.motion_prompt.clone(),
    });
    let mut body = json!({
        "success": true,
        "kind": "image",
        "assets": assets,
        "images": images,
        "transport": "flowy",
        "model": model,
        "upstream": upstream,
        "provenance": provenance,
        "prompts": prompts,
        "media_hint": media_hint,
    });
    if let Some(block) = user_prompt_block {
        body["user_prompt_block"] = json!(block);
        body["delivery_note"] = json!(
            "Include user_prompt_block in your reply so the user sees the final API prompt. \
             Deliver the image with MEDIA: when local_path is available."
        );
    }
    body.to_string()
}

/// Build a standard video generation JSON response.
pub fn video_generation_response(
    model: &str,
    video_url: &str,
    artifact: Option<&MediaArtifact>,
    task: &VideoTaskMeta,
    provenance: MediaProvenance,
    persist_warning: Option<&str>,
) -> String {
    let assets: Vec<MediaAssetDelivery> = artifact
        .map(|a| vec![MediaAssetDelivery::from(a)])
        .unwrap_or_else(|| {
            vec![MediaAssetDelivery {
                kind: "video",
                url: Some(video_url.to_string()),
                local_path: None,
                media_hint: None,
                mime: "video/mp4".into(),
                width: None,
                height: None,
                duration_secs: None,
                provider: "flowy".into(),
                model: model.to_string(),
                job_id: task.task_id.clone(),
            }]
        });
    let media_hint = assets
        .first()
        .and_then(|a| a.media_hint.clone())
        .map(Value::String)
        .unwrap_or(Value::Null);
    let user_prompt_block = format_user_prompt_block(&provenance);
    let prompts = json!({
        "original_prompt": provenance.original_prompt.clone(),
        "api_prompt": provenance.api_prompt.clone(),
        "refined_prompt": provenance.refined_prompt.clone(),
        "negative_prompt": provenance.negative_prompt.clone(),
        "motion_prompt": provenance.motion_prompt.clone(),
    });
    let mut body = json!({
        "success": true,
        "kind": "video",
        "assets": assets,
        "video": video_url,
        "local_path": assets.first().and_then(|a| a.local_path.clone()),
        "provider": "flowy",
        "model": model,
        "task_id": task.local_id,
        "upstream_task_id": task.task_id,
        "status": task.status,
        "provenance": provenance,
        "prompts": prompts,
        "persist_warning": persist_warning,
        "media_hint": media_hint,
    });
    let has_user_prompt_block = user_prompt_block.is_some();
    if let Some(block) = user_prompt_block {
        body["user_prompt_block"] = json!(block);
    }
    if media_hint.is_null() {
        body["delivery_note"] = json!(
            "Video URL is available; share user_prompt_block and the link, or retry if MEDIA: path is needed."
        );
    } else if has_user_prompt_block {
        body["delivery_note"] = json!(
            "Include user_prompt_block in your reply. Deliver the video with MEDIA: when local_path is available."
        );
    }
    body.to_string()
}

pub struct VideoTaskMeta {
    pub local_id: String,
    pub task_id: String,
    pub status: i32,
}
