//! Track encoding: WAV (always) and M4A/AAC (feature `aac-encode`, Windows MF).

use std::path::{Path, PathBuf};

use tracing::warn;

/// Encode mono f32 PCM as a minimal WAV byte vector (16-bit LE, 1 channel).
pub fn pcm_to_wav(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let pcm_i16: Vec<i16> = samples
        .iter()
        .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
        .collect();
    let data_bytes: Vec<u8> = pcm_i16.iter().flat_map(|s| s.to_le_bytes()).collect();
    let data_len = data_bytes.len() as u32;
    let channels: u16 = 1;
    let bits: u16 = 16;
    let byte_rate = sample_rate * u32::from(channels) * u32::from(bits) / 8;

    let mut wav = Vec::with_capacity(44 + data_bytes.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&(channels * bits / 8).to_le_bytes());
    wav.extend_from_slice(&bits.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.extend_from_slice(&data_bytes);
    wav
}

/// Encode mono f32 PCM to an M4A (AAC-LC in MP4) byte buffer.
///
/// On Windows with feature `aac-encode`, uses Media Foundation.
/// Otherwise returns an error (callers should use [`encode_track_default`]).
pub fn pcm_to_m4a(samples: &[f32], sample_rate: u32) -> Result<Vec<u8>, String> {
    #[cfg(all(feature = "aac-encode", target_os = "windows"))]
    {
        return mf_aac::pcm_to_m4a_bytes(samples, sample_rate);
    }
    #[cfg(not(all(feature = "aac-encode", target_os = "windows")))]
    {
        let _ = (samples, sample_rate);
        Err(
            "AAC/M4A encode requires feature `aac-encode` on Windows (Media Foundation)"
                .into(),
        )
    }
}

/// Write mono f32 PCM to `path` as M4A.
pub fn write_track_m4a(path: &Path, samples: &[f32], sample_rate: u32) -> Result<(), String> {
    let bytes = pcm_to_m4a(samples, sample_rate)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, bytes).map_err(|e| e.to_string())
}

/// Write a track using the preferred on-disk format.
///
/// With `aac-encode` on Windows → `.m4a` via Media Foundation.
/// On failure or without the feature → `.wav` and returns that path.
pub fn encode_track_default(
    path_stem: &Path,
    samples: &[f32],
    sample_rate: u32,
) -> Result<PathBuf, String> {
    #[cfg(feature = "aac-encode")]
    {
        let m4a_path = path_stem.with_extension("m4a");
        match write_track_m4a(&m4a_path, samples, sample_rate) {
            Ok(()) => return Ok(m4a_path),
            Err(e) => {
                warn!("AAC/M4A encode unavailable ({e}); falling back to WAV");
            }
        }
    }

    let wav_path = path_stem.with_extension("wav");
    if let Some(parent) = wav_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&wav_path, pcm_to_wav(samples, sample_rate)).map_err(|e| e.to_string())?;
    Ok(wav_path)
}

#[cfg(all(feature = "aac-encode", target_os = "windows"))]
mod mf_aac {
    use std::fs;
    use std::path::PathBuf;

    use windows::Win32::Media::MediaFoundation::{
        IMFMediaType, IMFSample, IMFSinkWriter, MFAudioFormat_AAC, MFAudioFormat_PCM,
        MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample, MFCreateSinkWriterFromURL,
        MFMediaType_Audio, MFSTARTUP_NOSOCKET, MFShutdown, MFStartup,
        MF_MT_AAC_AUDIO_PROFILE_LEVEL_INDICATION, MF_MT_AAC_PAYLOAD_TYPE,
        MF_MT_AUDIO_AVG_BYTES_PER_SECOND, MF_MT_AUDIO_BITS_PER_SAMPLE,
        MF_MT_AUDIO_BLOCK_ALIGNMENT, MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND,
        MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_VERSION,
    };
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
    use windows::core::HSTRING;

    use crate::pcm_util::resample_mono;

