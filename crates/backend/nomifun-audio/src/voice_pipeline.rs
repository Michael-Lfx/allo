//! Three-stage lock-free voice capture pipeline skeleton.
//!
//! Authoritative baseline: docs/architecture/voice-agent/interaction.zh.md §3.2
//!
//! Architecture:
//! - Level 1 (Producer): Zero-allocation, lock-free SPSC circular ring buffer (`SpscRingBuffer`).
//!   Safe for real-time audio callbacks (CPAL `data_callback`) and test replay injection.
//! - Level 2 (Worker): Background worker thread pulling from SPSC consumer, performing
//!   channel downmix (`downmix_to_mono`), 16kHz resampling (`resample_mono`), and backlog
//!   overflow protection (>100ms drops oldest and increments `overflow_count`).
//! - Level 3 (Consumer): `VoiceCapturePipeline` exposing async and non-blocking chunk pulling
//!   yielding `CaptureReadResult::Chunk` slices.
//!
//! Includes test constructor `VoiceCapturePipeline::from_replay` and `StubKwsDetector`
//! for VT-0-01 verification.

use crate::capture::CaptureReadResult;
use crate::pcm_util::{downmix_to_mono, resample_mono};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

// ============================================================================
// Level 1: Lock-Free SPSC Ring Buffer
// ============================================================================

struct SpscShared {
    capacity: usize,
    storage: Box<[AtomicU32]>,
    head: AtomicUsize,
    tail: AtomicUsize,
    closed: AtomicBool,
    overflow_count: AtomicU64,
}

impl SpscShared {
    fn new(capacity: usize) -> Self {
        let cap = capacity.max(1);
        let mut v = Vec::with_capacity(cap);
        for _ in 0..cap {
            v.push(AtomicU32::new(0));
        }
        Self {
            capacity: cap,
            storage: v.into_boxed_slice(),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            closed: AtomicBool::new(false),
            overflow_count: AtomicU64::new(0),
        }
    }

    /// Pushes a slice of f32 samples lock-free with zero memory allocation.
    /// Returns the count of samples actually written into the buffer.
    fn push_slice(&self, samples: &[f32]) -> usize {
        if samples.is_empty() || self.closed.load(Ordering::Acquire) {
            return 0;
        }

        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        let occupied = tail.wrapping_sub(head);
        let avail = self.capacity.saturating_sub(occupied);
        let write_len = samples.len().min(avail);

        for i in 0..write_len {
            let idx = (tail.wrapping_add(i)) % self.capacity;
            self.storage[idx].store(samples[i].to_bits(), Ordering::Relaxed);
        }

        self.tail.store(tail.wrapping_add(write_len), Ordering::Release);
        write_len
    }

    /// Pops up to `dest.len()` samples lock-free.
    /// Returns the number of samples read.
    fn pop_slice(&self, dest: &mut [f32]) -> usize {
        if dest.is_empty() {
            return 0;
        }

        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        let available = tail.wrapping_sub(head);
        let read_len = dest.len().min(available);

        for i in 0..read_len {
            let idx = (head.wrapping_add(i)) % self.capacity;
            dest[i] = f32::from_bits(self.storage[idx].load(Ordering::Relaxed));
        }

        self.head.store(head.wrapping_add(read_len), Ordering::Release);
        read_len
    }

    /// Drops oldest samples if backlog exceeds `max_allowed_backlog`, incrementing `overflow_count`.
    fn drop_oldest_if_overflow(&self, max_allowed_backlog: usize) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        let available = tail.wrapping_sub(head);

        if available > max_allowed_backlog && max_allowed_backlog > 0 {
            let to_drop = available - max_allowed_backlog;
            self.head.store(head.wrapping_add(to_drop), Ordering::Release);
            self.overflow_count.fetch_add(1, Ordering::Relaxed);
            to_drop
        } else {
            0
        }
    }

    fn available(&self) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        tail.wrapping_sub(head)
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    fn close(&self) {
        self.closed.store(true, Ordering::Release);
    }

    fn overflow_count(&self) -> u64 {
        self.overflow_count.load(Ordering::Relaxed)
    }
}

