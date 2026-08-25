//! `MeetingRecorder` — VAD-segmented real-time meeting recorder.
//!
//! Reads from a `DualTrackMixer`, segments audio via VAD, and calls an async
//! STT callback for each speech segment.  The caller receives incremental
//! transcript updates via an `mpsc` channel, enabling live caption display.
//!
//! # Architecture
//!
//! ```text
//! DualTrackMixer ──(TaggedFrame)──► MeetingRecorder
//!                                       │
//!                        per-channel VAD (EnergyVad / SileroVad)
//!                                       │
//!                             speech segment detected
//!                                       │
//!                    async SttCallback (background task)
//!                                       │
//!                         tx.send(TranscriptSegment)
//! ```
//!
//! Call `MeetingRecorder::record()` to start.  It runs until the mixer
//! channel closes (both sources exhausted) or `stop()` is called.

use std::collections::HashMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::frame::{AudioChannel, TaggedFrame};
use crate::keepawake::KeepAwakeGuard;
use crate::vad::{VadBackend, VadConfig, create_vad};

// ---------------------------------------------------------------------------
// Pipeline statistics
// ---------------------------------------------------------------------------

/// Per-node latency and throughput counters.
///
/// All durations are wall-clock.  Access via `MeetingRecorder::stats()`.
#[derive(Debug, Clone, Default)]
pub struct NodeStats {
    /// Number of frames processed.
    pub frames: u64,
    /// Total time spent in this node (sum over all frames).
    pub total_ns: u64,
    /// Maximum single-frame latency observed.
    pub max_ns: u64,
}

impl NodeStats {
    fn record(&mut self, elapsed: Duration) {
        let ns = elapsed.as_nanos() as u64;
        self.frames += 1;
        self.total_ns += ns;
        if ns > self.max_ns {
            self.max_ns = ns;
        }
    }

    /// Mean latency per frame in microseconds.
    pub fn mean_us(&self) -> f64 {
        if self.frames == 0 {
            return 0.0;
        }
        self.total_ns as f64 / self.frames as f64 / 1_000.0
    }

    /// Max single-frame latency in milliseconds.
    pub fn max_ms(&self) -> f64 {
        self.max_ns as f64 / 1_000_000.0
    }
}

/// Snapshot of all pipeline node statistics.
#[derive(Debug, Clone, Default)]
pub struct PipelineStats {
    /// VAD frame processing (per-frame cost).
    pub vad: NodeStats,
    /// STT call latency (per segment, i.e. per flush).
    pub stt: NodeStats,
    /// Total segments emitted (speech segments flushed to STT).
    pub segments_flushed: u64,
    /// Total wall-clock recording time in seconds.
    pub wall_secs: f32,
    /// Total speech time captured (sum of all flushed segment durations).
    pub speech_secs: f32,
}

impl PipelineStats {
    /// Speech ratio: fraction of wall time that contained speech.
    pub fn speech_ratio(&self) -> f32 {
        if self.wall_secs == 0.0 {
            return 0.0;
        }
        (self.speech_secs / self.wall_secs).min(1.0)
    }
}

/// Thread-safe handle to live pipeline statistics.
#[derive(Clone, Default)]
pub struct StatsHandle(Arc<Mutex<PipelineStats>>);

impl StatsHandle {
    pub fn snapshot(&self) -> PipelineStats {
        self.0.lock().unwrap().clone()
    }

    fn with<F: FnOnce(&mut PipelineStats)>(&self, f: F) {
        if let Ok(mut g) = self.0.lock() {
            f(&mut g);
        }
    }
}

// ---------------------------------------------------------------------------
// Output type
// ---------------------------------------------------------------------------

/// One recognized speech segment from the meeting.
///
/// `audio_file` + `start_s`/`end_s` enable timeline-aware playback:
/// the UI can jump to the exact position in the recorded audio file when
/// the user clicks a transcript line.
#[derive(Debug, Clone)]
pub struct TranscriptSegment {
    /// Stable id reused for partial → final updates of one utterance.
    pub segment_id: String,
    /// "Speaker A" (mic) or "Speaker B" (loopback).
    pub speaker: String,
    pub text: String,
    /// Seconds since recording start for the first sample of this segment.
    pub start_s: f32,
    /// Seconds since recording start for the last sample of this segment.
    pub end_s: f32,
    /// Path to the audio file segment (WAV) for this transcript line.
    ///
    /// Set to `Some` when `MeetingRecorder` is configured to save per-segment
    /// audio clips (e.g. `segment_audio_dir` is provided).  `None` when only
    /// streaming transcript is needed and no audio files are kept.
    pub audio_file: Option<String>,
    /// `true` while speech is still open or STT text is still streaming in.
    pub is_partial: bool,
}