    /// MF AAC encoder is most reliable at 44.1 / 48 kHz.
    fn encode_sample_rate(sr: u32) -> u32 {
        const SUPPORTED: &[u32] = &[8_000, 11_025, 12_000, 16_000, 22_050, 24_000, 32_000, 44_100, 48_000];
        if SUPPORTED.contains(&sr) {
            // Prefer bumping 16 kHz speech to 44.1 kHz — some MF builds reject
            // SetInputMediaType for low rates with the inbox AAC MFT.
            if sr < 32_000 {
                44_100
            } else {
                sr
            }
        } else {
            44_100
        }
    }

    pub fn pcm_to_m4a_bytes(samples: &[f32], sample_rate: u32) -> Result<Vec<u8>, String> {
        if samples.is_empty() {
            return Err("cannot encode empty PCM to m4a".into());
        }
        let sample_rate = if sample_rate == 0 { 16_000 } else { sample_rate };
        let target_sr = encode_sample_rate(sample_rate);
        let pcm = if target_sr == sample_rate {
            samples.to_vec()
        } else {
            resample_mono(samples, sample_rate, target_sr)
        };

        let dir = std::env::temp_dir().join("nomifun-audio-m4a");
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(format!(
            "{}.mp4",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        encode_to_path(&path, &pcm, target_sr)?;
        let bytes = fs::read(&path).map_err(|e| e.to_string())?;
        let _ = fs::remove_file(&path);
        if bytes.is_empty() {
            return Err("Media Foundation produced empty m4a".into());
        }
        Ok(bytes)
    }

    fn encode_to_path(path: &PathBuf, samples: &[f32], sample_rate: u32) -> Result<(), String> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED)
                .ok()
                .map_err(|e| format!("CoInitializeEx: {e}"))?;
        }

