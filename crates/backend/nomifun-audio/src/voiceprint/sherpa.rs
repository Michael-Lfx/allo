//! Optional Sherpa speaker-embedding encoder.

use sherpa_onnx::{SpeakerEmbeddingExtractor, SpeakerEmbeddingExtractorConfig};
use tracing::warn;

use super::VoiceprintEncoder;
use crate::pcm_util::resample_mono;

/// Model path / runtime options for [`SherpaVoiceprintEncoder`].
#[derive(Debug, Clone)]
pub struct SherpaVoiceprintEncoderConfig {
    pub model: String,
    pub num_threads: i32,
    pub provider: String,
    /// If set, PCM is resampled to this rate before encoding.
    pub sample_rate: u32,
}

impl SherpaVoiceprintEncoderConfig {
    pub fn from_env() -> Option<Self> {
        let model = std::env::var("SHERPA_VOICEPRINT_MODEL")
            .ok()
            .or_else(|| std::env::var("SHERPA_DIARIZATION_EMBEDDING_MODEL").ok())?;
        if model.is_empty() {
            return None;
        }
        Some(Self {
            model,
            num_threads: 2,
            provider: "cpu".into(),
            sample_rate: 16_000,
        })
    }
}

/// Sherpa [`SpeakerEmbeddingExtractor`] wrapper.
pub struct SherpaVoiceprintEncoder {
    inner: SpeakerEmbeddingExtractor,
    sample_rate: u32,
}

impl SherpaVoiceprintEncoder {
    pub fn try_create(config: &SherpaVoiceprintEncoderConfig) -> Option<Self> {
        let cfg = SpeakerEmbeddingExtractorConfig {
            model: Some(config.model.clone()),
            num_threads: config.num_threads,
            debug: false,
            provider: Some(config.provider.clone()),
        };
        let inner = match SpeakerEmbeddingExtractor::create(&cfg) {
            Some(e) => e,
            None => {
                warn!("Sherpa SpeakerEmbeddingExtractor::create returned null");
                return None;
            }
        };
        Some(Self {
            inner,
            sample_rate: config.sample_rate.max(1),
        })
    }

    pub fn try_from_env() -> Option<Self> {
        let cfg = SherpaVoiceprintEncoderConfig::from_env()?;
        Self::try_create(&cfg)
    }

    pub fn dim(&self) -> i32 {
        self.inner.dim()
    }
}

impl VoiceprintEncoder for SherpaVoiceprintEncoder {
    fn encode(&self, pcm: &[f32], sample_rate: u32) -> Result<Vec<f32>, String> {
        if pcm.is_empty() {
            return Err("empty pcm".into());
        }
        if sample_rate == 0 {
            return Err("sample_rate must be > 0".into());
        }
        let samples = if sample_rate == self.sample_rate {
            pcm.to_vec()
        } else {
            resample_mono(pcm, sample_rate, self.sample_rate)
        };
        let stream = self
            .inner
            .create_stream()
            .ok_or_else(|| "Sherpa embedding create_stream failed".to_string())?;
        stream.accept_waveform(self.sample_rate as i32, &samples);
        stream.input_finished();
        if !self.inner.is_ready(&stream) {
            return Err("Sherpa embedding extractor not ready".into());
        }
        self.inner
            .compute(&stream)
            .ok_or_else(|| "Sherpa embedding compute failed".to_string())
    }
}
