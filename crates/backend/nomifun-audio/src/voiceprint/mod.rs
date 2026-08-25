//! Optional voiceprint encode / match / persist helpers (R4 / I2).
//!
//! Pre-meeting wizard is API-only for now: enroll via [`VoiceprintStore`], then
//! match during [`crate::diarization::SpeakerAssigner`] resolution.

mod store;

#[cfg(any(feature = "local-stt", feature = "diarization"))]
mod sherpa;

pub use store::{
    VoiceprintEntry, VoiceprintGallery, VoiceprintMatch, VoiceprintStore, embedding_from_blob,
    embedding_to_blob,
};

#[cfg(any(feature = "local-stt", feature = "diarization"))]
pub use sherpa::{SherpaVoiceprintEncoder, SherpaVoiceprintEncoderConfig};

/// Encode mono PCM into a speaker embedding vector.
pub trait VoiceprintEncoder: Send {
    fn encode(&self, pcm: &[f32], sample_rate: u32) -> Result<Vec<f32>, String>;
}

/// Cosine similarity in `[-1, 1]`. Returns `0.0` for empty / mismatched lengths.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na <= f32::EPSILON || nb <= f32::EPSILON {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

/// Deterministic non-ML encoder for tests (energy / ZCR style features).
#[derive(Debug, Clone)]
pub struct FakeVoiceprintEncoder {
    dim: usize,
}

impl Default for FakeVoiceprintEncoder {
    fn default() -> Self {
        Self::new(16)
    }
}

impl FakeVoiceprintEncoder {
    pub fn new(dim: usize) -> Self {
        Self { dim: dim.max(4) }
    }

    pub fn dim(&self) -> usize {
        self.dim
    }
}

impl VoiceprintEncoder for FakeVoiceprintEncoder {
    fn encode(&self, pcm: &[f32], sample_rate: u32) -> Result<Vec<f32>, String> {
        if sample_rate == 0 {
            return Err("sample_rate must be > 0".into());
        }
        let mut emb = vec![0.0f32; self.dim];
        if pcm.is_empty() {
            return Ok(emb);
        }

        let rms = (pcm.iter().map(|s| s * s).sum::<f32>() / pcm.len() as f32).sqrt();
        let mut zc = 0usize;
        for w in pcm.windows(2) {
            if w[0].signum() != w[1].signum() {
                zc += 1;
            }
        }
        let zcr = zc as f32 / pcm.len().saturating_sub(1).max(1) as f32;
        let peak = pcm.iter().copied().fold(0.0f32, |a, b| a.max(b.abs()));
        let mean = pcm.iter().sum::<f32>() / pcm.len() as f32;

        emb[0] = rms;
        emb[1] = zcr;
        emb[2] = peak;
        emb[3] = mean;

        // Extra dims: banded absolute averages so similar waveforms stay close.
        let bands = self.dim - 4;
        if bands > 0 {
            let chunk = (pcm.len() / bands).max(1);
            for b in 0..bands {
                let start = b * chunk;
                if start >= pcm.len() {
                    break;
                }
                let end = (start + chunk).min(pcm.len());
                let slice = &pcm[start..end];
                emb[4 + b] = slice.iter().map(|s| s.abs()).sum::<f32>() / slice.len() as f32;
            }
        }
        Ok(emb)
    }
}

/// Slice mono PCM by millisecond range (clamped).
pub fn slice_pcm_ms(pcm: &[f32], sample_rate: u32, start_ms: i64, end_ms: i64) -> Vec<f32> {
    if pcm.is_empty() || sample_rate == 0 || end_ms <= start_ms {
        return Vec::new();
    }
    let start = ((start_ms.max(0) as u64 * sample_rate as u64) / 1000) as usize;
    let end = ((end_ms.max(0) as u64 * sample_rate as u64) / 1000) as usize;
    let start = start.min(pcm.len());
    let end = end.min(pcm.len()).max(start);
    pcm[start..end].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_identical_is_one() {
        let v = vec![1.0, 2.0, 3.0];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn cosine_orthogonal_is_zero() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!(cosine_similarity(&a, &b).abs() < 1e-5);
    }

    #[test]
    fn fake_encoder_is_deterministic() {
        let enc = FakeVoiceprintEncoder::new(8);
        let pcm = vec![0.1, -0.2, 0.3, -0.1, 0.0, 0.4];
        let a = enc.encode(&pcm, 16_000).unwrap();
        let b = enc.encode(&pcm, 16_000).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 8);
    }

    #[test]
    fn similar_waveforms_score_high() {
        let enc = FakeVoiceprintEncoder::new(8);
        let a: Vec<f32> = (0..1000).map(|i| ((i as f32) * 0.02).sin() * 0.3).collect();
        let mut b = a.clone();
        for s in &mut b {
            *s *= 1.02;
        }
        let ea = enc.encode(&a, 16_000).unwrap();
        let eb = enc.encode(&b, 16_000).unwrap();
        assert!(cosine_similarity(&ea, &eb) > 0.95);
    }
}
