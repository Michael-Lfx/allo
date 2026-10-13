use std::path::PathBuf;
use axum::extract::State;
use axum::Json;
use axum::Router;
use axum::routing::{get, post};
use nomifun_api_types::voice::{
    VoiceAsrTestRequest, VoiceAsrTestResponse, VoiceDeviceStatusResponse,
};

use crate::mic::{probe_default_device, record_mic_snippet};
use crate::stt::transcribe_test_pcm_detailed;

/// State carried by voice diagnostics routes.
#[derive(Debug, Clone, Default)]
pub struct VoiceDiagnosticsState {
    pub models_dir: Option<PathBuf>,
}

/// Axum routes for voice device and ASR test diagnostics.
pub fn voice_diagnostics_routes() -> Router {
    voice_diagnostics_routes_with_dir(None)
}

/// Axum routes for voice device and ASR test diagnostics with explicit models directory.
pub fn voice_diagnostics_routes_with_dir(models_dir: Option<PathBuf>) -> Router {
    let state = VoiceDiagnosticsState { models_dir };
    Router::new()
        .route("/api/voice/diagnostics/device", get(device_status_handler))
        .route("/api/voice/diagnostics/asr-test", post(asr_test_handler))
        .with_state(state)
}

async fn device_status_handler() -> Json<VoiceDeviceStatusResponse> {
    Json(probe_default_device())
}

