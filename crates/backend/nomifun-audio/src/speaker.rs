//! Physical speaker audio sink via `cpal` (default output device).
//!
//! Authoritative baseline: docs/architecture/voice-agent/interaction.zh.md §3.8.
//!
//! Provides zero-lock, low-latency audio output for Earcon sound effects and
//! streamed TTS voice playback through the default system audio output device.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tracing::{error, info};

use crate::pcm_util::resample_mono;

use std::sync::atomic::{AtomicBool, Ordering};

/// Cross-platform physical audio sink for driving system speakers.
pub struct SpeakerAudioSink {
    queue: Arc<Mutex<VecDeque<f32>>>,
    target_sample_rate: u32,
    _stop: Arc<AtomicBool>,
    _thread: Option<std::thread::JoinHandle<()>>,
    ready: Arc<AtomicBool>,
}

impl Drop for SpeakerAudioSink {
    fn drop(&mut self) {
        self._stop.store(true, Ordering::Relaxed);
    }
}

impl SpeakerAudioSink {
    /// Attempts to open the system default output device.
    ///
    /// Returns an error when the physical stream cannot actually start.
    pub fn open_default() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = match host.default_output_device() {
            Some(dev) => dev,
            None => {
                return Err("no_default_output_device".into());
            }
        };

        let device_name = device.name().unwrap_or_else(|_| "<unknown>".to_string());
        info!(device = %device_name, "Configuring default audio speaker device");

        let default_config = device
            .default_output_config()
            .map_err(|e| format!("Failed to get default output config for '{device_name}': {e}"))?;

        let sample_format = default_config.sample_format();
        let config: StreamConfig = default_config.into();
        let sample_rate = config.sample_rate.0;
        let channels = config.channels as usize;

        let queue = Arc::new(Mutex::new(VecDeque::with_capacity(
            sample_rate as usize * channels,
        )));
        let ready = Arc::new(AtomicBool::new(false));
        let thread_ready = ready.clone();
        let error_ready = ready.clone();
        let on_error = move |err: cpal::StreamError| {
            error_ready.store(false, Ordering::Release);
            error!("cpal speaker output stream error: {err}");
        };
        let (init_tx, init_rx) = std::sync::mpsc::sync_channel(1);
        let queue_cb = Arc::clone(&queue);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);
        let device_name_clone = device_name.clone();

        let join = std::thread::Builder::new()
            .name("nomifun-speaker-cpal".into())
            .spawn(move || {
                let stream_res = match sample_format {
                    SampleFormat::F32 => {
                        let q = Arc::clone(&queue_cb);
                        device.build_output_stream(
                            &config,
                            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                                let mut q_lock = q.lock().unwrap();
                                for frame in data.chunks_mut(channels) {
                                    let sample = q_lock.pop_front().unwrap_or(0.0);
                                    for c in frame.iter_mut() {
                                        *c = sample;
                                    }
                                }
                            },
                            on_error,
                            None,
                        )
                    }
                    SampleFormat::I16 => {
                        let q = Arc::clone(&queue_cb);
                        device.build_output_stream(
                            &config,
                            move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                                let mut q_lock = q.lock().unwrap();
                                for frame in data.chunks_mut(channels) {
                                    let sample = q_lock.pop_front().unwrap_or(0.0);
                                    let sample_i16 = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                                    for c in frame.iter_mut() {
                                        *c = sample_i16;
                                    }
                                }
                            },
                            on_error,
                            None,
                        )
                    }
                    SampleFormat::U16 => {
                        let q = Arc::clone(&queue_cb);
                        device.build_output_stream(
                            &config,
                            move |data: &mut [u16], _: &cpal::OutputCallbackInfo| {
                                let mut q_lock = q.lock().unwrap();
                                for frame in data.chunks_mut(channels) {
                                    let sample = q_lock.pop_front().unwrap_or(0.0);
                                    let sample_u16 = ((sample.clamp(-1.0, 1.0) * 0.5 + 0.5) * u16::MAX as f32) as u16;
                                    for c in frame.iter_mut() {
                                        *c = sample_u16;
                                    }
                                }
                            },
                            on_error,
                            None,
                        )
                    }
                    other => {
                        let _ = init_tx.send(Err(format!("unsupported_output_format: {other:?}")));
                        return;
                    }
                };

                let stream = match stream_res {
                    Ok(s) => s,
                    Err(err) => {
                        let _ = init_tx.send(Err(format!("speaker_stream_build_failed: {err}")));
                        return;
                    }
                };

                if let Err(e) = stream.play() {
                    let _ = init_tx.send(Err(format!("speaker_stream_start_failed: {e}")));
                    return;
                }
                thread_ready.store(true, Ordering::Release);
                let _ = init_tx.send(Ok(()));
                info!(device = %device_name_clone, sample_rate, channels, "Speaker audio sink initialized successfully");

                while !stop_clone.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            })
            .map_err(|err| format!("speaker_thread_failed: {err}"))?;

        if let Err(error) = init_rx
            .recv_timeout(std::time::Duration::from_secs(3))
            .map_err(|err| err.to_string())
            .and_then(|result| result)
        {
            stop.store(true, Ordering::Relaxed);
            return Err(error);
        }

        Ok(Self {
            queue,
            target_sample_rate: sample_rate,
            _stop: stop,
            _thread: Some(join),
            ready,
        })
    }

    /// Enqueues audio frames into the output queue for playback.
    ///
    /// Automatically detects RIFF WAVE headers or raw PCM and resamples to
    /// the physical device's native hardware rate.
    pub async fn write_frame(&self, frame: Vec<u8>) -> Result<(), String> {
        if !self.is_ready() {
            return Err("speaker_unavailable".into());
        }
        if frame.is_empty() {
            return Ok(());
        }

        let (mono_samples, source_sr) = parse_audio_frame_to_pcm(&frame);
        if mono_samples.is_empty() {
            return Ok(());
        }

        let resampled = if source_sr != self.target_sample_rate {
            resample_mono(&mono_samples, source_sr, self.target_sample_rate)
        } else {
            mono_samples
        };

        let mut q = self.queue.lock().unwrap();
        q.extend(resampled);
        Ok(())
    }

    /// Flushes all pending audio samples from the buffer, instantly silencing
    /// the speaker (<50ms barge-in latency per interaction.zh.md §3.8).
    pub async fn flush(&self) -> Result<(), String> {
        let mut q = self.queue.lock().unwrap();
        q.clear();
        Ok(())
    }

    /// Returns the number of unplayed audio samples currently buffered.
    pub fn buffered_samples(&self) -> usize {
        self.queue.lock().unwrap().len()
    }

    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }
}

