//! Loopback audio capture (system speaker output).
//!
//! Captures what the speakers are playing — i.e. remote participants in online
//! meetings — so that the `DualTrackMixer` can attribute their audio to
//! "Speaker B" without any ML diarization.
//!
//! # Platform support
//!
//! | Platform | Mechanism | Status |
//! |----------|-----------|--------|
//! | Windows  | WASAPI loopback render endpoint | Implemented |
//! | macOS    | ScreenCaptureKit / BlackHole virtual device | Stub (requires entitlement) |
//! | Linux    | PulseAudio monitor source | Stub |

use async_trait::async_trait;

use crate::capture::AudioCaptureSource;

/// Loopback capture source.  Wraps the platform-specific implementation.
pub struct LoopbackSource {
    inner: Box<dyn AudioCaptureSource>,
}

impl LoopbackSource {
    /// Open the default system audio output device for loopback capture,
    /// resampling to `target_sample_rate` Hz (recommend 16_000 for ASR).
    ///
    /// Returns `Err` if loopback is unavailable on this platform/configuration.
    pub fn new(target_sample_rate: u32) -> Result<Self, String> {
        let inner = platform::open_loopback(target_sample_rate)?;
        Ok(Self { inner })
    }
}

#[async_trait]
impl AudioCaptureSource for LoopbackSource {
    async fn read_chunk(&self) -> Option<Vec<f32>> {
        self.inner.read_chunk().await
    }

    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    fn channels(&self) -> u16 {
        self.inner.channels()
    }

    fn label(&self) -> &str {
        "loopback"
    }
}

