//! Speaker diarization backends for meeting audio (R4 / I2).
//!
//! Channel tags (`mic` / `loopback`) are **not** speaker identity — they remain
//! segment metadata only. Identity comes from diarization spans plus optional
//! voiceprint matching.

mod assign;
mod energy;

#[cfg(any(feature = "local-stt", feature = "diarization"))]
mod sherpa;

pub use assign::{SpeakerAssigner, SpeakerIdentity};
pub use energy::{EnergyClusterDiarizer, EnergyClusterDiarizerConfig};

#[cfg(any(feature = "local-stt", feature = "diarization"))]
pub use sherpa::{SherpaDiarizer, SherpaDiarizerConfig};

/// One contiguous region attributed to a diarization cluster key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeakerSpan {
    /// Backend-local cluster id (e.g. `"0"`, `"1"`). Not a persisted UUID.
    pub speaker_key: String,
    pub start_ms: i64,
    pub end_ms: i64,
}

/// Offline diarizer over mono f32 PCM.
pub trait Diarizer: Send {
    fn diarize(&self, pcm: &[f32], sample_rate: u32) -> Vec<SpeakerSpan>;
}

/// Best available diarizer: Sherpa when the feature is on and models load,
/// otherwise [`EnergyClusterDiarizer`].
pub fn create_diarizer() -> Box<dyn Diarizer> {
    #[cfg(any(feature = "local-stt", feature = "diarization"))]
    {
        match SherpaDiarizer::try_from_env() {
            Some(d) => {
                tracing::debug!("using Sherpa offline speaker diarization");
                return Box::new(d);
            }
            None => {
                tracing::warn!(
                    "Sherpa diarization unavailable; falling back to EnergyClusterDiarizer"
                );
            }
        }
    }
    tracing::debug!("using EnergyClusterDiarizer");
    Box::new(EnergyClusterDiarizer::default())
}

/// Pick the span with maximum temporal overlap for `[start_ms, end_ms)`.
pub fn dominant_speaker_key(spans: &[SpeakerSpan], start_ms: i64, end_ms: i64) -> Option<&str> {
    if end_ms <= start_ms {
        return None;
    }
    let mut best_key: Option<&str> = None;
    let mut best_overlap: i64 = 0;
    for span in spans {
        let overlap_start = start_ms.max(span.start_ms);
        let overlap_end = end_ms.min(span.end_ms);
        let overlap = overlap_end - overlap_start;
        if overlap > best_overlap {
            best_overlap = overlap;
            best_key = Some(span.speaker_key.as_str());
        }
    }
    best_key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dominant_speaker_picks_max_overlap() {
        let spans = vec![
            SpeakerSpan {
                speaker_key: "0".into(),
                start_ms: 0,
                end_ms: 1000,
            },
            SpeakerSpan {
                speaker_key: "1".into(),
                start_ms: 800,
                end_ms: 2000,
            },
        ];
        assert_eq!(dominant_speaker_key(&spans, 900, 1800), Some("1"));
        assert_eq!(dominant_speaker_key(&spans, 0, 500), Some("0"));
    }

    #[test]
    fn create_diarizer_returns_usable_backend() {
        let d = create_diarizer();
        let silent = vec![0.0f32; 16_000];
        let _ = d.diarize(&silent, 16_000);
    }
}
