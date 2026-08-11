//! Pipeline manifest loading, typed shape, and stage runtime status.

pub mod loader;
pub mod manifest;
pub mod stage;

pub use loader::PipelineRegistry;
pub use manifest::{
    CheckpointPolicyDefault, OrchestrationSpec, PipelineManifest, PipelineSummary, Stability,
    StageSpec,
};
pub use stage::StageStatus;
