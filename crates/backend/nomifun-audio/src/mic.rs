//! Microphone capture via `cpal` (default input device).
//!
//! Output is always **mono f32** at a caller-chosen target sample rate
//! (default recommendation: 16_000 Hz for ASR).

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use async_trait::async_trait;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use tokio::sync::mpsc;
use tracing::error;

use crate::capture::AudioCaptureSource;
use crate::pcm_util::{downmix_to_mono, resample_mono};

/// Default input mic source. Chunks are ~20 ms of mono f32 at `sample_rate`.
pub struct MicSource {
    rx: Mutex<mpsc::Receiver<Vec<f32>>>,
    sample_rate: u32,
    join: Mutex<Option<std::thread::JoinHandle<()>>>,
    stop: Arc<AtomicBool>,
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
        let (tx, rx) = mpsc::channel::<Vec<f32>>(128);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);

        let join = std::thread::Builder::new()
            .name("nomifun-mic-cpal".into())
            .spawn(move || {
                if let Err(e) = run_mic_capture(tx, target_sample_rate, stop_thread) {
                    error!("mic capture stopped: {e}");
                }
            })
            .map_err(|e| format!("mic thread spawn failed: {e}"))?;

        Ok(Self {
            rx: Mutex::new(rx),
            sample_rate: target_sample_rate,
            join: Mutex::new(Some(join)),
            stop,
        })
    }
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
    async fn read_chunk(&self) -> Option<Vec<f32>> {
        tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
        self.rx.lock().ok()?.try_recv().ok()
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

struct ChunkAccumulator {
    buf: Mutex<Vec<f32>>,
    tx: mpsc::Sender<Vec<f32>>,
    device_sr: u32,
    target_sr: u32,
    chunk_target: usize,
}

impl ChunkAccumulator {
    fn push_device_rate_mono(&self, mono_device_rate: &[f32]) -> bool {
        let resampled = resample_mono(mono_device_rate, self.device_sr, self.target_sr);
        let mut buf = self.buf.lock().unwrap_or_else(|e| e.into_inner());
        buf.extend_from_slice(&resampled);
        while buf.len() >= self.chunk_target {
            let chunk: Vec<f32> = buf.drain(..self.chunk_target).collect();
            if self.tx.blocking_send(chunk).is_err() {
                return false;
            }
        }
        true
    }
}

fn run_mic_capture(
    tx: mpsc::Sender<Vec<f32>>,
    target_sr: u32,
    stop: Arc<AtomicBool>,
) -> Result<(), String> {
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
    let channels = config.channels as usize;
    let chunk_target = (target_sr / 50).max(1) as usize;

    let accum = Arc::new(ChunkAccumulator {
        buf: Mutex::new(Vec::new()),
        tx: tx.clone(),
        device_sr,
        target_sr,
        chunk_target,
    });

    let err_fn = |err| error!("cpal mic stream error: {err}");

    let stream = match sample_format {
        SampleFormat::F32 => {
            let accum = Arc::clone(&accum);
            device
                .build_input_stream(
                    &config,
                    move |data: &[f32], _| {
                        let mono = downmix_to_mono(data, channels);
                        let _ = accum.push_device_rate_mono(&mono);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("build f32 input stream: {e}"))?
        }
        SampleFormat::I16 => {
            let accum = Arc::clone(&accum);
            device
                .build_input_stream(
                    &config,
                    move |data: &[i16], _| {
                        let f: Vec<f32> =
                            data.iter().map(|s| *s as f32 / i16::MAX as f32).collect();
                        let mono = downmix_to_mono(&f, channels);
                        let _ = accum.push_device_rate_mono(&mono);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("build i16 input stream: {e}"))?
        }
        SampleFormat::U16 => {
            let accum = Arc::clone(&accum);
            device
                .build_input_stream(
                    &config,
                    move |data: &[u16], _| {
                        let f: Vec<f32> = data
                            .iter()
                            .map(|s| (*s as f32 / u16::MAX as f32) * 2.0 - 1.0)
                            .collect();
                        let mono = downmix_to_mono(&f, channels);
                        let _ = accum.push_device_rate_mono(&mono);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("build u16 input stream: {e}"))?
        }
        other => {
            return Err(format!(
                "unsupported mic sample format {other:?} on device '{name}'"
            ));
        }
    };

    stream
        .play()
        .map_err(|e| format!("start mic stream '{name}': {e}"))?;

    while !stop.load(Ordering::Relaxed) && !tx.is_closed() {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    drop(stream);
    Ok(())
}

/// Test-only constructor that never opens a device.
#[cfg(test)]
impl MicSource {
    pub fn from_replay_chunks(sample_rate: u32, chunks: Vec<Vec<f32>>) -> Self {
        let (tx, rx) = mpsc::channel(chunks.len().max(1) + 1);
        for c in chunks {
            let _ = tx.try_send(c);
        }
        Self {
            rx: Mutex::new(rx),
            sample_rate,
            join: Mutex::new(None),
            stop: Arc::new(AtomicBool::new(true)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mic_replay_yields_chunks() {
        let src = MicSource::from_replay_chunks(16_000, vec![vec![0.1; 32], vec![0.2; 16]]);
        assert_eq!(src.label(), "mic");
        assert_eq!(src.sample_rate(), 16_000);
        assert_eq!(src.channels(), 1);
        assert_eq!(src.read_chunk().await.unwrap().len(), 32);
        assert_eq!(src.read_chunk().await.unwrap().len(), 16);
        assert!(src.read_chunk().await.is_none());
    }

    #[test]
    fn mic_new_reports_graceful_error_or_opens() {
        match MicSource::new(16_000) {
            Ok(src) => {
                assert_eq!(src.sample_rate(), 16_000);
                drop(src);
            }
            Err(e) => {
                assert!(
                    e.contains("no default input")
                        || e.contains("input config")
                        || e.contains("stream")
                        || e.contains("format"),
                    "unexpected error: {e}"
                );
            }
        }
    }
}
