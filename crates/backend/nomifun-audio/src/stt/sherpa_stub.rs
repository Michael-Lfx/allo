//! Stub when `local-stt` is disabled — no native sherpa link.

use std::path::Path;

use async_trait::async_trait;

use crate::frame::AudioChannel;
use crate::recorder::SttCallback;

/// Error type kept so call sites share signatures with the real module.
#[derive(Debug, Clone)]
pub struct SherpaSttError(pub String);

impl std::fmt::Display for SherpaSttError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SherpaSttError {}

/// Placeholder; construction always fails without `local-stt`.
pub struct SherpaSttCallback;

impl SherpaSttCallback {
    pub fn try_from_dir(_dir: &Path) -> Result<Self, SherpaSttError> {
        Err(SherpaSttError(
            "local sherpa STT requires feature `local-stt`".into(),
        ))
    }
}

#[async_trait]
impl SttCallback for SherpaSttCallback {
    async fn transcribe(
        &self,
        _channel: AudioChannel,
        _pcm: Vec<f32>,
        _sample_rate: u32,
    ) -> Option<String> {
        None
    }
}

/// Always `Ok(None)` when the feature is off (Auto/cloud still work).
pub fn try_load_sherpa(_model_dir: Option<&Path>) -> Result<Option<SherpaSttCallback>, SherpaSttError> {
    Ok(None)
}