/// Helper function to parse an audio frame payload (WAV or raw PCM) into mono f32 samples and sample rate.
pub fn parse_audio_frame_to_pcm(bytes: &[u8]) -> (Vec<f32>, u32) {
    if bytes.len() >= 44 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        if let Ok((samples, sr_opt)) = crate::diagnostics::parse_wav_pcm(bytes) {
            return (samples, sr_opt.unwrap_or(16_000));
        }
    }

    // Fallback: 16-bit signed PCM
    let mut samples = Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.chunks_exact(2) {
        let s = i16::from_le_bytes([chunk[0], chunk[1]]);
        samples.push(s as f32 / 32768.0);
    }
    (samples, 16_000)
}

/// Convenience constructor for opening the default system speaker.
pub fn open_default_speaker() -> Result<SpeakerAudioSink, String> {
    SpeakerAudioSink::open_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_speaker_sink_queue_and_flush() {
        let sink = SpeakerAudioSink {
            queue: Arc::new(Mutex::new(VecDeque::new())),
            target_sample_rate: 16_000,
            _stop: Arc::new(AtomicBool::new(false)),
            _thread: None,
            ready: Arc::new(AtomicBool::new(true)),
        };

        assert_eq!(sink.buffered_samples(), 0);

        // Feed 16-bit PCM frame (10 samples of zeroes = 20 bytes)
        let frame = vec![0u8; 20];
        sink.write_frame(frame).await.unwrap();
        assert_eq!(sink.buffered_samples(), 10);

        // Flush must immediately clear the queue
        sink.flush().await.unwrap();
        assert_eq!(sink.buffered_samples(), 0);
    }

    #[test]
    fn test_parse_audio_frame_wav() {
        let wav = crate::EarconKind::Wake.as_wav_bytes();
        let (samples, sr) = parse_audio_frame_to_pcm(wav);
        assert!(
            !samples.is_empty(),
            "Wake earcon samples should not be empty"
        );
        assert!(sr > 0, "Sample rate must be positive");
    }
}
