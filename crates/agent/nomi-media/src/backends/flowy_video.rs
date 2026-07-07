//! Flowy Seedance video generation backend.

use async_trait::async_trait;

use nomi_types::ToolError;
use nomifun_cloud::FlowyApiClient;
use nomifun_cloud::flowy::video_task_failure_message;
use nomifun_cloud::flowy::{
    VideoContentImage, VideoCreateParams, video_task_status_user_message_zh,
};
use nomi_tools::VideoGenerateBackend;
use nomi_tools::tools::video::VideoGenerateRequest;

use super::{FlowyMediaServices, map_server_err};
use crate::assets::persist_from_url;
use crate::delivery::{MediaProvenance, VideoTaskMeta, video_generation_response};
use crate::flowy_params::{normalize_video_duration, normalize_video_resolution};
use crate::progress::{report_media_progress, video_generate_started};
use crate::video_segment::normalize_video_first_frame_url;
use crate::workflows::control::WorkflowRunControl;

pub struct FlowyVideoGenBackend {
    services: FlowyMediaServices,
}

impl FlowyVideoGenBackend {
    pub fn new(services: FlowyMediaServices) -> Self {
        Self { services }
    }

    pub fn services(&self) -> &FlowyMediaServices {
        &self.services
    }

    pub async fn is_configured(services: &FlowyMediaServices) -> bool {
        services.is_authenticated().await
    }

    /// Workflow-aware generation with optional cancellation hooks.
    pub async fn generate_for_workflow(
        &self,
        request: VideoGenerateRequest,
        run_id: Option<&str>,
        control: Option<&WorkflowRunControl>,
    ) -> Result<String, ToolError> {
        self.generate_video_inner(request, run_id, control).await
    }
}

#[async_trait]
impl VideoGenerateBackend for FlowyVideoGenBackend {
    async fn generate_video(&self, request: VideoGenerateRequest) -> Result<String, ToolError> {
        self.generate_video_inner(request, None, None).await
    }
}

impl FlowyVideoGenBackend {
    async fn generate_video_inner(
        &self,
        request: VideoGenerateRequest,
        run_id: Option<&str>,
        control: Option<&WorkflowRunControl>,
    ) -> Result<String, ToolError> {
        self.services.require_token().await?;

        let model = self
            .services
            .resolve_video_model(request.model.as_deref())
            .await?;

        let raw_duration = request
            .duration
            .or(Some(self.services.media.video.default_duration));
        let duration = raw_duration.map(|d| normalize_video_duration(&model, d));
        let duration_for_credits = duration.unwrap_or(self.services.media.video.default_duration);
        self.services
            .ensure_video_credits(duration_for_credits)
            .await?;

        let aspect_ratio = if request.aspect_ratio.trim().is_empty() {
            self.services.media.video.default_aspect_ratio.clone()
        } else {
            request.aspect_ratio.clone()
        };

        let resolution_input = if request.resolution.trim().is_empty() {
            self.services.media.video.default_resolution.as_str()
        } else {
            request.resolution.as_str()
        };
        let resolution = normalize_video_resolution(&model, resolution_input);

        let mut images = Vec::new();
        if let Some(raw_url) = request
            .image_url
            .as_deref()
            .filter(|u| !u.trim().is_empty())
        {
            let url = match normalize_video_first_frame_url(raw_url) {
                Ok(normalized) => normalized,
                Err(err) => {
                    tracing::warn!(error = %err, "invalid first_frame image_url for video task");
                    return Err(err);
                }
            };
            images.push(VideoContentImage {
                url,
                role: "first_frame".into(),
            });
        }
        if let Some(url) = request
            .last_frame_url
            .as_deref()
            .filter(|u| !u.trim().is_empty())
        {
            images.push(VideoContentImage {
                url: url.to_string(),
                role: "last_frame".into(),
            });
        }
        for url in &request.reference_image_urls {
            if url.trim().is_empty() {
                continue;
            }
            images.push(VideoContentImage {
                url: url.clone(),
                role: "reference_image".into(),
            });
        }

        let params = VideoCreateParams {
            model: model.clone(),
            prompt: request.prompt.clone(),
            duration,
            aspect_ratio,
            resolution: resolution.map(|s| s.to_string()),
            negative_prompt: request.negative_prompt.clone(),
            seed: request.seed,
            watermark: false,
            generate_audio: request.generate_audio.or(request.audio),
            images,
            reference_video_url: request.reference_video_url.clone(),
            reference_audio_url: request.reference_audio_url.clone(),
        };

        let body = FlowyApiClient::build_video_create_params(params);

        let has_image = request.image_url.is_some();
        report_media_progress(video_generate_started(has_image, duration_for_credits));

        let poll_timeout = self.services.media.video.poll_timeout_seconds.max(30);
        let cancel_ctx = match (run_id, control) {
            (Some(id), Some(ctl)) => Some((id.to_string(), ctl.clone())),
            _ => None,
        };
        let should_cancel = cancel_ctx.as_ref().map(|(id, ctl)| {
            let id = id.clone();
            let ctl = ctl.clone();
            std::sync::Arc::new(move || ctl.is_cancelled(&id))
                as std::sync::Arc<dyn Fn() -> bool + Send + Sync>
        });
        let on_task_created = cancel_ctx.map(|(id, ctl)| {
            Box::new(move |local_id: i64| {
                ctl.set_active_video_task(&id, local_id);
            }) as Box<dyn FnMut(i64) + Send>
        });
        let record = self
            .services
            .api
            .generate_video_with_timeout_and_progress_cancellable(
                &self.services.session,
                body,
                poll_timeout,
                Some(Box::new(
                    |task: &nomifun_cloud::flowy::VideoTaskRecord, elapsed| {
                        report_media_progress(video_task_status_user_message_zh(
                            task.status,
                            elapsed,
                        ));
                    },
                )),
                should_cancel,
                on_task_created,
            )
            .await
            .map_err(map_server_err)?;

        if let (Some(id), Some(ctl)) = (run_id, control) {
            ctl.clear_active_video_task(id);
        }

        if record.is_success() {
            report_media_progress("视频已生成，正在保存到本地…");
        }

        if !record.is_success() {
            return Err(ToolError::ExecutionFailed(video_task_failure_message(
                &record,
            )));
        }

        let video_url = record.video_url().ok_or_else(|| {
            ToolError::ExecutionFailed("video task succeeded but no video_url in result".into())
        })?;

        let mut local_artifact = None;
        let mut persist_warning = None;
        if self.services.media.video.save_locally {
            match persist_from_url(&video_url, "flowy", &model).await {
                Ok(artifact) => local_artifact = Some(artifact),
                Err(err) => {
                    persist_warning = Some(err.to_string());
                    tracing::warn!(
                        error = %err,
                        video_url = %video_url,
                        "video generated but local persist failed; returning remote URL"
                    );
                }
            }
        }

        let task = VideoTaskMeta {
            local_id: record.id.to_string(),
            task_id: record.task_id.clone().unwrap_or_default(),
            status: record.status,
        };

        Ok(video_generation_response(
            &model,
            &video_url,
            local_artifact.as_ref(),
            &task,
            MediaProvenance::for_api_call(request.prompt, request.negative_prompt, None, None),
            persist_warning.as_deref(),
        ))
    }
}