/// Standalone handle for SPSC ring buffer.
#[derive(Clone)]
pub struct SpscRingBuffer {
    shared: Arc<SpscShared>,
}

impl SpscRingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            shared: Arc::new(SpscShared::new(capacity)),
        }
    }

    pub fn push(&self, samples: &[f32]) -> usize {
        self.shared.push_slice(samples)
    }

    pub fn close(&self) {
        self.shared.close();
    }

    pub fn overflow_count(&self) -> u64 {
        self.shared.overflow_count()
    }

    pub fn available(&self) -> usize {
        self.shared.available()
    }
}

/// Producer endpoint for pushing real-time audio into the SPSC ring buffer.
#[derive(Clone)]
pub struct SpscProducer {
    shared: Arc<SpscShared>,
}

impl SpscProducer {
    /// Pushes samples into the ring buffer without allocating or blocking.
    pub fn push(&self, samples: &[f32]) -> usize {
        self.shared.push_slice(samples)
    }

    /// Available unconsumed samples in the ring buffer.
    pub fn available(&self) -> usize {
        self.shared.available()
    }

    /// Closes the capture stream.
    pub fn close(&self) {
        self.shared.close();
    }

    /// Checks if the stream has been closed.
    pub fn is_closed(&self) -> bool {
        self.shared.is_closed()
    }
}

// ============================================================================
// Level 2 & 3: Voice Pipeline Configuration & Pipeline
// ============================================================================

/// Configuration for the voice capture pipeline.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Native sample rate from audio hardware or replay source (e.g. 16000, 44100, 48000)
    pub sample_rate: u32,
    /// Number of audio channels (e.g. 1 for mono, 2 for stereo)
    pub channels: u16,
    /// Size of 16kHz mono output chunks (e.g. 160 for 10ms, 320 for 20ms)
    pub chunk_size: usize,
    /// Ring buffer capacity in native samples (default: 48,000 samples)
    pub capacity_samples: usize,
    /// Backlog limit in milliseconds before triggering overflow discard (default: 100ms)
    pub backlog_limit_ms: u32,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            sample_rate: 16000,
            channels: 1,
            chunk_size: 160,
            capacity_samples: 48000,
            backlog_limit_ms: 100,
        }
    }
}

/// Three-stage voice capture pipeline.
pub struct VoiceCapturePipeline {
    receiver: tokio::sync::mpsc::Receiver<CaptureReadResult>,
    shared: Arc<SpscShared>,
    _worker_handle: Option<tokio::task::JoinHandle<()>>,
}

