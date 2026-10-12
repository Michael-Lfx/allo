//! Microphone capture via `cpal` (default input device).
//!
//! Refactored in Wave 1 for AI Mate voice agent:
//! - Stage 1 (Producer): Zero-lock, zero-allocation SPSC push in real-time CPAL callback.
//! - Stage 2 (Worker): Background worker thread performs channel downmixing and resampling.
//! - Stage 3 (Consumer): Exposes `CaptureReadResult` eliminating `try_recv()` ambiguity.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use async_trait::async_trait;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use tracing::error;

use crate::capture::{AudioCaptureSource, CaptureReadResult};
use crate::voice_pipeline::{PipelineConfig, VoiceCapturePipeline};

/// Default input mic source. Chunks are ~20 ms of mono f32 at `sample_rate`.
pub struct MicSource {
    pipeline: Arc<tokio::sync::Mutex<VoiceCapturePipeline>>,
    sample_rate: u32,
    stop: Arc<AtomicBool>,
    join: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl MicSource {
    /// Open the system default input device, resampled to `target_sample_rate`.
    ///
    /// Returns `Err` when no input device is available or the stream cannot
    /// be opened — callers should degrade to loopback-only / fail the session.
    pub fn new(target_sample_rate: u32) -> Result<Self, String> {
        let target_sample_rate = if target_sample_rate == 0 {
            16_000
        } else {
            target_sample_rate
        };

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| "no default input (microphone) device available".to_string())?;

        let name = device.name().unwrap_or_else(|_| "<unknown>".into());
        let supported = device
            .default_input_config()
            .map_err(|e| format!("default input config for '{name}': {e}"))?;

        let sample_format = supported.sample_format();
        let config: StreamConfig = supported.clone().into();
        let device_sr = config.sample_rate.0;
        let channels = config.channels;
        let chunk_target = (target_sample_rate / 50).max(1) as usize; // 20ms

        let pipeline_config = PipelineConfig {
            sample_rate: device_sr,
            channels,
            chunk_size: chunk_target,
            capacity_samples: (device_sr as usize * channels as usize).max(48000), // 1 second buffer
            backlog_limit_ms: 100,                                                 // 100ms
        };

        let (pipeline, producer) = VoiceCapturePipeline::new(pipeline_config);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let prod_thread = producer.clone();

        let prod_err = prod_thread.clone();
        let err_fn = move |err: cpal::StreamError| {
            error!("cpal mic stream error: {err}");
            prod_err.signal_device_error(format!("cpal mic stream error: {err}"));
        };

        let join = std::thread::Builder::new()
            .name("nomifun-mic-cpal".into())
            .spawn(move || {
                let stream_res = match sample_format {
                    SampleFormat::F32 => {
                        let prod = prod_thread.clone();
                        device.build_input_stream(
                            &config,
                            move |data: &[f32], _| {
                                // Stage 1 RT audio callback: 0 locks, 0 memory allocations
                                prod.push(data);
                            },
                            err_fn,
                            None,
                        )
                    }
                    SampleFormat::I16 => {
                        let prod = prod_thread.clone();
                        device.build_input_stream(
                            &config,
                            move |data: &[i16], _| {
                                // Stack buffer to avoid heap allocations in audio thread
                                let mut stack_buf = [0.0f32; 256];
                                for chunk in data.chunks(256) {
                                    for (i, &s) in chunk.iter().enumerate() {
                                        stack_buf[i] = s as f32 / i16::MAX as f32;
                                    }
                                    prod.push(&stack_buf[..chunk.len()]);
                                }
                            },
                            err_fn,
                            None,
                        )
                    }
                    SampleFormat::U16 => {
                        let prod = prod_thread.clone();
                        device.build_input_stream(
                            &config,
                            move |data: &[u16], _| {
                                let mut stack_buf = [0.0f32; 256];
                                for chunk in data.chunks(256) {
                                    for (i, &s) in chunk.iter().enumerate() {
                                        stack_buf[i] = (s as f32 / u16::MAX as f32) * 2.0 - 1.0;
                                    }
                                    prod.push(&stack_buf[..chunk.len()]);
                                }
                            },
                            err_fn,
                            None,
                        )
                    }
                    other => {
                        error!("unsupported mic sample format {other:?} on device '{name}'");
                        prod_thread.signal_device_error(format!("unsupported mic sample format {other:?}"));
                        return;
                    }
                };

                let stream = match stream_res {
                    Ok(s) => s,
                    Err(e) => {
                        error!("build input stream '{name}': {e}");
                        prod_thread.signal_device_error(format!("build input stream '{name}': {e}"));
                        return;
                    }
                };

                if let Err(e) = stream.play() {
                    error!("start mic stream '{name}': {e}");
                    prod_thread.signal_device_error(format!("start mic stream '{name}': {e}"));
                    return;
                }

                while !stop_thread.load(Ordering::Relaxed) && !prod_thread.is_closed() {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }

                drop(stream);
                prod_thread.close();
            })
            .map_err(|e| format!("mic thread spawn failed: {e}"))?;

        Ok(Self {
            pipeline: Arc::new(tokio::sync::Mutex::new(pipeline)),
            sample_rate: target_sample_rate,
            stop,
            join: Mutex::new(Some(join)),
        })
    }

