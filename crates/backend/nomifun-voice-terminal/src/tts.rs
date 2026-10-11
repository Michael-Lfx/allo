//! TTS Provider abstraction and stub implementation.
//!
//! Authoritative baseline: docs/architecture/voice-agent/interaction.zh.md §3.8.

use async_trait::async_trait;
use tokio::sync::mpsc;

#[derive(Debug, thiserror::Error)]
pub enum TtsError {
    #[error("Network connection error: {0}")]
    Network(String),
    #[error("Audio decode error: {0}")]
    Decode(String),
    #[error("TTS synthesis failed: {0}")]
    Synthesis(String),
    #[error("TTS request cancelled")]
    Cancelled,
}

#[async_trait]
pub trait TtsProvider: Send + Sync {
    /// Stream audio chunk synthesis. Returns an async Receiver for byte buffers.
    async fn synthesize_stream(
        &self,
        text: &str,
        voice: &str,
    ) -> Result<mpsc::Receiver<Vec<u8>>, TtsError>;
}

/// Null TTS stub implementation for tests and dev skeleton.
/// Red-line: Zero mock contamination in production runtime (AGENTS.md / interaction.zh.md P2).
#[cfg(any(test, feature = "voice-skeleton"))]
#[derive(Debug, Default, Clone)]
pub struct NullTts;

#[cfg(any(test, feature = "voice-skeleton"))]
impl NullTts {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(any(test, feature = "voice-skeleton"))]
#[async_trait]
impl TtsProvider for NullTts {
    async fn synthesize_stream(
        &self,
        text: &str,
        _voice: &str,
    ) -> Result<mpsc::Receiver<Vec<u8>>, TtsError> {
        let (tx, rx) = mpsc::channel(8);
        let payload = if text.is_empty() {
            vec![0u8; 64]
        } else {
            text.as_bytes().to_vec()
        };

        tokio::spawn(async move {
            let _ = tx.send(payload).await;
        });

        Ok(rx)
    }
}