impl VoiceCapturePipeline {
    /// Creates a new capture pipeline and returns the async consumer pipeline and RT producer handle.
    pub fn new(config: PipelineConfig) -> (Self, SpscProducer) {
        let shared = Arc::new(SpscShared::new(config.capacity_samples));
        let producer = SpscProducer {
            shared: Arc::clone(&shared),
        };

        let (tx, rx) = tokio::sync::mpsc::channel::<CaptureReadResult>(128);

        let worker_handle = if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let worker_shared = Arc::clone(&shared);
            let tx = tx.clone();
            let config = config.clone();
            Some(handle.spawn(async move {
                let max_backlog_samples = ((config.sample_rate as u64
                    * config.channels as u64
                    * config.backlog_limit_ms as u64)
                    / 1000) as usize;

                let mut raw_buf = vec![0.0f32; 4096];
                let mut accumulator = Vec::<f32>::with_capacity(4096);

                loop {
                    // 1. Check overflow and drop oldest samples if backlog > backlog_limit_ms
                    worker_shared.drop_oldest_if_overflow(max_backlog_samples);

                    // 2. Read available samples from SPSC
                    let read = worker_shared.pop_slice(&mut raw_buf);

                    if read > 0 {
                        let slice = &raw_buf[..read];

                        // Channel fold (mono average)
                        let mono = if config.channels > 1 {
                            downmix_to_mono(slice, config.channels as usize)
                        } else {
                            slice.to_vec()
                        };

                        // Resample to 16kHz
                        let resampled = if config.sample_rate != 16000 {
                            resample_mono(&mono, config.sample_rate, 16000)
                        } else {
                            mono
                        };

                        accumulator.extend_from_slice(&resampled);

                        // Drain full chunks of chunk_size
                        while accumulator.len() >= config.chunk_size {
                            let chunk: Vec<f32> = accumulator.drain(..config.chunk_size).collect();
                            if tx.send(CaptureReadResult::Chunk(chunk)).await.is_err() {
                                return;
                            }
                        }
                    }

                    if read == 0 {
                        if worker_shared.is_closed() {
                            // SPSC is closed and exhausted
                            if !accumulator.is_empty() {
                                let remaining = std::mem::take(&mut accumulator);
                                let _ = tx.send(CaptureReadResult::Chunk(remaining)).await;
                            }
                            let _ = tx.send(CaptureReadResult::StreamClosed).await;
                            break;
                        } else {
                            // Temporarily empty, yield to allow producer to push
                            tokio::task::yield_now().await;
                        }
                    }
                }
            }))
        } else {
            let worker_shared = Arc::clone(&shared);
            let tx = tx.clone();
            let config = config.clone();
            std::thread::Builder::new()
                .name("nomifun-voice-worker-thread".into())
                .spawn(move || {
                    let max_backlog_samples = ((config.sample_rate as u64
                        * config.channels as u64
                        * config.backlog_limit_ms as u64)
                        / 1000) as usize;

                    let mut raw_buf = vec![0.0f32; 4096];
                    let mut accumulator = Vec::<f32>::with_capacity(4096);

                    loop {
                        worker_shared.drop_oldest_if_overflow(max_backlog_samples);
                        let read = worker_shared.pop_slice(&mut raw_buf);

                        if read > 0 {
                            let slice = &raw_buf[..read];

                            let mono = if config.channels > 1 {
                                downmix_to_mono(slice, config.channels as usize)
                            } else {
                                slice.to_vec()
                            };

                            let resampled = if config.sample_rate != 16000 {
                                resample_mono(&mono, config.sample_rate, 16000)
                            } else {
                                mono
                            };

                            accumulator.extend_from_slice(&resampled);

                            while accumulator.len() >= config.chunk_size {
                                let chunk: Vec<f32> = accumulator.drain(..config.chunk_size).collect();
                                if tx.blocking_send(CaptureReadResult::Chunk(chunk)).is_err() {
                                    return;
                                }
                            }
                        }

                        if read == 0 {
                            if worker_shared.is_closed() {
                                if !accumulator.is_empty() {
                                    let remaining = std::mem::take(&mut accumulator);
                                    let _ = tx.blocking_send(CaptureReadResult::Chunk(remaining));
                                }
                                let _ = tx.blocking_send(CaptureReadResult::StreamClosed);
                                break;
                            } else {
                                std::thread::yield_now();
                            }
                        }
                    }
                })
                .ok();
            None
        };

