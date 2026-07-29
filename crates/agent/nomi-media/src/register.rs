//! Register Flowy media backends and workflow tools into the tool registry.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use base64::Engine;
use nomi_config::{GatewayConfig, flowy_media_exposed};
use nomi_tools::{HandlerTool, ImageGenerateHandler, Tool, ToolRegistry};
use nomi_types::ToolHandler;
use nomi_types::tool::{JsonSchema, ToolImage, ToolResult};
use serde_json::Value;
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

/// Flowy image generation needs to preserve the generated image as a tool
/// artifact. The generic handler adapter only carries text, which is not
/// enough for the conversation delivery contract.
struct FlowyImageGenerateTool {
    handler: Arc<dyn ToolHandler>,
    schema: HandlerTool,
    generated_media_root: PathBuf,
}

impl FlowyImageGenerateTool {
    fn new(handler: Arc<dyn ToolHandler>, data_dir: &Path) -> Self {
        Self {
            schema: HandlerTool::new(Arc::clone(&handler)),
            handler,
            generated_media_root: data_dir.join("media").join("generated"),
        }
    }
}

const MAX_INLINE_IMAGE_BYTES: u64 = 50 * 1024 * 1024;

fn generated_image_artifacts(content: &str, generated_media_root: &Path) -> Result<Vec<ToolImage>, String> {
    let response: Value = serde_json::from_str(content)
        .map_err(|error| format!("invalid Flowy image-generation response: {error}"))?;
    let assets = response
        .get("assets")
        .and_then(Value::as_array)
        .ok_or_else(|| "Flowy image-generation response has no assets".to_owned())?;
    let root = std::fs::canonicalize(generated_media_root)
        .map_err(|error| format!("cannot resolve generated media directory: {error}"))?;

    let mut images = Vec::new();
    for asset in assets {
        if asset.get("kind").and_then(Value::as_str) != Some("image") {
            continue;
        }
        let mime_type = asset
            .get("mime")
            .and_then(Value::as_str)
            .filter(|mime| mime.starts_with("image/"))
            .ok_or_else(|| "generated image has no supported image MIME type".to_owned())?;
        let declared_path = asset
            .get("local_path")
            .and_then(Value::as_str)
            .filter(|path| !path.trim().is_empty())
            .ok_or_else(|| "generated image was not persisted locally".to_owned())?;
        let declared_path = Path::new(declared_path);
        if !declared_path.is_absolute() {
            return Err("generated image path must be absolute".to_owned());
        }
        let image_path = std::fs::canonicalize(declared_path)
            .map_err(|error| format!("cannot resolve generated image: {error}"))?;
        if !image_path.starts_with(&root) {
            return Err("generated image path escapes the media directory".to_owned());
        }
        let metadata = std::fs::metadata(&image_path)
            .map_err(|error| format!("cannot inspect generated image: {error}"))?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err("generated image is not a non-empty regular file".to_owned());
        }
        if metadata.len() > MAX_INLINE_IMAGE_BYTES {
            return Err(format!(
                "generated image exceeds the {MAX_INLINE_IMAGE_BYTES}-byte delivery limit"
            ));
        }
        let bytes = std::fs::read(&image_path)
            .map_err(|error| format!("cannot read generated image: {error}"))?;
        if bytes.len() as u64 != metadata.len() {
            return Err("generated image changed while it was being read".to_owned());
        }
        images.push(ToolImage {
            media_type: mime_type.to_owned(),
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
        });
    }

    if images.is_empty() {
        return Err("Flowy image-generation response contains no persisted image assets".to_owned());
    }
    Ok(images)
}

#[async_trait]
impl Tool for FlowyImageGenerateTool {
    fn name(&self) -> &str {
        self.schema.name()
    }

    fn description(&self) -> &str {
        self.schema.description()
    }

    fn input_schema(&self) -> JsonSchema {
        self.schema.input_schema()
    }

    fn is_concurrency_safe(&self, input: &Value) -> bool {
        self.schema.is_concurrency_safe(input)
    }

    async fn execute(&self, input: Value) -> ToolResult {
        match self.handler.execute(input).await {
            Ok(content) => match generated_image_artifacts(&content, &self.generated_media_root) {
                Ok(images) => ToolResult::text(content).with_images(images),
                Err(error) => ToolResult::error(format!(
                    "image generated but could not prepare its conversation artifact: {error}"
                )),
            },
            Err(error) => ToolResult::error(error.to_string()),
        }
    }

    fn category(&self) -> nomi_protocol::events::ToolCategory {
        self.schema.category()
    }
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

    let handler: Arc<dyn ToolHandler> = Arc::new(ImageGenerateHandler::new(Arc::new(
        FlowyImageGenBackend::new(services),
    )));
    registry.register(Box::new(FlowyImageGenerateTool::new(handler, data_dir)));
    result.has_image = true;

    // Skip video_generate / media_workflow_* tools — replaced by nomi-vimax UI
    // (`/api/vimax/*` + video-generation page). Keep image_generate for other
    // agent surfaces that still need single-shot images.
    info!("Flowy image tool registered; video/workflow tools skipped (nomi-vimax UI)");
    result
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use nomi_types::{StructuredJsonSchema, ToolError, ToolSchema};
    use serde_json::json;

    struct StaticImageHandler {
        content: String,
    }

    #[async_trait]
    impl ToolHandler for StaticImageHandler {
        async fn execute(&self, _params: Value) -> Result<String, ToolError> {
            Ok(self.content.clone())
        }

        fn schema(&self) -> ToolSchema {
            ToolSchema {
                name: "image_generate".into(),
                description: "Generate an image".into(),
                parameters: StructuredJsonSchema::object(Default::default(), Vec::new()),
            }
        }
    }

    #[tokio::test]
    async fn successful_flowy_image_result_is_delivered_as_an_image_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let generated_root = dir.path().join("media").join("generated");
        std::fs::create_dir_all(&generated_root).unwrap();
        let image_path = generated_root.join("generated.jpg");
        std::fs::write(&image_path, b"jpeg bytes").unwrap();
        let handler: Arc<dyn ToolHandler> = Arc::new(StaticImageHandler {
            content: json!({
                "success": true,
                "kind": "image",
                "assets": [{
                    "kind": "image",
                    "local_path": PathBuf::from(&image_path),
                    "mime": "image/jpeg"
                }]
            })
            .to_string(),
        });

        let result = FlowyImageGenerateTool::new(handler, dir.path())
            .execute(json!({}))
            .await;

        assert!(!result.is_error);
        assert_eq!(result.images.len(), 1);
        assert_eq!(result.images[0].media_type, "image/jpeg");
    }
}
