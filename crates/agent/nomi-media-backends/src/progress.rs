//! Pipeline progress hook shared by video backends.

use std::sync::Arc;

use serde_json::Value;

/// Pipeline progress hook: `(stage, message, optional metadata)`.
pub type ProgressCallback = Arc<dyn Fn(&str, &str, Option<Value>) + Send + Sync>;
