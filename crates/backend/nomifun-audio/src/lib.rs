//! Desktop meeting audio capture abstractions.
//!
//! Shared by the meeting recorder pipeline and later voice surfaces:
//!
//! ```text
//!   MicSource          LoopbackSource
//!       └──────┬────────────┘
//!           DualTrackMixer (TaggedFrame)
//!                 │
//!         SilenceGuard
//!                 │
//!           VAD (Energy / Silero)
//!                 │
//!            STT callback
//!                 │
//!         TranscriptSegment
//! ```

pub mod capture;
pub mod devices;
pub mod diagnostics;
pub mod diarization;
pub mod earcon;
pub mod encode;
pub mod events;
pub mod frame;
pub mod hotplug;
pub mod keepawake;
pub mod kws;
pub mod loopback;
pub mod mic;
pub mod mixer;
pub(crate) mod pcm_util;
pub mod process_watch;
pub mod recorder;
pub mod routes;
pub mod runtime;
pub mod session;
pub mod speaker;
pub mod stt;
pub mod vad;
pub mod voice_pipeline;
pub mod voiceprint;

pub use capture::{AudioCaptureSource, CaptureReadResult};
pub use speaker::{open_default_speaker, parse_audio_frame_to_pcm, SpeakerAudioSink};
pub use devices::{AudioDeviceInfo, AudioDeviceManager, DeviceKind};
pub use diagnostics::{voice_diagnostics_routes, voice_diagnostics_routes_with_dir, VoiceDiagnosticsState};
pub use diarization::{
    Diarizer, EnergyClusterDiarizer, EnergyClusterDiarizerConfig, SpeakerAssigner, SpeakerIdentity,
    SpeakerSpan, create_diarizer, dominant_speaker_key,
};
pub use earcon::{
    ALERT_WAV, DONE_WAV, ERROR_WAV, Earcon, EarconKind, GUIDE_WAV, SLEEP_WAV, WAKE_WAV,
};
pub use encode::{encode_track_default, pcm_to_m4a, pcm_to_wav, write_track_m4a};
pub use events::spawn_meeting_event_bridge;
pub use frame::{AudioChannel, TaggedFrame};
pub use hotplug::{HotplugState, HotplugStats, UsbHotplugWatchdog, UsbHotplugWatchdogConfig};
pub use keepawake::KeepAwakeGuard;
pub use kws::{
    is_wake_phrase_match, match_wake_phrase, strip_wake_phrase, KwsEngine, KwsEngineConfig,
    KwsResult,
};
pub use loopback::LoopbackSource;
pub use mic::{MicSource, probe_default_device, record_mic_snippet};
pub use mixer::DualTrackMixer;
pub use process_watch::{ProcessWatcher, detect_meeting_process};
pub use recorder::{
    MeetingRecorder, NodeStats, PipelineStats, SilenceGuard, StatsHandle, SttCallback,
    TranscriptSegment,
};
pub use routes::{MeetingRouterState, meeting_routes};
pub use runtime::MeetingRuntime;
pub use session::{
    CreateMeetingSessionRequest, GenerateMeetingNotesResult, LatestMeetingCaption, ListenConfig,
    ListenWindowSegment, MeetingEvent, MeetingListenService, MeetingListenStatus, MeetingNoteTodo,
    MeetingNotes, MeetingNotesCompleter, MeetingNotesConversationSink, MeetingNotesSource,
    MeetingNotesStatus, MeetingNotesView, MeetingSegmentSnapshot, MeetingSessionService,
    MeetingSessionSnapshot, MeetingSessionStatus, MeetingSpeakerHighlight, SttBackendChoice,
    format_listen_context_block, format_listen_segment, select_relevant_segments,
    upsert_into_window,
};
pub use stt::{
    active_model_dir, register_active_model_dir, CloudSttCallback, FakeSttCallback, MeetingCloudStt,
    NullSttCallback, SHERPA_ASR_MODEL_DIR_ENV, SherpaSttCallback, SherpaSttError,
    SwitchableSttCallback, build_switchable_stt, transcribe_test_pcm, try_load_sherpa,
};
pub use vad::{EnergyVad, VadBackend, VadConfig, create_vad};
pub use voice_pipeline::{
    KwsWakeHit, PipelineConfig, SpscProducer, SpscRingBuffer, StubKwsDetector, StubKwsTrigger,
    VoiceCapturePipeline,
};
pub use voiceprint::{
    FakeVoiceprintEncoder, VoiceprintEncoder, VoiceprintEntry, VoiceprintGallery, VoiceprintMatch,
    VoiceprintStore, cosine_similarity, embedding_from_blob, embedding_to_blob, slice_pcm_ms,
};

#[cfg(any(feature = "local-stt", feature = "diarization"))]
pub use diarization::{SherpaDiarizer, SherpaDiarizerConfig};
#[cfg(any(feature = "local-stt", feature = "diarization"))]
pub use voiceprint::{SherpaVoiceprintEncoder, SherpaVoiceprintEncoderConfig};
