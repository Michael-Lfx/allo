//! Built-in event-driven earcon sound assets.
//!
//! Defined by AI Mate interaction contract C5 (Wave 0 M0):
//! - `guide.wav`: Self-check passed / onboarding & download guide
//! - `wake.wav`: Far-field wake-up success
//! - `alert.wav`: High-risk confirmation required
//! - `done.wav`: Background task completed
//! - `sleep.wav`: Timeout or false wake-up return to idle
//! - `error.wav`: Offline or action cancelled

pub const GUIDE_WAV: &[u8] = include_bytes!("../assets/earcon/guide.wav");
pub const WAKE_WAV: &[u8] = include_bytes!("../assets/earcon/wake.wav");
pub const ALERT_WAV: &[u8] = include_bytes!("../assets/earcon/alert.wav");
pub const DONE_WAV: &[u8] = include_bytes!("../assets/earcon/done.wav");
pub const SLEEP_WAV: &[u8] = include_bytes!("../assets/earcon/sleep.wav");
pub const ERROR_WAV: &[u8] = include_bytes!("../assets/earcon/error.wav");

/// Event-driven earcon sound variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Earcon {
    Guide,
    Wake,
    Alert,
    Done,
    Sleep,
    Error,
}

/// Alias for `Earcon` to support both naming styles seamlessly.
pub type EarconKind = Earcon;

impl Earcon {
    /// Array of all 6 earcon kinds.
    pub const ALL: [Earcon; 6] = [
        Earcon::Guide,
        Earcon::Wake,
        Earcon::Alert,
        Earcon::Done,
        Earcon::Sleep,
        Earcon::Error,
    ];

    /// Slice of all 6 earcon kinds.
    pub const fn all() -> &'static [Earcon] {
        &Self::ALL
    }

    /// Corresponding static WAV audio bytes.
    pub const fn as_wav_bytes(&self) -> &'static [u8] {
        match self {
            Earcon::Guide => GUIDE_WAV,
            Earcon::Wake => WAKE_WAV,
            Earcon::Alert => ALERT_WAV,
            Earcon::Done => DONE_WAV,
            Earcon::Sleep => SLEEP_WAV,
            Earcon::Error => ERROR_WAV,
        }
    }

    /// Corresponding filename for logging or referencing.
    pub const fn filename(&self) -> &'static str {
        match self {
            Earcon::Guide => "guide.wav",
            Earcon::Wake => "wake.wav",
            Earcon::Alert => "alert.wav",
            Earcon::Done => "done.wav",
            Earcon::Sleep => "sleep.wav",
            Earcon::Error => "error.wav",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn earcon_assets_are_non_empty_and_have_valid_wav_header() {
        let mut total_size = 0usize;

        for earcon in Earcon::ALL {
            let bytes = earcon.as_wav_bytes();
            let name = earcon.filename();
            total_size += bytes.len();

            assert!(!bytes.is_empty(), "Earcon {name} must not be empty");
            assert!(
                bytes.len() >= 44,
                "Earcon {name} too short for WAV header ({} bytes)",
                bytes.len()
            );

            // RIFF header
            assert_eq!(&bytes[0..4], b"RIFF", "Earcon {name} missing RIFF");
            let riff_size =
                u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
            assert_eq!(
                riff_size + 8,
                bytes.len(),
                "Earcon {name} RIFF chunk size mismatch"
            );

            // WAVE format
            assert_eq!(&bytes[8..12], b"WAVE", "Earcon {name} missing WAVE");

            // fmt subchunk
            assert_eq!(&bytes[12..16], b"fmt ", "Earcon {name} missing fmt ");
            let fmt_size = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
            assert!(fmt_size >= 16, "Earcon {name} invalid fmt size: {fmt_size}");

            let audio_format = u16::from_le_bytes([bytes[20], bytes[21]]);
            assert_eq!(audio_format, 1, "Earcon {name} must be PCM");

            let num_channels = u16::from_le_bytes([bytes[22], bytes[23]]);
            assert!(num_channels == 1 || num_channels == 2);

            let sample_rate =
                u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
            assert!(sample_rate > 0);

            let bits_per_sample = u16::from_le_bytes([bytes[34], bytes[35]]);
            assert_eq!(bits_per_sample, 16);

            // data subchunk
            assert_eq!(&bytes[36..40], b"data", "Earcon {name} missing data");
            let data_size =
                u32::from_le_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]) as usize;
            assert_eq!(
                44 + data_size,
                bytes.len(),
                "Earcon {name} data size mismatch"
            );
            assert!(data_size > 0);
        }

        // Must be under 40KB in total
        assert!(
            total_size < 40 * 1024,
            "Total earcons size ({total_size} bytes) must be < 40KB"
        );
    }
}
