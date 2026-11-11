//! Single shared [`ComputerTool`] for the gateway's computer-use capabilities.
//!
//! Unlike the browser (one Chrome per companion), the desktop is one physical
//! screen, so one shared `ComputerTool` is the right execution owner. The tool
//! keeps snapshot and screenshot state per caller session; all physical desktop
//! calls are still serialized behind one lock. Only compiled with the
//! `computer-use` feature.

use std::sync::Arc;

use nomi_computer::tool::ComputerTool;
use nomi_config::config::ComputerConfig;
use nomi_types::tool::ToolResult;
use serde_json::{Value, json};
use tokio::sync::Mutex;

/// Owns the shared desktop `ComputerTool` and serializes calls to it.
pub struct ComputerRegistry {
    tool: Arc<ComputerTool>,
    /// One global lock: the desktop is a single screen, so observe/input calls
    /// from different agents must not interleave physically. Logical state is
    /// isolated by the session id passed to `execute_for_session`.
    lock: Mutex<()>,
}

impl ComputerRegistry {
    pub fn new() -> Self {
        Self {
            tool: Arc::new(ComputerTool::new(&ComputerConfig::default())),
            lock: Mutex::new(()),
        }
    }

    /// Forward a `{"action": ..}` payload to the shared tool, serialized.
    pub async fn execute(&self, input: Value) -> ToolResult {
        self.execute_for_session("default", input).await
    }

    /// Forward an action under an isolated caller session. Physical desktop
    /// access remains serialized, while `[ref]` and screenshot geometry stay
    /// private to the caller.
    pub async fn execute_for_session(&self, session_id: &str, input: Value) -> ToolResult {
        let _guard = self.lock.lock().await;
        self.tool.execute_for_session(session_id, input).await
    }
}

impl Default for ComputerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Map a `ToolResult` onto the gateway result envelope: error → `{"error": ..}`;
/// success → `{"result": {"text": .., "images": [{media_type, data}]}}` (base64
/// screenshots / Set-of-Marks overlays flow straight through). Mirrors the
/// browser registry's helper of the same name (kept separate so a computer-only
/// build needs no browser-use feature).
pub fn tool_result_to_value(result: ToolResult) -> Value {
    if result.is_error {
        return json!({ "error": result.content });
    }
    let mut payload = json!({ "text": result.content });
    if !result.images.is_empty() {
        let imgs: Vec<Value> = result
            .images
            .iter()
            .map(|img| json!({ "media_type": img.media_type, "data": img.data }))
            .collect();
        payload["images"] = Value::Array(imgs);
    }
    json!({ "result": payload })
}
