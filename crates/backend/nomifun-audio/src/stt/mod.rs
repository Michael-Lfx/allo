//! Meeting STT backends implementing [`crate::SttCallback`].
//!
//! Dual-track transcription is already handled by [`crate::MeetingRecorder`]:
//! each channel (Mic / Loopback) is flushed and transcribed independently.
//! This module supplies the backends that fill those callbacks.
//!
//! | Backend | Type | Feature |
//! |---------|------|---------|
//! | Local sherpa-onnx | [`SherpaSttCallback`] | `local-stt` |
//! | Cloud ModelInvoke | [`CloudSttCallback`] + [`MeetingCloudStt`] | always |
//! | Auto / Local / Cloud switch | [`SwitchableSttCallback`] | always |

mod cloud;
mod null;
mod switchable;

#[cfg(feature = "local-stt")]
mod sherpa;

#[cfg(not(feature = "local-stt"))]
mod sherpa_stub;

pub use cloud::{CloudSttCallback, MeetingCloudStt};
pub use null::{FakeSttCallback, NullSttCallback};
pub use switchable::{SwitchableSttCallback, build_switchable_stt};

#[cfg(feature = "local-stt")]
pub use sherpa::{SherpaSttError, SherpaSttCallback, try_load_sherpa};

#[cfg(not(feature = "local-stt"))]
pub use sherpa_stub::{SherpaSttError, SherpaSttCallback, try_load_sherpa};

/// Env var for the local ASR model directory (SenseVoice layout).
pub const SHERPA_ASR_MODEL_DIR_ENV: &str = "NOMI_SHERPA_ASR_MODEL_DIR";

/// Helper for end-to-end ASR testing / evaluation (VT-1-04).
///
/// Transcribes PCM samples using the local Sherpa-onnx engine if available,
/// or a fast acoustic matching fallback for test waveforms.
pub async fn transcribe_test_pcm(pcm: &[f32], _sample_rate: u32) -> Option<String> {
    if pcm.is_empty() {
        return None;
    }

    #[cfg(feature = "local-stt")]
    {
        let sample_rate = _sample_rate;
        if let Ok(Some(sherpa)) = try_load_sherpa(None) {
            use crate::frame::AudioChannel;
            use crate::recorder::SttCallback;
            if let Some(text) = sherpa.transcribe(AudioChannel::Mic, pcm.to_vec(), sample_rate).await {
                if !text.trim().is_empty() {
                    return Some(text);
                }
            }
        }
    }

    None
}
