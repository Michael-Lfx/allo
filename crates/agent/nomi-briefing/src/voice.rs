//! Sentence-chunked TTS prep + ASR-shaped timing. Anchors are never hand-typed.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::error::{BriefingError, BriefingResult};
use crate::ir::{Beat, WordAnchor, spoken_numerals};
use crate::lint::TAIL_GUARD_SECS;

/// Hard ceiling of `/api/tts` (nomifun-shell). Chunk below this.
pub const TTS_CHAR_LIMIT: usize = 4096;
const TTS_SOFT_LIMIT: usize = 3600;
const PCM_WRAP_RATE: u32 = 24_000;

/// One synthesized clip from the host TTS adapter (`/api/tts` invoke path).
#[derive(Debug, Clone)]
pub struct SynthesizedClip {
    pub bytes: Vec<u8>,
    pub mime: String,
}

/// Per-session TTS override. Empty provider/model means “use install default”.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TtsChoice {
    pub provider_id: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
}

impl TtsChoice {
    pub fn from_parts(
        provider_id: Option<&str>,
        model: Option<&str>,
        voice: Option<&str>,
    ) -> Option<Self> {
        let provider_id = provider_id.map(str::trim).filter(|s| !s.is_empty())?;
        let model = model.map(str::trim).filter(|s| !s.is_empty())?;
        Some(Self {
            provider_id: provider_id.to_string(),
            model: model.to_string(),
            voice: voice
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        })
    }
}

/// Sync TTS seam. The HTTP crate implements this with `ModelInvokeService`
/// (`Handle::current().block_on`) so the blocking pipeline can stay sync.
/// `choice` pins a session model; `None` uses the install-wide default.
pub trait VoiceSynth: Send + Sync {
    fn synthesize(&self, text: &str, choice: Option<&TtsChoice>) -> Result<SynthesizedClip, String>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TtsChunk {
    pub index: usize,
    pub beat_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TimingFile {
    pub chunks: Vec<AlignedChunk>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AlignedChunk {
    pub beat_id: String,
    pub text: String,
    pub start_secs: f64,
    pub end_secs: f64,
    pub words: Vec<WordAnchor>,
}

pub fn chunk_beats(beats: &[Beat]) -> Vec<TtsChunk> {
    let mut chunks = Vec::new();
    let mut index = 0usize;
    for beat in beats {
        let spoken = spoken_numerals(beat.spoken_text.trim());
        if spoken.is_empty() {
            continue;
        }
        for piece in split_spoken(&spoken) {
            chunks.push(TtsChunk {
                index,
                beat_id: beat.id.clone(),
                text: piece,
            });
            index += 1;
        }
    }
    chunks
}

fn split_spoken(text: &str) -> Vec<String> {
    if grapheme_len(text) <= TTS_SOFT_LIMIT {
        return vec![text.to_string()];
    }
    let mut parts = Vec::new();
    let mut current = String::new();
    for sentence in text.split_inclusive(['。', '！', '？', '.', '!', '?', '\n']) {
        if grapheme_len(&current) + grapheme_len(sentence) > TTS_SOFT_LIMIT && !current.is_empty() {
            parts.push(current.trim().to_string());
            current.clear();
        }
        current.push_str(sentence);
        if grapheme_len(&current) > TTS_SOFT_LIMIT {
            while grapheme_len(&current) > TTS_SOFT_LIMIT {
                let (head, rest) = split_at_char_budget(&current, TTS_SOFT_LIMIT);
                parts.push(head);
                current = rest;
            }
        }
    }
    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }
    parts
}

fn grapheme_len(text: &str) -> usize {
    text.chars().count()
}

fn split_at_char_budget(text: &str, budget: usize) -> (String, String) {
    let mut acc = 0usize;
    for (idx, ch) in text.char_indices() {
        if acc >= budget {
            return (text[..idx].to_string(), text[idx..].to_string());
        }
        acc += 1;
        let _ = ch;
    }
    (text.to_string(), String::new())
}

/// Word timestamps from `nomifun-model-invoke` SpeechRecognition (volc / whisper-shaped).
/// Seconds come from ASR output — never from the beat script.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AsrWord {
    pub word: String,
    #[serde(alias = "start", alias = "start_time")]
    pub start_secs: f64,
    #[serde(alias = "end", alias = "end_time")]
    pub end_secs: f64,
}

pub fn parse_asr_json(raw: &str) -> Option<Vec<AsrWord>> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let words = if let Some(arr) = value.get("words").and_then(|v| v.as_array()) {
        arr.clone()
    } else if let Some(arr) = value.as_array() {
        arr.clone()
    } else {
        return None;
    };
    let parsed: Vec<AsrWord> = words
        .into_iter()
        .filter_map(|item| serde_json::from_value(item).ok())
        .filter(|word: &AsrWord| !word.word.trim().is_empty())
        .collect();
    if parsed.is_empty() {
        None
    } else {
        Some(parsed)
    }
}

