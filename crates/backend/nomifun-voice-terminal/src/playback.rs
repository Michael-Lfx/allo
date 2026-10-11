//! Playback pipeline and audio sink abstractions.
//!
//! Authoritative baseline: docs/architecture/voice-agent/interaction.zh.md §3.8.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use async_trait::async_trait;
use nomifun_audio::EarconKind;
use tokio::sync::mpsc;

#[derive(Debug, thiserror::Error)]
pub enum AudioSinkError {
    #[error("Audio sink write failed: {0}")]
    Write(String),
    #[error("Audio sink flush failed: {0}")]
    Flush(String),
}

#[async_trait]
pub trait AudioSink: Send + Sync {
    async fn write_frame(&self, frame: Vec<u8>) -> Result<(), AudioSinkError>;
    async fn flush(&self) -> Result<(), AudioSinkError>;
}

/// In-memory test sink for capturing playback frames during testing.
#[cfg(any(test, feature = "voice-skeleton"))]
#[derive(Default, Clone)]
pub struct TestAudioSink {
    frames: Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
}

#[cfg(any(test, feature = "voice-skeleton"))]
impl TestAudioSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn frames(&self) -> Vec<Vec<u8>> {
        self.frames.lock().unwrap().clone()
    }

    pub fn frame_count(&self) -> usize {
        self.frames.lock().unwrap().len()
    }

    pub fn clear(&self) {
        self.frames.lock().unwrap().clear();
    }
}

#[cfg(any(test, feature = "voice-skeleton"))]
#[async_trait]
impl AudioSink for TestAudioSink {
    async fn write_frame(&self, frame: Vec<u8>) -> Result<(), AudioSinkError> {
        self.frames.lock().unwrap().push(frame);
        Ok(())
    }

    async fn flush(&self) -> Result<(), AudioSinkError> {
        self.clear();
        Ok(())
    }
}

/// Jitter buffer resisting network packet jitter before playback emission.
/// Defined in interaction.zh.md §3.8 (100ms jitter buffer threshold).
#[derive(Debug, Clone)]
pub struct JitterBuffer {
    buffer: Vec<u8>,
    target_duration: Duration,
    sample_rate: u32,
    threshold_bytes: usize,
}

impl JitterBuffer {
    pub fn new(target_duration: Duration, sample_rate: u32) -> Self {
        let bytes_per_sec = (sample_rate as f32 * 2.0) as usize;
        let threshold_bytes = (target_duration.as_secs_f32() * bytes_per_sec as f32) as usize;
        Self {
            buffer: Vec::new(),
            target_duration,
            sample_rate,
            threshold_bytes,
        }
    }

    pub fn target_duration(&self) -> Duration {
        self.target_duration
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn push(&mut self, chunk: Vec<u8>) {
        self.buffer.extend_from_slice(&chunk);
    }

    pub fn is_ready(&self) -> bool {
        self.buffer.len() >= self.threshold_bytes
    }

    pub fn pop_all(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.buffer)
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

/// Playback pipeline driving earcon assets and TTS stream decoding.
pub struct PlaybackPipeline {
    sink: Arc<dyn AudioSink>,
    generation_id: Arc<AtomicU64>,
}

impl PlaybackPipeline {
    pub fn new(sink: Arc<dyn AudioSink>) -> Self {
        Self {
            sink,
            generation_id: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn generation_id(&self) -> u64 {
        self.generation_id.load(Ordering::SeqCst)
    }

    /// Increments generation_id and flushes audio sink to instantly cancel playback (<50ms barge-in).
    pub async fn interrupt(&self) -> Result<(), AudioSinkError> {
        self.generation_id.fetch_add(1, Ordering::SeqCst);
        self.sink.flush().await
    }

    /// Plays an earcon sound asset through the audio sink.
    pub async fn play_earcon(&self, kind: EarconKind) -> Result<(), AudioSinkError> {
        let expected_gen = self.generation_id();
        let wav_bytes = kind.as_wav_bytes();

        if self.generation_id() != expected_gen {
            return Ok(());
        }

        self.sink.write_frame(wav_bytes.to_vec()).await
    }

    /// Consumes incoming audio chunk stream and forwards frames to audio sink.
    pub async fn play_stream(&self, mut stream: mpsc::Receiver<Vec<u8>>) -> Result<(), AudioSinkError> {
        let expected_gen = self.generation_id();

        while let Some(chunk) = stream.recv().await {
            if self.generation_id() != expected_gen {
                self.sink.flush().await?;
                break;
            }
            if !chunk.is_empty() {
                self.sink.write_frame(chunk).await?;
            }
        }

        Ok(())
    }
}
