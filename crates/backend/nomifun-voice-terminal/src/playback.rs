//! Playback pipeline and audio sink abstractions.
//!
//! Authoritative baseline: docs/architecture/voice-agent/interaction.zh.md §3.8.

use async_trait::async_trait;
use nomifun_audio::EarconKind;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Debug, thiserror::Error)]
pub enum AudioSinkError {
    #[error("TTS synthesis failed: {0}")]
    Synthesis(String),
    #[error("TTS rate limited (HTTP {status})")]
    RateLimited {
        status: u16,
        retry_after: Option<Duration>,
    },
    #[error("TTS returned no audio")]
    NoAudio,
    #[error("Audio sink write failed: {0}")]
    Write(String),
    #[error("Audio sink flush failed: {0}")]
    Flush(String),
}

#[async_trait]
pub trait AudioSink: Send + Sync {
    /// True only while frames remain queued for this sink's output.
    fn has_pending_audio(&self) -> bool {
        false
    }
    fn is_available(&self) -> bool {
        true
    }
    async fn drain(&self) -> Result<(), AudioSinkError> {
        Ok(())
    }
    async fn write_frame(&self, frame: Vec<u8>) -> Result<(), AudioSinkError>;
    async fn flush(&self) -> Result<(), AudioSinkError>;
}

/// The production sink owns physical queue writes, cancellation and completion.
pub struct PhysicalAudioSink {
    speaker: Option<Arc<nomifun_audio::SpeakerAudioSink>>,
}
impl PhysicalAudioSink {
    pub fn open_default() -> Self {
        match nomifun_audio::open_default_speaker() {
            Ok(speaker) => Self {
                speaker: Some(Arc::new(speaker)),
            },
            Err(error) => {
                tracing::warn!(error = %error, "voice_speaker_unavailable");
                Self { speaker: None }
            }
        }
    }
}
#[async_trait]
impl AudioSink for PhysicalAudioSink {
    fn has_pending_audio(&self) -> bool {
        self.speaker.as_ref().is_some_and(|speaker| speaker.buffered_samples() > 0)
    }
    fn is_available(&self) -> bool {
        self.speaker
            .as_ref()
            .is_some_and(|speaker| speaker.is_ready())
    }
    async fn write_frame(&self, frame: Vec<u8>) -> Result<(), AudioSinkError> {
        self.speaker
            .as_ref()
            .ok_or_else(|| AudioSinkError::Write("speaker_unavailable".into()))?
            .write_frame(frame)
            .await
            .map_err(AudioSinkError::Write)
    }
    async fn flush(&self) -> Result<(), AudioSinkError> {
        if let Some(speaker) = &self.speaker {
            speaker.flush().await.map_err(AudioSinkError::Flush)?;
        }
        Ok(())
    }
    async fn drain(&self) -> Result<(), AudioSinkError> {
        let start = std::time::Instant::now();
        let speaker = self
            .speaker
            .as_ref()
            .ok_or_else(|| AudioSinkError::Write("speaker_unavailable".into()))?;
        while speaker.buffered_samples() > 0 {
            if !speaker.is_ready() {
                return Err(AudioSinkError::Write("speaker_stream_failed".into()));
            }
            if start.elapsed() > Duration::from_secs(60) {
                speaker.flush().await.map_err(AudioSinkError::Flush)?;
                return Err(AudioSinkError::Write("speaker_drain_timeout".into()));
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Ok(())
    }
}

/// Channel-based audio sink forwarding playback frames to a consumer channel.
pub struct ChannelAudioSink {
    sender: mpsc::Sender<Vec<u8>>,
}

impl ChannelAudioSink {
    pub fn new(sender: mpsc::Sender<Vec<u8>>) -> Self {
        Self { sender }
    }
}

#[async_trait]
impl AudioSink for ChannelAudioSink {
    async fn write_frame(&self, frame: Vec<u8>) -> Result<(), AudioSinkError> {
        self.sender
            .send(frame)
            .await
            .map_err(|e| AudioSinkError::Write(e.to_string()))
    }

    async fn flush(&self) -> Result<(), AudioSinkError> {
        Ok(())
    }
}

/// In-memory test sink for capturing playback frames during testing.
#[derive(Default, Clone)]
pub struct TestAudioSink {
    frames: Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
}

impl TestAudioSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn frames(&self) -> Vec<Vec<u8>> {
        self.frames.lock().unwrap().clone()
    }

    pub fn frame_count(&self) -> usize {
        self.frames.lock().unwrap().len()
    }

    pub fn clear(&self) {
        self.frames.lock().unwrap().clear();
    }
}

#[async_trait]
impl AudioSink for TestAudioSink {
    async fn write_frame(&self, frame: Vec<u8>) -> Result<(), AudioSinkError> {
        self.frames.lock().unwrap().push(frame);
        Ok(())
    }