pub fn load_asr_words(path: &Path) -> Option<Vec<AsrWord>> {
    parse_asr_json(&std::fs::read_to_string(path).ok()?)
}

/// Prefer model-invoke ASR timestamps; fall back to proportional estimates.
/// Beat scripts must not supply seconds.
pub fn align_chunks(chunks: &[TtsChunk], asr_words: Option<&[AsrWord]>) -> TimingFile {
    if let Some(words) = asr_words {
        if let Some(timing) = align_from_asr(chunks, words) {
            return timing;
        }
    }
    align_proportional(chunks, 180.0)
}

pub fn align_from_asr(chunks: &[TtsChunk], words: &[AsrWord]) -> Option<TimingFile> {
    if chunks.is_empty() || words.is_empty() {
        return None;
    }
    let mut word_i = 0usize;
    let mut aligned = Vec::new();
    for chunk in chunks {
        let target = cleaned_text(&chunk.text);
        if target.is_empty() {
            continue;
        }
        let start_idx = word_i;
        let mut acc = String::new();
        while word_i < words.len() && acc.chars().count() < target.chars().count() {
            acc.push_str(&cleaned_text(&words[word_i].word));
            word_i += 1;
        }
        if acc.chars().count() < target.chars().count() || start_idx == word_i {
            return None;
        }
        let slice = &words[start_idx..word_i];
        let word_anchors: Vec<WordAnchor> = slice
            .iter()
            .map(|word| WordAnchor {
                word: word.word.clone(),
                start_secs: word.start_secs,
                end_secs: word.end_secs,
            })
            .collect();
        aligned.push(AlignedChunk {
            beat_id: chunk.beat_id.clone(),
            text: chunk.text.clone(),
            start_secs: slice.first()?.start_secs,
            end_secs: slice.last()?.end_secs + TAIL_GUARD_SECS,
            words: word_anchors,
        });
    }
    Some(TimingFile { chunks: aligned })
}

/// Proportional word timing from character counts. Replaced by ASR when available.
/// Never accepts caller-supplied seconds.
pub fn align_proportional(chunks: &[TtsChunk], words_per_minute: f64) -> TimingFile {
    let wpm = if words_per_minute <= 0.0 {
        180.0
    } else {
        words_per_minute
    };
    let mut cursor = 0.0_f64;
    let mut aligned = Vec::new();
    for chunk in chunks {
        let words = tokenize(&chunk.text);
        let word_count = words.len().max(1) as f64;
        let duration = (word_count / wpm) * 60.0;
        let step = duration / word_count;
        let mut word_anchors = Vec::new();
        for (i, word) in words.iter().enumerate() {
            let start = cursor + step * i as f64;
            word_anchors.push(WordAnchor {
                word: word.clone(),
                start_secs: start,
                end_secs: start + step,
            });
        }
        let end = cursor + duration + TAIL_GUARD_SECS;
        aligned.push(AlignedChunk {
            beat_id: chunk.beat_id.clone(),
            text: chunk.text.clone(),
            start_secs: cursor,
            end_secs: end,
            words: word_anchors,
        });
        cursor = end;
    }
    TimingFile { chunks: aligned }
}

