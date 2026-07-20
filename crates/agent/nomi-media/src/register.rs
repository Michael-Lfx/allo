//! Register Flowy media backends and workflow tools into the tool registry.

use std::path::Path;
use std::sync::Arc;

use nomi_config::{GatewayConfig, flowy_media_exposed};
use nomi_tools::{HandlerTool, ImageGenerateHandler, ToolRegistry};
use tracing::{debug, info, warn};

use crate::backends::FlowyMediaServices;
use crate::backends::flowy_image::FlowyImageGenBackend;

/// Which Flowy media tools were registered for this session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WireFlowyMediaResult {
    pub has_image: bool,
    pub has_video: bool,
    pub has_workflow: bool,
}

/// Wire Flowy image/video backends and workflow tools when server login is available.
pub fn wire_flowy_media(
    registry: &mut ToolRegistry,
    config: &GatewayConfig,
    data_dir: &Path,
) -> WireFlowyMediaResult {
    // Ensure ffmpeg auto-install hooks are registered once media tools are wired.
    // Idempotent: subsequent calls keep the first registered hooks.
    nomi_config::register_dep_gate_hooks();

    let mut result = WireFlowyMediaResult::default();
    if !flowy_media_exposed(config) {
        debug!(
            provider = %config.media.provider,
            server_base_url = %config.server.base_url,
            "Flowy media wiring skipped (provider != flowy or server.base_url missing)"
        );
        return result;
    }

    let Some(services) = FlowyMediaServices::try_new(config, data_dir) else {
        warn!("Flowy media services could not be initialized");
        return result;
    };

    register_handler(
        registry,
        Arc::new(ImageGenerateHandler::new(Arc::new(FlowyImageGenBackend::new(
            services,
        )))),
    );
    result.has_image = true;

    // Skip video_generate / media_workflow_* tools — replaced by nomi-vimax UI
    // (`/api/vimax/*` + video-generation page). Keep image_generate for other
    // agent surfaces that still need single-shot images.
    info!("Flowy image tool registered; video/workflow tools skipped (nomi-vimax UI)");
    result
}

fn register_handler(registry: &mut ToolRegistry, handler: Arc<dyn nomi_types::ToolHandler>) {
    registry.register(Box::new(HandlerTool::new(handler)));
}
