//! ViMax-faithful video generation pipelines (Flowy LLM / image / video only).

pub mod agents;
pub mod aspect;
pub mod backends;
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

pub use aspect::{
    DEFAULT_ASPECT_RATIO, SEEDANCE_ASPECT_RATIOS, aspect_prompt_clause, aspect_to_dashscope_size,
    aspect_to_seedream_size, aspect_to_upload_dims, image_request_extra_for_aspect,
    normalize_aspect_ratio,
};

pub use backends::{FlowyChat, FlowyImage, FlowyVideo, FlowyVimaxServices, VimaxChat, VimaxImage, VimaxVideo};
pub use domain::WorkflowKind;
pub use error::{VimaxError, VimaxResult};
pub use progress::{ProgressCallback, ProgressEvent, RenderStatus, RunStatus};
pub use service::VimaxService;
pub use session::{
    ARCHIVE_EXTENSION, ArtifactNode, CameoManifest, CameoPhotoEntry, CameoUpdate, SessionIndex,
    SessionRecord,
};
