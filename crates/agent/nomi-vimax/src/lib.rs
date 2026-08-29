//! ViMax-faithful video generation pipelines (Flowy LLM / image / video only).

pub mod agents;
pub mod artifact_edit;
pub mod aspect;
pub mod backends;
pub mod creative;
pub mod domain;
pub mod error;
pub mod json_util;
pub mod media_local;
pub mod pipelines;
pub mod planning;
pub mod progress;
pub mod prompt_safety;
pub mod rag;
pub mod revise;
pub mod service;
pub mod session;
pub mod skills;
pub mod video_quality;

pub use creative::{
    build_canvas_document, scan_session_film, CreativeFilm, CreativeMediaFile, IngestedMedia,
    MediaIdMap, CREATIVE_IR_VERSION,
};
pub use aspect::{
    DEFAULT_ASPECT_RATIO, SEEDANCE_ASPECT_RATIOS, aspect_prompt_clause, aspect_to_dashscope_size,
    aspect_to_seedream_size, aspect_to_upload_dims, image_request_extra_for_aspect,
    normalize_aspect_ratio, video_aspect_framing_clause,
};

pub use video_quality::{
    DEFAULT_VIDEO_FPS, DEFAULT_VIDEO_RESOLUTION, VIDEO_RESOLUTIONS, VideoModelCapabilities,
    normalize_fps_for_model, normalize_resolution_for_model, video_model_capabilities,
};

pub use artifact_edit::ImagePromptInfo;
pub use backends::{
    FlowyChat, FlowyImage, FlowyVideo, FlowyVimaxServices, ImageGenerateOpts, VimaxChat, VimaxImage,
    VimaxVideo,
};
pub use domain::WorkflowKind;
pub use error::{VimaxError, VimaxResult};
pub use progress::{
    INTERRUPTED_SUMMARY, ProgressCallback, ProgressEvent, RenderStatus, RunStatus,
};
pub use revise::ReviseResult;
pub use service::VimaxService;
pub use session::{
    ARCHIVE_EXTENSION, ActionAssetsInfo, ArtifactNode, CameoManifest, CameoPhotoEntry, CameoUpdate,
    SessionIndex, SessionRecord, SessionSummary, apply_video_task_credits,
};
pub use skills::{
    pack_skill_dir, SkillCatalog, SkillId, SkillOverlay, SkillSource, SkillVisibility,
    VerticalSkill, VerticalSkillDraft, VerticalSkillSummary,
};