async fn asr_test_handler(
    State(state): State<VoiceDiagnosticsState>,
    payload: Option<Json<VoiceAsrTestRequest>>,
) -> Json<VoiceAsrTestResponse> {
    let req = payload.map(|Json(r)| r).unwrap_or_default();
    let start = std::time::Instant::now();

    // 1. Resolve PCM samples either from provided base64 audio or from system mic recording
    let (pcm, header_sample_rate) = match req.audio_base64 {
        Some(ref b64) if !b64.trim().is_empty() => {
            match decode_audio_base64(b64) {
                Ok(res) => res,
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
                Ok(samples) => (samples, None),
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

    let sample_rate = if req.sample_rate != 0 {
        req.sample_rate
    } else if let Some(sr) = header_sample_rate {
        sr
    } else {
        16_000
    };

    if pcm.is_empty() {
        return Json(VoiceAsrTestResponse {
            success: false,
            text: String::new(),
            duration_ms: start.elapsed().as_millis() as u64,
            error: Some("Audio buffer was empty".to_string()),
        });
    }

    // 2. Check audio energy (RMS) to distinguish silence/muted mic from ASR model failure
    let sum_sq: f32 = pcm.iter().map(|&s| s * s).sum();
    let rms = (sum_sq / pcm.len() as f32).sqrt();

    if rms < 0.005 {
        return Json(VoiceAsrTestResponse {
            success: false,
            text: String::new(),
            duration_ms: start.elapsed().as_millis() as u64,
            error: Some(format!(
                "麦克风录音音量过低或静音 (RMS={rms:.4})。检测到声音输入微弱，若使用的是远程虚拟音频设备，请检查远程软件的声音重定向或麦克风开关。"
            )),
        });
    }

    // 3. Perform ASR transcription via SenseVoice model with 16kHz resampling
    match transcribe_test_pcm_detailed(&pcm, sample_rate, state.models_dir.as_deref()).await {
        Ok(text) => {
            let duration_ms = start.elapsed().as_millis() as u64;
            tracing::info!(
                text = %text,
                duration_ms,
                rms = %format!("{:.4}", rms),
                samples = pcm.len(),
                "Voice ASR trial test completed successfully"
            );
            Json(VoiceAsrTestResponse {
                success: true,
                text,
                duration_ms,
                error: None,
            })
        }
        Err(err) => {
            let duration_ms = start.elapsed().as_millis() as u64;
            tracing::warn!(
                error = %err,
                duration_ms,
                rms = %format!("{:.4}", rms),
                "Voice ASR trial test failed"
            );
            Json(VoiceAsrTestResponse {
                success: false,
                text: String::new(),
                duration_ms,
                error: Some(err),
            })
        }
    }
}

/// Decodes base64 payload into mono f32 PCM samples.
/// Supports RIFF WAVE header parsing or raw 16-bit signed PCM.
fn decode_audio_base64(b64: &str) -> Result<(Vec<f32>, Option<u32>), String> {
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

    if bytes.len() >= 4 && &bytes[0..4] == &[0x1A, 0x45, 0xDF, 0xA3] {
        return Err("WebM audio format not supported directly; please provide 16kHz WAV or PCM".into());
    }

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
        Ok((samples, None))
    }
}

fn parse_wav_pcm(wav: &[u8]) -> Result<(Vec<f32>, Option<u32>), String> {
    if wav.len() < 44 {
        return Err("WAV data too short".into());
    }

    let mut cursor = 12;
    let mut audio_format = 1u16; // 1 = PCM, 3 = IEEE Float
    let mut channels = 1u16;
    let mut bits_per_sample = 16u16;
    let mut wav_sample_rate = 16_000u32;
    let mut found_fmt = false;

    while cursor + 8 <= wav.len() {
        let chunk_id = &wav[cursor..cursor + 4];
        let chunk_size = u32::from_le_bytes([
            wav[cursor + 4],
            wav[cursor + 5],
            wav[cursor + 6],
            wav[cursor + 7],
        ]) as usize;
        cursor += 8;

        if chunk_id == b"fmt " {
            if cursor + 16 <= wav.len() {
                audio_format = u16::from_le_bytes([wav[cursor], wav[cursor + 1]]);
                channels = u16::from_le_bytes([wav[cursor + 2], wav[cursor + 3]]);
                wav_sample_rate = u32::from_le_bytes([
                    wav[cursor + 4],
                    wav[cursor + 5],
                    wav[cursor + 6],
                    wav[cursor + 7],
                ]);
                bits_per_sample = u16::from_le_bytes([wav[cursor + 14], wav[cursor + 15]]);
                found_fmt = true;
            }
        } else if chunk_id == b"data" {
            let data_end = (cursor + chunk_size).min(wav.len());
            let data_bytes = &wav[cursor..data_end];

            let samples = match (audio_format, bits_per_sample, channels) {
                // 16-bit PCM mono
                (1, 16, 1) => {
                    let mut samples = Vec::with_capacity(data_bytes.len() / 2);
                    for chunk in data_bytes.chunks_exact(2) {
                        let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
                        samples.push(sample as f32 / i16::MAX as f32);
                    }
                    Ok(samples)
                }
                // 16-bit PCM stereo (downmix to mono)
                (1, 16, 2) => {
                    let mut samples = Vec::with_capacity(data_bytes.len() / 4);
                    for chunk in data_bytes.chunks_exact(4) {
                        let left = i16::from_le_bytes([chunk[0], chunk[1]]) as f32 / i16::MAX as f32;
                        let right = i16::from_le_bytes([chunk[2], chunk[3]]) as f32 / i16::MAX as f32;
                        samples.push((left + right) * 0.5);
                    }
                    Ok(samples)
                }
                // 32-bit Float mono
                (3, 32, 1) => {
                    let mut samples = Vec::with_capacity(data_bytes.len() / 4);
                    for chunk in data_bytes.chunks_exact(4) {
                        let sample = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                        samples.push(sample.clamp(-1.0, 1.0));
                    }
                    Ok(samples)
                }
                // 32-bit Float stereo (downmix to mono)
                (3, 32, 2) => {
                    let mut samples = Vec::with_capacity(data_bytes.len() / 8);
                    for chunk in data_bytes.chunks_exact(8) {
                        let left = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                        let right = f32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
                        samples.push(((left + right) * 0.5).clamp(-1.0, 1.0));
                    }
                    Ok(samples)
                }
                _ => {
                    if !found_fmt {
                        // Fallback assuming 16-bit PCM mono
                        let mut samples = Vec::with_capacity(data_bytes.len() / 2);
                        for chunk in data_bytes.chunks_exact(2) {
                            let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
                            samples.push(sample as f32 / i16::MAX as f32);
                        }
                        Ok(samples)
                    } else {
                        Err(format!(
                            "Unsupported WAV format: format={audio_format}, bits={bits_per_sample}, channels={channels}"
                        ))
                    }
                }
            }?;
            return Ok((samples, if found_fmt { Some(wav_sample_rate) } else { None }));
        }

        // RIFF chunks are padded to even number of bytes
        let padded_size = chunk_size + (chunk_size % 2);
        cursor = cursor.saturating_add(padded_size);
    }
    Err("WAV missing 'data' chunk".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_wav_pcm_mono() {
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&36u32.to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        // fmt chunk (16 bytes)
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // Mono
        wav.extend_from_slice(&16000u32.to_le_bytes()); // 16kHz
        wav.extend_from_slice(&32000u32.to_le_bytes()); // Byte rate
        wav.extend_from_slice(&2u16.to_le_bytes()); // Block align
        wav.extend_from_slice(&16u16.to_le_bytes()); // 16-bit
        // data chunk
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&4u32.to_le_bytes());
        wav.extend_from_slice(&0i16.to_le_bytes());
        wav.extend_from_slice(&16384i16.to_le_bytes());

        let (samples, sr) = parse_wav_pcm(&wav).expect("should parse valid mono wav");
        assert_eq!(samples.len(), 2);
        assert_eq!(sr, Some(16000));
        assert!((samples[0] - 0.0).abs() < 1e-4);
        assert!((samples[1] - 0.5).abs() < 1e-3);
    }

    #[test]
    fn test_parse_wav_pcm_stereo_downmix() {
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&36u32.to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        // fmt chunk
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&2u16.to_le_bytes()); // Stereo
        wav.extend_from_slice(&16000u32.to_le_bytes());
        wav.extend_from_slice(&64000u32.to_le_bytes());
        wav.extend_from_slice(&4u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        // data chunk: 2 frames = 8 bytes
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&8u32.to_le_bytes());
        // frame 1: left=0, right=16384 -> avg ~ 0.25
        wav.extend_from_slice(&0i16.to_le_bytes());
        wav.extend_from_slice(&16384i16.to_le_bytes());
        // frame 2: left=32767, right=32767 -> avg ~ 1.0
        wav.extend_from_slice(&32767i16.to_le_bytes());
        wav.extend_from_slice(&32767i16.to_le_bytes());

        let (samples, sr) = parse_wav_pcm(&wav).expect("should parse valid stereo wav");
        assert_eq!(samples.len(), 2);
        assert_eq!(sr, Some(16000));
        assert!((samples[0] - 0.25).abs() < 1e-2);
        assert!((samples[1] - 1.0).abs() < 1e-2);
    }

    #[test]
    fn test_webm_detection_rejection() {
        use base64::Engine;
        let webm_header = vec![0x1A, 0x45, 0xDF, 0xA3, 0x9F, 0x42, 0x86, 0x81];
        let b64 = base64::engine::general_purpose::STANDARD.encode(&webm_header);
        let res = decode_audio_base64(&b64);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("WebM audio format not supported"));
    }

    #[tokio::test]
    async fn test_asr_handler_silence_rejection() {
        use base64::Engine;
        // 16000 samples of flat silence (zeros)
        let silence_pcm = vec![0.0f32; 16000];
        let mut raw_bytes = Vec::with_capacity(silence_pcm.len() * 2);
        for &s in &silence_pcm {
            let i = (s * i16::MAX as f32) as i16;
            raw_bytes.extend_from_slice(&i.to_le_bytes());
        }
        let b64 = base64::engine::general_purpose::STANDARD.encode(&raw_bytes);
        let req = VoiceAsrTestRequest {
            audio_base64: Some(b64),
            sample_rate: 16000,
        };
        let Json(resp) = asr_test_handler(
            State(VoiceDiagnosticsState::default()),
            Some(Json(req)),
        )
        .await;
        assert!(!resp.success);
        assert!(resp.error.as_deref().unwrap_or("").contains("静音") || resp.error.as_deref().unwrap_or("").contains("RMS"));
    }
}
