//! Action imitation — one character still + one motion video → a single clip.
//!
//! No LLM planning, no storyboard, no image model. The clip prompt is built-in;
//! duration follows the reference video (clamped to MiniMax-H3 4–15s).

use std::path::{Path, PathBuf};

use nomifun_cloud::{
    clamp_minimax_h3_duration, is_minimax_h3_model, MINIMAX_H3_DURATION_MAX, MINIMAX_H3_DURATION_MIN,
};

use crate::agents::{ensure_cover_from_final_video, COVER_FILENAME};
use crate::error::{VimaxError, VimaxResult};
use crate::media_local;
use crate::progress::ProgressCallback;
use crate::session::action_assets::{
    self, builtin_prompt, CHARACTER_STEM, FINAL_VIDEO_FILENAME, PROMPT_FILENAME, REFERENCE_STEM,
};
use crate::session::write_text_artifact;

use super::{PipelineBackends, emit_pct};

pub struct Action2VideoPipeline {
    backends: PipelineBackends,
    working_dir: PathBuf,
}

impl Action2VideoPipeline {
    pub fn new(backends: PipelineBackends, working_dir: PathBuf) -> Self {
        Self {
            backends,
            working_dir,
        }
    }

    /// Validate inputs and write the locked prompt (no LLM).
    pub async fn prepare(&self, progress: Option<ProgressCallback>) -> VimaxResult<u32> {
        tokio::fs::create_dir_all(&self.working_dir).await?;
        emit_pct(&progress, "action_prepare", "正在校验角色图与参考视频", 15.0);
        let (_character, video) = action_assets::require_assets(&self.working_dir)?;
        let duration = duration_from_reference(&video).await;
        write_text_artifact(
            &self.working_dir.join("target_duration_secs.txt"),
            &duration.to_string(),
        )
        .await?;
        write_text_artifact(&self.working_dir.join(PROMPT_FILENAME), builtin_prompt()).await?;
        emit_pct(
            &progress,
            "planned",
            &format!("素材已就绪，将生成约 {duration} 秒成片"),
            100.0,
        );
        Ok(duration)
    }

    /// Upload refs and generate a single clip. Resume-safe: existing `final_video.mp4` is kept.
    pub async fn render(
        &self,
        progress: Option<ProgressCallback>,
    ) -> VimaxResult<PathBuf> {
        if self.backends.is_cancelled() {
            return Err(VimaxError::Cancelled);
        }
        tokio::fs::create_dir_all(&self.working_dir).await?;
        let (character, video) = action_assets::require_assets(&self.working_dir)?;
        let duration = duration_from_reference(&video).await;
        write_text_artifact(
            &self.working_dir.join("target_duration_secs.txt"),
            &duration.to_string(),
        )
        .await?;
        let prompt = builtin_prompt();
        write_text_artifact(&self.working_dir.join(PROMPT_FILENAME), prompt).await?;

        let out = self.working_dir.join(FINAL_VIDEO_FILENAME);
        emit_pct(
            &progress,
            "action_generate",
            &format!(
                "正在让角色模仿参考动作（{CHARACTER_STEM} + {REFERENCE_STEM}，约 {duration}s）"
            ),
            20.0,
        );
        self.backends
            .video
            .generate_from_action_refs(prompt, &character, &video, duration, &out)
            .await?;

        emit_pct(&progress, "film_cover_start", "正在生成封面", 90.0);
        let _ = ensure_cover_from_final_video(&self.working_dir, &out).await;
        if media_local::is_usable_image_file(&self.working_dir.join(COVER_FILENAME)) {
            emit_pct(&progress, "film_cover_done", "封面已就绪", 96.0);
        }
        emit_pct(&progress, "render_done", "动作模仿成片已生成", 100.0);
        Ok(out)
    }
}

async fn duration_from_reference(video: &Path) -> u32 {
    let probed = media_local::probe_media_duration_secs(video)
        .await
        .map(|d| d.ceil() as u32)
        .unwrap_or(MINIMAX_H3_DURATION_MIN)
        .max(1);
    clamp_minimax_h3_duration(probed.clamp(MINIMAX_H3_DURATION_MIN, MINIMAX_H3_DURATION_MAX))
}

/// True when the resolved video model can accept `reference_video`.
pub fn model_supports_action_imitation(model: &str) -> bool {
    is_minimax_h3_model(model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h3_is_supported_seedance_is_not() {
        assert!(model_supports_action_imitation("flowy/MiniMax-H3"));
        assert!(model_supports_action_imitation("AIPC-MiniMax-H3"));
        assert!(!model_supports_action_imitation("AIPC-Doubao-Seedance-2.0"));
    }

    #[test]
    fn duration_clamp_matches_h3() {
        assert_eq!(clamp_minimax_h3_duration(1), 4);
        assert_eq!(clamp_minimax_h3_duration(9), 9);
        assert_eq!(clamp_minimax_h3_duration(40), 15);
    }
}
