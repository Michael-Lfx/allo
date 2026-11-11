//! Built-in pre-rendered localized vocal feedback phrases.
//!
//! Provides zero-latency (<5ms) pre-rendered speech for fixed phrases
//! such as wake reply ("我在", "I'm here") and dismissal ("好的，再见", "Goodbye"),
//! avoiding on-the-fly TTS synthesis latency entirely.

pub const WAKE_ZH_WAV: &[u8] = include_bytes!("../assets/vocal/wake_zh.wav");
pub const WAKE_EN_WAV: &[u8] = include_bytes!("../assets/vocal/wake_en.wav");
pub const BARGE_ZH_WAV: &[u8] = include_bytes!("../assets/vocal/barge_zh.wav");
pub const BARGE_EN_WAV: &[u8] = include_bytes!("../assets/vocal/barge_en.wav");
pub const DISMISS_ZH_WAV: &[u8] = include_bytes!("../assets/vocal/dismiss_zh.wav");
pub const DISMISS_EN_WAV: &[u8] = include_bytes!("../assets/vocal/dismiss_en.wav");

/// Pre-rendered vocal phrase variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VocalPhrase {
    WakeZh,
    WakeEn,
    BargeZh,
    BargeEn,
    DismissZh,
    DismissEn,
}

impl VocalPhrase {
    /// Array of all 6 vocal phrase kinds.
    pub const ALL: [VocalPhrase; 6] = [
        VocalPhrase::WakeZh,
        VocalPhrase::WakeEn,
        VocalPhrase::BargeZh,
        VocalPhrase::BargeEn,
        VocalPhrase::DismissZh,
        VocalPhrase::DismissEn,
    ];