        (
            Self {
                receiver: rx,
                shared,
                _worker_handle: worker_handle,
            },
            producer,
        )
    }

    /// Constructs a pipeline initialized from pre-recorded PCM samples for testing and replays.
    pub fn from_replay(
        sample_rate: u32,
        channels: u16,
        pcm: Vec<f32>,
        chunk_size: usize,
    ) -> Self {
        let capacity = pcm.len().max(sample_rate as usize * channels as usize).max(16000);
        let config = PipelineConfig {
            sample_rate,
            channels,
            chunk_size,
            capacity_samples: capacity,
            backlog_limit_ms: 100,
        };

        let (pipeline, producer) = Self::new(config);
        producer.push(&pcm);
        producer.close();
        pipeline
    }

    /// Asynchronously awaits the next chunk from the capture pipeline.
    pub async fn next_chunk(&mut self) -> CaptureReadResult {
        match self.receiver.recv().await {
            Some(res) => res,
            None => CaptureReadResult::StreamClosed,
        }
    }

    /// Attempts to read the next chunk non-blockingly.
    pub fn try_next_chunk(&mut self) -> CaptureReadResult {
        match self.receiver.try_recv() {
            Ok(res) => res,
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                CaptureReadResult::TemporarilyEmpty
            }
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                CaptureReadResult::StreamClosed
            }
        }
    }

    /// Returns the number of backlog overflow occurrences.
    pub fn overflow_count(&self) -> u64 {
        self.shared.overflow_count()
    }

    /// Checks if the capture stream is still active.
    pub fn is_running(&self) -> bool {
        !self.shared.is_closed()
    }

    /// Closes the pipeline stream.
    pub fn close(&self) {
        self.shared.close();
    }

    /// Acquires a producer handle to push data into the active pipeline.
    pub fn producer(&self) -> SpscProducer {
        SpscProducer {
            shared: Arc::clone(&self.shared),
        }
    }
}

// ============================================================================
// Stub KWS Detector for Test Harnesses & VT-0-01 (Isolated from Prod Runtime)
// ============================================================================

/// Information on a wake word hit event.
#[derive(Debug, Clone, PartialEq)]
pub struct KwsWakeHit {
    /// Triggered keyword (defaults to standard "HEY FLOWY")
    pub keyword: String,
    /// Confidence score (0.0 ~ 1.0)
    pub score: f32,
    /// Audio timestamp in milliseconds
    pub timestamp_ms: u64,
}

/// Trigger condition for the stub KWS detector.
#[derive(Debug, Clone, PartialEq)]
pub enum StubKwsTrigger {
    /// Triggers when peak sample absolute amplitude exceeds threshold
    EnergyThreshold(f32),
    /// Triggers after observing N chunks
    ChunkCount(usize),
    /// Triggers after observing N samples
    SampleCount(usize),
    /// Triggers immediately on first chunk
    Immediate,
}

/// Minimal stub KWS detector for Wave 0 skeleton verification.
#[derive(Debug, Clone)]
pub struct StubKwsDetector {
    trigger: StubKwsTrigger,
    chunks_seen: usize,
    samples_seen: usize,
    triggered: bool,
}

impl StubKwsDetector {
    pub fn new(trigger: StubKwsTrigger) -> Self {
        Self {
            trigger,
            chunks_seen: 0,
            samples_seen: 0,
            triggered: false,
        }
    }

    /// Feeds an audio chunk into the detector and returns `Some(KwsWakeHit)` on hit.
    pub fn process_chunk(&mut self, chunk: &[f32]) -> Option<KwsWakeHit> {
        if chunk.is_empty() {
            return None;
        }

        self.chunks_seen += 1;
        self.samples_seen += chunk.len();

        let should_trigger = match &self.trigger {
            StubKwsTrigger::Immediate => true,
            StubKwsTrigger::ChunkCount(limit) => self.chunks_seen >= *limit,
            StubKwsTrigger::SampleCount(limit) => self.samples_seen >= *limit,
            StubKwsTrigger::EnergyThreshold(threshold) => {
                let peak = chunk.iter().fold(0.0f32, |acc, &s| acc.max(s.abs()));
                peak >= *threshold
            }
        };

        if should_trigger && !self.triggered {
            self.triggered = true;
            Some(KwsWakeHit {
                keyword: "HEY FLOWY".to_string(),
                score: 1.0,
                timestamp_ms: (self.samples_seen as u64 * 1000) / 16000,
            })
        } else {
            None
        }
    }

    pub fn is_triggered(&self) -> bool {
        self.triggered
    }

