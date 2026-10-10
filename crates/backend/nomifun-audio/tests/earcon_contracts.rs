use nomifun_audio::capture::CaptureReadResult;
use nomifun_audio::earcon::{
    ALERT_WAV, DONE_WAV, ERROR_WAV, Earcon, EarconKind, GUIDE_WAV, SLEEP_WAV, WAKE_WAV,
};

#[test]
fn test_capture_read_result_contract() {
    let chunk = CaptureReadResult::Chunk(vec![0.0; 160]);
    let empty = CaptureReadResult::TemporarilyEmpty;
    let closed = CaptureReadResult::StreamClosed;
    let err = CaptureReadResult::DeviceError("usb disconnected".to_string());

    assert_eq!(chunk, CaptureReadResult::Chunk(vec![0.0; 160]));
    assert_ne!(chunk, empty);
    assert_ne!(empty, closed);
    assert_ne!(closed, err);

    let cloned_err = err.clone();
    assert_eq!(cloned_err, CaptureReadResult::DeviceError("usb disconnected".to_string()));
    assert!(format!("{:?}", empty).contains("TemporarilyEmpty"));
}

#[test]
fn test_earcon_enum_and_constants_contract() {
    assert_eq!(Earcon::Guide.as_wav_bytes(), GUIDE_WAV);
    assert_eq!(Earcon::Wake.as_wav_bytes(), WAKE_WAV);
    assert_eq!(Earcon::Alert.as_wav_bytes(), ALERT_WAV);
    assert_eq!(Earcon::Done.as_wav_bytes(), DONE_WAV);
    assert_eq!(Earcon::Sleep.as_wav_bytes(), SLEEP_WAV);
    assert_eq!(Earcon::Error.as_wav_bytes(), ERROR_WAV);

    assert_eq!(Earcon::ALL.len(), 6);
    assert_eq!(Earcon::all().len(), 6);

    // Verify EarconKind type alias
    let kind: EarconKind = Earcon::Wake;
    assert_eq!(kind, EarconKind::Wake);
}

#[test]
fn test_earcon_wav_headers_validity() {
    let earcons = Earcon::ALL;
    let mut total_bytes = 0usize;

    for earcon in earcons {
        let bytes = earcon.as_wav_bytes();
        let name = earcon.filename();
        total_bytes += bytes.len();

        // 1. Non-empty and at least header size
        assert!(!bytes.is_empty(), "Earcon {} must not be empty", name);
        assert!(
            bytes.len() >= 44,
            "Earcon {} is too small for a WAV header: {} bytes",
            name,
            bytes.len()
        );

        // 2. RIFF header check
        assert_eq!(
            &bytes[0..4],
            b"RIFF",
            "Earcon {} missing 'RIFF' signature",
            name
        );
        let riff_chunk_size = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
        assert_eq!(
            riff_chunk_size + 8,
            bytes.len(),
            "Earcon {} RIFF chunk size mismatch with actual file size",
            name
        );

        // 3. WAVE format check
        assert_eq!(
            &bytes[8..12],
            b"WAVE",
            "Earcon {} missing 'WAVE' format",
            name
        );

        // 4. fmt chunk check
        assert_eq!(
            &bytes[12..16],
            b"fmt ",
            "Earcon {} missing 'fmt ' subchunk",
            name
        );
        let fmt_chunk_size = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        assert!(
            fmt_chunk_size >= 16,
            "Earcon {} fmt chunk size too small: {}",
            name,
            fmt_chunk_size
        );

        let audio_format = u16::from_le_bytes([bytes[20], bytes[21]]);
        assert_eq!(
            audio_format, 1,
            "Earcon {} must be uncompressed PCM (format 1)",
            name
        );

        let num_channels = u16::from_le_bytes([bytes[22], bytes[23]]);
        assert!(
            num_channels == 1 || num_channels == 2,
            "Earcon {} channels must be mono or stereo",
            name
        );

        let sample_rate = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
        assert!(
            sample_rate == 16000 || sample_rate == 44100 || sample_rate == 48000,
            "Earcon {} sample rate unexpected: {}",
            name,
            sample_rate
        );

        let bits_per_sample = u16::from_le_bytes([bytes[34], bytes[35]]);
        assert_eq!(
            bits_per_sample, 16,
            "Earcon {} expected 16-bit PCM",
            name
        );

        // 5. data chunk check
        assert_eq!(
            &bytes[36..40],
            b"data",
            "Earcon {} missing 'data' subchunk",
            name
        );
        let data_chunk_size = u32::from_le_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]) as usize;
        assert_eq!(
            44 + data_chunk_size,
            bytes.len(),
            "Earcon {} data chunk size mismatch",
            name
        );
        assert!(data_chunk_size > 0, "Earcon {} data chunk must contain samples", name);
    }

    // 6. Total size must be < 40KB as per interaction.zh.md line 449
    assert!(
        total_bytes < 40 * 1024,
        "Total earcon size ({} bytes) must be < 40KB",
        total_bytes
    );
}
