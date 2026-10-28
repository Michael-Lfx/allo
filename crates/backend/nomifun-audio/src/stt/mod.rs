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
pub use sherpa::{
    active_model_dir, register_active_model_dir, try_load_sherpa, SherpaSttCallback, SherpaSttError,
};

#[cfg(not(feature = "local-stt"))]
pub use sherpa_stub::{
    active_model_dir, register_active_model_dir, try_load_sherpa, SherpaSttCallback, SherpaSttError,
};

/// Env var for the local ASR model directory (SenseVoice layout).
pub const SHERPA_ASR_MODEL_DIR_ENV: &str = "NOMI_SHERPA_ASR_MODEL_DIR";

/// Helper for end-to-end ASR testing / evaluation (VT-1-04).
///
/// Transcribes PCM samples using the local Sherpa-onnx engine if available,
/// resampling input to 16kHz mono automatically.
pub async fn transcribe_test_pcm(pcm: &[f32], sample_rate: u32) -> Option<String> {
    transcribe_test_pcm_detailed(pcm, sample_rate, None).await.ok()
}

/// Detailed ASR test transcription returning descriptive error messages for diagnostics.
pub async fn transcribe_test_pcm_detailed(
    pcm: &[f32],
    sample_rate: u32,
    model_dir: Option<&std::path::Path>,
) -> Result<String, String> {
    if pcm.is_empty() {
        return Err("录音数据为空，未接收到有效音频采样".to_string());
    }

    // Automatically resample to 16kHz mono required by SenseVoice ASR
    let (pcm_16k, target_sr) = if sample_rate != 16_000 && sample_rate > 0 {
        (crate::pcm_util::resample_mono(pcm, sample_rate, 16_000), 16_000)
    } else {
        (pcm.to_vec(), 16_000)
    };

    #[cfg(feature = "local-stt")]
    {
        match try_load_sherpa(model_dir) {
            Ok(Some(sherpa)) => {
                use crate::frame::AudioChannel;
                use crate::recorder::SttCallback;
                match sherpa.transcribe_checked(AudioChannel::Mic, pcm_16k, target_sr).await {
                    Ok(Some(text)) if !text.trim().is_empty() => Ok(text.trim().to_string()),
                    Ok(_) => Err("SenseVoice 未识别到文字 (可能环境嘈杂或发音不清晰，请清晰说话后重试)".to_string()),
                    Err(error) => Err(format!("SenseVoice 语音识别失败: {error}")),
                }
            }
            Ok(None) => Err(
                "未找到本地 SenseVoice 语音模型。请在【设置 - 语音伴侣】中点击“下载并启用”安装语音模型套件。".to_string()
            ),
            Err(e) => Err(format!("本地 SenseVoice 模型初始化失败: {e}")),
        }
    }

    #[cfg(not(feature = "local-stt"))]
    {
        let _ = (pcm_16k, target_sr, model_dir);
        Err("本地离线语音识别 (local-stt) 未在当前桌面程序中启用。".to_string())
    }
}
