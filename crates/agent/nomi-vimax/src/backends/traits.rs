//! Backend traits — mirrors ViMax ImageGenerator / VideoGenerator / chat model protocols.

use async_trait::async_trait;
use std::path::Path;

use crate::error::VimaxResult;

/// Optional controls for img2img-style edits (privacy face repair, mild restyle).
///
/// Upstream may ignore unknown fields; callers should also bake constraints into the prompt.
#[derive(Debug, Clone, Default)]
pub struct ImageGenerateOpts {
    pub negative_prompt: Option<String>,
    /// Mild rewrite strength in `[0, 1]` when the channel supports it (e.g. 0.42–0.48).
    pub denoising_strength: Option<f32>,
}

#[async_trait]
pub trait VimaxChat: Send + Sync {
    async fn complete_text(&self, system: &str, user: &str) -> VimaxResult<String>;

    async fn complete_vision(
        &self,
        system: &str,
        user_text: &str,
        image_paths: &[&Path],
    ) -> VimaxResult<String>;
}

#[async_trait]
pub trait VimaxImage: Send + Sync {
    /// Generate an image and write/copy it to `out_path`.
    async fn generate(
        &self,
        prompt: &str,
        ref_image_paths: &[&Path],
        out_path: &Path,
    ) -> VimaxResult<()>;

    /// Like [`Self::generate`], with optional negative prompt / denoise strength.
    async fn generate_with_opts(
        &self,
        prompt: &str,
        ref_image_paths: &[&Path],
        out_path: &Path,
        opts: ImageGenerateOpts,
    ) -> VimaxResult<()> {
        let _ = opts;
        self.generate(prompt, ref_image_paths, out_path).await
    }
}

#[async_trait]
pub trait VimaxVideo: Send + Sync {
    /// Generate a video clip.
    ///
    /// - `first_frame` / `last_frame`: Seedance frame roles (mutually exclusive with refs).
    /// - `ref_images`: `reference_image` roles (multi-ref R2V). Prefer this for Seedance 2.0.
    /// - `last_frame_out`: when set, request `return_last_frame` and save the still here
    ///   (caller may still ffmpeg-extract as fallback).
    /// - `ref_video`: MiniMax-H3 `reference_video` (mutually exclusive with first/last_frame).
    /// - `ref_audio`: Seedance `reference_audio` for speaker timbre lock.
    async fn generate(
        &self,
        prompt: &str,
        first_frame: Option<&Path>,
        last_frame: Option<&Path>,
        ref_images: &[&Path],
        duration_secs: u32,
        out_path: &Path,
        last_frame_out: Option<&Path>,
        ref_video: Option<&Path>,
        ref_audio: Option<&Path>,
    ) -> VimaxResult<()>;

    /// Character still + motion video (MiniMax-H3 multimodal reference).
    async fn generate_from_action_refs(
        &self,
        prompt: &str,
        character_image: &Path,
        reference_video: &Path,
        duration_secs: u32,
        out_path: &Path,
    ) -> VimaxResult<()> {
        self.generate(
            prompt,
            None,
            None,
            &[character_image],
            duration_secs,
            out_path,
            None,
            Some(reference_video),
            None,
        )
        .await
    }
}
