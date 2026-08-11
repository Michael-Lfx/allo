//! Backend traits — mirrors ViMax ImageGenerator / VideoGenerator / chat model protocols.

use async_trait::async_trait;
use std::path::Path;

use crate::error::MediaBackendResult;

#[async_trait]
pub trait MediaChat: Send + Sync {
    async fn complete_text(&self, system: &str, user: &str) -> MediaBackendResult<String>;

    async fn complete_vision(
        &self,
        system: &str,
        user_text: &str,
        image_paths: &[&Path],
    ) -> MediaBackendResult<String>;
}

#[async_trait]
pub trait MediaImage: Send + Sync {
    /// Generate an image and write/copy it to `out_path`.
    async fn generate(
        &self,
        prompt: &str,
        ref_image_paths: &[&Path],
        out_path: &Path,
    ) -> MediaBackendResult<()>;
}

#[async_trait]
pub trait MediaVideo: Send + Sync {
    /// Generate a video clip.
    ///
    /// - `first_frame` / `last_frame`: Seedance frame roles (mutually exclusive with refs).
    /// - `ref_images`: `reference_image` roles (multi-ref R2V). Prefer this for Seedance 2.0.
    /// - `last_frame_out`: when set, request `return_last_frame` and save the still here
    ///   (caller may still ffmpeg-extract as fallback).
    async fn generate(
        &self,
        prompt: &str,
        first_frame: Option<&Path>,
        last_frame: Option<&Path>,
        ref_images: &[&Path],
        duration_secs: u32,
        out_path: &Path,
        last_frame_out: Option<&Path>,
    ) -> MediaBackendResult<()>;
}