//! Per-turn POI prefetch block injected into outgoing ACP prompts.

use std::sync::Arc;

use nomifun_poi::PoiService;

use crate::capability::prompt_pipeline::{PreSendHook, PromptCtx};
use crate::manager::acp::hooks::emit_hook_warning;

pub struct PoiPrefetchHook {
    poi_service: Arc<PoiService>,
}

impl PoiPrefetchHook {
    pub fn new(poi_service: Arc<PoiService>) -> Self {
        Self { poi_service }
    }

    fn render_prefetch(&self, query: &str) -> Option<String> {
        if !self.poi_service.interest_config().enabled {
            return None;
        }
        let store = self.poi_service.store();
        let guard = store.lock().ok()?;
        guard.render_prefetch_block(query)
    }
}

#[async_trait::async_trait]
impl PreSendHook for PoiPrefetchHook {
    async fn pre_send(&self, ctx: &mut PromptCtx<'_>, prompt: String) -> String {
        match self.render_prefetch(&prompt) {
            Some(block) if !block.trim().is_empty() => format!("{block}\n\n{prompt}"),
            _ if self.poi_service.interest_config().enabled && self.poi_service.store().lock().is_err() => {
                emit_hook_warning(ctx, "poi_prefetch", "interest store lock poisoned");
                prompt
            }
            _ => prompt,
        }
    }
}