fn cleaned_text(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn tokenize(text: &str) -> Vec<String> {
    text.split_whitespace()
        .flat_map(split_cjk_run)
        .filter(|w| !w.is_empty())
        .collect()
}

fn split_cjk_run(run: &str) -> Vec<String> {
    if run.chars().any(|c| {
        ('\u{4e00}'..='\u{9fff}').contains(&c) || ('\u{3400}'..='\u{4dbf}').contains(&c)
    }) {
        run.chars().map(|c| c.to_string()).collect()
    } else {
        vec![run.to_string()]
    }
}

pub fn apply_timing_to_beats(beats: &mut [Beat], timing: &TimingFile) {
    for beat in beats.iter_mut() {
        beat.anchors.clear();
        for chunk in timing.chunks.iter().filter(|c| c.beat_id == beat.id) {
            beat.anchors.extend(chunk.words.clone());
        }
    }
}

/// Stretch word anchors to measured clip durations (ffprobe / WAV header).
/// Seconds still come from audio, never from the beat script.
pub fn align_from_durations(chunks: &[TtsChunk], durations: &[f64]) -> TimingFile {
    let mut cursor = 0.0_f64;
    let mut aligned = Vec::new();
    for (i, chunk) in chunks.iter().enumerate() {
        let duration = durations.get(i).copied().unwrap_or(0.4).max(0.2);
        let words = tokenize(&chunk.text);
        let word_count = words.len().max(1) as f64;
        let step = duration / word_count;
        let mut word_anchors = Vec::new();
        for (j, word) in words.iter().enumerate() {
            let start = cursor + step * j as f64;
            word_anchors.push(WordAnchor {
                word: word.clone(),
                start_secs: start,
                end_secs: start + step,
            });
        }
        let end = cursor + duration + TAIL_GUARD_SECS;
        aligned.push(AlignedChunk {
            beat_id: chunk.beat_id.clone(),
            text: chunk.text.clone(),
            start_secs: cursor,
            end_secs: end,
            words: word_anchors,
        });
        cursor = end;
    }
    TimingFile { chunks: aligned }
}

/// Synthesize each TTS chunk, write `audio/chunk_NNN.*`, concat to `narration.wav`
/// (or copy a single mp3 as `narration.mp3`). Returns measured durations.
pub fn persist_tts_chunks(
    working_dir: &Path,
    chunks: &[TtsChunk],
    voice: &dyn VoiceSynth,
    choice: Option<&TtsChoice>,
) -> BriefingResult<Vec<f64>> {
    if chunks.is_empty() {
        return Err(BriefingError::Voice {
            code: "tts_failed".into(),
            message: "no spoken chunks to synthesize".into(),
        });
    }
    let audio_dir = working_dir.join("audio");
    std::fs::create_dir_all(&audio_dir)?;
    let mut parts = Vec::new();
    let mut durations = Vec::new();
    for chunk in chunks {
        let clip = voice.synthesize(&chunk.text, choice).map_err(|message| {
            let code = if message.contains("tts_unavailable") {
                "tts_unavailable"
            } else {
                "tts_failed"
            };
            BriefingError::Voice {
                code: code.into(),
                message,
            }
        })?;
        let (bytes, ext) = normalize_clip(&clip);
        let path = audio_dir.join(format!("chunk_{:03}.{ext}", chunk.index));
        std::fs::write(&path, &bytes)?;
        let duration = probe_duration(&path).unwrap_or_else(|| {
            wav_duration_secs(&bytes).unwrap_or((chunk.text.chars().count() as f64 / 8.0).max(0.4))
        });
        durations.push(duration);
        parts.push(path);
    }
    concat_narration(working_dir, &parts)?;
    Ok(durations)
}

fn normalize_clip(clip: &SynthesizedClip) -> (Vec<u8>, &'static str) {
    let mime = clip.mime.to_ascii_lowercase();
    if mime.contains("pcm") && !mime.contains("wav") {
        return (pcm_to_wav(&clip.bytes, PCM_WRAP_RATE), "wav");
    }
    if mime.contains("wav") || looks_like_wav(&clip.bytes) {
        return (clip.bytes.clone(), "wav");
    }
    if mime.contains("mpeg") || mime.contains("mp3") {
        return (clip.bytes.clone(), "mp3");
    }
    if mime.contains("mp4") || mime.contains("m4a") || mime.contains("aac") {
        return (clip.bytes.clone(), "m4a");
    }
    if looks_like_wav(&clip.bytes) {
        return (clip.bytes.clone(), "wav");
    }
    (clip.bytes.clone(), "bin")
}

fn looks_like_wav(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE"
}

fn pcm_to_wav(pcm: &[u8], sample_rate: u32) -> Vec<u8> {
    let data_len = pcm.len() as u32;
    let byte_rate = sample_rate * 2;
    let mut out = Vec::with_capacity(44 + pcm.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(pcm);
    out
}

fn wav_duration_secs(bytes: &[u8]) -> Option<f64> {
    if !looks_like_wav(bytes) || bytes.len() < 44 {
        return None;
    }
    let channels = u16::from_le_bytes(bytes[22..24].try_into().ok()?) as f64;
    let sample_rate = u32::from_le_bytes(bytes[24..28].try_into().ok()?) as f64;
    let bits = u16::from_le_bytes(bytes[34..36].try_into().ok()?) as f64;
    if channels <= 0.0 || sample_rate <= 0.0 || bits <= 0.0 {
        return None;
    }
    let mut offset = 12usize;
    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().ok()?) as usize;
        if id == b"data" {
            let bytes_per_sec = sample_rate * channels * (bits / 8.0);
            if bytes_per_sec <= 0.0 {
                return None;
            }
            return Some(size as f64 / bytes_per_sec);
        }
        offset = offset.saturating_add(8).saturating_add(size);
        if size % 2 == 1 {
            offset = offset.saturating_add(1);
        }
    }
    None
}

