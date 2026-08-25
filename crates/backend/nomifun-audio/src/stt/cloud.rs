//! Cloud STT via a host-provided [`MeetingCloudStt`] (ModelInvoke-compatible).

use std::sync::Arc;

use async_trait::async_trait;

use crate::encode::pcm_to_wav;
use crate::frame::AudioChannel;
use crate::recorder::SttCallback;

/// Host-side cloud transcription (shell / app implements this).
///
/// Keeps `nomifun-audio` free of a hard dependency on `nomifun-model-invoke`.
/// Typical wiring: encode is done by [`CloudSttCallback`]; the implementor
/// forwards WAV bytes to `ModelInvokeService` `SpeechRecognition` (same path
/// as `nomifun-shell::SttService`).
#[async_trait]
pub trait MeetingCloudStt: Send + Sync + 'static {
    /// Transcribe encoded audio. `mime_type` is typically `"audio/wav"`.
    async fn transcribe(&self, audio: Vec<u8>, mime_type: &str) -> Result<String, String>;
}

/// [`SttCallback`] that encodes PCM to WAV and calls [`MeetingCloudStt`].
pub struct CloudSttCallback {
    cloud: Arc<dyn MeetingCloudStt>,
}

impl CloudSttCallback {
    pub fn new(cloud: Arc<dyn MeetingCloudStt>) -> Self {
        Self { cloud }
    }
}

#[async_trait]
impl SttCallback for CloudSttCallback {
    async fn transcribe(
        &self,
        _channel: AudioChannel,
        pcm: Vec<f32>,
        sample_rate: u32,
    ) -> Option<String> {
        if pcm.is_empty() {
            return None;
        }
        let wav = pcm_to_wav(&pcm, sample_rate);
        match self.cloud.transcribe(wav, "audio/wav").await {
            Ok(text) => {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            }
            Err(err) => {
                tracing::warn!("CloudSttCallback: transcribe failed: {err}");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct FakeCloud {
        last_mime: Mutex<Option<String>>,
        reply: String,
    }

    #[async_trait]
    impl MeetingCloudStt for FakeCloud {
        async fn transcribe(&self, audio: Vec<u8>, mime_type: &str) -> Result<String, String> {
            assert!(audio.len() > 44, "expected WAV with header");
            *self.last_mime.lock().unwrap() = Some(mime_type.to_string());
            Ok(self.reply.clone())
        }
    }

    #[tokio::test]
    async fn cloud_callback_encodes_wav_and_returns_text() {
        let cloud = Arc::new(FakeCloud {
            last_mime: Mutex::new(None),
            reply: " hello ".into(),
        });
        let cb = CloudSttCallback::new(cloud.clone());
        let text = cb
            .transcribe(AudioChannel::Mic, vec![0.1; 1600], 16_000)
            .await;
        assert_eq!(text.as_deref(), Some("hello"));
        assert_eq!(
            cloud.last_mime.lock().unwrap().as_deref(),
            Some("audio/wav")
        );
    }

    #[tokio::test]
    async fn cloud_callback_empty_pcm_returns_none() {
        let cloud = Arc::new(FakeCloud {
            last_mime: Mutex::new(None),
            reply: "x".into(),
        });
        let cb = CloudSttCallback::new(cloud);
        assert!(cb.transcribe(AudioChannel::Mic, vec![], 16_000).await.is_none());
    }
}
