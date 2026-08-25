//! Live capture pipeline controller for meeting start/pause/resume/stop.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use dashmap::DashMap;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::capture::AudioCaptureSource;
use crate::diarization::{SpeakerAssigner, SpeakerSpan};
use crate::encode::encode_track_default;
use crate::frame::{AudioChannel, TaggedFrame};
use crate::loopback::LoopbackSource;
use crate::mic::MicSource;
use crate::mixer::DualTrackMixer;
use crate::recorder::{MeetingRecorder, TranscriptSegment};
use crate::session::{
    MeetingSegmentSnapshot, MeetingSessionService, MeetingSessionSnapshot, MeetingSessionStatus,
    SttBackendChoice,
};
use crate::stt::build_switchable_stt;
use crate::vad::VadConfig;

const TARGET_SAMPLE_RATE: u32 = 16_000;

struct LiveMeeting {
    recorder: Arc<MeetingRecorder>,
    pcm_mic: Arc<Mutex<Vec<f32>>>,
    pcm_loopback: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    data_dir: PathBuf,
    _record_join: JoinHandle<()>,
    _segment_join: JoinHandle<()>,
    _tee_join: JoinHandle<()>,
}

/// Owns in-process capture/recorder tasks keyed by session id.
#[derive(Clone)]
pub struct MeetingRuntime {
    service: MeetingSessionService,
    live: Arc<DashMap<String, LiveMeeting>>,
}

impl MeetingRuntime {
    pub fn new(service: MeetingSessionService) -> Self {
        Self {
            service,
            live: Arc::new(DashMap::new()),
        }
    }

    pub fn service(&self) -> &MeetingSessionService {
        &self.service
    }

    /// Best-effort start: capture failures degrade (V1) or mark failed — never panics.
    pub async fn start(
        &self,
        session: MeetingSessionSnapshot,
    ) -> Result<MeetingSessionSnapshot, String> {
        if self.live.contains_key(&session.session_id) {
            return Err("meeting session already recording".into());
        }

        let mic_result = MicSource::new(TARGET_SAMPLE_RATE);
        let loopback_result = LoopbackSource::new(TARGET_SAMPLE_RATE);

        let mic_available = mic_result.is_ok();
        let loopback_available = loopback_result.is_ok();

        if !mic_available && !loopback_available {
            let mic_err = mic_result.err().unwrap_or_else(|| "mic unavailable".into());
            let lb_err = loopback_result
                .err()
                .unwrap_or_else(|| "loopback unavailable".into());
            let message = format!("capture failed: mic={mic_err}; loopback={lb_err}");
            let _ = self
                .service
                .update_capture_availability(&session.session_id, false, false)
                .await;
            self.service.publish_capability_degraded(
                &session.session_id,
                false,
                false,
                &message,
            );
            self.service
                .publish_error(&session.session_id, &message);
            return self
                .service
                .set_status(&session.session_id, MeetingSessionStatus::Failed)
                .await;
        }

        if !mic_available || !loopback_available {
            let message = match (&mic_result, &loopback_result) {
                (Err(e), Ok(_)) => format!("mic unavailable, continuing loopback-only: {e}"),
                (Ok(_), Err(e)) => format!("loopback unavailable, continuing mic-only: {e}"),
                _ => "partial capture degradation".into(),
            };
            self.service.publish_capability_degraded(
                &session.session_id,
                mic_available,
                loopback_available,
                &message,
            );
        }

        let _ = self
            .service
            .update_capture_availability(
                &session.session_id,
                mic_available,
                loopback_available,
            )
            .await;

        let mixer = match (mic_result, loopback_result) {
            (Ok(mic), Ok(lb)) => DualTrackMixer::new(
                Arc::new(mic) as Arc<dyn AudioCaptureSource>,
                Arc::new(lb) as Arc<dyn AudioCaptureSource>,
            ),
            (Ok(mic), Err(_)) => {
                DualTrackMixer::mic_only(Arc::new(mic) as Arc<dyn AudioCaptureSource>)
            }
            (Err(_), Ok(lb)) => {
                DualTrackMixer::loopback_only(Arc::new(lb) as Arc<dyn AudioCaptureSource>)
            }
            (Err(_), Err(_)) => unreachable!("both capture failures handled above"),
        };

        let stt = build_switchable_stt(session.stt_backend, None, None);
        if matches!(
            session.stt_backend,
            SttBackendChoice::CloudModelInvoke | SttBackendChoice::Auto
        ) {
            // Cloud STT host is not wired yet; Switchable falls back to Null.
            info!(
                session_id = %session.session_id,
                backend = session.stt_backend.as_str(),
                "meeting STT: cloud hook not attached; using available local/null backend"
            );
        }

        let recorder = Arc::new(MeetingRecorder::new(VadConfig::for_meeting(), stt));
        let frames_rx = mixer.into_stream(64);

        let pcm_mic = Arc::new(Mutex::new(Vec::<f32>::new()));
        let pcm_loopback = Arc::new(Mutex::new(Vec::<f32>::new()));
        let (tee_tx, tee_rx) = mpsc::channel::<TaggedFrame>(64);
        let tee_join = spawn_pcm_tee(
            frames_rx,
            tee_tx,
            Arc::clone(&pcm_mic),
            Arc::clone(&pcm_loopback),
        );

        let (mut seg_rx, record_join) = recorder.record(tee_rx);

        let service = self.service.clone();
        let session_id = session.session_id.clone();
        let assigner = Arc::new(Mutex::new(SpeakerAssigner::new()));
        let segment_join = tokio::spawn(async move {
            while let Some(seg) = seg_rx.recv().await {
                if let Err(e) = persist_transcript_segment(
                    &service,
                    &session_id,
                    &assigner,
                    seg,
                )
                .await
                {
                    warn!(error = %e, "meeting segment upsert failed");
                }
            }
        });

        let data_dir = PathBuf::from(&session.data_dir);
        self.live.insert(
            session.session_id.clone(),
            LiveMeeting {
                recorder,
                pcm_mic,
                pcm_loopback,
                sample_rate: TARGET_SAMPLE_RATE,
                data_dir,
                _record_join: record_join,
                _segment_join: segment_join,
                _tee_join: tee_join,
            },
        );

        self.service
            .set_status(&session.session_id, MeetingSessionStatus::Recording)
            .await
    }