    /// Read next chunk non-blockingly or asynchronously.
    pub async fn read_capture(&self) -> CaptureReadResult {
        self.pipeline.lock().await.next_chunk().await
    }

    /// Number of xrun / backlog overflow drops observed.
    pub async fn overflow_count(&self) -> u64 {
        self.pipeline.lock().await.overflow_count()
    }

    /// Exposes the underlying capture pipeline.
    pub fn pipeline(&self) -> Arc<tokio::sync::Mutex<VoiceCapturePipeline>> {
        Arc::clone(&self.pipeline)
    }

    /// Test constructor: replay pre-recorded PCM chunks.
    pub fn from_replay_chunks(sample_rate: u32, chunks: Vec<Vec<f32>>) -> Self {
        let mut flat = Vec::new();
        for c in chunks {
            flat.extend_from_slice(&c);
        }
        let chunk_size = (sample_rate / 50).max(1) as usize;
        let pipeline = VoiceCapturePipeline::from_replay(sample_rate, 1, flat, chunk_size);
        Self {
            pipeline: Arc::new(tokio::sync::Mutex::new(pipeline)),
            sample_rate,
            stop: Arc::new(AtomicBool::new(true)),
            join: Mutex::new(None),
        }
    }
}

/// Probes the system default microphone input device.
pub fn probe_default_device() -> nomifun_api_types::voice::VoiceDeviceStatusResponse {
    let host = cpal::default_host();
    let device = match host.default_input_device() {
        Some(d) => d,
        None => {
            return nomifun_api_types::voice::VoiceDeviceStatusResponse {
                mic_available: false,
                device_name: None,
                sample_rate: None,
                channels: None,
                error: Some("No default audio input device found".to_string()),
            };
        }
    };

    let name = device.name().unwrap_or_else(|_| "<unknown>".into());
    match device.default_input_config() {
        Ok(config) => nomifun_api_types::voice::VoiceDeviceStatusResponse {
            mic_available: true,
            device_name: Some(name),
            sample_rate: Some(config.sample_rate().0),
            channels: Some(config.channels()),
            error: None,
        },
        Err(err) => nomifun_api_types::voice::VoiceDeviceStatusResponse {
            mic_available: false,
            device_name: Some(name),
            sample_rate: None,
            channels: None,
            error: Some(format!("Failed to query input config: {err}")),
        },
    }
}

/// Records a short mono 16kHz PCM snippet from the default microphone.
pub async fn record_mic_snippet(duration: std::time::Duration) -> Result<Vec<f32>, String> {
    let mic = MicSource::new(16_000)?;
    let target_samples = (16_000.0 * duration.as_secs_f32()).round() as usize;
    let mut collected = Vec::with_capacity(target_samples);
    let start = std::time::Instant::now();
    let timeout = duration + std::time::Duration::from_millis(2000);

    while collected.len() < target_samples && start.elapsed() < timeout {
        match mic.read_capture().await {
            CaptureReadResult::Chunk(samples) => {
                collected.extend_from_slice(&samples);
            }
            CaptureReadResult::TemporarilyEmpty => {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            CaptureReadResult::StreamClosed => break,
            CaptureReadResult::DeviceError(err) => return Err(err),
        }
    }

    if collected.is_empty() {
        return Err("No audio captured from microphone".to_string());
    }

    Ok(collected)
}

impl Drop for MicSource {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Ok(mut g) = self.join.lock() {
            if let Some(h) = g.take() {
                let _ = h.join();
            }
        }
    }
}

#[async_trait]
impl AudioCaptureSource for MicSource {
    async fn read_chunk(&self) -> CaptureReadResult {
        self.pipeline.lock().await.next_chunk().await
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn channels(&self) -> u16 {
        1
    }

    fn label(&self) -> &str {
        "mic"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mic_replay_yields_chunks() {
        let src = MicSource::from_replay_chunks(16_000, vec![vec![0.1; 320], vec![0.2; 320]]);
        assert_eq!(src.label(), "mic");
        assert_eq!(src.sample_rate(), 16_000);
        assert_eq!(src.channels(), 1);

        match src.read_chunk().await {
            CaptureReadResult::Chunk(c) => assert_eq!(c.len(), 320),
            other => panic!("Expected chunk, got {other:?}"),
        }
        match src.read_chunk().await {
            CaptureReadResult::Chunk(c) => assert_eq!(c.len(), 320),
            other => panic!("Expected chunk, got {other:?}"),
        }
        assert_eq!(src.read_chunk().await, CaptureReadResult::StreamClosed);
    }

    #[test]
    fn mic_new_reports_graceful_error_or_opens() {
        match MicSource::new(16_000) {
            Ok(src) => {
                assert_eq!(src.sample_rate(), 16_000);
            }
            Err(e) => {
                assert!(!e.is_empty());
            }
        }
    }
}
