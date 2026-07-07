//! Memory provider plugin trait used by [`crate::plugin::InterestMemoryPlugin`].

use serde_json::Value;

/// Higher-level memory provider lifecycle hook (mirrors the agent memory manager ABC).
pub trait MemoryProviderPlugin: Send + Sync {
    fn name(&self) -> &str;

    fn system_prompt_block(&self) -> String {
        String::new()
    }

    fn prefetch(&self, query: &str, session_id: &str) -> String {
        let _ = (query, session_id);
        String::new()
    }

    fn sync_turn(&self, user_content: &str, assistant_content: &str, session_id: &str) {
        let _ = (user_content, assistant_content, session_id);
    }

    fn on_session_end(&self, messages: &[Value]) {
        let _ = messages;
    }

    fn is_available(&self) -> bool {
        true
    }

    fn get_config_schema(&self) -> Option<Value> {
        None
    }
}