// ---------------------------------------------------------------------------
// STT callback trait
// ---------------------------------------------------------------------------

/// Async callback that converts a PCM buffer into transcript text.
///
/// Implementors typically wrap `SttEngine::transcribe_file` (via a temp WAV)
/// or a WebSocket streaming client.
#[async_trait::async_trait]
pub trait SttCallback: Send + Sync + 'static {
    async fn transcribe(
        &self,
        channel: AudioChannel,
        pcm: Vec<f32>,
        sample_rate: u32,
    ) -> Option<String>;
}

// ---------------------------------------------------------------------------
// Per-channel state
// ---------------------------------------------------------------------------

struct ChannelState {
    vad: Box<dyn VadBackend>,
    buffer: Vec<f32>,
    recording: bool,
    silence_start: Option<std::time::Instant>,
    /// Active utterance id while speech is buffered (partial captions).
    active_segment_id: Option<String>,
    utterance_start_s: f32,
    last_partial_emit: Option<Instant>,
}

impl ChannelState {
    fn new(vad_cfg: VadConfig) -> Self {
        Self {
            vad: create_vad(vad_cfg),
            buffer: Vec::new(),
            recording: false,
            silence_start: None,
            active_segment_id: None,
            utterance_start_s: 0.0,
            last_partial_emit: None,
        }
    }

    fn clear_utterance(&mut self) {
        self.active_segment_id = None;
        self.utterance_start_s = 0.0;
        self.last_partial_emit = None;
    }
}

/// How often to emit `is_partial` placeholders while speech buffers grow.
const PARTIAL_EMIT_INTERVAL: Duration = Duration::from_millis(280);

/// Placeholder preview while waiting for STT (grows with buffer length).
fn speech_buffer_preview(buf_len: usize, sample_rate: u32) -> String {
    let secs = if sample_rate == 0 {
        0.0
    } else {
        buf_len as f32 / sample_rate as f32
    };
    let dots = ((secs * 4.0).ceil() as usize).clamp(1, 16);
    "·".repeat(dots)
}

// ---------------------------------------------------------------------------
// MeetingRecorder
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Silence guard (warn when no real audio arrives)
// ---------------------------------------------------------------------------

/// Detects sustained microphone silence and fires a callback.
///
/// Silence is defined as every frame having RMS below `threshold_rms`.
/// After `timeout_secs` of continuous silence the `on_silent` callback is
/// invoked **once**.  It resets when a voiced frame is observed.
pub struct SilenceGuard {
    pub threshold_rms: f32,
    pub timeout_secs: f32,
    last_voiced: Option<Instant>,
    fired: bool,
}

impl SilenceGuard {
    pub fn new(threshold_rms: f32, timeout_secs: f32) -> Self {
        Self {
            threshold_rms,
            timeout_secs,
            last_voiced: None,
            fired: false,
        }
    }

    /// Feed one frame.  Returns `true` the first time the silence threshold
    /// is crossed (i.e. the caller should warn the user).
    pub fn feed(&mut self, samples: &[f32]) -> bool {
        let rms = if samples.is_empty() {
            0.0f32
        } else {
            let sq: f32 = samples.iter().map(|s| s * s).sum();
            (sq / samples.len() as f32).sqrt()
        };

        if rms >= self.threshold_rms {
            self.last_voiced = Some(Instant::now());
            self.fired = false;
            return false;
        }

        // Silent frame — check elapsed time
        let elapsed = self
            .last_voiced
            .map(|t| t.elapsed().as_secs_f32())
            .unwrap_or_else(|| {
                // Never had a voiced frame: start counting from recording start
                self.last_voiced
                    .get_or_insert(Instant::now())
                    .elapsed()
                    .as_secs_f32()
            });

        if !self.fired && elapsed >= self.timeout_secs {
            self.fired = true;
            return true;
        }
        false
    }
}

// ---------------------------------------------------------------------------
// MeetingRecorder
// ---------------------------------------------------------------------------

