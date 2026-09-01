//! News briefing engine: cited beats, research gates, voice align, compose.

pub mod cards;
pub mod compose;
pub mod error;
pub mod ir;
pub mod lint;
pub mod pipeline;
pub mod progress;
pub mod research;
pub mod service;
pub mod session;
pub mod stills;
pub mod tools;
pub mod voice;

pub use cards::{card_exists, CARD_CATALOG};
pub use error::{BriefingError, BriefingResult};
pub use ir::{
    Beat, BeatScript, Citation, Claim, Dossier, ResearchDepth, ResearchPlan, VisualKind,
    WordAnchor, domain_of, spoken_numerals,
};
pub use progress::{
    briefing_event_name, BriefingTerminalTelemetry, ProgressCallback, RunSnapshot, RunStatus,
};
pub use research::{
    clamp_format_secs, dossier_from_urls, merge_citations, SourceRetriever, FORMAT_SECS_DEFAULT,
    FORMAT_SECS_MAX, FORMAT_SECS_MIN,
};
pub use service::{
    BriefingService, CreateBriefingInput, SourceRetrieverHook, TerminalTelemetryHook, VoiceSynthHook,
};
pub use session::{SessionIndex, SessionRecord, SessionSummary};
pub use stills::{persist_atmosphere_stills, GeneratedStill, ImageChoice, StillSynth, ATMOSPHERE_CARDS};
pub use tools::wire_briefing_tools;
pub use voice::{
    align_chunks, align_from_asr, align_from_durations, align_proportional, apply_timing_to_beats,
    chunk_beats, parse_asr_json, persist_tts_chunks, AsrWord, SynthesizedClip, TimingFile, TtsChoice,
    TtsChunk, VoiceSynth, TTS_CHAR_LIMIT,
};
