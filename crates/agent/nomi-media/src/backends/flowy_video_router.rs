//! Routes `video_generate` to long-video workflow when duration exceeds Seedance clip limit.

use std::sync::Arc;

use async_trait::async_trait;
use nomi_types::ToolError;
use nomi_tools::VideoGenerateBackend;
use nomi_tools::tools::video::VideoGenerateRequest;
use tracing::info;

use super::flowy_video::FlowyVideoGenBackend;
use crate::long_video_active::find_resumable_long_video_run;
use crate::long_video_plan::{
    build_long_video_plan_from_request, resolve_target_duration, video_request_needs_long_pipeline,
    video_tool_response_from_workflow,
};
use crate::progress::report_media_progress;
use crate::video_segment::{long_video_work_dir, read_long_video_checkpoint};
use crate::workflows::runner::WorkflowRunner;

/// Flowy video backend that transparently runs long-video workflows for >10s targets.
pub struct FlowyVideoGenerateRouter {
    inner: FlowyVideoGenBackend,
    runner: Arc<WorkflowRunner>,
}

impl FlowyVideoGenerateRouter {
    pub fn new(inner: FlowyVideoGenBackend, runner: Arc<WorkflowRunner>) -> Self {
        Self { inner, runner }
    }
}

#[async_trait]
impl VideoGenerateBackend for FlowyVideoGenerateRouter {
    async fn generate_video(&self, request: VideoGenerateRequest) -> Result<String, ToolError> {
        let model = self
            .inner
            .services()
            .resolve_video_model(request.model.as_deref())
            .await?;
        let default_duration = self.inner.services().media.video.default_duration;

        if !video_request_needs_long_pipeline(&request, &model, default_duration) {
            return self.inner.generate_video(request).await;
        }

        let target = resolve_target_duration(request.duration, &request.prompt, default_duration);
        if let Some(prior) =
            find_resumable_long_video_run(self.runner.store().as_ref(), Some(target))
        {
            info!(
                run_id = %prior.run_id,
                target_duration = target,
                "video_generate auto-resuming incomplete long-video workflow"
            );
            let (done, total) = read_long_video_checkpoint(&long_video_work_dir(&prior.run_id))
                .map(|cp| (cp.next_segment_index, cp.segment_total()))
                .unwrap_or((0, 0));
            report_media_progress(format!(
                "检测到未完成的长视频任务（已完成 {done}/{total} 段），正在续传并拼接…"
            ));
            let record = self.runner.resume_run_sync(&prior.run_id).await?;
            return video_tool_response_from_workflow(&record);
        }

        info!(
            target_duration = target,
            model = %model,
            "video_generate routing to long-video workflow"
        );
        report_media_progress(format!(
            "目标时长 {target} 秒超过 Seedance 单次上限，自动分段生成长视频…"
        ));

        let plan = build_long_video_plan_from_request(&request, target, &model)?;
        let record = self.runner.run_plan_sync(&plan).await?;
        video_tool_response_from_workflow(&record)
    }
}
