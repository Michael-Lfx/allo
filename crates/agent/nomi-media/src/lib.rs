//! Flowy-backed media generation and multi-step workflow orchestration.

pub mod assets;
pub mod backends;
pub mod credits;
pub mod delivery;
pub mod flowy_params;
pub mod llm_refine;
pub mod long_video_active;
pub mod long_video_plan;
pub mod platform;
pub mod preview;
pub mod progress;
pub mod prompt_guidance;
pub mod prompt_refine;
pub mod qa;
pub mod register;
pub mod tool_schemas;
pub mod tools;
pub mod video_segment;
pub mod workflows;

pub use assets::{MediaArtifact, extract_image_urls, persist_bytes, persist_from_url};
pub use delivery::{MediaAssetDelivery, MediaProvenance};
pub use prompt_guidance::gateway_media_system_hint;
pub use register::{WireFlowyMediaResult, wire_flowy_media};
pub use video_segment::{
    SegmentPlan, needs_long_video_pipeline, parse_duration_secs_from_text, plan_segment_durations,
    route_long_video_template,
};
pub use workflows::store::WorkflowRunStore;
