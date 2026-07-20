//! Backend traits — mirrors ViMax ImageGenerator / VideoGenerator / chat model protocols.

use async_trait::async_trait;
use std::path::Path;

use crate::error::VimaxResult;

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
}

#[async_trait]
pub trait VimaxVideo: Send + Sync {
    async fn generate(
        &self,
        prompt: &str,
        first_frame: Option<&Path>,
        last_frame: Option<&Path>,
        ref_images: &[&Path],
        duration_secs: u32,
        out_path: &Path,
    ) -> VimaxResult<()>;
}
