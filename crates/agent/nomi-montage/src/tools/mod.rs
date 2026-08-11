//! Tool contract, registry, and the concrete tools the Executive Producer can call.

pub mod artifact_io;
pub mod checkpoint_tools;
pub mod compose;
pub mod contract;
pub mod ffmpeg;
pub mod flowy;
pub mod registry;
pub mod selectors;

pub use contract::{MontageTool, ToolContext, ToolResult, ToolRuntime, ToolSpec, ToolStability, ToolTier};
pub use registry::{ToolRegistry, UnavailableTool};

use std::sync::Arc;

/// Tool names that are always registered (no external dependency).
pub const CORE_TOOL_NAMES: &[&str] = &[
    "write_artifact",
    "read_artifact",
    "checkpoint_note",
    "decision_log_append",
    "cost_estimate",
    "cost_reconcile",
    "video_stitch",
    "extract_last_frame",
    "video_compose",
];

/// Tool names that additionally require a signed-in Flowy session.
pub const FLOWY_TOOL_NAMES: &[&str] = &["flowy_image", "flowy_video", "image_selector", "video_selector"];

/// Build the full tool registry. `flowy_available` controls whether the
/// Flowy-backed creative tools are registered at all — when `false`, stages
/// that list them see them as [`UnavailableTool`] rather than a silently
/// no-op'd success, per Rule Zero.
pub fn build_default_registry(flowy_available: bool) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(artifact_io::WriteArtifactTool));
    registry.register(Arc::new(artifact_io::ReadArtifactTool));
    registry.register(Arc::new(checkpoint_tools::CheckpointNoteTool));
    registry.register(Arc::new(checkpoint_tools::DecisionLogAppendTool));
    registry.register(Arc::new(checkpoint_tools::CostEstimateTool));
    registry.register(Arc::new(checkpoint_tools::CostReconcileTool));
    registry.register(Arc::new(ffmpeg::VideoStitchTool));
    registry.register(Arc::new(ffmpeg::ExtractLastFrameTool));
    registry.register(Arc::new(compose::VideoComposeTool));

    if flowy_available {
        registry.register(Arc::new(flowy::FlowyImageTool));
        registry.register(Arc::new(flowy::FlowyVideoTool));
        registry.register(Arc::new(selectors::ImageSelectorTool));
        registry.register(Arc::new(selectors::VideoSelectorTool));
    }

    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_tools_always_registered() {
        let registry = build_default_registry(false);
        for name in CORE_TOOL_NAMES {
            assert!(registry.is_registered(name), "missing core tool: {name}");
        }
        for name in FLOWY_TOOL_NAMES {
            assert!(!registry.is_registered(name), "flowy tool registered without media: {name}");
        }
    }

    #[test]
    fn flowy_tools_registered_when_available() {
        let registry = build_default_registry(true);
        for name in FLOWY_TOOL_NAMES {
            assert!(registry.is_registered(name), "missing flowy tool: {name}");
        }
    }
}
