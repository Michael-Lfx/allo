//! Optional Sherpa-ONNX offline speaker diarization.
//!
//! Enabled by feature `local-stt` or `diarization`. Creation fails soft when
//! models are missing so callers can fall back to [`super::EnergyClusterDiarizer`].

use sherpa_onnx::{
    FastClusteringConfig, OfflineSpeakerDiarization, OfflineSpeakerDiarizationConfig,
    OfflineSpeakerSegmentationModelConfig, OfflineSpeakerSegmentationPyannoteModelConfig,
    SpeakerEmbeddingExtractorConfig,
};
use tracing::warn;

use super::{Diarizer, SpeakerSpan};
use crate::pcm_util::resample_mono;

/// Paths / clustering options for [`SherpaDiarizer`].
#[derive(Debug, Clone)]
pub struct SherpaDiarizerConfig {
    pub segmentation_model: String,
    pub embedding_model: String,
    pub num_threads: i32,
    pub provider: String,
    pub clustering_threshold: f32,
    pub num_clusters: i32,
    pub min_duration_on: f32,
    pub min_duration_off: f32,
}

impl SherpaDiarizerConfig {
    /// Read model paths from env (`SHERPA_DIARIZATION_SEGMENTATION_MODEL`,
    /// `SHERPA_DIARIZATION_EMBEDDING_MODEL`). Returns `None` if either is unset.
    pub fn from_env() -> Option<Self> {
        let segmentation_model = std::env::var("SHERPA_DIARIZATION_SEGMENTATION_MODEL").ok()?;
        let embedding_model = std::env::var("SHERPA_DIARIZATION_EMBEDDING_MODEL").ok()?;
        if segmentation_model.is_empty() || embedding_model.is_empty() {
            return None;
        }
        Some(Self {
            segmentation_model,
            embedding_model,
            num_threads: 2,
            provider: "cpu".into(),
            clustering_threshold: 0.5,
            num_clusters: -1,
            min_duration_on: 0.3,
            min_duration_off: 0.5,
        })
    }
}

/// Sherpa offline diarizer wrapper.
pub struct SherpaDiarizer {
    inner: OfflineSpeakerDiarization,
    expected_sample_rate: u32,
}

impl SherpaDiarizer {
    pub fn try_create(config: &SherpaDiarizerConfig) -> Option<Self> {
        let sd_config = OfflineSpeakerDiarizationConfig {
            segmentation: OfflineSpeakerSegmentationModelConfig {
                pyannote: OfflineSpeakerSegmentationPyannoteModelConfig {
                    model: Some(config.segmentation_model.clone()),
                },
                num_threads: config.num_threads,
                debug: false,
                provider: Some(config.provider.clone()),
            },
            embedding: SpeakerEmbeddingExtractorConfig {
                model: Some(config.embedding_model.clone()),
                num_threads: config.num_threads,
                debug: false,
                provider: Some(config.provider.clone()),
            },
            clustering: FastClusteringConfig {
                num_clusters: config.num_clusters,
                threshold: config.clustering_threshold,
            },
            min_duration_on: config.min_duration_on,
            min_duration_off: config.min_duration_off,
        };

        let inner = match OfflineSpeakerDiarization::create(&sd_config) {
            Some(d) => d,
            None => {
                warn!("Sherpa OfflineSpeakerDiarization::create returned null");
                return None;
            }
        };
        let expected_sample_rate = inner.sample_rate().max(0) as u32;
        if expected_sample_rate == 0 {
            warn!("Sherpa diarizer reported sample_rate=0");
            return None;
        }
        Some(Self {
            inner,
            expected_sample_rate,
        })
    }

    pub fn try_from_env() -> Option<Self> {
        let cfg = SherpaDiarizerConfig::from_env()?;
        Self::try_create(&cfg)
    }

    pub fn expected_sample_rate(&self) -> u32 {
        self.expected_sample_rate
    }
}

impl Diarizer for SherpaDiarizer {
    fn diarize(&self, pcm: &[f32], sample_rate: u32) -> Vec<SpeakerSpan> {
        if pcm.is_empty() || sample_rate == 0 {
            return Vec::new();
        }
        let samples = if sample_rate == self.expected_sample_rate {
            pcm.to_vec()
        } else {
            resample_mono(pcm, sample_rate, self.expected_sample_rate)
        };

        let Some(result) = self.inner.process(&samples) else {
            warn!("Sherpa diarization process returned null");
            return Vec::new();
        };

        result
            .sort_by_start_time()
            .into_iter()
            .filter(|s| s.end > s.start)
            .map(|s| SpeakerSpan {
                speaker_key: s.speaker.to_string(),
                start_ms: (s.start * 1000.0).round() as i64,
                end_ms: (s.end * 1000.0).round() as i64,
            })
            .collect()
    }
}