    async fn flush(&self) -> Result<(), AudioSinkError> {
        self.clear();
        Ok(())
    }
}

/// Jitter buffer resisting network packet jitter before playback emission.
/// Defined in interaction.zh.md §3.8 (100ms jitter buffer threshold).
#[derive(Debug, Clone)]
pub struct JitterBuffer {
    buffer: Vec<u8>,
    target_duration: Duration,
    sample_rate: u32,
    threshold_bytes: usize,
}

impl JitterBuffer {
    pub fn new(target_duration: Duration, sample_rate: u32) -> Self {
        let bytes_per_sec = (sample_rate as f32 * 2.0) as usize;
        let threshold_bytes = (target_duration.as_secs_f32() * bytes_per_sec as f32) as usize;
        Self {
            buffer: Vec::new(),
            target_duration,
            sample_rate,
            threshold_bytes,
        }
    }

    pub fn target_duration(&self) -> Duration {
        self.target_duration
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn push(&mut self, chunk: Vec<u8>) {
        self.buffer.extend_from_slice(&chunk);
    }

    pub fn is_ready(&self) -> bool {
        self.buffer.len() >= self.threshold_bytes
    }

    pub fn pop_all(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.buffer)
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

struct PendingOutput<'a>(&'a AtomicUsize);
impl Drop for PendingOutput<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Playback pipeline driving earcon assets and TTS stream decoding.
pub struct PlaybackPipeline {
    sink: Arc<dyn AudioSink>,
    generation_id: Arc<AtomicU64>,
    interruption: tokio::sync::watch::Sender<u64>,
    output: tokio::sync::Mutex<()>,
    pending_output: AtomicUsize,
}

impl PlaybackPipeline {
    /// Explicit self-check playback: writes the fixed sample and waits for the
    /// sink to actually consume it. A flush/discard must never be reported as
    /// successful playback; interruption and drain failure fail closed.
    pub async fn play_wav_for_self_check(&self, wav: Vec<u8>) -> Result<(), AudioSinkError> {
        self.play_wav_for_self_check_with_timeout(
            wav,
            std::time::Duration::from_secs(10),
        )
        .await
    }

    async fn play_wav_for_self_check_with_timeout(
        &self,
        wav: Vec<u8>,
        drain_timeout: std::time::Duration,
    ) -> Result<(), AudioSinkError> {
        if !self.sink.is_available() {
            return Err(AudioSinkError::Write("speaker_unavailable".into()));
        }
        let generation = self.generation_id();
        let (tx, rx) = mpsc::channel(1);
        tx.send(wav)
            .await
            .map_err(|error| AudioSinkError::Write(error.to_string()))?;
        drop(tx);
        self.play_stream(rx).await?;
        if self.generation_id() != generation {
            return Err(AudioSinkError::Write("self_check_cancelled".into()));
        }
        // A caller may poll this immediate-sink contract without a Tokio
        // runtime (the independent diagnosis probe does). Timers require a
        // runtime, so only arm the bounded timeout when one exists.
        if tokio::runtime::Handle::try_current().is_ok() {
            match tokio::time::timeout(drain_timeout, self.sink.drain()).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    let _ = self.sink.flush().await;
                    return Err(error);
                }
                Err(_) => {
                    let _ = self.sink.flush().await;
                    return Err(AudioSinkError::Write("self_check_drain_timeout".into()));
                }
            }
        } else if let Err(error) = self.sink.drain().await {
            let _ = self.sink.flush().await;
            return Err(error);
        }
        if self.generation_id() != generation {
            return Err(AudioSinkError::Write("self_check_cancelled".into()));
        }
        Ok(())
    }
    pub fn is_available(&self) -> bool {
        self.sink.is_available()
    }
    pub fn new(sink: Arc<dyn AudioSink>) -> Self {
        Self {
            sink,
            generation_id: Arc::new(AtomicU64::new(0)),
            interruption: tokio::sync::watch::channel(0).0,
            output: tokio::sync::Mutex::new(()),
            pending_output: AtomicUsize::new(0),
        }
    }

    pub fn generation_id(&self) -> u64 {
        self.generation_id.load(Ordering::SeqCst)
    }

    pub fn has_pending_audio(&self) -> bool {
        self.sink.has_pending_audio()
    }

    /// Wakes even when synthesis, a channel receive or an output lock is stalled.
    pub async fn interrupted(&self, expected_gen: u64) {
        let mut changed = self.interruption.subscribe();
        while *changed.borrow_and_update() <= expected_gen {
            if changed.changed().await.is_err() { return; }
        }
    }