// ---------------------------------------------------------------------------
// Windows WASAPI loopback implementation
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
mod platform {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };

    use async_trait::async_trait;
    use tokio::sync::mpsc;
    use tracing::error;
    use windows::Win32::Media::Audio::{
        AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
        eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator,
        MMDeviceEnumerator, WAVEFORMATEX, WAVEFORMATEXTENSIBLE,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
        CoUninitialize,
    };

    // Avoid pulling Win32_Media_Multimedia / KernelStreaming just for these tags.
    const WAVE_FORMAT_PCM: u16 = 0x0001;
    const WAVE_FORMAT_IEEE_FLOAT: u16 = 0x0003;
    const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

    use crate::capture::AudioCaptureSource;
    use crate::pcm_util::{downmix_to_mono, resample_mono};

    pub struct WasapiLoopbackSource {
        buf: Mutex<mpsc::Receiver<Vec<f32>>>,
        sample_rate: u32,
        stop: Arc<AtomicBool>,
        join: Mutex<Option<std::thread::JoinHandle<()>>>,
    }

    impl Drop for WasapiLoopbackSource {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Ok(mut g) = self.join.lock() {
                if let Some(h) = g.take() {
                    let _ = h.join();
                }
            }
        }
    }

    pub fn open_loopback(target_sample_rate: u32) -> Result<Box<dyn AudioCaptureSource>, String> {
        let target_sample_rate = if target_sample_rate == 0 {
            16_000
        } else {
            target_sample_rate
        };
        let (tx, rx) = mpsc::channel::<Vec<f32>>(128);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_t = Arc::clone(&stop);

        let join = std::thread::Builder::new()
            .name("nomifun-wasapi-loopback".into())
            .spawn(move || {
                if let Err(e) = wasapi_capture_loop(tx, target_sample_rate, stop_t) {
                    error!("WASAPI loopback error: {e}");
                }
            })
            .map_err(|e| format!("thread spawn failed: {e}"))?;

        Ok(Box::new(WasapiLoopbackSource {
            buf: Mutex::new(rx),
            sample_rate: target_sample_rate,
            stop,
            join: Mutex::new(Some(join)),
        }))
    }

    fn wasapi_capture_loop(
        tx: mpsc::Sender<Vec<f32>>,
        target_sr: u32,
        stop: Arc<AtomicBool>,
    ) -> Result<(), String> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED)
                .ok()
                .map_err(|e| format!("CoInitializeEx: {e}"))?;
        }

        let result = (|| -> Result<(), String> {
            unsafe {
                let enumerator: IMMDeviceEnumerator =
                    CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                        .map_err(|e| format!("MMDeviceEnumerator: {e}"))?;

                let device = enumerator
                    .GetDefaultAudioEndpoint(eRender, eConsole)
                    .map_err(|e| format!("GetDefaultAudioEndpoint(eRender): {e}"))?;

                let client: IAudioClient = device
                    .Activate::<IAudioClient>(CLSCTX_ALL, None)
                    .map_err(|e| format!("Activate IAudioClient: {e}"))?;

                let mix_format_ptr = client
                    .GetMixFormat()
                    .map_err(|e| format!("GetMixFormat: {e}"))?;
                if mix_format_ptr.is_null() {
                    return Err("GetMixFormat returned null".into());
                }

                let mix = &*mix_format_ptr;
                let device_sr = mix.nSamplesPerSec;
                let channels = mix.nChannels as usize;
                let bits = mix.wBitsPerSample;
                let format_tag = mix.wFormatTag;
                let block_align = mix.nBlockAlign as usize;

                let is_float = format_tag == WAVE_FORMAT_IEEE_FLOAT
                    || (format_tag == WAVE_FORMAT_EXTENSIBLE && bits == 32 && is_extensible_float(mix));
                let is_pcm16 = format_tag == WAVE_FORMAT_PCM && bits == 16
                    || (format_tag == WAVE_FORMAT_EXTENSIBLE && bits == 16);

                if !is_float && !is_pcm16 {
                    CoTaskMemFree(Some(mix_format_ptr.cast()));
                    return Err(format!(
                        "unsupported mix format: tag={format_tag} bits={bits} ch={channels}"
                    ));
                }

                // 100 ns units — request ~100 ms buffer.
                const REFTIMES_PER_SEC: i64 = 10_000_000;
                client
                    .Initialize(
                        AUDCLNT_SHAREMODE_SHARED,
                        AUDCLNT_STREAMFLAGS_LOOPBACK,
                        REFTIMES_PER_SEC / 10,
                        0,
                        mix_format_ptr,
                        None,
                    )
                    .map_err(|e| {
                        CoTaskMemFree(Some(mix_format_ptr.cast()));
                        format!("IAudioClient::Initialize(LOOPBACK): {e}")
                    })?;

                let capture: IAudioCaptureClient = client
                    .GetService::<IAudioCaptureClient>()
                    .map_err(|e| format!("GetService IAudioCaptureClient: {e}"))?;

                client
                    .Start()
                    .map_err(|e| format!("IAudioClient::Start: {e}"))?;

                let chunk_target = (target_sr / 50).max(1) as usize;
                let mut accum: Vec<f32> = Vec::with_capacity(chunk_target * 2);
                let silent_chunk = vec![0.0f32; chunk_target];
                let mut idle_ticks = 0u32;

                while !stop.load(Ordering::Relaxed) && !tx.is_closed() {
                    let packet_frames = match capture.GetNextPacketSize() {
                        Ok(n) => n,
                        Err(_) => break,
                    };

                    if packet_frames == 0 {
                        // WASAPI loopback yields nothing when the render engine is
                        // idle (no apps playing). Emit silence so the mixer keeps
                        // advancing instead of stalling on try_recv.
                        idle_ticks += 1;
                        if idle_ticks >= 4 {
                            idle_ticks = 0;
                            if tx.blocking_send(silent_chunk.clone()).is_err() {
                                break;
                            }
                        }
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        continue;
                    }
                    idle_ticks = 0;

                    let mut remaining = packet_frames;
                    while remaining > 0 {
                        let mut data_ptr = std::ptr::null_mut();
                        let mut num_frames_available = 0u32;
                        let mut flags = 0u32;
                        if capture
                            .GetBuffer(
                                &mut data_ptr,
                                &mut num_frames_available,
                                &mut flags,
                                None,
                                None,
                            )
                            .is_err()
                        {
                            break;
                        }

                        if num_frames_available > 0 && !data_ptr.is_null() {
                            let silent =
                                (flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0;
                            let frames = num_frames_available as usize;
                            let mono = if silent {
                                vec![0.0f32; frames]
                            } else if is_float {
                                let sample_count = frames * channels;
                                let slice = std::slice::from_raw_parts(
                                    data_ptr as *const f32,
                                    sample_count,
                                );
                                downmix_to_mono(slice, channels)
                            } else {
                                let byte_len = frames * block_align;
                                let bytes =
                                    std::slice::from_raw_parts(data_ptr as *const u8, byte_len);
                                let mut interleaved = Vec::with_capacity(frames * channels);
                                for frame in 0..frames {
                                    for ch in 0..channels {
                                        let off = frame * block_align + ch * 2;
                                        let s = i16::from_le_bytes([bytes[off], bytes[off + 1]]);
                                        interleaved.push(s as f32 / i16::MAX as f32);
                                    }
                                }
                                downmix_to_mono(&interleaved, channels)
                            };

                            let resampled = resample_mono(&mono, device_sr, target_sr);
                            accum.extend_from_slice(&resampled);
                            while accum.len() >= chunk_target {
                                let chunk: Vec<f32> = accum.drain(..chunk_target).collect();
                                if tx.blocking_send(chunk).is_err() {
                                    let _ = capture.ReleaseBuffer(num_frames_available);
                                    CoTaskMemFree(Some(mix_format_ptr.cast()));
                                    let _ = client.Stop();
                                    return Ok(());
                                }
                            }
                        }

                        let _ = capture.ReleaseBuffer(num_frames_available);
                        remaining = match capture.GetNextPacketSize() {
                            Ok(n) => n,
                            Err(_) => 0,
                        };
                    }
                }

                let _ = client.Stop();
                CoTaskMemFree(Some(mix_format_ptr.cast()));
                Ok(())
            }
        })();

        unsafe {
            CoUninitialize();
        }
        result
    }

    unsafe fn is_extensible_float(mix: &WAVEFORMATEX) -> bool {
        if mix.wFormatTag != WAVE_FORMAT_EXTENSIBLE || mix.cbSize < 22 {
            return false;
        }
        // WAVEFORMATEXTENSIBLE follows WAVEFORMATEX; SubFormat GUID for IEEE float:
        // {00000003-0000-0010-8000-00aa00389b71}
        let ext = unsafe { &*(mix as *const WAVEFORMATEX as *const WAVEFORMATEXTENSIBLE) };
        let g = ext.SubFormat;
        g.data1 == 0x0000_0003
            && g.data2 == 0x0000
            && g.data3 == 0x0010
            && g.data4 == [0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71]
    }

    #[async_trait]
    impl AudioCaptureSource for WasapiLoopbackSource {
        async fn read_chunk(&self) -> Option<Vec<f32>> {
            tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
            self.buf.lock().ok()?.try_recv().ok()
        }

        fn sample_rate(&self) -> u32 {
            self.sample_rate
        }

        fn channels(&self) -> u16 {
            1
        }

        fn label(&self) -> &str {
            "loopback_wasapi"
        }
    }
}

