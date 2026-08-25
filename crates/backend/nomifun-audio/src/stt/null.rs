//! Null / fake STT backends for wiring and unit tests.

use std::sync::Mutex;

use async_trait::async_trait;

use crate::frame::AudioChannel;
use crate::recorder::SttCallback;

/// Always returns `None` (no transcript).
pub struct NullSttCallback;

#[async_trait]
impl SttCallback for NullSttCallback {
    async fn transcribe(
        &self,
        _channel: AudioChannel,
        _pcm: Vec<f32>,
        _sample_rate: u32,
    ) -> Option<String> {
        None
    }
}

/// Returns a fixed string (or a scripted sequence) for tests.
pub struct FakeSttCallback {
    fixed: Option<String>,
    script: Mutex<Vec<Option<String>>>,
}

impl FakeSttCallback {
    pub fn always(text: impl Into<String>) -> Self {
        Self {
            fixed: Some(text.into()),
            script: Mutex::new(Vec::new()),
        }
    }

    pub fn script(responses: Vec<Option<String>>) -> Self {
        Self {
            fixed: None,
            script: Mutex::new(responses),
        }
    }
}

#[async_trait]
impl SttCallback for FakeSttCallback {
    async fn transcribe(
        &self,
        _channel: AudioChannel,
        _pcm: Vec<f32>,
        _sample_rate: u32,
    ) -> Option<String> {
        if let Some(ref text) = self.fixed {
            return Some(text.clone());
        }
        let mut q = self.script.lock().ok()?;
        if q.is_empty() {
            None
        } else {
            q.remove(0)
        }
    }
}
