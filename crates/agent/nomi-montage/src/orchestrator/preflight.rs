//! Preflight — capability menu surfaced before a project starts, and per-pipeline
//! availability checks so the EP (and the human) never discover a missing tool
//! mid-run.

use serde::{Deserialize, Serialize};

use crate::pipeline::PipelineManifest;
use crate::tools::ToolRegistry;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolAvailability {
    pub name: String,
    pub available: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderMenu {
    pub chat_ready: bool,
    pub image_ready: bool,
    pub video_ready: bool,
    pub tools: Vec<ToolAvailability>,
}

/// Build the provider/tool availability menu for the whole runtime (independent
/// of any one pipeline) — surfaced by `MontageService::provider_menu`.
pub fn build_provider_menu(registry: &ToolRegistry, flowy_ready: bool) -> ProviderMenu {
    let mut names: Vec<String> = crate::tools::CORE_TOOL_NAMES
        .iter()
        .chain(crate::tools::FLOWY_TOOL_NAMES.iter())
        .map(|s| s.to_string())
        .collect();
    names.sort();
    names.dedup();

    let tools = names
        .into_iter()
        .map(|name| {
            let available = registry.is_registered(&name);
            let reason = if available {
                None
            } else if crate::tools::FLOWY_TOOL_NAMES.contains(&name.as_str()) {
                Some("requires a signed-in Flowy cloud session".to_string())
            } else {
                Some("not implemented in this build".to_string())
            };
            ToolAvailability { name, available, reason }
        })
        .collect();

    ProviderMenu {
        chat_ready: flowy_ready,
        image_ready: flowy_ready,
        video_ready: flowy_ready,
        tools,
    }
}

/// Tools a specific pipeline needs that are not currently available. An empty
/// result means the pipeline can run end-to-end with the current registry.
pub fn missing_tools_for_pipeline(manifest: &PipelineManifest, registry: &ToolRegistry) -> Vec<String> {
    registry.unavailable_of(&manifest.all_tool_names())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::build_default_registry;

    #[test]
    fn menu_reflects_registry_state() {
        let registry = build_default_registry(false);
        let menu = build_provider_menu(&registry, false);
        assert!(!menu.chat_ready);
        let flowy_image = menu.tools.iter().find(|t| t.name == "flowy_image").unwrap();
        assert!(!flowy_image.available);
        assert!(flowy_image.reason.is_some());
    }
}