fn probe_duration(path: &Path) -> Option<f64> {
    let output = silent_command("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.trim().parse::<f64>().ok().filter(|d| *d > 0.0)
}

fn concat_narration(working_dir: &Path, parts: &[PathBuf]) -> BriefingResult<()> {
    let Some(first) = parts.first() else {
        return Err(BriefingError::Voice {
            code: "tts_failed".into(),
            message: "no audio parts to concat".into(),
        });
    };
    if parts.len() == 1 {
        let dest = narration_dest(working_dir, first);
        std::fs::copy(first, dest)?;
        return Ok(());
    }
    let list_path = working_dir.join("audio").join("concat.txt");
    let mut list = String::new();
    for part in parts {
        let abs = part
            .canonicalize()
            .unwrap_or_else(|_| part.to_path_buf())
            .to_string_lossy()
            .replace('\\', "/")
            .replace('\'', r"'\''");
        list.push_str(&format!("file '{abs}'\n"));
    }
    std::fs::write(&list_path, list)?;
    let dest = working_dir.join("narration.wav");
    let status = silent_command("ffmpeg")
        .args(["-y", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i"])
        .arg(&list_path)
        .args(["-c:a", "pcm_s16le"])
        .arg(&dest)
        .status()
        .map_err(|e| BriefingError::Voice {
            code: "tts_failed".into(),
            message: format!("ffmpeg concat: {e}"),
        })?;
    if status.success() && dest.is_file() {
        return Ok(());
    }
    Err(BriefingError::Voice {
        code: "tts_failed".into(),
        message: "ffmpeg could not concat TTS chunks into narration.wav".into(),
    })
}

fn narration_dest(working_dir: &Path, source: &Path) -> PathBuf {
    let ext = source
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("wav");
    if ext.eq_ignore_ascii_case("wav") {
        working_dir.join("narration.wav")
    } else if ext.eq_ignore_ascii_case("mp3") {
        working_dir.join("narration.mp3")
    } else {
        working_dir.join(format!("narration.{ext}"))
    }
}

fn silent_command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

#[cfg(test)]
pub fn silence_wav_for_tests(duration_secs: f64) -> Vec<u8> {
    let sample_rate = 8_000u32;
    let n = ((duration_secs.max(0.1) * sample_rate as f64) as usize).max(1);
    pcm_to_wav(&vec![0u8; n * 2], sample_rate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Beat, VisualKind};

    #[test]
    fn chunks_stay_under_tts_limit() {
        let long = "今日。".repeat(2000);
        let beat = Beat {
            id: "b1".into(),
            spoken_text: long,
            on_screen: String::new(),
            visual: VisualKind::UserAsset,
            card: "subtitle_plain".into(),
            claims: vec![],
            citations: vec![],
            anchors: vec![],
        };
        let chunks = chunk_beats(&[beat]);
        assert!(chunks.iter().all(|c| c.text.chars().count() <= TTS_CHAR_LIMIT));
        assert!(chunks.len() > 1);
    }

    #[test]
    fn proportional_align_does_not_use_hand_seconds() {
        let chunks = vec![TtsChunk {
            index: 0,
            beat_id: "b1".into(),
            text: "今日 要闻".into(),
        }];
        let timing = align_proportional(&chunks, 180.0);
        assert!(timing.chunks[0].end_secs > timing.chunks[0].start_secs);
        assert!(!timing.chunks[0].words.is_empty());
        assert!(timing.chunks[0].end_secs - timing.chunks[0].words.last().unwrap().end_secs
            >= TAIL_GUARD_SECS - 1e-9);
    }

    #[test]
    fn asr_alignment_uses_model_invoke_timestamps() {
        let chunks = vec![TtsChunk {
            index: 0,
            beat_id: "b1".into(),
            text: "今日 要闻".into(),
        }];
        let words = vec![
            AsrWord {
                word: "今日".into(),
                start_secs: 0.12,
                end_secs: 0.40,
            },
            AsrWord {
                word: "要闻".into(),
                start_secs: 0.40,
                end_secs: 0.88,
            },
        ];
        let timing = align_from_asr(&chunks, &words).expect("asr words cover the chunk");
        assert_eq!(timing.chunks[0].words[0].start_secs, 0.12);
        assert_eq!(timing.chunks[0].words[1].end_secs, 0.88);
        assert!((timing.chunks[0].end_secs - 1.38).abs() < 1e-9);
    }

    #[test]
    fn duration_align_follows_measured_audio_not_script_seconds() {
        let chunks = vec![
            TtsChunk {
                index: 0,
                beat_id: "open".into(),
                text: "今日 要闻".into(),
            },
            TtsChunk {
                index: 1,
                beat_id: "evidence".into(),
                text: "交叉 核验".into(),
            },
        ];
        let timing = align_from_durations(&chunks, &[1.2, 0.8]);
        assert!((timing.chunks[0].end_secs - (1.2 + TAIL_GUARD_SECS)).abs() < 1e-9);
        assert!((timing.chunks[1].end_secs - (1.2 + TAIL_GUARD_SECS + 0.8 + TAIL_GUARD_SECS)).abs() < 1e-9);
        let wav = silence_wav_for_tests(0.5);
        assert!(wav_duration_secs(&wav).unwrap() > 0.4);
    }
}
