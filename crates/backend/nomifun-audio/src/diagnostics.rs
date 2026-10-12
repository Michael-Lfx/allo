//! Voice & ASR diagnostics endpoints for developer observation & testing.

use axum::Json;
use axum::Router;
use axum::routing::{get, post};
use nomifun_api_types::voice::{
    VoiceAsrTestRequest, VoiceAsrTestResponse, VoiceDeviceStatusResponse,
};

use crate::mic::{probe_default_device, record_mic_snippet};
use crate::stt::transcribe_test_pcm;

/// Axum routes for voice device and ASR test diagnostics.
pub fn voice_diagnostics_routes() -> Router {
    Router::new()
        .route("/api/voice/diagnostics/device", get(device_status_handler))
        .route("/api/voice/diagnostics/asr-test", post(asr_test_handler))
}

async fn device_status_handler() -> Json<VoiceDeviceStatusResponse> {
    Json(probe_default_device())
}

async fn asr_test_handler(
    payload: Option<Json<VoiceAsrTestRequest>>,
) -> Json<VoiceAsrTestResponse> {
    let req = payload.map(|Json(r)| r).unwrap_or_default();
    let start = std::time::Instant::now();
    let sample_rate = if req.sample_rate == 0 { 16_000 } else { req.sample_rate };

    // 1. Resolve PCM samples either from provided base64 audio or from system mic recording
    let pcm = match req.audio_base64 {
        Some(ref b64) if !b64.trim().is_empty() => {
            match decode_audio_base64(b64) {
                Ok(samples) => samples,
                Err(err) => {
                    return Json(VoiceAsrTestResponse {
                        success: false,
                        text: String::new(),
                        duration_ms: start.elapsed().as_millis() as u64,
                        error: Some(format!("Failed to decode audio base64: {err}")),
                    });
                }
            }
        }
        _ => {
            // No audio provided: record ~2.5s snippet from default system mic
            match record_mic_snippet(std::time::Duration::from_millis(2500)).await {
                Ok(samples) => samples,
                Err(err) => {
                    return Json(VoiceAsrTestResponse {
                        success: false,
                        text: String::new(),
                        duration_ms: start.elapsed().as_millis() as u64,
                        error: Some(format!("Microphone capture failed: {err}")),
                    });
                }
            }
        }
    };

    if pcm.is_empty() {
        return Json(VoiceAsrTestResponse {
            success: false,
            text: String::new(),
            duration_ms: start.elapsed().as_millis() as u64,
            error: Some("Audio buffer was empty".to_string()),
        });
    }

    // 2. Perform ASR transcription via SenseVoice model
    match transcribe_test_pcm(&pcm, sample_rate).await {
        Some(text) if !text.trim().is_empty() => {
            Json(VoiceAsrTestResponse {
                success: true,
                text,
                duration_ms: start.elapsed().as_millis() as u64,
                error: None,
            })
        }
        _ => {
            Json(VoiceAsrTestResponse {
                success: false,
                text: String::new(),
                duration_ms: start.elapsed().as_millis() as u64,
                error: Some("ASR produced no transcription (audio may be silent, or SenseVoice model not installed)".to_string()),
            })
        }
    }
}

/// Decodes base64 payload into mono f32 PCM samples.
/// Supports RIFF WAVE header parsing or raw 16-bit signed PCM.
fn decode_audio_base64(b64: &str) -> Result<Vec<f32>, String> {
    use base64::Engine;
    // Strip optional data:audio/...;base64, prefix
    let clean_b64 = if let Some(idx) = b64.find(";base64,") {
        &b64[idx + 8..]
    } else {
        b64
    };

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(clean_b64.trim())
        .map_err(|e| format!("invalid base64: {e}"))?;

    if bytes.len() >= 44 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        // Parse RIFF WAV PCM
        parse_wav_pcm(&bytes)
    } else {
        // Assume raw 16-bit signed little-endian PCM
        let mut samples = Vec::with_capacity(bytes.len() / 2);
        for chunk in bytes.chunks_exact(2) {
            let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
            samples.push(sample as f32 / i16::MAX as f32);
        }
        Ok(samples)
    }
}

fn parse_wav_pcm(wav: &[u8]) -> Result<Vec<f32>, String> {
    if wav.len() < 44 {
        return Err("WAV data too short".into());
    }
    // Find "data" chunk
    let mut cursor = 12;
    while cursor + 8 <= wav.len() {
        let chunk_id = &wav[cursor..cursor + 4];
        let chunk_size = u32::from_le_bytes([
            wav[cursor + 4],
            wav[cursor + 5],
            wav[cursor + 6],
            wav[cursor + 7],
        ]) as usize;
        cursor += 8;

        if chunk_id == b"data" {
            let data_end = (cursor + chunk_size).min(wav.len());
            let data_bytes = &wav[cursor..data_end];
            let mut samples = Vec::with_capacity(data_bytes.len() / 2);
            for chunk in data_bytes.chunks_exact(2) {
                let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
                samples.push(sample as f32 / i16::MAX as f32);
            }
            return Ok(samples);
        }
        cursor += chunk_size;
    }
    Err("WAV missing 'data' chunk".into())
}