    /// Slice of all 6 vocal phrases.
    pub const fn all() -> &'static [VocalPhrase] {
        &Self::ALL
    }

    /// Corresponding static WAV audio bytes.
    pub const fn as_wav_bytes(&self) -> &'static [u8] {
        match self {
            Self::WakeZh => WAKE_ZH_WAV,
            Self::WakeEn => WAKE_EN_WAV,
            Self::BargeZh => BARGE_ZH_WAV,
            Self::BargeEn => BARGE_EN_WAV,
            Self::DismissZh => DISMISS_ZH_WAV,
            Self::DismissEn => DISMISS_EN_WAV,
        }
    }

    /// Corresponding filename for logging or referencing.
    pub const fn filename(&self) -> &'static str {
        match self {
            Self::WakeZh => "wake_zh.wav",
            Self::WakeEn => "wake_en.wav",
            Self::BargeZh => "barge_zh.wav",
            Self::BargeEn => "barge_en.wav",
            Self::DismissZh => "dismiss_zh.wav",
            Self::DismissEn => "dismiss_en.wav",
        }
    }

    /// Returns the pre-rendered wake reply WAV bytes for the given language mode.
    pub const fn wake_bytes(is_english: bool) -> &'static [u8] {
        if is_english {
            WAKE_EN_WAV
        } else {
            WAKE_ZH_WAV
        }
    }

    /// Returns the pre-rendered barge-in prompt WAV bytes ("你说" / "Go ahead") for the given language mode.
    pub const fn barge_bytes(is_english: bool) -> &'static [u8] {
        if is_english {
            BARGE_EN_WAV
        } else {
            BARGE_ZH_WAV
        }
    }

    /// Returns the pre-rendered dismiss WAV bytes for the given language mode.
    pub const fn dismiss_bytes(is_english: bool) -> &'static [u8] {
        if is_english {
            DISMISS_EN_WAV
        } else {
            DISMISS_ZH_WAV
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vocal_assets_are_non_empty_and_have_valid_wav_header() {
        for phrase in VocalPhrase::ALL {
            let bytes = phrase.as_wav_bytes();
            let name = phrase.filename();
            assert!(!bytes.is_empty(), "Vocal asset {name} must not be empty");
            assert!(
                bytes.len() >= 44,
                "Vocal asset {name} too short for WAV header ({} bytes)",
                bytes.len()
            );

            // RIFF header
            assert_eq!(&bytes[0..4], b"RIFF", "Vocal asset {name} missing RIFF");
            let riff_size =
                u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
            assert_eq!(
                riff_size + 8,
                bytes.len(),
                "Vocal asset {name} RIFF chunk size mismatch"
            );

            // WAVE format
            assert_eq!(&bytes[8..12], b"WAVE", "Vocal asset {name} missing WAVE");

            // fmt chunk
            assert_eq!(&bytes[12..16], b"fmt ", "Vocal asset {name} missing fmt chunk");
            let format_tag = u16::from_le_bytes([bytes[20], bytes[21]]);
            assert_eq!(format_tag, 1, "Vocal asset {name} must be uncompressed PCM (1)");

            let channels = u16::from_le_bytes([bytes[22], bytes[23]]);
            assert_eq!(channels, 1, "Vocal asset {name} must be mono (1 channel)");

            let sample_rate = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
            assert_eq!(sample_rate, 16000, "Vocal asset {name} must be 16kHz sample rate");

            let bits_per_sample = u16::from_le_bytes([bytes[34], bytes[35]]);
            assert_eq!(bits_per_sample, 16, "Vocal asset {name} must be 16-bit PCM");

            // data chunk
            assert_eq!(&bytes[36..40], b"data", "Vocal asset {name} missing data chunk");
            let data_size =
                u32::from_le_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]) as usize;
            assert_eq!(
                data_size + 44,
                bytes.len(),
                "Vocal asset {name} data size header mismatch"
            );
        }
    }

    #[test]
    fn vocal_locale_dispatch() {
        assert_eq!(VocalPhrase::wake_bytes(true), WAKE_EN_WAV);
        assert_eq!(VocalPhrase::wake_bytes(false), WAKE_ZH_WAV);
        assert_eq!(VocalPhrase::barge_bytes(true), BARGE_EN_WAV);
        assert_eq!(VocalPhrase::barge_bytes(false), BARGE_ZH_WAV);
        assert_eq!(VocalPhrase::dismiss_bytes(true), DISMISS_EN_WAV);
        assert_eq!(VocalPhrase::dismiss_bytes(false), DISMISS_ZH_WAV);
    }

    #[test]
    fn vocal_assets_are_bounded_audible_and_padded() {
        for phrase in VocalPhrase::ALL {
            let bytes = phrase.as_wav_bytes();
            let name = phrase.filename();
            let data_size =
                u32::from_le_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]) as usize;
            let samples: Vec<i16> = bytes[44..44 + data_size]
                .chunks_exact(2)
                .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
                .collect();
            assert!(
                samples.len() > 480 * 2,
                "{name} is too short for the fixed 30ms padding"
            );
            let duration_ms = samples.len() as u64 * 1000 / 16_000;
            assert!(
                (300..=1600).contains(&duration_ms),
                "{name} duration {duration_ms}ms is outside the 300-1600ms contract"
            );
            let leading = &samples[..480];
            let trailing = &samples[samples.len() - 480..];
            assert!(
                leading.iter().all(|sample| sample.abs() <= 2),
                "{name} must start with 30ms silence"
            );
            assert!(
                trailing.iter().all(|sample| sample.abs() <= 2),
                "{name} must end with 30ms silence"
            );
            let peak = samples
                .iter()
                .map(|sample| sample.unsigned_abs() as f32 / 32768.0)
                .fold(0.0f32, f32::max);
            assert!(
                (0.05..=0.999).contains(&peak),
                "{name} peak {peak} is outside the audible-but-unclipped contract"
            );
            let rms = (samples
                .iter()
                .map(|sample| {
                    let value = *sample as f64 / 32768.0;
                    value * value
                })
                .sum::<f64>()
                / samples.len() as f64)
                .sqrt();
            assert!(rms > 0.01, "{name} looks silent (rms {rms})");
        }
    }
}