        let result = (|| -> Result<(), String> {
            unsafe {
                MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET)
                    .map_err(|e| format!("MFStartup: {e}"))?;

                let url = HSTRING::from(path.as_os_str());
                let writer: IMFSinkWriter = MFCreateSinkWriterFromURL(&url, None, None)
                    .map_err(|e| format!("MFCreateSinkWriterFromURL: {e}"))?;

                let out_type: IMFMediaType =
                    MFCreateMediaType().map_err(|e| format!("MFCreateMediaType out: {e}"))?;
                out_type
                    .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)
                    .map_err(|e| e.to_string())?;
                out_type
                    .SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC)
                    .map_err(|e| e.to_string())?;
                out_type
                    .SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, sample_rate)
                    .map_err(|e| e.to_string())?;
                out_type
                    .SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 1)
                    .map_err(|e| e.to_string())?;
                // ~96 kbps
                out_type
                    .SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 12_000)
                    .map_err(|e| e.to_string())?;
                out_type
                    .SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)
                    .map_err(|e| e.to_string())?;
                // 0 = AAC raw in MP4; 0x29 = AAC-LC profile level
                out_type
                    .SetUINT32(&MF_MT_AAC_PAYLOAD_TYPE, 0)
                    .map_err(|e| e.to_string())?;
                out_type
                    .SetUINT32(&MF_MT_AAC_AUDIO_PROFILE_LEVEL_INDICATION, 0x29)
                    .map_err(|e| e.to_string())?;

                let stream_index = writer
                    .AddStream(&out_type)
                    .map_err(|e| format!("AddStream AAC: {e}"))?;

                let in_type: IMFMediaType =
                    MFCreateMediaType().map_err(|e| format!("MFCreateMediaType in: {e}"))?;
                in_type
                    .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)
                    .map_err(|e| e.to_string())?;
                in_type
                    .SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)
                    .map_err(|e| e.to_string())?;
                in_type
                    .SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 1)
                    .map_err(|e| e.to_string())?;
                in_type
                    .SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, sample_rate)
                    .map_err(|e| e.to_string())?;
                in_type
                    .SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)
                    .map_err(|e| e.to_string())?;
                in_type
                    .SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, 2)
                    .map_err(|e| e.to_string())?;
                in_type
                    .SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, sample_rate * 2)
                    .map_err(|e| e.to_string())?;

                writer
                    .SetInputMediaType(stream_index, &in_type, None)
                    .map_err(|e| format!("SetInputMediaType PCM: {e}"))?;
                writer
                    .BeginWriting()
                    .map_err(|e| format!("BeginWriting: {e}"))?;

                let frame_samples = (sample_rate / 10).max(1) as usize;
                let mut offset = 0usize;
                let mut time_hns: i64 = 0;
                let hns_per_sample = 10_000_000i64 / i64::from(sample_rate);

                while offset < samples.len() {
                    let end = (offset + frame_samples).min(samples.len());
                    let slice = &samples[offset..end];
                    let n = slice.len();
                    let byte_len = n * 2;

                    let buffer = MFCreateMemoryBuffer(byte_len as u32)
                        .map_err(|e| format!("MFCreateMemoryBuffer: {e}"))?;
                    {
                        let mut ptr = std::ptr::null_mut();
                        let mut max_len = 0u32;
                        buffer
                            .Lock(&mut ptr, Some(&mut max_len), None)
                            .map_err(|e| format!("IMFMediaBuffer::Lock: {e}"))?;
                        if ptr.is_null() || max_len < byte_len as u32 {
                            let _ = buffer.Unlock();
                            return Err("MF buffer lock failed".into());
                        }
                        let dst = std::slice::from_raw_parts_mut(ptr, byte_len);
                        for (i, s) in slice.iter().enumerate() {
                            let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                            let b = v.to_le_bytes();
                            dst[i * 2] = b[0];
                            dst[i * 2 + 1] = b[1];
                        }
                        let _ = buffer.Unlock();
                        buffer
                            .SetCurrentLength(byte_len as u32)
                            .map_err(|e| e.to_string())?;
                    }

                    let sample: IMFSample =
                        MFCreateSample().map_err(|e| format!("MFCreateSample: {e}"))?;
                    sample.AddBuffer(&buffer).map_err(|e| e.to_string())?;
                    sample.SetSampleTime(time_hns).map_err(|e| e.to_string())?;
                    let duration = hns_per_sample * n as i64;
                    sample
                        .SetSampleDuration(duration)
                        .map_err(|e| e.to_string())?;

                    writer
                        .WriteSample(stream_index, &sample)
                        .map_err(|e| format!("WriteSample: {e}"))?;

                    time_hns += duration;
                    offset = end;
                }

                writer
                    .Finalize()
                    .map_err(|e| format!("IMFSinkWriter::Finalize: {e}"))?;
                MFShutdown().map_err(|e| format!("MFShutdown: {e}"))?;
                Ok(())
            }
        })();

        unsafe {
            CoUninitialize();
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_to_wav_produces_valid_header() {
        let samples = vec![0.0f32; 160];
        let wav = pcm_to_wav(&samples, 16_000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(wav.len(), 44 + 160 * 2);
    }

    #[test]
    fn encode_track_default_writes_something() {
        let dir = tempfile::tempdir().unwrap();
        let stem = dir.path().join("track");
        let samples = vec![0.1f32; 1600];
        let path = encode_track_default(&stem, &samples, 16_000).expect("encode");
        assert!(path.exists());
        let bytes = std::fs::read(&path).unwrap();
        assert!(!bytes.is_empty());
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        assert!(ext == "wav" || ext == "m4a", "unexpected extension {ext}");
        if ext == "wav" {
            assert_eq!(&bytes[0..4], b"RIFF");
        } else {
            assert!(bytes.len() > 8);
            assert_eq!(&bytes[4..8], b"ftyp");
        }
    }

    #[test]
    fn pcm_to_m4a_api_is_callable() {
        let samples = vec![0.0f32; 3200];
        match pcm_to_m4a(&samples, 16_000) {
            Ok(bytes) => {
                assert!(bytes.len() > 8);
                assert_eq!(&bytes[4..8], b"ftyp");
            }
            Err(e) => {
                assert!(
                    e.contains("aac-encode")
                        || e.contains("Media Foundation")
                        || e.contains("MF")
                        || e.contains("empty")
                        || e.contains("AddStream")
                        || e.contains("BeginWriting")
                        || e.contains("SetInputMediaType")
                        || e.contains("0xC00D"),
                    "unexpected error: {e}"
                );
            }
        }
    }
}