    pub async fn pause(&self, session_id: &str) -> Result<MeetingSessionSnapshot, String> {
        let Some(live) = self.live.get(session_id) else {
            return Err("meeting session is not recording".into());
        };
        live.recorder.pause();
        drop(live);
        self.service
            .set_status(session_id, MeetingSessionStatus::Paused)
            .await
    }

    pub async fn resume(&self, session_id: &str) -> Result<MeetingSessionSnapshot, String> {
        let Some(live) = self.live.get(session_id) else {
            return Err("meeting session is not recording".into());
        };
        live.recorder.resume();
        drop(live);
        self.service
            .set_status(session_id, MeetingSessionStatus::Recording)
            .await
    }

    pub async fn stop(&self, session_id: &str) -> Result<MeetingSessionSnapshot, String> {
        let Some((_, live)) = self.live.remove(session_id) else {
            // Idempotent stop: still flip status if row exists.
            return self
                .service
                .set_status(session_id, MeetingSessionStatus::Stopped)
                .await;
        };

        let _ = self
            .service
            .set_status(session_id, MeetingSessionStatus::Stopping)
            .await;

        live.recorder.stop();

        persist_tracks(
            &live.data_dir,
            &live.pcm_mic,
            &live.pcm_loopback,
            live.sample_rate,
        );

        self.service
            .set_status(session_id, MeetingSessionStatus::Stopped)
            .await
    }

    /// First live session id, if any (desktop tray / global-shortcut target).
    pub fn first_live_session_id(&self) -> Option<String> {
        self.live.iter().next().map(|entry| entry.key().clone())
    }

    /// Whether the live recorder for `session_id` is currently paused.
    pub fn is_live_paused(&self, session_id: &str) -> Option<bool> {
        self.live
            .get(session_id)
            .map(|live| live.recorder.is_paused())
    }
}

fn spawn_pcm_tee(
    mut frames_rx: mpsc::Receiver<TaggedFrame>,
    tee_tx: mpsc::Sender<TaggedFrame>,
    pcm_mic: Arc<Mutex<Vec<f32>>>,
    pcm_loopback: Arc<Mutex<Vec<f32>>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(frame) = frames_rx.recv().await {
            if !frame.samples.is_empty() {
                let sink = match frame.channel {
                    AudioChannel::Mic => &pcm_mic,
                    AudioChannel::Loopback => &pcm_loopback,
                };
                if let Ok(mut buf) = sink.lock() {
                    buf.extend_from_slice(&frame.samples);
                }
            }
            if tee_tx.send(frame).await.is_err() {
                break;
            }
        }
    })
}

async fn persist_transcript_segment(
    service: &MeetingSessionService,
    session_id: &str,
    assigner: &Arc<Mutex<SpeakerAssigner>>,
    seg: TranscriptSegment,
) -> Result<(), String> {
    let channel = if seg.speaker.contains('B') || seg.speaker.to_lowercase().contains("loopback")
    {
        AudioChannel::Loopback
    } else {
        AudioChannel::Mic
    };
    let start_ms = (seg.start_s * 1000.0) as i64;
    let end_ms = (seg.end_s * 1000.0) as i64;
    let key = channel.label().to_string();
    let spans = vec![SpeakerSpan {
        speaker_key: key.clone(),
        start_ms,
        end_ms,
    }];

    let snapshot = {
        let mut guard = assigner
            .lock()
            .map_err(|_| "speaker assigner lock poisoned".to_string())?;
        guard.bind_keys([key], &HashMap::new());
        let base = MeetingSegmentSnapshot {
            segment_id: seg.segment_id,
            session_id: session_id.to_string(),
            channel: Some(channel),
            speaker_id: None,
            speaker_label: String::new(),
            text: seg.text,
            is_partial: seg.is_partial,
            is_manual_edit: false,
            start_ms,
            end_ms,
        };
        guard.assign_segment(base, &spans)
    };

    service.upsert_segment(snapshot).await.map(|_| ())
}

fn persist_tracks(
    data_dir: &Path,
    pcm_mic: &Arc<Mutex<Vec<f32>>>,
    pcm_loopback: &Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
) {
    if let Ok(mic) = pcm_mic.lock() {
        if !mic.is_empty() {
            match encode_track_default(&data_dir.join("mic"), &mic, sample_rate) {
                Ok(path) => info!(path = %path.display(), "wrote meeting mic track"),
                Err(e) => warn!(error = %e, "encode mic track failed"),
            }
        }
    }
    if let Ok(lb) = pcm_loopback.lock() {
        if !lb.is_empty() {
            match encode_track_default(&data_dir.join("loopback"), &lb, sample_rate) {
                Ok(path) => info!(path = %path.display(), "wrote meeting loopback track"),
                Err(e) => warn!(error = %e, "encode loopback track failed"),
            }
        }
    }
}