/// Drives a `DualTrackMixer` stream through per-channel VAD and emits
/// `TranscriptSegment` values whenever speech ends.
pub struct MeetingRecorder {
    vad_config: VadConfig,
    stt: Arc<dyn SttCallback>,
    /// Maximum recording length per segment (prevents runaway buffers).
    max_segment_secs: f32,
    stop_flag: Arc<AtomicBool>,
    /// Pause flag: when true the recorder drops incoming frames without processing.
    pause_flag: Arc<AtomicBool>,
    stats: StatsHandle,
    /// RMS floor below which a frame is considered silence (for SilenceGuard).
    silence_threshold_rms: f32,
    /// Seconds of continuous silence before warning the user.
    silence_timeout_secs: f32,
}

impl MeetingRecorder {
    pub fn new(vad_config: VadConfig, stt: Arc<dyn SttCallback>) -> Self {
        Self {
            vad_config,
            stt,
            max_segment_secs: 60.0,
            stop_flag: Arc::new(AtomicBool::new(false)),
            pause_flag: Arc::new(AtomicBool::new(false)),
            stats: StatsHandle::default(),
            silence_threshold_rms: 0.002,
            silence_timeout_secs: 10.0,
        }
    }

    /// Request graceful shutdown.
    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }

    /// Pause recording: incoming frames are discarded until `resume()` is called.
    ///
    /// The current speech buffer is flushed to STT before pausing so no audio
    /// is lost.  Use this for meeting breaks; prefer this over filling silence.
    pub fn pause(&self) {
        self.pause_flag.store(true, Ordering::Relaxed);
        info!("MeetingRecorder: paused");
    }

    /// Resume recording after `pause()`.
    pub fn resume(&self) {
        self.pause_flag.store(false, Ordering::Relaxed);
        info!("MeetingRecorder: resumed");
    }

    /// Whether recording is currently paused.
    pub fn is_paused(&self) -> bool {
        self.pause_flag.load(Ordering::Relaxed)
    }

    /// Snapshot of current pipeline performance statistics.
    ///
    /// Safe to call at any time during or after recording.
    pub fn stats(&self) -> PipelineStats {
        self.stats.snapshot()
    }

    /// Start recording.  Returns a receiver that yields `TranscriptSegment`
    /// values and a `JoinHandle` for the background task.
    ///
    /// `frames_rx`: output of `DualTrackMixer::into_stream()`.
    pub fn record(
        &self,
        mut frames_rx: mpsc::Receiver<TaggedFrame>,
    ) -> (
        mpsc::Receiver<TranscriptSegment>,
        tokio::task::JoinHandle<()>,
    ) {
        let (seg_tx, seg_rx) = mpsc::channel::<TranscriptSegment>(64);
        let vad_cfg = self.vad_config.clone();
        let stt = Arc::clone(&self.stt);
        let max_secs = self.max_segment_secs;
        let stop = Arc::clone(&self.stop_flag);
        let pause = Arc::clone(&self.pause_flag);
        let stats = self.stats.clone();
        let silence_rms = self.silence_threshold_rms;
        let silence_timeout = self.silence_timeout_secs;

        let handle = tokio::spawn(async move {
            // Prevent OS sleep for the duration of this recording session.
            let _keep_awake = KeepAwakeGuard::acquire("nomifun meeting recorder");

            let mut channels: HashMap<AudioChannel, ChannelState> = HashMap::new();
            let start = Instant::now();
            let mut silence_guard = SilenceGuard::new(silence_rms, silence_timeout);

            while let Some(frame) = frames_rx.recv().await {
                if stop.load(Ordering::Relaxed) {
                    debug!("MeetingRecorder: stop requested");
                    break;
                }
                if frame.samples.is_empty() {
                    continue;
                }

                // Pause: flush current buffers and drop new frames.
                if pause.load(Ordering::Relaxed) {
                    // flush any in-progress buffers so we don't lose audio
                    for (ch, state) in channels.iter_mut() {
                        if !state.buffer.is_empty() {
                            let pcm = std::mem::take(&mut state.buffer);
                            let segment_id = state
                                .active_segment_id
                                .take()
                                .unwrap_or_else(|| Uuid::now_v7().to_string());
                            let start_s = state.utterance_start_s;
                            state.recording = false;
                            state.clear_utterance();
                            let sr = frame.sample_rate;
                            let end_s = start.elapsed().as_secs_f32();
                            Self::spawn_stt(
                                *ch,
                                pcm,
                                sr,
                                start_s,
                                end_s,
                                segment_id,
                                Arc::clone(&stt),
                                seg_tx.clone(),
                                stats.clone(),
                            );
                        }
                    }
                    // Sleep briefly and keep draining the channel without processing
                    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                    continue;
                }

                // Silence guard: warn user if no real audio detected
                if silence_guard.feed(&frame.samples) {
                    warn!(
                        "MeetingRecorder: no audio input detected for {:.0}s. \
                         Check that your microphone is enabled and the correct \
                         input device is selected.",
                        silence_timeout
                    );
                }

                let elapsed_s = start.elapsed().as_secs_f32();
                let ch = frame.channel;
                let sample_rate = frame.sample_rate;

                let state = channels
                    .entry(ch)
                    .or_insert_with(|| ChannelState::new(vad_cfg.clone()));

                // ── Node: VAD ─────────────────────────────────────────────
                let vad_t0 = Instant::now();
                let is_speech = state.vad.process_frame(&frame.samples);
                stats.with(|s| s.vad.record(vad_t0.elapsed()));

                if is_speech {
                    let was_recording = state.recording;
                    if !was_recording {
                        state.active_segment_id = Some(Uuid::now_v7().to_string());
                        state.utterance_start_s = elapsed_s;
                        state.last_partial_emit = None;
                    }
                    state.recording = true;
                    state.silence_start = None;
                    state.buffer.extend_from_slice(&frame.samples);

                    let should_emit_partial = state
                        .last_partial_emit
                        .map(|t| t.elapsed() >= PARTIAL_EMIT_INTERVAL)
                        .unwrap_or(true);
                    if should_emit_partial {
                        if let Some(segment_id) = state.active_segment_id.clone() {
                            let preview =
                                speech_buffer_preview(state.buffer.len(), sample_rate);
                            let start_s = state.utterance_start_s;
                            state.last_partial_emit = Some(Instant::now());
                            let _ = seg_tx
                                .try_send(TranscriptSegment {
                                    segment_id,
                                    speaker: ch.speaker_label().to_string(),
                                    text: preview,
                                    start_s,
                                    end_s: elapsed_s,
                                    audio_file: None,
                                    is_partial: true,
                                });
                        }
                    }

                    // Safety cap: flush if segment grows too long
                    let seg_secs = state.buffer.len() as f32 / sample_rate as f32;
                    if seg_secs >= max_secs {
                        debug!("MeetingRecorder: max_segment_secs reached on {ch:?}, flushing");
                        let pcm = std::mem::take(&mut state.buffer);
                        let segment_id = state
                            .active_segment_id
                            .take()
                            .unwrap_or_else(|| Uuid::now_v7().to_string());
                        let start_s = state.utterance_start_s;
                        state.recording = false;
                        state.clear_utterance();
                        state.vad.reset();
                        Self::spawn_stt(
                            ch,
                            pcm,
                            sample_rate,
                            start_s,
                            elapsed_s,
                            segment_id,
                            Arc::clone(&stt),
                            seg_tx.clone(),
                            stats.clone(),
                        );
                    }
                } else if state.recording {
                    let now = Instant::now();
                    if state.silence_start.is_none() {
                        state.silence_start = Some(now);
                    }
                    let silence_ms = state
                        .silence_start
                        .map(|t| t.elapsed().as_millis() as u64)
                        .unwrap_or(0);

                    if silence_ms >= vad_cfg.silence_timeout_ms {
                        let pcm = std::mem::take(&mut state.buffer);
                        let segment_id = state
                            .active_segment_id
                            .take()
                            .unwrap_or_else(|| Uuid::now_v7().to_string());
                        let start_s = state.utterance_start_s;
                        state.recording = false;
                        state.silence_start = None;
                        state.clear_utterance();

                        if pcm.len() > sample_rate as usize / 4 {
                            let speech_secs = pcm.len() as f32 / sample_rate as f32;
                            stats.with(|s| {
                                s.segments_flushed += 1;
                                s.speech_secs += speech_secs;
                            });
                            Self::spawn_stt(
                                ch,
                                pcm,
                                sample_rate,
                                start_s,
                                elapsed_s,
                                segment_id,
                                Arc::clone(&stt),
                                seg_tx.clone(),
                                stats.clone(),
                            );
                        }
                    }
                }
            }

            // Flush remaining buffers on clean exit
            for (ch, mut state) in channels {
                if !state.buffer.is_empty() {
                    let pcm = std::mem::take(&mut state.buffer);
                    let sr = 16_000u32;
                    let speech_secs = pcm.len() as f32 / sr as f32;
                    stats.with(|s| {
                        s.segments_flushed += 1;
                        s.speech_secs += speech_secs;
                    });
                    let segment_id = state
                        .active_segment_id
                        .take()
                        .unwrap_or_else(|| Uuid::now_v7().to_string());
                    let start_s = state.utterance_start_s;
                    let end_s = start.elapsed().as_secs_f32();
                    Self::spawn_stt(
                        ch,
                        pcm,
                        sr,
                        start_s,
                        end_s,
                        segment_id,
                        Arc::clone(&stt),
                        seg_tx.clone(),
                        stats.clone(),
                    );
                }
            }

            stats.with(|s| s.wall_secs = start.elapsed().as_secs_f32());
            let snap = stats.snapshot();
            info!(
                "MeetingRecorder: stream ended — wall={:.1}s speech={:.1}s ({:.0}%) \
                 vad_mean={:.1}µs vad_max={:.1}ms stt_mean={:.1}ms stt_max={:.1}ms segments={}",
                snap.wall_secs,
                snap.speech_secs,
                snap.speech_ratio() * 100.0,
                snap.vad.mean_us(),
                snap.vad.max_ms(),
                snap.stt.mean_us() / 1_000.0,
                snap.stt.max_ms(),
                snap.segments_flushed,
            );
        });

        (seg_rx, handle)
    }

    /// Spawn a background task that calls STT, streams character partials, then
    /// emits the final segment (same `segment_id`).
    fn spawn_stt(
        ch: AudioChannel,
        pcm: Vec<f32>,
        sample_rate: u32,
        start_s: f32,
        end_s: f32,
        segment_id: String,
        stt: Arc<dyn SttCallback>,
        tx: mpsc::Sender<TranscriptSegment>,
        stats: StatsHandle,
    ) {
        tokio::spawn(async move {
            let t0 = Instant::now();
            let speaker = ch.speaker_label().to_string();
            let text = stt.transcribe(ch, pcm, sample_rate).await;
            stats.with(|s| s.stt.record(t0.elapsed()));

            let final_text = text.unwrap_or_default();
            if !final_text.is_empty() {
                // Character-by-character streaming feel for batch STT results.
                let mut acc = String::new();
                for ch_unit in final_text.chars() {
                    acc.push(ch_unit);
                    if tx
                        .send(TranscriptSegment {
                            segment_id: segment_id.clone(),
                            speaker: speaker.clone(),
                            text: acc.clone(),
                            start_s,
                            end_s,
                            audio_file: None,
                            is_partial: true,
                        })
                        .await
                        .is_err()
                    {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }

            let _ = tx
                .send(TranscriptSegment {
                    segment_id,
                    speaker,
                    text: final_text,
                    start_s,
                    end_s,
                    audio_file: None,
                    is_partial: false,
                })
                .await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NullStt;
    #[async_trait::async_trait]
    impl SttCallback for NullStt {
        async fn transcribe(&self, _ch: AudioChannel, _pcm: Vec<f32>, _sr: u32) -> Option<String> {
            Some("test".to_string())
        }
    }

    #[tokio::test]
    async fn recorder_emits_segment_from_loud_frames() {
        use crate::capture::PcmReplaySource;
        use crate::mixer::DualTrackMixer;
        use std::sync::Arc;

        // 2s of loud audio at 16kHz → should trigger speech → segment
        let loud = vec![0.8f32; 16_000 * 2];
        let silent = vec![0.0f32; 16_000];
        let mic = Arc::new(PcmReplaySource::new("mic", 16_000, loud, 512));
        let lb = Arc::new(PcmReplaySource::new("loopback", 16_000, silent, 512));

        let vad_cfg = VadConfig {
            threshold: 0.01,
            min_speech_frames: 2,
            silence_timeout_ms: 100,
            frame_size: 512,
            max_zcr: 1.0,
        };

        let mixer = DualTrackMixer::new(mic, lb);
        let frames_rx = mixer.into_stream(64);
        let recorder = MeetingRecorder::new(vad_cfg, Arc::new(NullStt));
        let (mut seg_rx, _handle) = recorder.record(frames_rx);

        // Wait up to 2s for at least one segment
        let timeout = tokio::time::timeout(Duration::from_secs(4), seg_rx.recv());
        let seg = timeout.await;
        assert!(
            seg.is_ok() && seg.unwrap().is_some(),
            "expected at least one transcript segment"
        );
    }
}