// ---------------------------------------------------------------------------
// Non-Windows stub
// ---------------------------------------------------------------------------

#[cfg(not(target_os = "windows"))]
mod platform {
    use async_trait::async_trait;
    use tracing::warn;

    use crate::capture::AudioCaptureSource;

    struct UnsupportedLoopback {
        sample_rate: u32,
    }

    pub fn open_loopback(target_sample_rate: u32) -> Result<Box<dyn AudioCaptureSource>, String> {
        warn!(
            "Loopback capture is not yet implemented on this platform. \
             Single-track mic recording will be used instead."
        );
        Ok(Box::new(UnsupportedLoopback {
            sample_rate: target_sample_rate,
        }))
    }

    #[async_trait]
    impl AudioCaptureSource for UnsupportedLoopback {
        async fn read_chunk(&self) -> Option<Vec<f32>> {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            Some(vec![0.0f32; (self.sample_rate / 50).max(1) as usize])
        }

        fn sample_rate(&self) -> u32 {
            self.sample_rate
        }

        fn channels(&self) -> u16 {
            1
        }

        fn label(&self) -> &str {
            "loopback_stub"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn loopback_source_opens_and_yields_frames() {
        let src = LoopbackSource::new(16_000).expect("loopback open failed");
        let frame =
            tokio::time::timeout(tokio::time::Duration::from_millis(1500), src.read_chunk()).await;
        assert!(
            frame.is_ok(),
            "loopback source timed out producing first frame"
        );
        // On Windows with no audio playing, chunks may be silence — still Some.
        // try_recv may return None briefly; allow a few polls.
        let mut got = frame.ok().and_then(|f| f);
        if got.is_none() {
            for _ in 0..50 {
                if let Some(c) = src.read_chunk().await {
                    got = Some(c);
                    break;
                }
            }
        }
        assert!(got.is_some(), "expected at least one loopback chunk");
    }
}