    fn request_output(&self) -> PendingOutput<'_> {
        self.pending_output.fetch_add(1, Ordering::SeqCst);
        PendingOutput(&self.pending_output)
    }

    /// Optional feedback never queues behind speech and yields to necessary output.
    pub async fn play_optional_frame(
        &self,
        frame: Vec<u8>,
        expected_gen: u64,
        still_needed: impl Fn() -> bool + Send,
        started: impl FnOnce() + Send,
    ) -> Result<bool, AudioSinkError> {
        let Ok(_output) = self.output.try_lock() else {
            return Ok(false);
        };
        let eligible = || {
            self.generation_id() == expected_gen
                && self.pending_output.load(Ordering::SeqCst) == 0
                && still_needed()
        };
        if !eligible() {
            return Ok(false);
        }
        if !self.sink.is_available() {
            return Err(AudioSinkError::Write("speaker_unavailable".into()));
        }
        self.sink.write_frame(frame).await?;
        if !eligible() {
            self.sink.flush().await?;
            return Ok(false);
        }
        started();
        let drain = self.sink.drain();
        tokio::pin!(drain);
        let timeout = tokio::time::sleep(Duration::from_secs(2));
        tokio::pin!(timeout);
        loop {
            if !eligible() {
                self.sink.flush().await?;
                return Ok(false);
            }
            tokio::select! {
                result = &mut drain => return result.map(|_| true),
                _ = &mut timeout => {
                    self.sink.flush().await?;
                    return Err(AudioSinkError::Write("earcon_drain_timeout".into()));
                }
                _ = tokio::time::sleep(Duration::from_millis(10)) => {}
            }
        }
    }

    /// Increments generation_id and flushes audio sink to instantly cancel playback (<50ms barge-in).
    pub async fn interrupt(&self) -> Result<(), AudioSinkError> {
        self.generation_id.fetch_add(1, Ordering::SeqCst);
        // Flush before releasing the old owner; a new output must not be cleared by this flush.
        let result = self.sink.flush().await;
        self.interruption.send_replace(self.generation_id());
        result
    }

    /// Plays an earcon sound asset through the audio sink.
    pub async fn play_earcon(&self, kind: EarconKind) -> Result<(), AudioSinkError> {
        self.play_earcon_with_started(kind, self.generation_id(), || {})
            .await
    }

    pub async fn play_earcon_with_started(
        &self,
        kind: EarconKind,
        expected_gen: u64,
        started: impl FnOnce() + Send,
    ) -> Result<(), AudioSinkError> {
        self.play_cue_frame_with_started(kind.as_wav_bytes().to_vec(), expected_gen, started)
            .await
    }

    pub async fn play_cue_frame_with_started(
        &self,
        frame: Vec<u8>,
        expected_gen: u64,
        started: impl FnOnce() + Send,
    ) -> Result<(), AudioSinkError> {
        let _intent = self.request_output();
        let _output = self.output.lock().await;
        if self.generation_id() != expected_gen {
            return Ok(());
        }
        if !self.sink.is_available() {
            return Err(AudioSinkError::Write("speaker_unavailable".into()));
        }
        self.sink.write_frame(frame).await?;
        if self.generation_id() != expected_gen {
            // This guard still owns output: only its late write can be queued here.
            self.sink.flush().await?;
            return Ok(());
        }
        started();
        self.wait_for_consumption(expected_gen, Duration::from_secs(2), "earcon_drain_timeout")
            .await
    }

    async fn wait_for_consumption(
        &self,
        expected_gen: u64,
        budget: Duration,
        timeout_code: &'static str,
    ) -> Result<(), AudioSinkError> {
        let drain = self.sink.drain();
        tokio::pin!(drain);
        let timeout = tokio::time::sleep(budget);
        tokio::pin!(timeout);
        loop {
            if self.generation_id() != expected_gen {
                // interrupt already flushed this generation; never flush a newer owner.
                return Ok(());
            }
            tokio::select! {
                result = &mut drain => return result,
                _ = &mut timeout => {
                    if self.generation_id() == expected_gen {
                        self.sink.flush().await?;
                    }
                    return Err(AudioSinkError::Write(timeout_code.into()));
                }
                _ = tokio::time::sleep(Duration::from_millis(10)) => {}
            }
        }
    }

    /// Consumes incoming audio chunk stream and forwards frames to audio sink.
    pub async fn play_stream(
        &self,
        mut stream: mpsc::Receiver<Vec<u8>>,
    ) -> Result<(), AudioSinkError> {
        let expected_gen = self.generation_id();
        let _intent = self.request_output();
        let _output = self.output.lock().await;

        while let Some(chunk) = stream.recv().await {
            if self.generation_id() != expected_gen {
                break;
            }
            if !chunk.is_empty() {
                self.sink.write_frame(chunk).await?;
                if self.generation_id() != expected_gen {
                    self.sink.flush().await?;
                    return Ok(());
                }
            }
        }

        Ok(())
    }

    /// Speaks synthesized text through the audio sink using the provided TTS provider.
    /// Interruption-safe: flushes the sink and aborts immediately if generation_id increments.
    pub async fn speak_text(
        &self,
        tts: &dyn crate::tts::TtsProvider,
        text: &str,
        voice: &str,
    ) -> Result<(), AudioSinkError> {
        self.speak_text_with_started(tts, text, voice, || {}).await
    }

    fn synthesis_error(error:crate::tts::TtsError)->AudioSinkError {
        match error {
            crate::tts::TtsError::RateLimited{status,retry_after}=>AudioSinkError::RateLimited{status,retry_after},
            other=>AudioSinkError::Synthesis(other.to_string()),
        }
    }

    /// One reply owns output and drains once; only one subsequent synthesis is prefetched.
    pub async fn speak_sentence_stream(
        &self,tts:&dyn crate::tts::TtsProvider,sentences:mpsc::Receiver<String>,expected_gen:u64,
        cancelled:tokio::sync::watch::Receiver<bool>,started:impl FnOnce()+Send,
    )->Result<(),AudioSinkError> {
        self.speak_sentence_stream_with_voice(tts, sentences, expected_gen, cancelled, "", started).await
    }

    /// Speaks sentence stream with a specific voice selection (e.g. for locale-specific TTS).
    pub async fn speak_sentence_stream_with_voice(
        &self,tts:&dyn crate::tts::TtsProvider,mut sentences:mpsc::Receiver<String>,expected_gen:u64,
        mut cancelled:tokio::sync::watch::Receiver<bool>,voice:&str,started:impl FnOnce()+Send,
    )->Result<(),AudioSinkError> {
        let (audio,mut ready)=mpsc::channel(1);
        let synthesize=async {
            let mut failed=false;
            while let Some(text)=sentences.recv().await {
                if self.generation_id()!=expected_gen {break;}
                if failed {continue;}
                let Ok(permit)=audio.reserve().await else {break;};
                let began=std::time::Instant::now();
                let result=tts.synthesize_stream(&text,voice).await.map_err(Self::synthesis_error);
                tracing::info!(generation=expected_gen,chars=text.chars().count(),elapsed_ms=began.elapsed().as_millis() as u64,ok=result.is_ok(),"voice_sentence_synthesis_finished");
                failed=result.is_err();permit.send(result);
            }
            drop(audio);
        };
        let consume=async {
            let mut started=Some(started);let mut failure=None;let mut has_audio=false;
            let mut output=None;let mut pending=None;
            while let Some(result)=ready.recv().await {
                let mut stream=match result {Ok(stream)=>stream,Err(error)=>{if failure.is_none(){failure=Some(error);}continue;}};
                let mut segment_audio=false;
                while let Some(chunk)=stream.recv().await {
                    if self.generation_id()!=expected_gen {return Ok(());}
                    if chunk.is_empty(){continue;}segment_audio=true;
                    if failure.is_some(){continue;}
                    if !self.sink.is_available(){failure=Some(AudioSinkError::Write("speaker_unavailable".into()));continue;}
                    if output.is_none(){pending=Some(self.request_output());output=Some(self.output.lock().await);}
                    if let Err(error)=self.sink.write_frame(chunk).await {failure=Some(error);continue;}
                    if self.generation_id()!=expected_gen {self.sink.flush().await?;return Ok(());}
                    has_audio=true;if let Some(started)=started.take(){started();}
                }
                if !segment_audio && failure.is_none(){failure=Some(AudioSinkError::NoAudio);}
            }
            let result=if let Some(error)=failure {Err(error)}else if has_audio {
                self.wait_for_consumption(expected_gen,Duration::from_secs(60),"speaker_drain_timeout").await
            }else{Ok(())};
            tracing::info!(generation=expected_gen,has_audio,ok=result.is_ok(),"voice_sentence_playback_finished");
            drop(output);drop(pending);result
        };
        let work=async {let((),result)=tokio::join!(synthesize,consume);result};
        tokio::select!{
            biased;
            _=self.interrupted(expected_gen)=>Ok(()),
            result=work=>result,
            _=cancelled.changed()=>Err(AudioSinkError::Write("model_stream_failed".into())),
        }
    }

    /// Notify only after the first synthesized frame reaches the output sink.
    pub async fn speak_text_with_started(
        &self,
        tts: &dyn crate::tts::TtsProvider,
        text: &str,
        voice: &str,
        started: impl FnOnce() + Send,
    ) -> Result<(), AudioSinkError> {
        let expected_gen = self.generation_id();
        self.speak_text_for_generation(tts, text, voice, expected_gen, started).await
    }

    pub async fn speak_text_for_generation(
        &self,
        tts: &dyn crate::tts::TtsProvider,
        text: &str,
        voice: &str,
        expected_gen: u64,
        started: impl FnOnce() + Send,
    ) -> Result<(), AudioSinkError> {
        tokio::select! {
            biased;
            _ = self.interrupted(expected_gen) => Ok(()),
            result = self.speak_text_generation(tts, text, voice, expected_gen, started) => result,
        }
    }

    async fn speak_text_generation(
        &self,
        tts: &dyn crate::tts::TtsProvider,
        text: &str,
        voice: &str,
        expected_gen: u64,
        started: impl FnOnce() + Send,
    ) -> Result<(), AudioSinkError> {
        if !self.sink.is_available() {
            return Err(AudioSinkError::Write("speaker_unavailable".into()));
        }
        let mut stream = tts
            .synthesize_stream(text, voice)
            .await
            .map_err(Self::synthesis_error)?;
        let mut has_audio = false;
        let mut started = Some(started);
        let _intent = self.request_output();
        // Synthesis may wait on the network while short status cues use the output.
        let _output = self.output.lock().await;

        while let Some(chunk) = stream.recv().await {
            if self.generation_id() != expected_gen {
                // interrupt() already flushed the cancelled generation. A late
                // synthesis result must not flush a newer reply's audio.
                return Ok(());
            }
            if !chunk.is_empty() {
                has_audio = true;
                self.sink.write_frame(chunk).await?;
                if self.generation_id() != expected_gen {
                    self.sink.flush().await?;
                    return Ok(());
                }
                if let Some(started) = started.take() {
                    started();
                }
            }
        }

        if !has_audio {
            return Err(AudioSinkError::NoAudio);
        }
        self.wait_for_consumption(
            expected_gen,
            Duration::from_secs(60),
            "speaker_drain_timeout",
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    #[derive(Default)]
    struct CountingSink {
        queued: std::sync::Mutex<usize>,
        consumed: std::sync::Mutex<usize>,
        discarded: std::sync::Mutex<usize>,
        drain_error: bool,
        hang_drain: bool,
        slow_write_ms: u64,
    }

    #[async_trait::async_trait]
    impl AudioSink for CountingSink {
        fn has_pending_audio(&self) -> bool {
            *self.queued.lock().unwrap() > 0
        }
        async fn write_frame(&self, frame: Vec<u8>) -> Result<(), AudioSinkError> {
            if self.slow_write_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(self.slow_write_ms)).await;
            }
            *self.queued.lock().unwrap() += frame.len();
            Ok(())
        }
        async fn drain(&self) -> Result<(), AudioSinkError> {
            if self.hang_drain {
                std::future::pending::<()>().await;
            }
            if self.drain_error {
                return Err(AudioSinkError::Write("drain_failed".into()));
            }
            let mut queued = self.queued.lock().unwrap();
            *self.consumed.lock().unwrap() += *queued;
            *queued = 0;
            Ok(())
        }
        async fn flush(&self) -> Result<(), AudioSinkError> {
            let mut queued = self.queued.lock().unwrap();
            *self.discarded.lock().unwrap() += *queued;
            *queued = 0;
            Ok(())
        }
    }

    #[tokio::test]
    async fn self_check_playback_waits_for_real_drain_consumption() {
        let sink = Arc::new(CountingSink::default());
        let pipeline = PlaybackPipeline::new(sink.clone());
        pipeline
            .play_wav_for_self_check_with_timeout(
                vec![1; 100],
                std::time::Duration::from_secs(2),
            )
            .await
            .unwrap();
        assert_eq!(*sink.consumed.lock().unwrap(), 100);
        assert_eq!(*sink.discarded.lock().unwrap(), 0);
        assert_eq!(*sink.queued.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn self_check_playback_fails_closed_on_drain_error_or_timeout() {
        for sink in [
            CountingSink { drain_error: true, ..Default::default() },
            CountingSink { hang_drain: true, ..Default::default() },
        ] {
            let sink = Arc::new(sink);
            let pipeline = PlaybackPipeline::new(sink.clone());
            let result = pipeline
                .play_wav_for_self_check_with_timeout(
                    vec![1; 100],
                    std::time::Duration::from_millis(50),
                )
                .await;
            assert!(result.is_err(), "an invalid drain must not report success");
            assert_eq!(*sink.consumed.lock().unwrap(), 0);
            assert_eq!(*sink.discarded.lock().unwrap(), 100);
        }
    }

    #[tokio::test]
    async fn self_check_playback_cancel_is_not_success() {
        let sink = Arc::new(CountingSink {
            slow_write_ms: 50,
            ..Default::default()
        });
        let pipeline = Arc::new(PlaybackPipeline::new(sink.clone()));
        let play = pipeline.clone();
        let task = tokio::spawn(async move { play.play_wav_for_self_check(vec![1; 100]).await });
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        pipeline.interrupt().await.unwrap();
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
        assert!(result.is_err(), "cancelled playback must not report sink_consumed");
    }
    use super::*;
    use crate::tts::NullTts;

    #[tokio::test]
    async fn interruption_releases_a_stalled_sentence_stream_fifty_times() {
        let sink = Arc::new(TestAudioSink::new());
        let playback = Arc::new(PlaybackPipeline::new(sink.clone()));
        for _ in 0..50 {
            let (sentences, rx) = mpsc::channel(1);
            let (_cancel, cancel_rx) = tokio::sync::watch::channel(false);
            sentences.send("unfinished old reply".into()).await.unwrap();
            let generation = playback.generation_id();
            let output = playback.clone();
            let old = tokio::spawn(async move {
                output.speak_sentence_stream(&NullTts::new(), rx, generation, cancel_rx, || {}).await
            });
            tokio::time::timeout(Duration::from_secs(1), async {
                while sink.frame_count() == 0 { tokio::task::yield_now().await; }
            }).await.unwrap();
            playback.interrupt().await.unwrap();
            tokio::time::timeout(Duration::from_millis(200), old).await.unwrap().unwrap().unwrap();
            assert!(sentences.is_closed(), "old synthesis and model input must be dropped");
            assert_eq!(sink.frame_count(), 0);
        }
        let old_generation = playback.generation_id();
        playback.interrupt().await.unwrap();
        playback.speak_text_for_generation(&NullTts::new(), "late old reply", "", old_generation, || panic!("stale speech started")).await.unwrap();
        assert_eq!(sink.frame_count(), 0);
        playback.speak_text(&NullTts::new(), "new reply", "").await.unwrap();
        assert_eq!(sink.frame_count(), 1);
    }

    struct DelayedTts {
        entered: tokio::sync::Notify,
        released: tokio::sync::Notify,
    }
    #[async_trait]
    impl crate::tts::TtsProvider for DelayedTts {
        async fn synthesize_stream(
            &self,
            _: &str,
            _: &str,
        ) -> Result<mpsc::Receiver<Vec<u8>>, crate::tts::TtsError> {
            self.entered.notify_one();
            self.released.notified().await;
            let (tx, rx) = mpsc::channel(1);
            tx.send(vec![1, 2]).await.unwrap();
            Ok(rx)
        }
    }
    #[tokio::test]
    async fn late_cancelled_synthesis_cannot_flush_a_new_reply() {
        let sink = Arc::new(TestAudioSink::new());
        let playback = Arc::new(PlaybackPipeline::new(sink.clone()));
        let tts = Arc::new(DelayedTts {
            entered: tokio::sync::Notify::new(),
            released: tokio::sync::Notify::new(),
        });
        let job = {
            let playback = playback.clone();
            let tts = tts.clone();
            tokio::spawn(async move { playback.speak_text(tts.as_ref(), "old", "").await })
        };
        tokio::time::timeout(Duration::from_secs(1), tts.entered.notified())
            .await
            .unwrap();
        playback.interrupt().await.unwrap();
        sink.write_frame(vec![3, 4]).await.unwrap();
        tts.released.notify_one();
        assert!(job.await.unwrap().is_ok());
        assert_eq!(sink.frames(), vec![vec![3, 4]]);
    }

    struct WaitingSink {
        entered: tokio::sync::Notify,
        released: tokio::sync::Notify,
    }
    #[async_trait]
    impl AudioSink for WaitingSink {
        async fn write_frame(&self, _: Vec<u8>) -> Result<(), AudioSinkError> {
            Ok(())
        }
        async fn flush(&self) -> Result<(), AudioSinkError> {
            Ok(())
        }
        async fn drain(&self) -> Result<(), AudioSinkError> {
            self.entered.notify_one();
            self.released.notified().await;
            Ok(())
        }
    }
    #[tokio::test]
    async fn earcon_completion_waits_for_consumption_and_interrupt_releases_it() {
        let sink = Arc::new(WaitingSink {
            entered: tokio::sync::Notify::new(),
            released: tokio::sync::Notify::new(),
        });
        let playback = Arc::new(PlaybackPipeline::new(sink.clone()));
        let playing = playback.clone();
        let job = tokio::spawn(async move { playing.play_earcon(EarconKind::Wake).await });
        tokio::time::timeout(Duration::from_millis(500), sink.entered.notified())
            .await
            .expect("earcon must wait for output consumption, not just enqueue");
        assert!(!job.is_finished());
        playback.interrupt().await.unwrap();
        tokio::time::timeout(Duration::from_millis(200), job)
            .await
            .expect("cancelled cue must release the output without waiting for old drain")
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn interrupt_releases_reply_consumption_before_the_next_output() {
        let sink = Arc::new(WaitingSink {
            entered: tokio::sync::Notify::new(),
            released: tokio::sync::Notify::new(),
        });
        let playback = Arc::new(PlaybackPipeline::new(sink.clone()));
        let playing = playback.clone();
        let old_reply =
            tokio::spawn(async move { playing.speak_text(&NullTts::new(), "旧回复", "").await });
        tokio::time::timeout(Duration::from_secs(1), sink.entered.notified())
            .await
            .unwrap();
        playback.interrupt().await.unwrap();
        tokio::time::timeout(Duration::from_millis(200), old_reply)
            .await
            .expect("stopping a reply must not leave its drain holding the next output")
            .unwrap()
            .unwrap();
        let playing = playback.clone();
        let new_cue = tokio::spawn(async move { playing.play_earcon(EarconKind::Wake).await });
        tokio::time::timeout(Duration::from_millis(200), sink.entered.notified())
            .await
            .unwrap();
        sink.released.notify_one();
        new_cue.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn cancelled_write_cannot_enqueue_an_old_cue_after_flush() {
        struct DeferredSink {
            frames: std::sync::Mutex<Vec<Vec<u8>>>,
            entered: tokio::sync::Notify,
            release: tokio::sync::Notify,
            defer: std::sync::atomic::AtomicBool,
        }
        #[async_trait]
        impl AudioSink for DeferredSink {
            async fn write_frame(&self, frame: Vec<u8>) -> Result<(), AudioSinkError> {
                if self.defer.swap(false, Ordering::SeqCst) {
                    self.entered.notify_one();
                    self.release.notified().await;
                }
                self.frames.lock().unwrap().push(frame);
                Ok(())
            }
            async fn flush(&self) -> Result<(), AudioSinkError> {
                self.frames.lock().unwrap().clear();
                Ok(())
            }
        }
        let sink = Arc::new(DeferredSink {
            frames: std::sync::Mutex::new(Vec::new()),
            entered: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
            defer: std::sync::atomic::AtomicBool::new(true),
        });
        let playback = Arc::new(PlaybackPipeline::new(sink.clone()));
        let playing = playback.clone();
        let old = tokio::spawn(async move { playing.play_earcon(EarconKind::Wake).await });
        tokio::time::timeout(Duration::from_secs(1), sink.entered.notified())
            .await
            .unwrap();
        playback.interrupt().await.unwrap();
        let playing = playback.clone();
        let new = tokio::spawn(async move { playing.play_earcon(EarconKind::Sleep).await });
        sink.release.notify_one();
        old.await.unwrap().unwrap();
        new.await.unwrap().unwrap();
        let frames = sink.frames.lock().unwrap();
        assert_eq!(
            frames.len(),
            1,
            "a cancelled late write must be cleared before the next output"
        );
        assert!(
            frames[0].as_slice() == EarconKind::Sleep.as_wav_bytes(),
            "only the new cue may remain; do not print PCM in failure diagnostics"
        );
    }

    #[tokio::test]
    async fn optional_feedback_yields_output_to_a_reply_without_cancelling_its_generation() {
        let sink = Arc::new(WaitingSink {
            entered: tokio::sync::Notify::new(),
            released: tokio::sync::Notify::new(),
        });
        let playback = Arc::new(PlaybackPipeline::new(sink.clone()));
        let generation = playback.generation_id();
        let optional = playback.clone();
        let cue = tokio::spawn(async move {
            optional
                .play_optional_frame(
                    EarconKind::Guide.as_wav_bytes().to_vec(),
                    generation,
                    || true,
                    || {},
                )
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), sink.entered.notified())
            .await
            .unwrap();
        let required = playback.clone();
        let reply =
            tokio::spawn(async move { required.speak_text(&NullTts::new(), "真实回复", "").await });
        assert!(
            !tokio::time::timeout(Duration::from_millis(200), cue)
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
            "optional cue must be preempted when real speech is ready"
        );
        tokio::time::timeout(Duration::from_millis(200), sink.entered.notified())
            .await
            .unwrap();
        assert_eq!(
            playback.generation_id(),
            generation,
            "preemption is not a voice-session cancellation"
        );
        sink.released.notify_one();
        reply.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn optional_feedback_is_skipped_instead_of_queued_behind_speech() {
        let sink = Arc::new(WaitingSink {
            entered: tokio::sync::Notify::new(),
            released: tokio::sync::Notify::new(),
        });
        let playback = Arc::new(PlaybackPipeline::new(sink.clone()));
        let required = playback.clone();
        let reply =
            tokio::spawn(async move { required.speak_text(&NullTts::new(), "真实回复", "").await });
        tokio::time::timeout(Duration::from_secs(1), sink.entered.notified())
            .await
            .unwrap();
        let cue = tokio::time::timeout(
            Duration::from_millis(100),
            playback.play_optional_frame(
                EarconKind::Guide.as_wav_bytes().to_vec(),
                playback.generation_id(),
                || true,
                || panic!("a skipped cue must not claim it started"),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(!cue);
        sink.released.notify_one();
        reply.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn completion_waits_for_the_sink_to_consume_audio() {
        let sink = Arc::new(WaitingSink {
            entered: tokio::sync::Notify::new(),
            released: tokio::sync::Notify::new(),
        });
        let playback = Arc::new(PlaybackPipeline::new(sink.clone()));
        let job =
            tokio::spawn(async move { playback.speak_text(&NullTts::new(), "真实回复", "").await });
        tokio::time::timeout(Duration::from_secs(1), sink.entered.notified())
            .await
            .unwrap();
        assert!(!job.is_finished());
        sink.released.notify_one();
        assert!(job.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn test_speak_text_success() {
        let sink = Arc::new(TestAudioSink::new());
        let pipeline = PlaybackPipeline::new(sink.clone());
        let tts = NullTts::new();

        pipeline
            .speak_text(&tts, "你好，我是 Flowy 伴侣", "zh-CN-XiaoxiaoNeural")
            .await
            .unwrap();

        assert_eq!(sink.frame_count(), 1);
        let frames = sink.frames();
        assert_eq!(frames[0], "你好，我是 Flowy 伴侣".as_bytes());
    }

    #[tokio::test]
    async fn test_speak_text_interrupted() {
        let sink = Arc::new(TestAudioSink::new());
        let pipeline = PlaybackPipeline::new(sink.clone());

        pipeline.interrupt().await.unwrap();
        assert_eq!(pipeline.generation_id(), 1);
    }

    struct RateLimitedTts;
    #[async_trait]
    impl crate::tts::TtsProvider for RateLimitedTts {
        async fn synthesize_stream(
            &self,
            _: &str,
            _: &str,
        ) -> Result<mpsc::Receiver<Vec<u8>>, crate::tts::TtsError> {
            Err(crate::tts::TtsError::RateLimited {
                status: 429,
                retry_after: Some(Duration::from_secs(5)),
            })
        }
    }

    #[tokio::test]
    async fn test_speak_text_rate_limited_propagates_error() {
        let sink = Arc::new(TestAudioSink::new());
        let pipeline = PlaybackPipeline::new(sink);
        let tts = RateLimitedTts;
        let err = pipeline.speak_text(&tts, "你好", "").await.unwrap_err();
        match err {
            AudioSinkError::RateLimited {
                status,
                retry_after,
            } => {
                assert_eq!(status, 429);
                assert_eq!(retry_after, Some(Duration::from_secs(5)));
            }
            other => panic!("expected RateLimited, got {other:?}"),
        }
    }

    struct CapturingTts {
        captured_voice: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    }
    #[async_trait]
    impl crate::tts::TtsProvider for CapturingTts {
        async fn synthesize_stream(
            &self,
            _text: &str,
            voice: &str,
        ) -> Result<mpsc::Receiver<Vec<u8>>, crate::tts::TtsError> {
            *self.captured_voice.lock().unwrap() = Some(voice.to_string());
            let (tx, rx) = mpsc::channel(1);
            tx.send(vec![1, 2, 3]).await.unwrap();
            Ok(rx)
        }
    }

    #[tokio::test]
    async fn test_speak_sentence_stream_with_voice_passes_voice() {
        let sink = Arc::new(TestAudioSink::new());
        let pipeline = PlaybackPipeline::new(sink);
        let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
        let tts = CapturingTts {
            captured_voice: captured.clone(),
        };
        let (tx, rx) = mpsc::channel(1);
        tx.send("Hello world".to_string()).await.unwrap();
        drop(tx);
        let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let expected_gen = pipeline.generation_id();
        pipeline
            .speak_sentence_stream_with_voice(&tts, rx, expected_gen, cancel_rx, "en-US-JennyNeural", || {})
            .await
            .unwrap();
        assert_eq!(
            captured.lock().unwrap().as_deref(),
            Some("en-US-JennyNeural")
        );
    }
}
