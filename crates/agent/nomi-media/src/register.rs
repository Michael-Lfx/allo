//! Register Flowy media backends and workflow tools into the tool registry.

use std::path::Path;
use std::sync::Arc;

use nomi_config::{GatewayConfig, flowy_media_exposed};
use nomi_tools::{
    HandlerTool, ImageGenerateHandler, ToolRegistry, VideoGenerateBackend, VideoGenerateHandler,
};
use nomi_types::ToolHandler;
use tracing::{debug, info, warn};

use crate::backends::FlowyMediaServices;
use crate::backends::flowy_image::FlowyImageGenBackend;
use crate::backends::flowy_video::FlowyVideoGenBackend;
use crate::backends::flowy_video_router::FlowyVideoGenerateRouter;
use crate::tools::{
    MediaWorkflowCancelHandler, MediaWorkflowPlanHandler, MediaWorkflowRunHandler,
    MediaWorkflowStatusHandler,
};
use crate::workflows::runner::WorkflowRunner;
use crate::workflows::store::WorkflowRunStore;

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
            services.clone(),
        )))),
    );
    result.has_image = true;

    if !config.media.workflows.enabled {
        register_handler(
            registry,
            Arc::new(VideoGenerateHandler::new(Arc::new(FlowyVideoGenBackend::new(
                services.clone(),
            )))),
        );
        result.has_video = true;
        info!("Flowy image/video tools registered (workflows disabled)");
        return result;
    }

    let store = Arc::new(WorkflowRunStore::with_root(
        data_dir.join("media").join("workflows"),
    ));
    let runner = Arc::new(WorkflowRunner::new(services.clone(), Arc::clone(&store)));
    let video_backend: Arc<dyn VideoGenerateBackend> = Arc::new(FlowyVideoGenerateRouter::new(
        FlowyVideoGenBackend::new(services.clone()),
        Arc::clone(&runner),
    ));
    register_handler(
        registry,
        Arc::new(VideoGenerateHandler::new(video_backend)),
    );
    result.has_video = true;

    register_handler(
        registry,
        Arc::new(MediaWorkflowPlanHandler::new(
            config.media.clone(),
            Some(services.clone()),
        )),
    );
    let run_handler: Arc<dyn ToolHandler> =
        Arc::new(MediaWorkflowRunHandler::new(Arc::clone(&runner)));
    register_handler(registry, Arc::clone(&run_handler));
    let status_handler = MediaWorkflowStatusHandler::new(Arc::clone(&store)).with_default_timeout(
        std::time::Duration::from_secs(services.media.video.poll_timeout_seconds.max(600)),
    );
    register_handler(registry, Arc::new(status_handler));
    register_handler(
        registry,
        Arc::new(MediaWorkflowCancelHandler::new(runner)) as Arc<dyn ToolHandler>,
    );
    result.has_workflow = true;

    info!("Flowy media backends and workflow tools registered");
    result
}

fn register_handler(registry: &mut ToolRegistry, handler: Arc<dyn ToolHandler>) {
    registry.register(Box::new(HandlerTool::new(handler)));
}