    pub fn reset(&mut self) {
        self.chunks_seen = 0;
        self.samples_seen = 0;
        self.triggered = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spsc_ring_buffer_push_pop_wrap_around() {
        let ring = SpscShared::new(10);
        let data1 = [1.0f32, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(ring.push_slice(&data1), 5);
        assert_eq!(ring.available(), 5);

        let mut read_buf = [0.0f32; 3];
        assert_eq!(ring.pop_slice(&mut read_buf), 3);
        assert_eq!(read_buf, [1.0, 2.0, 3.0]);
        assert_eq!(ring.available(), 2);

        // Push 6 more items, forcing wrap-around in circular buffer of size 10
        let data2 = [6.0f32, 7.0, 8.0, 9.0, 10.0, 11.0];
        assert_eq!(ring.push_slice(&data2), 6);
        assert_eq!(ring.available(), 8);

        let mut read_buf2 = [0.0f32; 8];
        assert_eq!(ring.pop_slice(&mut read_buf2), 8);
        assert_eq!(read_buf2, [4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0]);
        assert_eq!(ring.available(), 0);
    }

    #[tokio::test]
    async fn test_pipeline_stereo_downmix_and_resampling() {
        // 48kHz stereo: 2 channels, 48000 samples/sec
        // Send 960 stereo pairs = 1920 interleaved samples (20ms)
        // Left = 0.4, Right = 0.2 -> Downmixed mono = 0.3
        let mut interleaved = Vec::with_capacity(1920);
        for _ in 0..960 {
            interleaved.push(0.4f32);
            interleaved.push(0.2f32);
        }

        // 20ms at 16kHz is 320 samples
        let mut pipeline = VoiceCapturePipeline::from_replay(48000, 2, interleaved, 320);

        let mut total_output_samples = 0;
        let mut received_chunks = 0;

        loop {
            match pipeline.next_chunk().await {
                CaptureReadResult::Chunk(chunk) => {
                    received_chunks += 1;
                    total_output_samples += chunk.len();
                    for sample in &chunk {
                        // Resampled & downmixed should be very close to 0.3
                        assert!(
                            (*sample - 0.3).abs() < 0.05,
                            "Expected ~0.3 after downmix, got {}",
                            *sample
                        );
                    }
                }
                CaptureReadResult::StreamClosed => break,
                CaptureReadResult::TemporarilyEmpty => {
                    tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
                }
                CaptureReadResult::DeviceError(err) => panic!("{err}"),
            }
        }

        assert!(received_chunks > 0);
        // 20ms of audio at 16kHz is 320 samples
        assert_eq!(total_output_samples, 320);
    }

    #[test]
    fn test_stub_kws_triggers() {
        // 1. Immediate trigger
        let mut kws_imm = StubKwsDetector::new(StubKwsTrigger::Immediate);
        assert!(!kws_imm.is_triggered());
        let hit = kws_imm.process_chunk(&[0.1f32]);
        assert!(hit.is_some());
        assert_eq!(hit.unwrap().keyword, "HEY FLOWY");
        assert!(kws_imm.is_triggered());
        // Second chunk should not re-trigger without reset
        assert!(kws_imm.process_chunk(&[0.1f32]).is_none());

        // Reset
        kws_imm.reset();
        assert!(!kws_imm.is_triggered());

        // 2. Chunk count trigger
        let mut kws_chunk = StubKwsDetector::new(StubKwsTrigger::ChunkCount(2));
        assert!(kws_chunk.process_chunk(&[0.1f32]).is_none());
        assert!(kws_chunk.process_chunk(&[0.1f32]).is_some());

        // 3. Energy threshold trigger
        let mut kws_energy = StubKwsDetector::new(StubKwsTrigger::EnergyThreshold(0.8));
        assert!(kws_energy.process_chunk(&[0.1f32, 0.5f32]).is_none());
        let energy_hit = kws_energy.process_chunk(&[0.1f32, 0.95f32]);
        assert!(energy_hit.is_some());
    }
}

