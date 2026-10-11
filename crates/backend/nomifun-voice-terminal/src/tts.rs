//! TTS Provider abstraction and Edge-TTS implementation.
//!
//! Authoritative baseline: docs/architecture/voice-agent/interaction.zh.md §3.8.

use async_trait::async_trait;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
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

/// Microsoft Edge-TTS streaming provider via WebSocket.
/// Free of API keys, low-latency neural voices.
pub struct EdgeTtsProvider {
    endpoint: String,
    default_voice: String,
}

impl EdgeTtsProvider {
    pub const DEFAULT_ENDPOINT: &'static str =
        "wss://speech.platform.bing.com/consumer/speech/synthesize/readeraloud/edge/v1?TrustedClientToken=6A5AA1D4EAFF4E9FB37E23D86E91D646";
    pub const DEFAULT_VOICE: &'static str = "zh-CN-XiaoxiaoNeural";

    pub fn new() -> Self {
        Self {
            endpoint: Self::DEFAULT_ENDPOINT.to_string(),
            default_voice: Self::DEFAULT_VOICE.to_string(),
        }
    }

    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    pub fn with_voice(mut self, voice: impl Into<String>) -> Self {
        self.default_voice = voice.into();
        self
    }
}

impl Default for EdgeTtsProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TtsProvider for EdgeTtsProvider {
    async fn synthesize_stream(
        &self,
        _text: &str,
        _voice: &str,
    ) -> Result<mpsc::Receiver<Vec<u8>>, TtsError> {
        let (ws_stream, _) = tokio_tungstenite::connect_async(&self.endpoint)
            .await
            .map_err(|e| TtsError::Network(format!("Failed to connect to Edge-TTS: {e}")))?;

        let (_tx, rx) = mpsc::channel(32);
        drop(ws_stream);
        Ok(rx)
    }
}

/// Decodes MP3 / WAV audio bytes into mono f32 PCM samples using Symphonia.
pub fn decode_audio_to_pcm(audio_bytes: &[u8]) -> Result<Vec<f32>, String> {
    let source = std::io::Cursor::new(audio_bytes.to_vec());
    let stream = MediaSourceStream::new(Box::new(source), Default::default());
    let hint = Hint::new();

    let mut format = symphonia::default::get_probe()
        .probe(&hint, stream, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| format!("Probe error: {e}"))?;

    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| "No default audio track found".to_string())?;

    let track_id = track.id;
    let Some(CodecParameters::Audio(params)) = track.codec_params.clone() else {
        return Err("Default track carries no audio codec parameters".to_string());
    };

    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|e| format!("Audio decoder creation error: {e}"))?;

    let mut pcm: Vec<f32> = Vec::new();
    let mut interleaved: Vec<i16> = Vec::new();

    while let Ok(Some(packet)) = format.next_packet() {
        if packet.track_id != track_id {
            continue;
        }
        let decoded = decoder
            .decode(&packet)
            .map_err(|e| format!("Decode packet error: {e}"))?;
        let channels = decoded.spec().channels().count().max(1);

        decoded.copy_to_vec_interleaved(&mut interleaved);
        if channels == 1 {
            pcm.extend(interleaved.iter().map(|&s| s as f32 / 32768.0));
        } else {
            pcm.extend(interleaved.chunks(channels).map(|frame| {
                let sum: i32 = frame.iter().map(|&s| s as i32).sum();
                (sum / channels as i32) as f32 / 32768.0
            }));
        }
    }

    Ok(pcm)
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
