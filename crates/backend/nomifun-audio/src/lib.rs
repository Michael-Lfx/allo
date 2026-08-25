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
pub mod frame;
pub mod keepawake;
pub mod loopback;
pub mod mixer;
pub mod process_watch;
pub mod recorder;
pub mod vad;

pub use capture::AudioCaptureSource;
pub use devices::{AudioDeviceInfo, AudioDeviceManager, DeviceKind};
pub use frame::{AudioChannel, TaggedFrame};
pub use keepawake::KeepAwakeGuard;
pub use loopback::LoopbackSource;
pub use mixer::DualTrackMixer;
pub use process_watch::{ProcessWatcher, detect_meeting_process};
pub use recorder::{
    MeetingRecorder, NodeStats, PipelineStats, SilenceGuard, StatsHandle, SttCallback,
    TranscriptSegment, pcm_to_wav,
};
pub use vad::{EnergyVad, VadBackend, VadConfig, create_vad};
