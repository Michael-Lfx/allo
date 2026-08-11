//! Reusable Flowy media backends and local ffmpeg helpers.

pub mod aspect;
pub mod backends;
pub mod error;
pub mod language_lock;
pub mod media_local;
pub mod progress;
pub mod prompt_safety;
pub mod video_quality;

pub use aspect::{
    DEFAULT_ASPECT_RATIO, SEEDANCE_ASPECT_RATIOS, aspect_prompt_clause, aspect_to_dashscope_size,
    aspect_to_seedream_size, aspect_to_upload_dims, image_request_extra_for_aspect,
    load_aspect_from_dir, normalize_aspect_ratio,
};
pub use backends::{
    FlowyChat, FlowyImage, FlowyMediaServices, FlowyVideo, MediaChat, MediaImage, MediaVideo,
};
pub use error::{MediaBackendError, MediaBackendResult};
pub use language_lock::{
    OutputLanguage, detect_output_language, language_lock_clause, language_lock_for_sources,
    language_lock_for_text, with_language_lock,
};
pub use progress::ProgressCallback;
pub use video_quality::{
    DEFAULT_VIDEO_FPS, DEFAULT_VIDEO_RESOLUTION, MAX_CLIP_DURATION_SECS, MIN_CLIP_DURATION_SECS,
    VIDEO_RESOLUTIONS, VideoModelCapabilities, normalize_fps_for_model,
    normalize_resolution_for_model, video_model_capabilities,
};
