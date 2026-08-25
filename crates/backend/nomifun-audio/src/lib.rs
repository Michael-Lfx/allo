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
pub mod diarization;
pub mod encode;
pub mod events;
pub mod frame;
pub mod keepawake;
pub mod loopback;
pub mod mic;
pub mod mixer;
pub(crate) mod pcm_util;
pub mod process_watch;
pub mod recorder;
pub mod routes;
pub mod runtime;
pub mod session;
pub mod stt;
pub mod vad;
pub mod voiceprint;

pub use capture::AudioCaptureSource;
pub use devices::{AudioDeviceInfo, AudioDeviceManager, DeviceKind};
pub use diarization::{
    Diarizer, EnergyClusterDiarizer, EnergyClusterDiarizerConfig, SpeakerAssigner, SpeakerIdentity,
    SpeakerSpan, create_diarizer, dominant_speaker_key,
};
pub use encode::{encode_track_default, pcm_to_m4a, pcm_to_wav, write_track_m4a};
pub use events::spawn_meeting_event_bridge;
pub use frame::{AudioChannel, TaggedFrame};
pub use keepawake::KeepAwakeGuard;
pub use loopback::LoopbackSource;
pub use mic::MicSource;
pub use mixer::DualTrackMixer;
pub use process_watch::{ProcessWatcher, detect_meeting_process};
pub use recorder::{
    MeetingRecorder, NodeStats, PipelineStats, SilenceGuard, StatsHandle, SttCallback,
    TranscriptSegment,
};
pub use routes::{MeetingRouterState, meeting_routes};
pub use runtime::MeetingRuntime;
pub use session::{
    CreateMeetingSessionRequest, MeetingEvent, MeetingSegmentSnapshot, MeetingSessionService,
    MeetingSessionSnapshot, MeetingSessionStatus, SttBackendChoice,
};
pub use stt::{
    CloudSttCallback, FakeSttCallback, MeetingCloudStt, NullSttCallback, SHERPA_ASR_MODEL_DIR_ENV,
    SherpaSttCallback, SherpaSttError, SwitchableSttCallback, build_switchable_stt, try_load_sherpa,
};
pub use vad::{EnergyVad, VadBackend, VadConfig, create_vad};
pub use voiceprint::{
    FakeVoiceprintEncoder, VoiceprintEncoder, VoiceprintEntry, VoiceprintGallery, VoiceprintMatch,
    VoiceprintStore, cosine_similarity, embedding_from_blob, embedding_to_blob, slice_pcm_ms,
};

#[cfg(any(feature = "local-stt", feature = "diarization"))]
pub use diarization::{SherpaDiarizer, SherpaDiarizerConfig};
#[cfg(any(feature = "local-stt", feature = "diarization"))]
pub use voiceprint::{SherpaVoiceprintEncoder, SherpaVoiceprintEncoderConfig};
