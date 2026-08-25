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
